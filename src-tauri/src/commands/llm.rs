use crate::budget::SharedBudget;
use crate::errors::{AppError, ErrorKind};
use crate::models::{LLMResponse, StreamChunk, TokenUsage};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};

/// 截断错误响应体（防敏感信息/超长回显，历史修复）
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

/// R4：退避抖动 0–5 秒（无 rand 依赖，用纳秒取模；纯函数阈值不动，抖动包在外面）。
/// 四角色并发同时 429 时错峰重发，避免同秒齐射撞出第二波 60s。
fn backoff_jitter_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 6)
        .unwrap_or(0)
}

/// 终稿保底额度（R1）：讨论轮内每次尝试/等待最多花掉 remaining - 该值，
/// 保证阶段 2（run_audit_format）至少有一次完整尝试 + 30s 退避的额度。
/// 阶段 2 入口传 reserve=ZERO 即解除约束，全额使用剩余预算。
pub(crate) const FINAL_STAGE_RESERVE: std::time::Duration = std::time::Duration::from_secs(120);

/// 带退避重试的请求发送：429/5xx/网络错误按 retry_plan 退避，最多 3 次（4xx 凭据类错误不重试）。
/// 错误分类——reqwest 层失败=Network，重试耗尽=Network。
/// 每次尝试前查共享预算——剩余不足则跳过重试直接 Timeout；退避等待可被预算到期中断。
/// R1：讨论轮传 reserve=FINAL_STAGE_RESERVE 预留终稿额度；阶段 2 传 reserve=ZERO 全额使用。
async fn send_with_retry(
    req: reqwest::RequestBuilder,
    budget: &SharedBudget,
    run_id: &str,
    reserve: std::time::Duration,
) -> Result<reqwest::Response, AppError> {
    /// 预算不足以再尝试一次的最低门槛（一次 HTTP 往返的悲观下限）
    const MIN_ATTEMPT_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);
    let mut last_err = "unknown".to_string();
    for attempt in 0..3 {
        // 取消检查优先于预算（按 run_id 隔离）
        if crate::commands::cancel::is_cancelled(run_id) {
            return Err(AppError::cancelled());
        }
        // 预算闸门——不够一次尝试就直接失败，不再烧钱等超时强杀
        // R1：讨论轮按 remaining - 保底判定，终稿（reserve=ZERO）按全额判定
        if budget.remaining_for_discussion(reserve) < MIN_ATTEMPT_BUDGET {
            // 终稿保底已解除仍不足：如实报全额剩余；讨论轮被保底拦：明示保底存在
            let hint = if reserve.is_zero() {
                format!("流水线预算不足（剩余 {:?}），停止重试", budget.remaining())
            } else {
                format!(
                    "讨论轮预算不足（可用 {:?}，已预留终稿 {:?}），停止重试",
                    budget.remaining_for_discussion(reserve),
                    reserve
                )
            };
            return Err(AppError::new(ErrorKind::Timeout, hint));
        }
        let builder = req.try_clone().ok_or_else(|| AppError::new(ErrorKind::Internal, "请求无法克隆（重试不可用）"))?;
        match builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                match retry_plan(attempt, Some(status.as_u16()), false) {
                    Some(wait) => {
                        last_err = format!("{} 错误", status);
                        // R4：计划等待 + 0–5s 抖动，四路并发错峰（阈值不动，离散包在外面）
                        let planned = wait + backoff_jitter_secs();
                        tracing::warn!(status = %status, wait_secs = planned, attempt = attempt + 1, "LLM 请求错误，退避重试");
                        // 退避等待可被预算到期中断——不等满，只等到 deadline
                        // R1：讨论轮等待上限为 remaining - 保底，不等满时按保底直接进终稿
                        let wait_dur = std::time::Duration::from_secs(planned)
                            .min(budget.remaining_for_discussion(reserve));
                        if tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_dur)).await.is_err() {
                            return Err(AppError::new(
                                ErrorKind::Timeout,
                                "等待重试时流水线预算耗尽".to_string(),
                            ));
                        }
                        // 保底截断了等待：不等满直接中止本轮，不再消耗（Q3：此前文案写“转入终稿”，
                        // 实际讨论轮 Timeout 经 .await? 直接中止整线，无跳终稿分支；文案如实改中止）。
                        if wait_dur < std::time::Duration::from_secs(planned) {
                            return Err(AppError::new(
                                ErrorKind::Timeout,
                                format!("讨论轮退避被终稿保底截断（已等 {:?}，计划 {}s），中止本轮", wait_dur, planned),
                            ));
                        }
                        continue;
                    }
                    None => return Ok(resp),
                }
            }
            Err(e) => {
                // 网络层错误（连接失败/超时/断流）：可重试
                if let Some(wait) = retry_plan(attempt, None, true) {
                    last_err = format!("网络错误: {}", e);
                    // R4：同 HTTP 分支叠加抖动
                    let planned = wait + backoff_jitter_secs();
                    tracing::warn!(wait_secs = planned, attempt = attempt + 1, error = %e, "LLM 网络错误，退避重试");
                    // R1：同 HTTP 分支，等待按保底截断
                    let wait_dur = std::time::Duration::from_secs(planned)
                        .min(budget.remaining_for_discussion(reserve));
                    if tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_dur)).await.is_err() {
                        return Err(AppError::new(
                            ErrorKind::Timeout,
                            "等待重试时流水线预算耗尽".to_string(),
                        ));
                    }
                    if wait_dur < std::time::Duration::from_secs(planned) {
                        return Err(AppError::new(
                            ErrorKind::Timeout,
                            format!("讨论轮退避被终稿保底截断（已等 {:?}，计划 {}s），中止本轮", wait_dur, planned),
                        ));
                    }
                    continue;
                }
                return Err(AppError::new(ErrorKind::Network, format!("API request failed: {}", e)));
            }
        }
    }
    Err(AppError::new(ErrorKind::Network, format!("API 请求失败（重试 3 次后仍失败）：{}", last_err)))
}

