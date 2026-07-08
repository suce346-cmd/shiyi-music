use crate::commands::prompts;
use crate::models::{LLMRequest, LLMResponse, RefineRequest, StreamChunk, Mode};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tauri::{command, AppHandle, Emitter};

#[command]
pub async fn generate_prompt(
    app: AppHandle,
    request: LLMRequest,
) -> Result<LLMResponse, String> {
    let system_prompt = match request.mode {
        Mode::ModeA => prompts::mode_a_system_prompt(),
        Mode::ModeD => prompts::mode_d_system_prompt(),
    };

    let client = Client::new();
    let url = format!(
        "{}/chat/completions",
        request.base_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "model": request.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": request.user_input}
        ],
        "stream": true,
        "temperature": 0.7,
        "max_tokens": 4096,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", request.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    stream_response(app, response).await
}

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
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let chunk_str = String::from_utf8_lossy(&chunk);

        for line in chunk_str.lines() {
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
                                finished: false,
                            },
                        );
                    }
                }
            }
        }
    }

    Ok(LLMResponse {
        raw: full_content,
    })
}

#[command]
pub async fn refine_prompt(
    app: AppHandle,
    request: RefineRequest,
) -> Result<LLMResponse, String> {
    let system_prompt = match request.mode {
        Mode::ModeA => prompts::mode_a_system_prompt(),
        Mode::ModeD => prompts::mode_d_system_prompt(),
    };

    let client = Client::new();
    let url = format!(
        "{}/chat/completions",
        request.base_url.trim_end_matches('/')
    );

    let mut messages: Vec<Value> = Vec::new();
    messages.push(serde_json::json!({"role": "system", "content": system_prompt}));
    for msg in &request.history {
        messages.push(serde_json::json!({"role": msg.role, "content": msg.content}));
    }
    messages.push(serde_json::json!({"role": "user", "content": format!(
        "请根据以下反馈优化上面的结果：\n\n{}", request.feedback
    )}));

    let body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
        "temperature": 0.7,
        "max_tokens": 4096,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", request.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    stream_response(app, response).await
}