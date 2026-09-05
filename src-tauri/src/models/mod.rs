use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::errors::{AppError, ErrorKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    ModeA,
    ModeB,
    ModeC,
    ModeD,
}

impl Mode {
    pub fn to_str_name(&self) -> &'static str {
        match self {
            Mode::ModeA => "mode_a",
            Mode::ModeB => "mode_b",
            Mode::ModeC => "mode_c",
            Mode::ModeD => "mode_d",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub raw: String,
    pub finish_reason: Option<String>,
    /// F4：token 用量（网关不返回时为 None，不阻塞流程）
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

/// F4：单次 LLM 调用的 token 用量（只计数，不估算金额——各渠道单价不同）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    /// 宽容提取：缺字段/非数字一律按 0 处理（部分网关不返回 usage）
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let u = v.get("usage")?;
        let p = u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let c = u.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if p == 0 && c == 0 {
            None
        } else {
            Some(Self { prompt_tokens: p, completion_tokens: c })
        }
    }

    /// 流式 usage 块累加（OpenAI 流式 usage 在尾部独立块出现）
    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub content: String,
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn mode_to_str_name() {
        assert_eq!(Mode::ModeA.to_str_name(), "mode_a");
        assert_eq!(Mode::ModeB.to_str_name(), "mode_b");
        assert_eq!(Mode::ModeC.to_str_name(), "mode_c");
        assert_eq!(Mode::ModeD.to_str_name(), "mode_d");
    }