/// 构建 HTTP 客户端：connect 10s；总超时按调用场景（普通 120s，思考模式 300s——思维链+长输出耗时更长）
pub(crate) fn build_client(timeout_secs: u64) -> Result<Client, AppError> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Failed to build HTTP client: {}", e)))
}

/// 全部调用统一的 max_tokens 上限（用户决策 2026-09-06：放开截断限制到 30000——
/// max_tokens 是输出上限不是预扣费用，实际按生成量计费；B4 的 3000→6000 提额阶梯随之取消）
pub(crate) const MAX_TOKENS_CAP: u32 = 30000;

/// 思考模式开启时的 max_tokens 下限：思维链会吃掉一部分输出预算，防长输出被截断
/// （与 MAX_TOKENS_CAP 同值——上限放开后思维链同样有充足预算）
const THINKING_MAX_TOKENS: u32 = MAX_TOKENS_CAP;

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

/// 截断判定——finish_reason == "length"（网关 max_tokens 截断）
/// 或 "truncated_by_budget"（R8 保底断流：可用预算耗尽主动断开，内容不全）都算截断，
/// 调用方按既有分级处置（阶段 0 报错、汇总继续、格式进 issue）。
pub(crate) fn is_truncated(finish_reason: &Option<String>) -> bool {
    matches!(finish_reason.as_deref(), Some("length") | Some("truncated_by_budget"))
}

/// 从非流式响应中提取（正文 content, finish_reason, usage）（纯函数，可测）。
/// content 缺失或空字符串都算"无正文"（思考模式下思维链吃满 max_tokens 时 content 可能为空）。
/// finish_reason 一并返回——"length"（截断）由调用方分级处置，不再静默丢弃。
/// usage 一并返回（网关不返回时为 None，不阻塞流程）。
fn extract_message(json: &Value) -> Option<(String, Option<String>, Option<TokenUsage>)> {
    let c = json["choices"][0]["message"]["content"].as_str()?;
    if c.trim().is_empty() { return None; }
    Some((
        c.to_string(),
        json["choices"][0]["finish_reason"].as_str().map(String::from),
        TokenUsage::from_json(json),
    ))
}

