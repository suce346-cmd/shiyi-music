//! 结构化错误：kind 供程序判断/前端差异化提示，message 为用户可读文本。
//! 兼容策略：历史 String 错误经 From 自动归为 Internal，函数体无需逐处改写；
//! 仅关键构造点（网络/鉴权/限流/超时/取消/解析）显式分类。

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Network,
    Auth,
    RateLimit,
    Timeout,
    Cancelled,
    Parse,
    Validation,
    Internal,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    /// 用户取消
    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "生成已取消")
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self { kind: ErrorKind::Internal, message }
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        Self { kind: ErrorKind::Internal, message: message.to_string() }
    }
}

impl AppError {
    /// API HTTP 状态分类：401/403=Auth，429=RateLimit，5xx=Network（瞬时故障可重试含义见 llm.rs），其余=Network
    pub fn api_status(status: u16, body: &str) -> Self {
        let kind = match status {
            401 | 403 => ErrorKind::Auth,
            429 => ErrorKind::RateLimit,
            _ => ErrorKind::Network,
        };
        Self::new(kind, format!("API returned {}: {}", status, body))
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for AppError {}
