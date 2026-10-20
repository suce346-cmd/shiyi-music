//! P1-2 结构化输出能力表（BLUEPRINT_v2 §2.2）。
//!
//! 三档：`json_schema`（网关按 schema 约束）/ `json_object`（只保证是 JSON，schema 放 system）/
//! `none`（两者都被拒 → 结构化阶段直接报错提示换模型，**不自由文本兜底**）。
//! 任何档位的返回都必须过本地 `jsonschema` 二次校验（国产网关常忽略 `strict`）。
//!
//! 解析顺序：① 角色显式选择 → ② 探测缓存（按 `cap_key(base_url, model)`，前端存 settings）→
//! ③ 都没有 → `json_object`（兼容面最大；本地校验兜住，失败按 Parse 报错，不静默降级）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredMode {
    JsonSchema,
    JsonObject,
    None,
}

impl StructuredMode {
    pub const ALL: [StructuredMode; 3] = [Self::JsonSchema, Self::JsonObject, Self::None];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::JsonSchema => "json_schema",
            Self::JsonObject => "json_object",
            Self::None => "none",
        }
    }
}

/// 未显式选择、也无探测缓存时的档位。
pub const DEFAULT_MODE: StructuredMode = StructuredMode::JsonObject;

/// 缓存键：`(base_url 去尾斜杠, model)`。前端 `capKey` 与此同形（跨语言锁见 vitest）。
pub fn cap_key(base_url: &str, model: &str) -> String {
    format!("{}|{}", base_url.trim().trim_end_matches('/'), model.trim())
}

pub fn resolve_mode(
    explicit: Option<StructuredMode>,
    caps: Option<&HashMap<String, StructuredMode>>,
    base_url: &str,
    model: &str,
) -> StructuredMode {
    explicit
        .or_else(|| caps.and_then(|c| c.get(&cap_key(base_url, model)).copied()))
        .unwrap_or(DEFAULT_MODE)
}

/// 请求体里的 `response_format`；`none` 档不发请求，故返回 None。
pub fn response_format(mode: StructuredMode, name: &str, schema: &Value) -> Option<Value> {
    match mode {
        StructuredMode::JsonSchema => Some(json!({
            "type": "json_schema",
            "json_schema": { "name": name, "strict": true, "schema": schema }
        })),
        StructuredMode::JsonObject => Some(json!({ "type": "json_object" })),
        StructuredMode::None => None,
    }
}

/// json_object 档：schema 文本进 system（OpenAI 兼容网关要求 messages 内出现 "JSON" 字样）。
pub fn schema_system_message(schema: &Value) -> Value {
    let text = format!(
        "Output exactly one JSON object that validates against this JSON Schema. \
         No markdown fences, no extra text.\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    );
    json!({ "role": "system", "content": text })
}

/// 探测用最小 schema。
pub fn probe_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"],
        "additionalProperties": false
    })
}

pub fn probe_body(model: &str, mode: StructuredMode) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "Reply with JSON {\"ok\": true}." }],
        "stream": false,
        "max_tokens": 16,
    });
    if let Some(rf) = response_format(mode, "probe", &probe_schema()) {
        body["response_format"] = rf;
    }
    body
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    Supported,
    /// 网关明确拒绝该参数 → 试下一档
    Rejected,
    /// 与档位无关的失败（鉴权/限流/5xx）→ 不记能力，原样报错
    Fatal,
}

pub fn classify_probe_status(status: u16) -> ProbeOutcome {
    match status {
        200..=299 => ProbeOutcome::Supported,
        400 | 404 | 415 | 422 => ProbeOutcome::Rejected,
        _ => ProbeOutcome::Fatal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_match_serde() {
        for m in StructuredMode::ALL {
            assert_eq!(serde_json::to_value(m).unwrap(), Value::String(m.as_str().into()));
        }
    }

    #[test]
    fn resolve_order_explicit_then_cache_then_default() {
        let mut caps = HashMap::new();
        caps.insert(cap_key("https://x/v1/", "m"), StructuredMode::JsonSchema);
        assert_eq!(resolve_mode(Some(StructuredMode::None), Some(&caps), "https://x/v1", "m"), StructuredMode::None);
        assert_eq!(resolve_mode(None, Some(&caps), "https://x/v1", " m "), StructuredMode::JsonSchema);
        assert_eq!(resolve_mode(None, Some(&caps), "https://x/v1", "other"), DEFAULT_MODE);
        assert_eq!(resolve_mode(None, None, "u", "m"), DEFAULT_MODE);
        // 与前端 capKey 同一组向量（跨语言锁见 src/test/structuredCaps.test.ts）
        assert_eq!(cap_key(" https://x/v1// ", " m "), "https://x/v1|m");
    }

    #[test]
    fn response_format_shapes() {
        let s = probe_schema();
        let rf = response_format(StructuredMode::JsonSchema, "n", &s).unwrap();
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(rf["json_schema"]["schema"], s);
        assert_eq!(response_format(StructuredMode::JsonObject, "n", &s).unwrap(), json!({"type":"json_object"}));
        assert!(response_format(StructuredMode::None, "n", &s).is_none());
        assert!(schema_system_message(&s)["content"].as_str().unwrap().contains("JSON Schema"));
    }

    #[test]
    fn probe_status_classes() {
        assert_eq!(classify_probe_status(200), ProbeOutcome::Supported);
        assert_eq!(classify_probe_status(400), ProbeOutcome::Rejected);
        assert_eq!(classify_probe_status(422), ProbeOutcome::Rejected);
        for s in [401, 403, 429, 500, 502] {
            assert_eq!(classify_probe_status(s), ProbeOutcome::Fatal, "{s}");
        }
        assert!(probe_body("m", StructuredMode::None).get("response_format").is_none());
    }
}