/// 发送请求并解析正文；无正文返回 Err（供思考模式降级重试判断）。
/// HTTP 状态分类（401/403=Auth，429=RateLimit，5xx/其他=Network）；解析失败=Parse。
/// budget 透传给 send_with_retry（预算闸门）。
async fn send_and_extract(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
    budget: &SharedBudget,
    run_id: &str,
    reserve: std::time::Duration,
) -> Result<LLMResponse, AppError> {
    // R7：非流式 body 解码失败重试（网关抖动下 body 半截是常态）。
    // 仅解码路径重试：HTTP 状态错误仍直接分类返回，不碰 retry_plan 通道，避免双重退避。
    // body 字节一次读完后解析，失败则同参重发（预算闸门同样生效）。
    // 注意：首次发送在 extract_once 内，不在此处预发（避免空耗一次预算）。
    async fn extract_once(
        client: &Client,
        url: &str,
        api_key: &str,
        body: &Value,
        budget: &SharedBudget,
        run_id: &str,
        reserve: std::time::Duration,
    ) -> Result<LLMResponse, AppError> {
        let r = send_with_retry(
            client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(body),
            budget,
            run_id,
            reserve,
        )
        .await?;
        let status = r.status();
        if !status.is_success() {
            let err = r
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(AppError::api_status(status.as_u16(), &truncate_err(&err)));
        }
        let bytes = r
            .bytes()
            .await
            .map_err(|e| AppError::new(ErrorKind::Network, format!("Stream error: {}", e)))?;
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::new(ErrorKind::Parse, format!("API response parse failed: {}", e)))?;
        let (raw, finish_reason, usage) = extract_message(&json)
            .ok_or_else(|| AppError::new(ErrorKind::Parse, "API response missing content".to_string()))?;
        Ok(LLMResponse { raw, finish_reason, usage })
    }
    match extract_once(client, url, api_key, body, budget, run_id, reserve).await {
        Ok(resp) => Ok(resp),
        Err(e) if e.kind == ErrorKind::Parse || e.message.starts_with("Stream error") => {
            // R9：解码失败提到最多 3 次（首次 + 2 次重发，等待 5s/10s，可被预算打断）。
            // 实网重跑证明连续半截 body 是常态，一次不够。
            tracing::warn!(error = %e.message, "非流式响应解码失败，进入重发循环");
            let mut last = e;
            for attempt in 1..3 {
                // Q3：等待按 remaining - 保底截断，不吃终稿额度（与 send_with_retry 同口径）。
                let wait = std::time::Duration::from_secs(if attempt == 1 { 5 } else { 10 });
                let wait_capped = wait.min(budget.remaining_for_discussion(reserve));
                if wait_capped.is_zero() || tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_capped)).await.is_err() {
                    tracing::warn!("非流式重发等待被预算打断，不再重试");
                    break;
                }
                match extract_once(client, url, api_key, body, budget, run_id, reserve).await {
                    Ok(resp) => return Ok(resp),
                    Err(e2) if e2.kind == ErrorKind::Parse || e2.message.starts_with("Stream error") => {
                        tracing::warn!(attempt = attempt + 1, error = %e2.message, "非流式响应解码失败，重发");
                        last = e2;
                    }
                    Err(e2) => return Err(e2),
                }
            }
            Err(last)
        }
        Err(e) => Err(e),
    }
}

