use crate::models::{LLMResponse, StreamChunk};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};

/// 截断错误响应体（防敏感信息/超长回显，M7 修复）
pub(crate) fn truncate_err(s: &str) -> String {
    const MAX: usize = 300;
    if s.chars().count() > MAX {
        s.chars().take(MAX).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// 重试决策（纯函数，可测）：返回(是否可重试, 等待秒数)。
/// - 429 限流：30s/60s 退避（API 配额恢复后自动续跑）
/// - 5xx 服务端错误：15s/30s 退避（瞬时故障，重试通常可恢复）
/// - 网络错误（连接失败/超时）：10s/20s 退避（断网恢复后重试）
/// - 其他 4xx（400/401/403 等凭据或请求问题）：不可重试，重试无意义
fn retry_plan(attempt: usize, status: Option<u16>, network_err: bool) -> Option<u64> {
    match (status, network_err) {
        (Some(429), _) => Some(if attempt == 0 { 30 } else { 60 }),
        (Some(s), _) if s >= 500 => Some(if attempt == 0 { 15 } else { 30 }),
        (None, true) => Some(if attempt == 0 { 10 } else { 20 }),
        _ => None, // 4xx 或其他：不重试
    }
}

/// 带退避重试的请求发送：429/5xx/网络错误按 retry_plan 退避，最多 3 次（4xx 凭据类错误不重试）
async fn send_with_retry(req: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let mut last_err = "unknown".to_string();
    for attempt in 0..3 {
        let builder = req.try_clone().ok_or("请求无法克隆（重试不可用）")?;
        match builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                match retry_plan(attempt, Some(status.as_u16()), false) {
                    Some(wait) => {
                        last_err = format!("{} 错误", status);
                        eprintln!("[llm] {} 错误，{}s 后重试（{}/3）", status, wait, attempt + 1);
                        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                        continue;
                    }
                    None => return Ok(resp),
                }
            }
            Err(e) => {
                // 网络层错误（连接失败/超时/断流）：可重试
                if let Some(wait) = retry_plan(attempt, None, true) {
                    last_err = format!("网络错误: {}", e);
                    eprintln!("[llm] 网络错误，{}s 后重试（{}/3）: {}", wait, attempt + 1, e);
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(format!("API request failed: {}", e));
            }
        }
    }
    Err(format!("API 请求失败（重试 3 次后仍失败）：{}", last_err))
}

/// 构建 HTTP 客户端：connect 10s；总超时按调用场景（普通 120s，思考模式 300s——思维链+长输出耗时更长）
pub(crate) fn build_client(timeout_secs: u64) -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// 思考模式开启时的 max_tokens 下限：思维链会吃掉一部分输出预算，防长输出被截断
const THINKING_MAX_TOKENS: u32 = 16384;

/// 思考参数路由表（纯函数，可测）：按模型名匹配各厂商的思考参数模板。
/// - DeepSeek 系（含讯飞 MaaS xopdeepseek*）：thinking 二元开关（实测网关接受，返回 reasoning_content）
/// - OpenAI o 系 / gpt-5：reasoning_effort 等级（o 系专用，gpt-4o 传了会 400，故只匹配 o 系/gpt-5）
/// - Anthropic 系（OpenAI 兼容网关）：thinking + budget_tokens
/// - 未知模型：None = 不带任何参数（安全默认，避免不支持的网关 400 报错）
/// 未来换模型只需在此加一行。
pub(crate) fn thinking_params(model: &str) -> Option<Value> {
    let m = model.to_lowercase();
    if m.contains("deepseek") {
        Some(serde_json::json!({"thinking": {"type": "enabled"}}))
    } else if m.contains("claude") {
        Some(serde_json::json!({"thinking": {"type": "enabled", "budget_tokens": 2048}}))
    } else if ["o1", "o3", "o4", "gpt-5"].iter().any(|k| m.contains(k)) {
        Some(serde_json::json!({"reasoning_effort": "medium"}))
    } else {
        None
    }
}

/// 开思考时：注入厂商思考参数 + 抬升 max_tokens 防思维链挤占输出空间（只抬不降）
fn apply_thinking(body: &mut Value, model: &str, max_tokens: u32) {
    if let Some(params) = thinking_params(model) {
        if let Value::Object(map) = params {
            for (k, v) in map {
                body[k] = v;
            }
        }
    }
    body["max_tokens"] = max_tokens.max(THINKING_MAX_TOKENS).into();
}

/// 从非流式响应中提取正文 content（纯函数，可测）。
/// 缺失或空字符串都算"无正文"（思考模式下思维链吃满 max_tokens 时 content 可能为空）
fn content_from_json(json: &Value) -> Option<String> {
    let c = json["choices"][0]["message"]["content"].as_str()?;
    if c.trim().is_empty() { None } else { Some(c.to_string()) }
}

/// 发送请求并解析正文 content；无正文返回 Err（供思考模式降级重试判断）
async fn send_and_extract(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> Result<String, String> {
    let resp = send_with_retry(
        client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(body),
    )
    .await?;
    let status = resp.status();
    if !status.is_success() {
        let err = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(format!("API returned {}: {}", status, truncate_err(&err)));
    }
    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("API response parse failed: {}", e))?;
    content_from_json(&json).ok_or_else(|| "API response missing content".to_string())
}

/// 无流式的单次 LLM 调用（流水线步骤用，小 max_tokens）。
/// 思考模式下若响应无正文（思维链偶发吃满预算），自动降级为无思考重试一次，保证流水线不中断。
pub(crate) async fn call_llm_silent(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
    max_tokens: u32,
    thinking: bool,
) -> Result<String, String> {
    let client = build_client(if thinking { 300 } else { 120 })?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "temperature": 0.6,
        "max_tokens": max_tokens,
    });
    if thinking {
        apply_thinking(&mut body, model, max_tokens);
    }
    match send_and_extract(&client, &url, &api_key, &body).await {
        Ok(content) => Ok(content),
        Err(e) if thinking && e.contains("missing content") => {
            // 降级：去掉思考参数重试一次（同一 prompt 无思考直接输出，必有正文）
            eprintln!("[llm] 思考模式响应无正文，降级为无思考重试一次");
            let fallback = serde_json::json!({
                "model": model,
                "messages": body["messages"],
                "stream": false,
                "temperature": 0.6,
                "max_tokens": max_tokens,
            });
            send_and_extract(&client, &url, &api_key, &fallback).await
        }
        Err(e) => Err(e),
    }
}

