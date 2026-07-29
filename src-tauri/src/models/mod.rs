use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    ModeA,
    ModeC,
    ModeD,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub mode: Mode,
    pub user_input: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub content: String,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefineRequest {
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub mode: Mode,
    pub history: Vec<ChatMessage>,
    pub feedback: String,
}