/// 无流式的单次 LLM 调用（流水线步骤用）。
/// 返回 LLMResponse（含 finish_reason，供调用方做截断分级处置）。
/// 思考模式下若响应无正文（思维链偶发吃满预算），自动降级为无思考重试一次，保证流水线不中断。
/// budget 透传（单调用超时按剩余预算收紧，见 build_client_for_budget）。
/// gen 缺省走内置默认（temperature 0.6 / max_tokens=调用方传入值）。
pub(crate) async fn call_llm_silent(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
    max_tokens: u32,
    thinking: bool,
    budget: &SharedBudget,
    gen: &crate::models::GenerationConfig,
    run_id: &str,
    reserve: std::time::Duration,
) -> Result<LLMResponse, AppError> {
    // 单调用超时取 min(场景默认, 剩余预算)——预算不足时 reqwest 层即快速失败
    let client_timeout = budget.remaining().as_secs().min(if thinking { 300 } else { 120 }).max(10);
    let client = build_client(client_timeout)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    // temperature 缺省 0.6；max_tokens 取 min(调用方, 配置)——配置只收紧不放宽旧行为
    let temperature = gen.silent_temperature();
    let max_tokens = max_tokens.min(gen.max_tokens());
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "temperature": temperature,
        "max_tokens": max_tokens,
    });
    if thinking {
        apply_thinking(&mut body, model, max_tokens);
    }
    match send_and_extract(&client, &url, &api_key, &body, budget, run_id, reserve).await {
        Ok(resp) => Ok(resp),
        Err(e) if thinking && e.message.contains("missing content") => {
            // 降级：去掉思考参数重试一次（同一 prompt 无思考直接输出，必有正文）
            tracing::warn!("思考模式响应无正文，降级为无思考重试一次");
            let fallback = serde_json::json!({
                "model": model,
                "messages": body["messages"],
                "stream": false,
                "temperature": temperature,
                "max_tokens": max_tokens,
            });
            send_and_extract(&client, &url, &api_key, &fallback, budget, run_id, reserve).await
        }
        Err(e) => Err(e),
    }
}

/// 调用 LLM 流式接口并逐 chunk 转发给前端（budget 透传gen 缺省 temperature 0.7）
pub(crate) async fn call_llm_stream<R: Runtime>(
    app: AppHandle<R>,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
    thinking: bool,
    budget: &SharedBudget,
    gen: &crate::models::GenerationConfig,
    run_id: &str,
    reserve: std::time::Duration,
) -> Result<LLMResponse, AppError> {
    let client_timeout = budget.remaining().as_secs().min(if thinking { 300 } else { 120 }).max(10);
    let client = build_client(client_timeout)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "temperature": gen.stream_temperature(),
        "max_tokens": gen.max_tokens(),
    });
    if thinking {
        apply_thinking(&mut body, model, gen.max_tokens());
    }

    let response = send_with_retry(
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body),
        budget,
        run_id,
        reserve,
    )
    .await?;

    stream_response(app, response, run_id, budget, reserve).await
}