/// 调用 LLM 流式接口并逐 chunk 转发给前端
pub(crate) async fn call_llm_stream<R: Runtime>(
    app: AppHandle<R>,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
    thinking: bool,
) -> Result<LLMResponse, String> {
    let client = build_client(if thinking { 300 } else { 120 })?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "temperature": 0.7,
        "max_tokens": 8192,
    });
    if thinking {
        apply_thinking(&mut body, model, 8192);
    }

    let response = send_with_retry(
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body),
    )
    .await?;

    stream_response(app, response).await
}

/// 解析单行 SSE 数据（`data: ...` 前缀行，兼容 `data:{...}` 无空格变体，M8 修复）。
/// 返回本次解析出的增量内容（未 emit，由调用方决定是否转发）。
/// 纯函数，可独立单测（不含网络）。
fn parse_sse_line(line: &str, full_content: &mut String, finish_reason: &mut Option<String>) -> Option<String> {
    let trimmed = line.trim();
    if let Some(data) = trimmed.strip_prefix("data:") {
        let data = data.trim_start();
        if data == "[DONE]" {
            return None;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
            let mut delta = None;
            if let Some(d) = parsed["choices"][0]["delta"]["content"].as_str() {
                full_content.push_str(d);
                delta = Some(d.to_string());
            }
            if let Some(fr) = parsed["choices"][0]["finish_reason"].as_str() {
                *finish_reason = Some(fr.to_string());
            }
            return delta;
        }
    }
    None
}