    #[test]
    fn pipeline_event_serializes_snake_case() {
        let e = PipelineEvent::StepStart { role: PipelineRole::Emotion };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"step_start\""));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["role"], "emotion");
    }

    #[test]
    fn host_events_carry_stage() {
        let start = PipelineEvent::HostStart { stage: HostStage::Summarize };
        let done = PipelineEvent::HostDone { stage: HostStage::Initial };
        let j1: Value = serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
        assert_eq!(j1["type"], "host_start");
        assert_eq!(j1["stage"], "summarize");
        let j2: Value = serde_json::from_str(&serde_json::to_string(&done).unwrap()).unwrap();
        assert_eq!(j2["type"], "host_done");
        assert_eq!(j2["stage"], "initial");
        // 反序列化回枚举（前后端契约一致）
        let back: PipelineEvent = serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
        match back {
            PipelineEvent::HostStart { stage } => assert_eq!(stage, HostStage::Summarize),
            _ => panic!("expected host_start"),
        }
    }

    /// B3：Cancelled 事件序列化为 {"type":"cancelled"}（前端 usePipeline 分支）
    #[test]
    fn cancelled_event_serializes() {
        let e = PipelineEvent::Cancelled;
        let j: Value = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(j["type"], "cancelled");
    }

    /// F4：TokenUsage 宽容提取——正常 / 缺字段 / 全零 / 非数字
    #[test]
    fn token_usage_extracts_tolerantly() {
        let ok = serde_json::json!({"usage": {"prompt_tokens": 120, "completion_tokens": 34}});
        let u = TokenUsage::from_json(&ok).unwrap();
        assert_eq!(u.prompt_tokens, 120);
        assert_eq!(u.completion_tokens, 34);
        // 缺 usage 字段 → None（不阻塞）
        assert!(TokenUsage::from_json(&serde_json::json!({})).is_none());
        // 全零 → None（无意义计数）
        assert!(TokenUsage::from_json(&serde_json::json!({"usage": {"prompt_tokens": 0, "completion_tokens": 0}})).is_none());
        // 缺单字段 → 按 0 处理，另一字段有效即 Some
        let half = serde_json::json!({"usage": {"prompt_tokens": 50}});
        let uh = TokenUsage::from_json(&half).unwrap();
        assert_eq!((uh.prompt_tokens, uh.completion_tokens), (50, 0));
    }

    /// F4：StepUsage 事件序列化形态（前端累计分支）
    #[test]
    fn step_usage_event_serializes() {
        let e = PipelineEvent::StepUsage { role: PipelineRole::Emotion, prompt_tokens: 100, completion_tokens: 20 };
        let j: Value = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(j["type"], "step_usage");
        assert_eq!(j["role"], "emotion");
        assert_eq!(j["prompt_tokens"], 100);
    }

    #[test]
    fn host_stage_roundtrip() {
        for s in [HostStage::Initial, HostStage::Summarize] {
            let json = serde_json::to_string(&s).unwrap();
            let back: HostStage = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn pipeline_request_parses_without_role_overrides() {
        // 旧版 JSON（无 role_overrides 字段）必须兼容解析
        let json = r#"{"mode":"mode_b","user_input":"雨天","model":"m1","api_key":"k1","base_url":"u1","extra":null}"#;
        let req: PipelineRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "m1");
        assert!(req.role_overrides.is_none());
        // 无 thinking 字段 → 默认关闭（旧前端兼容）
        assert!(!req.thinking);
    }

    #[test]
    fn pipeline_request_parses_thinking_flag() {
        // 新前端显式传 thinking: true 必须生效
        let json = r#"{"mode":"mode_b","user_input":"雨天","model":"m1","api_key":"k1","base_url":"u1","extra":null,"thinking":true}"#;
        let req: PipelineRequest = serde_json::from_str(json).unwrap();
        assert!(req.thinking);
        // 序列化回 JSON 保留字段（前后端契约一致）
        let back: PipelineRequest =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert!(back.thinking);
    }

    /// A11：GenerationConfig 缺省=现行值（silent 0.6/stream 0.7/max 30000），越界 Validation
    #[test]
    fn generation_config_defaults_and_validation() {
        let d = GenerationConfig::default();
        assert_eq!(d.silent_temperature(), 0.6);
        assert_eq!(d.stream_temperature(), 0.7);
        assert_eq!(d.max_tokens(), 30000);
        assert!(d.validate().is_ok());
        // 钳制：超 32000 收敛到 32000
        let big = GenerationConfig { temperature: None, max_tokens: Some(99999) };
        assert_eq!(big.max_tokens(), 32000);
        // 越界
        let bad_t = GenerationConfig { temperature: Some(2.5), max_tokens: None };
        assert_eq!(bad_t.validate().unwrap_err().kind, ErrorKind::Validation);
        let bad_m = GenerationConfig { temperature: None, max_tokens: Some(500) };
        assert_eq!(bad_m.validate().unwrap_err().kind, ErrorKind::Validation);
        // 边界通过
        let edge = GenerationConfig { temperature: Some(2.0), max_tokens: Some(1000) };
        assert!(edge.validate().is_ok());
        // generation 越界经 validate_request 透出
        let mut req = PipelineRequest {
            mode: Mode::ModeB,
            user_input: "雨天".into(),
            model: "m".into(),
            api_key: "k".into(),
            base_url: "https://api.example.com/v1".into(),
            extra: None,
            original_lyrics: None,
            role_overrides: None,
            thinking: false,
            refine_targets: None,
            generation: Some(GenerationConfig { temperature: Some(9.0), max_tokens: None }),
        };
        assert_eq!(validate_request(&req, None).unwrap_err().kind, ErrorKind::Validation);
        req.generation = None;
        assert!(validate_request(&req, None).is_ok());
    }

    /// F13：准入校验——空/超长/非法 URL/缺配置一律 Validation（构造请求用 struct 直写，无凭据字面量 JSON）
    #[test]
    fn validate_request_rejects_bad_input() {
        fn good() -> PipelineRequest {
            PipelineRequest {
                mode: Mode::ModeB,
                user_input: "雨天".into(),
                model: "m".into(),
                api_key: "k".into(),
                base_url: "https://api.example.com/v1".into(),
                extra: None,
                original_lyrics: None,
                role_overrides: None,
                thinking: false,
                refine_targets: None,
                generation: None,
            }
        }
        use crate::errors::ErrorKind;
        // 合法通过
        assert!(validate_request(&good(), None).is_ok());
        // 空输入
        let mut r = good();
        r.user_input = "   ".into();
        let e = validate_request(&r, None).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        // 超长输入
        r = good();
        r.user_input = "啊".repeat(20001);
        assert_eq!(validate_request(&r, None).unwrap_err().kind, ErrorKind::Validation);
        // 边界 20000 通过
        r = good();
        r.user_input = "啊".repeat(20000);
        assert!(validate_request(&r, None).is_ok());
        // 非法 base_url
        r = good();
        r.base_url = "ftp://x".into();
        assert_eq!(validate_request(&r, None).unwrap_err().kind, ErrorKind::Validation);
        // 空 model/key
        r = good();
        r.model = " ".into();
        assert_eq!(validate_request(&r, None).unwrap_err().kind, ErrorKind::Validation);
        // feedback 超长/空
        assert_eq!(validate_request(&good(), Some(&"啊".repeat(2001))).unwrap_err().kind, ErrorKind::Validation);
        assert_eq!(validate_request(&good(), Some("  ")).unwrap_err().kind, ErrorKind::Validation);
        // 原歌词超长
        r = good();
        r.original_lyrics = Some("啊".repeat(20001));
        assert_eq!(validate_request(&r, None).unwrap_err().kind, ErrorKind::Validation);
    }

    #[test]
    fn pipeline_request_parses_role_overrides() {
        let json = r#"{
            "mode":"mode_b","user_input":"x","model":"global","api_key":"gk","base_url":"gu",
            "extra":null,
            "role_overrides":{
                "auditor":{"model":"strong-model","api_key":"ak"},
                "emotion":{"base_url":"https://small.example.com/v1"}
            }
        }"#;
        let req: PipelineRequest = serde_json::from_str(json).unwrap();
        let map = req.role_overrides.unwrap();
        let auditor = map.get(&PipelineRole::Auditor).unwrap();
        assert_eq!(auditor.model.as_deref(), Some("strong-model"));
        assert_eq!(auditor.api_key.as_deref(), Some("ak"));
        assert_eq!(auditor.base_url, None); // 未覆盖字段为空
        let emotion = map.get(&PipelineRole::Emotion).unwrap();
        assert_eq!(emotion.base_url.as_deref(), Some("https://small.example.com/v1"));
        assert_eq!(emotion.model, None);
        assert!(map.get(&PipelineRole::Host).is_none());
    }

    #[test]
    fn role_api_override_roundtrip() {
        let ov = RoleApiOverride {
            model: Some("m".into()),
            api_key: None,
            base_url: None,
        };
        let json = serde_json::to_string(&ov).unwrap();
        let back: RoleApiOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model.as_deref(), Some("m"));
        assert_eq!(back.api_key, None);
        // 缺字段 JSON 也能解析（serde default）
        let sparse: RoleApiOverride = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
        assert_eq!(sparse.api_key, None);
        assert_eq!(sparse.base_url, None);
    }
}

