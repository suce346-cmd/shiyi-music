use crate::commands::prompts;
use crate::models::{LLMRequest, LLMResponse, RefineRequest, StreamChunk, Mode};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tauri::{command, AppHandle, Emitter};

/// 根据模式获取对应的系统提示词
fn system_prompt_for(mode: &Mode) -> &'static str {
    match mode {
        Mode::ModeA => prompts::mode_a_system_prompt(),
        Mode::ModeB => prompts::mode_b_system_prompt(),
        Mode::ModeC => prompts::mode_c_system_prompt(),
        Mode::ModeD => prompts::mode_d_system_prompt(),
    }
}

/// 构建 HTTP 客户端，配置连接超时和请求超时
fn build_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// 调用 LLM 流式接口并逐 chunk 转发给前端
async fn call_llm_stream(
    app: AppHandle,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
) -> Result<LLMResponse, String> {
    let client = build_client()?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "temperature": 0.7,
        "max_tokens": 8192,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    stream_response(app, response).await
}

/// 解析 SSE 流并逐 chunk 发送给前端。
/// 维护跨 chunk 行缓冲区，确保被拆分的 JSON 行不会丢失。
async fn stream_response(
    app: AppHandle,
    response: reqwest::Response,
) -> Result<LLMResponse, String> {
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(format!("API returned {}: {}", status, error_text));
    }

    let mut full_content = String::new();
    let mut finish_reason: Option<String> = None;
    let mut line_buf = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // 只处理以 \n 结尾的完整行，不完整的尾部留到下一个 chunk
        while let Some(pos) = line_buf.find('\n') {
            let line = line_buf[..pos].to_string();
            line_buf = line_buf[pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                        full_content.push_str(delta);
                        let _ = app.emit(
                            "llm-chunk",
                            StreamChunk {
                                content: delta.to_string(),
                            },
                        );
                    }
                    if let Some(fr) = parsed["choices"][0]["finish_reason"].as_str() {
                        finish_reason = Some(fr.to_string());
                    }
                }
            }
        }
    }

    Ok(LLMResponse {
        raw: full_content,
        finish_reason,
    })
}

/// 生成 Suno 提示词。根据模式选择系统提示词，流式返回结果。
#[command]
pub async fn generate_prompt(
    app: AppHandle,
    request: LLMRequest,
) -> Result<LLMResponse, String> {
    let system_prompt = system_prompt_for(&request.mode);

    let messages = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": request.user_input}),
    ];

    call_llm_stream(app, &request.base_url, &request.api_key, &request.model, messages).await
}

/// 基于历史对话和用户反馈优化提示词。
#[command]
pub async fn refine_prompt(
    app: AppHandle,
    request: RefineRequest,
) -> Result<LLMResponse, String> {
    let system_prompt = system_prompt_for(&request.mode);

    let mut messages: Vec<Value> = Vec::new();
    messages.push(serde_json::json!({"role": "system", "content": system_prompt}));
    for msg in &request.history {
        messages.push(serde_json::json!({"role": msg.role, "content": msg.content}));
    }
    messages.push(serde_json::json!({"role": "user", "content": format!(
        "请根据以下反馈优化上面的结果：\n\n{}", request.feedback
    )}));

    call_llm_stream(app, &request.base_url, &request.api_key, &request.model, messages).await
}