/// 解析 SSE 流并逐 chunk 发送给前端。
/// 维护跨 chunk 行缓冲区，确保被拆分的 JSON 行不会丢失。
async fn stream_response<R: Runtime>(
    app: AppHandle<R>,
    response: reqwest::Response,
) -> Result<LLMResponse, String> {
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(format!("API returned {}: {}", status, truncate_err(&error_text)));
    }

    let mut full_content = String::new();
    let mut finish_reason: Option<String> = None;
    let mut line_buf = String::new();
    let mut stream = response.bytes_stream();
    let started = std::time::Instant::now();
    let mut last_log = std::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        if last_log.elapsed().as_secs() >= 30 {
            eprintln!("[llm] 流式进行中 {}s，已收 {} 字符", started.elapsed().as_secs(), full_content.chars().count());
            last_log = std::time::Instant::now();
        }
        let chunk = chunk_result.map_err(|e| {
            eprintln!("[llm] 流式错误（{}s 时）: {}", started.elapsed().as_secs(), e);
            format!("Stream error: {}", e)
        })?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // 只处理以 \n 结尾的完整行，不完整的尾部留到下一个 chunk
        while let Some(pos) = line_buf.find('\n') {
            let line = line_buf[..pos].to_string();
            line_buf = line_buf[pos + 1..].to_string();

            if let Some(delta) = parse_sse_line(&line, &mut full_content, &mut finish_reason) {
                let _ = app.emit(
                    "llm-chunk",
                    StreamChunk {
                        content: delta,
                    },
                );
            }
        }
    }

    // 流结束后 flush 残留的不完整行（M8 修复：尾部 data 行不再静默丢弃）
    if !line_buf.trim().is_empty() {
        if let Some(delta) = parse_sse_line(&line_buf, &mut full_content, &mut finish_reason) {
            let _ = app.emit("llm-chunk", StreamChunk { content: delta });
        }
    }

    // 思考模式兜底：思维链吃满预算时可能全程无正文，禁止静默产出空方案
    if full_content.trim().is_empty() {
        return Err("思考模式流式响应无正文（思维链可能耗尽预算）".to_string());
    }

    Ok(LLMResponse {
        raw: full_content,
        finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_extracts_delta_and_finish() {
        let mut full = String::new();
        let mut finish = None;
        let line = r#"data: {"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#;
        let delta = parse_sse_line(line, &mut full, &mut finish);
        assert_eq!(delta.as_deref(), Some("你好"));
        assert_eq!(full, "你好");
        assert_eq!(finish, None);
    }

    #[test]
    fn sse_handles_done() {
        let mut full = String::new();
        let mut finish = None;
        assert_eq!(parse_sse_line("data: [DONE]", &mut full, &mut finish), None);
        assert_eq!(full, "");
    }

    #[test]
    fn sse_extracts_finish_reason_stop() {
        let mut full = String::new();
        let mut finish = None;
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        parse_sse_line(line, &mut full, &mut finish);
        assert_eq!(finish.as_deref(), Some("stop"));
    }

    #[test]
    fn sse_ignores_non_data_lines() {
        let mut full = String::new();
        let mut finish = None;
        assert_eq!(parse_sse_line(": keep-alive", &mut full, &mut finish), None);
        assert_eq!(parse_sse_line("", &mut full, &mut finish), None);
    }

    /// 重试决策：429 退避 30s/60s；5xx 退避 15s/30s；网络错误 10s/20s；4xx 不重试
    #[test]
    fn retry_plan_classifies_errors() {
        // 429 限流：可重试，30s/60s
        assert_eq!(retry_plan(0, Some(429), false), Some(30));
        assert_eq!(retry_plan(1, Some(429), false), Some(60));
        // 5xx 服务端错误：可重试，15s/30s
        assert_eq!(retry_plan(0, Some(500), false), Some(15));
        assert_eq!(retry_plan(1, Some(502), false), Some(30));
        assert_eq!(retry_plan(0, Some(503), false), Some(15));
        // 网络错误：可重试，10s/20s
        assert_eq!(retry_plan(0, None, true), Some(10));
        assert_eq!(retry_plan(1, None, true), Some(20));
        // 4xx 凭据/请求错误：不可重试
        assert_eq!(retry_plan(0, Some(400), false), None);
        assert_eq!(retry_plan(0, Some(401), false), None);
        assert_eq!(retry_plan(0, Some(403), false), None);
        assert_eq!(retry_plan(0, Some(404), false), None);
        // 成功响应无需重试决策（调用方直接返回）
        assert_eq!(retry_plan(0, Some(200), false), None);
    }

    #[test]
    fn sse_ignores_malformed_json() {
        let mut full = String::new();
        let mut finish = None;
        assert_eq!(parse_sse_line("data: not-json", &mut full, &mut finish), None);
        assert_eq!(full, "");
    }

    #[test]
    fn build_client_configures_timeouts() {
        let client = build_client(120).unwrap();
        // 仅验证构建成功即可（超时值在 reqwest 内部）
        assert!(client.get("https://example.com").build().is_ok());
        assert!(build_client(300).is_ok());
    }

    #[test]
    fn sse_content_with_finish_reason_accumulates() {
        // 模拟完整 SSE 流：内容 + finish_reason
        let lines = [
            r#"data: {"choices":[{"delta":{"content":"你"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"好"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ];
        let mut full = String::new();
        let mut finish = None;
        for line in lines {
            parse_sse_line(line, &mut full, &mut finish);
        }
        assert_eq!(full, "你好");
        assert_eq!(finish.as_deref(), Some("stop"));
    }

    /// 思考模式流式：思维链 chunk（delta 只带 reasoning_content）必须被跳过，不污染正文
    #[test]
    fn sse_skips_reasoning_content_chunks() {
        let mut full = String::new();
        let mut finish = None;
        // 思考阶段：delta 只有 reasoning_content，无 content
        let thinking = r#"data: {"choices":[{"delta":{"reasoning_content":"让我想想这个方案…"},"finish_reason":null}]}"#;
        assert_eq!(parse_sse_line(thinking, &mut full, &mut finish), None);
        assert_eq!(full, "");
        // 思考结束进入正文：delta 带 content
        let content = r#"data: {"choices":[{"delta":{"content":"最终方案"},"finish_reason":null}]}"#;
        assert_eq!(parse_sse_line(content, &mut full, &mut finish), Some("最终方案".to_string()));
        assert_eq!(full, "最终方案");
    }

    /// 思考参数路由：DeepSeek 系（含讯飞 MaaS）→ thinking 开关
    #[test]
    fn thinking_params_deepseek_family() {
        for m in ["xopdeepseekv4flash0731", "deepseek-chat", "DeepSeek-V3"] {
            let p = thinking_params(m).unwrap();
            assert_eq!(p["thinking"]["type"], "enabled", "模型 {} 应注入 thinking.enabled", m);
        }
    }

    /// 思考参数路由：OpenAI o 系/gpt-5 → reasoning_effort；gpt-4o 等旧模型不匹配（传了会 400）
    #[test]
    fn thinking_params_openai_family() {
        for m in ["o3-mini", "o4-mini", "gpt-5"] {
            let p = thinking_params(m).unwrap();
            assert_eq!(p["reasoning_effort"], "medium", "模型 {} 应注入 reasoning_effort", m);
        }
        for m in ["gpt-4o", "gpt-4o-mini"] {
            assert_eq!(thinking_params(m), None, "{} 不支持 reasoning_effort，必须不带参数", m);
        }
    }

    /// 思考参数路由：Anthropic 系 → thinking + budget；未知模型 → 安全默认不带参数
    #[test]
    fn thinking_params_claude_and_unknown() {
        let p = thinking_params("claude-sonnet-4-5").unwrap();
        assert_eq!(p["thinking"]["type"], "enabled");
        assert_eq!(p["thinking"]["budget_tokens"], 2048);
        for m in ["sensenova-6.7-flash-lite", "qwen-max", "glm-4", "kimi"] {
            assert_eq!(thinking_params(m), None, "未登记模型 {} 必须不带参数（安全默认）", m);
        }
    }

    /// 开思考时 max_tokens 抬升到 16384 下限（防思维链挤占输出），且只抬不降
    #[test]
    fn apply_thinking_raises_max_tokens() {
        let mut body = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 3000});
        apply_thinking(&mut body, "xopdeepseekv4flash0731", 3000);
        assert_eq!(body["max_tokens"], THINKING_MAX_TOKENS);
        assert_eq!(body["thinking"]["type"], "enabled");
        // 调用方 max_tokens 已超下限：保持原值不降低
        let mut body2 = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 20000});
        apply_thinking(&mut body2, "xopdeepseekv4flash0731", 20000);
        assert_eq!(body2["max_tokens"], 20000);
    }

    /// 正文提取：正常 content / 缺失 / 空字符串（思考模式思维链吃满预算）都要正确判定
    #[test]
    fn content_from_json_handles_missing_and_empty() {
        // 正常
        let ok = serde_json::json!({"choices": [{"message": {"content": "方案内容"}}]});
        assert_eq!(content_from_json(&ok).as_deref(), Some("方案内容"));
        // 缺失 content（思维链吃满预算时网关可能不返回 content）
        let missing = serde_json::json!({"choices": [{"message": {"reasoning_content": "思考..."}}]});
        assert_eq!(content_from_json(&missing), None);
        // content 为空字符串也算无正文
        let empty = serde_json::json!({"choices": [{"message": {"content": ""}}]});
        assert_eq!(content_from_json(&empty), None);
        let blank = serde_json::json!({"choices": [{"message": {"content": "   "}}]});
        assert_eq!(content_from_json(&blank), None);
    }
}

/// 测试 API 连接（前端"测试连接"按钮用）。
/// 走 Rust 后端发请求，绕过 WebView CORS 限制（前端 fetch 跨域会被拦）。
#[tauri::command]
pub async fn test_api(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    let client = build_client(120)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1,
        "stream": false,
    });
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", truncate_err(&e.to_string())))?;
    let status = resp.status();
    if status.is_success() {
        Ok("连接成功".into())
    } else {
        let err = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(format!("HTTP {}: {}", status, truncate_err(&err)))
    }
}