// ---------------------------------------------------------------------------
// 流水线架构 v3 类型（7 角色：固定 2 + 动态 5）
// ---------------------------------------------------------------------------

/// 流水线角色（固定 2 + 动态 5）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRole {
    /// 👑 主持人：全局统领 + 汇总分发任务
    Host,
    /// 🔍 校验员：讨论轮审查提观点 + 最终格式输出端口
    Auditor,
    /// 🎭 情感分析师
    Emotion,
    /// 📝 作词人
    Lyricist,
    /// ✍️ 改词人
    Reviser,
    /// 🎤 制作人（编曲+配器合并）
    Producer,
    /// 🔥 流行风格分析师（抖音）
    StyleAnalyst,
}

impl PipelineRole {
    pub fn name(&self) -> &'static str {
        match self {
            PipelineRole::Host => "主持人",
            PipelineRole::Auditor => "校验员",
            PipelineRole::Emotion => "情感分析师",
            PipelineRole::Lyricist => "作词人",
            PipelineRole::Reviser => "改词人",
            PipelineRole::Producer => "制作人",
            PipelineRole::StyleAnalyst => "流行风格分析师",
        }
    }
}

/// 流水线步骤定义（动态角色，按序执行）
#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub role: PipelineRole,
}

/// 角色级 API 覆盖（可选，全字段可空；空字段 fallback 全局配置）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleApiOverride {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

/// 流水线请求（前端 → 后端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRequest {
    pub mode: Mode,
    pub user_input: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 额外上下文（Mode C 原歌词）。
    /// F12：deprecated——保留解析兼容一个版本（旧请求 extra 仍生效），新请求走 original_lyrics。
    pub extra: Option<String>,
    /// F12：Mode C 原歌词独立字段（替代 extra 的字符串拼接协议）。
    /// 旧前端无此字段 → None（serde default），后端回退读 extra。
    #[serde(default)]
    pub original_lyrics: Option<String>,
    /// 角色级 API 覆盖：某角色配了就用配的，没配的字段 fallback 全局
    #[serde(default)]
    pub role_overrides: Option<HashMap<PipelineRole, RoleApiOverride>>,
    /// 思考模式：开启后按模型能力路由表注入厂商思考参数（旧前端无此字段 → 默认关闭）
    #[serde(default)]
    pub thinking: bool,
    /// F1：增量优化目标角色（None = 全量，旧行为；Some(空) 也视为全量，防前端误传）。
    /// 旧前端无此字段 → None（serde default）。
    #[serde(default)]
    pub refine_targets: Option<Vec<PipelineRole>>,
    /// A11：生成参数覆盖（缺省走内置默认；旧前端无此字段 → None）。
    #[serde(default)]
    pub generation: Option<GenerationConfig>,
}