/// 解析单行 SSE 数据（`data: ...` 前缀行，兼容 `data:{...}` 无空格变体，尾部行修复）。
/// 返回本次解析出的增量内容（未 emit，由调用方决定是否转发）。
/// 流式 usage 块（`{"choices":[],"usage":{...}}` 尾部形态）累加进 stream_usage。
/// 纯函数，可独立单测（不含网络）。
fn parse_sse_line(
    line: &str,
    full_content: &mut String,
    finish_reason: &mut Option<String>,
    stream_usage: &mut TokenUsage,
) -> Option<String> {
    let trimmed = line.trim();
    if let Some(data) = trimmed.strip_prefix("data:") {
        let data = data.trim_start();
        if data == "[DONE]" {
            return None;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
            // usage 块可能独立出现（choices 为空），先累加再处理 delta
            if let Some(u) = TokenUsage::from_json(&parsed) {
                stream_usage.add(&u);
            }
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
/// R8：按保底断流——可用预算（remaining - reserve）耗尽时不断死，
/// 带着已收内容正常返回（finish_reason 记 truncated_by_budget，调用方按截断分级处置）。
async fn stream_response<R: Runtime>(
    app: AppHandle<R>,
    response: reqwest::Response,
    run_id: &str,
    budget: &SharedBudget,
    reserve: std::time::Duration,
) -> Result<LLMResponse, AppError> {
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(AppError::api_status(status.as_u16(), &truncate_err(&error_text)));
    }

    let mut full_content = String::new();
    let mut finish_reason: Option<String> = None;
    // 流式 usage 累加（尾部独立块形态）
    let mut stream_usage = TokenUsage::default();
    let mut line_buf = String::new();
    let mut stream = response.bytes_stream();
    let started = std::time::Instant::now();
    let mut last_log = std::time::Instant::now();
    // R8：是否触发过保底断流（触发后 finish_reason 记 truncated_by_budget）
    let mut budget_cut = false;

    while let Some(chunk_result) = stream.next().await {
        // 取消检查点——按 run_id 隔离
        if crate::commands::cancel::is_cancelled(run_id) {
            return Err(AppError::cancelled());
        }
        // R8：保底断流——可用耗尽时不断死，带已收内容返回（空正文才报错）
        if budget.remaining_for_discussion(reserve).is_zero() {
            tracing::warn!(
                elapsed_secs = started.elapsed().as_secs(),
                chars = full_content.chars().count(),
                "流式可用预算耗尽，保底断流（已收内容返回，调用方按截断处置）"
            );
            budget_cut = true;
            break;
        }
        if last_log.elapsed().as_secs() >= 30 {
            tracing::info!(elapsed_secs = started.elapsed().as_secs(), chars = full_content.chars().count(), "流式进行中");
            last_log = std::time::Instant::now();
        }
        let chunk = chunk_result.map_err(|e| {
            tracing::warn!(elapsed_secs = started.elapsed().as_secs(), error = %e, "流式错误");
            AppError::new(ErrorKind::Network, format!("Stream error: {}", e))
        })?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // 只处理以 \n 结尾的完整行，不完整的尾部留到下一个 chunk
        while let Some(pos) = line_buf.find('\n') {
            let line = line_buf[..pos].to_string();
            line_buf = line_buf[pos + 1..].to_string();

            if let Some(delta) = parse_sse_line(&line, &mut full_content, &mut finish_reason, &mut stream_usage) {
                let _ = app.emit(
                    "llm-chunk",
                    StreamChunk {
                        content: delta,
                    },
                );
            }
        }
    }

    // 流结束后 flush 残留的不完整行（尾部行修复：尾部 data 行不再静默丢弃）
    if !line_buf.trim().is_empty() {
        if let Some(delta) = parse_sse_line(&line_buf, &mut full_content, &mut finish_reason, &mut stream_usage) {
            let _ = app.emit("llm-chunk", StreamChunk { content: delta });
        }
    }

    // 思考模式兜底：思维链吃满预算时可能全程无正文，禁止静默产出空方案
    if full_content.trim().is_empty() {
        return Err(AppError::new(
            ErrorKind::Parse,
            "思考模式流式响应无正文（思维链可能耗尽预算）",
        ));
    }
    // R8：保底断流且网关没给 finish_reason（没发完）→ 记 truncated_by_budget；
    // 网关给了（正常 stop/length）则保留网关的。调用方 is_truncated 认该标记，
    // 按既有截断分级处置（阶段 0 报错、汇总继续、格式进 issue）。
    if budget_cut && finish_reason.is_none() {
        finish_reason = Some("truncated_by_budget".to_string());
    }

    Ok(LLMResponse {
        raw: full_content,
        finish_reason,
        // 流式 usage 全零时记 None（与非流式宽容语义一致）
        usage: if stream_usage.prompt_tokens == 0 && stream_usage.completion_tokens == 0 {
            None
        } else {
            Some(stream_usage)
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_extracts_delta_and_finish() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#;
        let delta = parse_sse_line(line, &mut full, &mut finish, &mut usage);
        assert_eq!(delta.as_deref(), Some("你好"));
        assert_eq!(full, "你好");
        assert_eq!(finish, None);
    }

    #[test]
    fn sse_handles_done() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line("data: [DONE]", &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
    }

    #[test]
    fn sse_extracts_finish_reason_stop() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        parse_sse_line(line, &mut full, &mut finish, &mut usage);
        assert_eq!(finish.as_deref(), Some("stop"));
    }

    #[test]
    fn sse_ignores_non_data_lines() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line(": keep-alive", &mut full, &mut finish, &mut usage), None);
        assert_eq!(parse_sse_line("", &mut full, &mut finish, &mut usage), None);
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

    /// R4：退避抖动恒在 0–5s（阈值不动，离散包在外面；多次采样不越界）
    #[test]
    fn backoff_jitter_within_five_secs() {
        for _ in 0..50 {
            assert!(backoff_jitter_secs() <= 5, "抖动必须在 0–5s 内");
        }
    }

    #[test]
    fn sse_ignores_malformed_json() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line("data: not-json", &mut full, &mut finish, &mut usage), None);
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
        let mut usage = TokenUsage::default();
        for line in lines {
            parse_sse_line(line, &mut full, &mut finish, &mut usage);
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
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line(thinking, &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
        // 思考结束进入正文：delta 带 content
        let content = r#"data: {"choices":[{"delta":{"content":"最终方案"},"finish_reason":null}]}"#;
        assert_eq!(parse_sse_line(content, &mut full, &mut finish, &mut usage), Some("最终方案".to_string()));
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

    /// 开思考时 max_tokens 抬升到上限（防思维链挤占输出），且只抬不降
    #[test]
    fn apply_thinking_raises_max_tokens() {
        let mut body = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 3000});
        apply_thinking(&mut body, "xopdeepseekv4flash0731", 3000);
        assert_eq!(body["max_tokens"], THINKING_MAX_TOKENS);
        assert_eq!(body["thinking"]["type"], "enabled");
        // 调用方 max_tokens 已超上限：保持原值不降低
        let mut body2 = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 32000});
        apply_thinking(&mut body2, "xopdeepseekv4flash0731", 32000);
        assert_eq!(body2["max_tokens"], 32000);
    }

    /// 正文与 finish_reason 联合提取——正常 / 缺失 / 空字符串（思考模式思维链吃满预算）都要正确判定
    #[test]
    fn extract_message_handles_missing_and_empty() {
        // 正常：content + finish_reason 一起返回
        let ok = serde_json::json!({"choices": [{"message": {"content": "方案内容"}, "finish_reason": "stop"}]});
        let (c, fr, u) = extract_message(&ok).unwrap();
        assert_eq!(c, "方案内容");
        assert_eq!(fr.as_deref(), Some("stop"));
        assert!(u.is_none(), "无 usage 字段应为 None");
        // finish_reason 缺省也要容忍（部分网关不返回）
        let no_fr = serde_json::json!({"choices": [{"message": {"content": "方案内容"}}]});
        let (_, fr2, u2) = extract_message(&no_fr).unwrap();
        assert_eq!(fr2, None);
        assert!(u2.is_none(), "无 usage 字段应为 None");
        // 缺失 content（思维链吃满预算时网关可能不返回 content）
        let missing = serde_json::json!({"choices": [{"message": {"reasoning_content": "思考..."}}]});
        assert!(extract_message(&missing).is_none());
        // content 为空字符串也算无正文
        let empty = serde_json::json!({"choices": [{"message": {"content": ""}}]});
        assert!(extract_message(&empty).is_none());
        let blank = serde_json::json!({"choices": [{"message": {"content": "   "}}]});
        assert!(extract_message(&blank).is_none());
    }

    /// 预算耗尽时 send_with_retry 不发起请求，直接 Timeout（用不可达地址验证：若发起请求会是 Network 错误）
    #[tokio::test]
    async fn send_with_retry_budget_exhausted_skips() {
        use crate::budget::Budget;
        use std::sync::Arc;
        let client = build_client(10).unwrap();
        let req = client.get("http://127.0.0.1:1/unreachable");
        // 预算已耗尽（0ms）：必须直接 Timeout，不得尝试发送
        let spent = Arc::new(Budget::with_timeout(std::time::Duration::from_millis(0)));
        std::thread::sleep(std::time::Duration::from_millis(5));
        let err = send_with_retry(req, &spent, "", std::time::Duration::ZERO).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Timeout, "预算耗尽应直接 Timeout: {:?}", err);
        // 预算充足时同样不可达地址应走 Network 路径（证明闸门是预算触发的，不是地址问题）
        let rich = Arc::new(Budget::unlimited());
        let req2 = build_client(2).unwrap().get("http://127.0.0.1:1/unreachable");
        let err2 = send_with_retry(req2, &rich, "", std::time::Duration::ZERO).await.unwrap_err();
        assert_eq!(err2.kind, ErrorKind::Network, "预算充足时应尝试发送并报 Network: {:?}", err2);
    }

    /// R1：讨论轮保底——剩余刚好等于保底时，可用归零，门直接拦（终稿靠全额续命）
    #[tokio::test]
    async fn send_with_retry_reserve_blocks_discussion_but_not_final() {
        use crate::budget::Budget;
        use std::sync::Arc;
        let client = build_client(10).unwrap();
        // 剩余 130s，保底 120s：可用 10s < 15s 门 → 讨论轮被拦，文案明示保底
        let tight = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(130)));
        let req = client.get("http://127.0.0.1:1/unreachable");
        let err = send_with_retry(req, &tight, "", FINAL_STAGE_RESERVE).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Timeout);
        assert!(err.message.contains("终稿"), "讨论轮被拦文案应明示保底，实际: {}", err.message);
        // 同一预算终稿（reserve=ZERO）按全额判定：130s 充足 → 放行尝试（不可达地址走 Network）
        let req2 = build_client(2).unwrap().get("http://127.0.0.1:1/unreachable");
        let err2 = send_with_retry(req2, &tight, "", std::time::Duration::ZERO).await.unwrap_err();
        assert_eq!(err2.kind, ErrorKind::Network, "终稿全额下应放行尝试: {:?}", err2);
    }

    /// 非流式 usage 提取——正常返回 Some，缺字段/全零为 None
    #[test]
    fn extract_message_returns_usage() {
        let ok = serde_json::json!({
            "choices": [{"message": {"content": "x"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 120, "completion_tokens": 34, "total_tokens": 154}
        });
        let (_, _, u) = extract_message(&ok).unwrap();
        let u = u.unwrap();
        assert_eq!((u.prompt_tokens, u.completion_tokens), (120, 34));
        let no_u = serde_json::json!({"choices": [{"message": {"content": "x"}}]});
        let (_, _, u2) = extract_message(&no_u).unwrap();
        assert!(u2.is_none());
    }

    /// 流式 usage 尾部块（choices 为空）累加，不污染正文
    #[test]
    fn sse_usage_block_accumulates() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":200,"completion_tokens":50}}"#;
        assert_eq!(parse_sse_line(line, &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
        assert_eq!((usage.prompt_tokens, usage.completion_tokens), (200, 50));
    }

    /// test_api_body 组装——max_tokens=16（旧 1 会让思考模型误报），模型名透传
    #[test]
    fn test_api_body_uses_16_tokens() {
        let b = test_api_body("my-model");
        assert_eq!(b["max_tokens"], 16);
        assert_eq!(b["model"], "my-model");
        assert_eq!(b["stream"], false);
    }

    /// 截断判定——只有 finish_reason == "length" 算截断（stop/缺失/其他值都不算）
    #[test]
    fn is_truncated_only_for_length() {
        assert!(is_truncated(&Some("length".to_string())));
        // R8：保底断流标记同样算截断（调用方按既有分级处置）
        assert!(is_truncated(&Some("truncated_by_budget".to_string())));
        assert!(!is_truncated(&Some("stop".to_string())));
        assert!(!is_truncated(&None));
        assert!(!is_truncated(&Some("tool_calls".to_string())));
    }
}

/// 测试连接请求体组装（纯函数，可测）——max_tokens=16（足够返回 hi 级响应；
/// 旧 max_tokens=1 会让思考型模型无正文/400 误报"连接失败"）
fn test_api_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16,
        "stream": false,
    })
}

/// 测试 API 连接（前端"测试连接"按钮用）。
/// 走 Rust 后端发请求，绕过 WebView CORS 限制（前端 fetch 跨域会被拦）。
/// 成功判定：HTTP 200 即成功（不要求 content 非空——思考模型可能只回 reasoning）。
/// 失败分类复用 api_status（401/403=Auth，429=RateLimit，其余=Network）。
#[tauri::command]
pub async fn test_api(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, AppError> {
    // R5：改走 send_with_retry（间歇 429 自动退避重试，不再一次定胜负误报）。
    // 预算独立 120s（不占流水线 15 分钟池，天然隔离）；成功判定与失败分类原样保留。
    use crate::budget::Budget;
    use std::sync::Arc;
    let client = build_client(120)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = test_api_body(&model);
    let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(120)));
    let resp = send_with_retry(
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body),
        &budget,
        "",
        std::time::Duration::ZERO,
    )
    .await
    .map_err(|e| AppError::new(ErrorKind::Network, format!("请求失败: {}", truncate_err(&e.message))))?;
    let status = resp.status();
    if status.is_success() {
        Ok("连接成功".into())
    } else {
        let err = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(AppError::api_status(status.as_u16(), &truncate_err(&err)))
    }
}