/// A11：生成参数（全字段可选，缺省=现行硬编码值，零行为变化）。
/// temperature 默认：静默 0.6 / 流式 0.7（调用方区分）；max_tokens 默认 30000（MAX_TOKENS_CAP）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationConfig {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl GenerationConfig {
    /// 静默调用 temperature（缺省 0.6）
    pub fn silent_temperature(&self) -> f32 {
        self.temperature.unwrap_or(0.6)
    }
    /// 流式调用 temperature（缺省 0.7）
    pub fn stream_temperature(&self) -> f32 {
        self.temperature.unwrap_or(0.7)
    }
    /// max_tokens（缺省 30000，上限钳制 32000）
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(30000).min(32000)
    }
    /// F13 扩展：范围校验（temperature 0~2，max_tokens 1000~32000）
    pub fn validate(&self) -> Result<(), AppError> {
        if let Some(t) = self.temperature {
            if !(0.0..=2.0).contains(&t) {
                return Err(AppError::new(
                    ErrorKind::Validation,
                    format!("temperature 越界（{}，允许 0~2）", t),
                ));
            }
        }
        if let Some(m) = self.max_tokens {
            if !(1000..=32000).contains(&m) {
                return Err(AppError::new(
                    ErrorKind::Validation,
                    format!("max_tokens 越界（{}，允许 1000~32000）", m),
                ));
            }
        }
        Ok(())
    }
}

impl PipelineRequest {
    /// F12：取 Mode C 原歌词统一入口——新字段优先，旧 extra 回退（兼容旧前端/旧请求）。
    pub fn original_lyrics_text(&self) -> Option<&str> {
        self.original_lyrics
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.extra.as_deref().filter(|s| !s.trim().is_empty()))
    }
}

/// F13：请求准入校验（后端兜底——前端 InputPanel 保留快速反馈，后端为准入闸门）。
/// 限额：user_input ≤20000 字符、原歌词 ≤20000、feedback ≤2000；
/// base_url 必须 http(s)；model/api_key 去空白后非空。
/// 失败返回 Validation kind（A5 预留正式启用），前端 errText 原样展示。
pub fn validate_request(req: &PipelineRequest, feedback: Option<&str>) -> Result<(), AppError> {
    /// 字符数超限报错
    fn too_long(field: &str, len: usize, max: usize) -> AppError {
        AppError::new(
            ErrorKind::Validation,
            format!("{}过长（{} 字符，上限 {}），请精简后重试", field, len, max),
        )
    }
    const MAX_INPUT: usize = 20000;
    const MAX_FEEDBACK: usize = 2000;
    let input_len = req.user_input.chars().count();
    if req.user_input.trim().is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "输入为空，请输入内容后重试"));
    }
    if input_len > MAX_INPUT {
        return Err(too_long("输入", input_len, MAX_INPUT));
    }
    if let Some(lyrics) = req.original_lyrics_text() {
        let n = lyrics.chars().count();
        if n > MAX_INPUT {
            return Err(too_long("原歌词", n, MAX_INPUT));
        }
    }
    if let Some(fb) = feedback {
        let n = fb.chars().count();
        if n > MAX_FEEDBACK {
            return Err(too_long("优化反馈", n, MAX_FEEDBACK));
        }
        if fb.trim().is_empty() {
            return Err(AppError::new(ErrorKind::Validation, "优化反馈为空"));
        }
    }
    if req.model.trim().is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "模型未配置，请在设置中填写"));
    }
    if req.api_key.trim().is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "API Key 未配置，请在设置中填写"));
    }
    let url = req.base_url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::new(
            ErrorKind::Validation,
            "API 地址非法（必须 http(s) 开头），请在设置中检查",
        ));
    }
    // A11：生成参数范围校验（缺省跳过）
    if let Some(g) = &req.generation {
        g.validate()?;
    }
    Ok(())
}

/// 主持人阶段（阶段 0 统领初稿 / 阶段 1 汇总分发），供前端区分文案
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStage {
    /// 阶段 0：用模式完整指令产出方案初稿
    Initial,
    /// 阶段 1：汇总角色修订 + 校验员观点，输出新版方案与任务分发
    Summarize,
}

/// 流水线事件（后端 → 前端，复用 'pipeline' 通道）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// 某步骤开始
    StepStart { role: PipelineRole },
    /// 某步骤完成（带摘要）
    StepDone { role: PipelineRole, summary: String },
    /// 主持人阶段开始（stage 区分统领/汇总）
    HostStart { stage: HostStage },
    /// 主持人阶段完成
    HostDone { stage: HostStage },
    /// 校验开始
    AuditStart,
    /// 校验结果（通过/不通过+问题）
    AuditResult { pass: bool, findings: Vec<String> },
    /// 打回某步骤重跑
    Retry { role: PipelineRole, reason: String },
    /// 讨论轮：校验发现问题，主持人把修订任务分发到角色（下一轮讨论）
    DiscussionRound { round: u32, roles: Vec<PipelineRole>, reason: String },
    /// 整体失败
    Failed { error: String },
    /// B3：用户取消（前端"停止"按钮）——与 Failed 区别：不标红，只回到空闲
    Cancelled,
    /// F4：单次调用的 token 用量（前端累计展示，不阻塞流程）
    StepUsage { role: PipelineRole, prompt_tokens: u32, completion_tokens: u32 },
}
