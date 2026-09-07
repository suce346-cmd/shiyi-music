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
    /// token 用量（网关不返回时为 None，不阻塞流程）
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

/// 单次 LLM 调用的 token 用量（只计数，不估算金额——各渠道单价不同）
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

    /// envelope 序列化形态 {run_id, event:{type,...}}（前端拆包过滤）
    #[test]
    fn envelope_serializes_with_run_id() {
        let e = PipelineEnvelope::new("r1", PipelineEvent::StepStart { role: PipelineRole::Host });
        let j: Value = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(j["run_id"], "r1");
        assert_eq!(j["event"]["type"], "step_start");
        assert_eq!(j["event"]["role"], "host");
    }

    /// Cancelled 事件序列化为 {"type":"cancelled"}（前端 usePipeline 分支）
    #[test]
    fn cancelled_event_serializes() {
        let e = PipelineEvent::Cancelled;
        let j: Value = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(j["type"], "cancelled");
    }

    /// TokenUsage 宽容提取——正常 / 缺字段 / 全零 / 非数字
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

    /// StepUsage 事件序列化形态（前端累计分支）
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

    /// GenerationConfig 缺省=现行值（silent 0.6/stream 0.7/max 30000），越界 Validation
    #[test]
    fn generation_config_defaults_and_validation() {
        let d = GenerationConfig::default();
        assert_eq!(d.silent_temperature(), 0.6);
        assert_eq!(d.stream_temperature(), 0.7);
        assert_eq!(d.max_tokens(), 30000);
        assert!(d.validate().is_ok());
        // 钳制：超 30000 收敛到 30000（Q5：与 MAX_TOKENS_CAP 对齐）
        let big = GenerationConfig { temperature: None, max_tokens: Some(99999) };
        assert_eq!(big.max_tokens(), 30000);
        // 32000 已越界（Q5：上限 30000）
        let over = GenerationConfig { temperature: None, max_tokens: Some(32000) };
        assert_eq!(over.validate().unwrap_err().kind, ErrorKind::Validation);
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
            base_url: "https://8.8.8.8/v1".into(),
            extra: None,
            original_lyrics: None,
            role_overrides: None,
            thinking: false,
            refine_targets: None,
            generation: Some(GenerationConfig { temperature: Some(9.0), max_tokens: None }),
            run_id: None,
        };
        assert_eq!(validate_request(&req, None).unwrap_err().kind, ErrorKind::Validation);
        req.generation = None;
        assert!(validate_request(&req, None).is_ok());
    }

    /// 旧请求无 run_id 字段 → None（后端生成）；新字段透传
    #[test]
    fn request_run_id_defaults_none() {
        let req: PipelineRequest = serde_json::from_str(
            r#"{"mode":"mode_b","user_input":"x","model":"m","api_key":"k","base_url":"u","extra":null}"#,
        )
        .unwrap();
        assert!(req.run_id.is_none());
    }

    /// 准入校验——空/超长/非法 URL/缺配置一律 Validation（构造请求用 struct 直写，无凭据字面量 JSON）
    #[test]
    fn validate_request_rejects_bad_input() {
        fn good() -> PipelineRequest {
            PipelineRequest {
                mode: Mode::ModeB,
                user_input: "雨天".into(),
                model: "m".into(),
                api_key: "k".into(),
                base_url: "https://8.8.8.8/v1".into(),
                extra: None,
                original_lyrics: None,
                role_overrides: None,
                thinking: false,
                refine_targets: None,
                generation: None,
                run_id: None,
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

    /// S-1：validate_url 内网/环回/保留/变形 URL 全拒绝（IP 字面量直测，不依赖 DNS）
    #[test]
    fn validate_url_rejects_internal_and_malformed() {
        let bad = [
            "ftp://x.com/v1",                  // 非 http(s) 协议
            "http://localhost/v1",             // localhost 字符串
            "http://sub.localhost/v1",         // localhost 子域
            "http://127.0.0.1:8080/v1",        // 环回
            "http://0.0.0.0/v1",               // 未指定/本网络
            "http://10.1.2.3/v1",              // 10/8 私有
            "http://192.168.1.1/v1",           // 192.168/16 私有
            "http://172.16.0.9/v1",            // 172.16/12 私有
            "http://169.254.169.254/latest",   // 链路本地（云元数据端点）
            "http://100.64.0.1/v1",            // CGNAT 共享段
            "http://192.0.2.1/v1",             // 文档段
            "http://198.18.0.1/v1",            // 基准测试段
            "http://255.255.255.255/v1",       // 广播
            "http://[::1]/v1",                 // IPv6 环回
            "http://[fe80::1]/v1",             // IPv6 链路本地
            "http://[fd00::1]/v1",             // IPv6 唯一本地
            "http://[::ffff:127.0.0.1]/v1",    // IPv4-mapped 壳套环回
            "https://user@evil.com/v1",        // userinfo 变形
            "https://user:pass@evil.com/v1",   // userinfo 带密码
            "not a url at all",                // 无法解析
            "",                                // 空串
        ];
        for u in bad {
            let err = validate_url(u).unwrap_err();
            assert!(!err.is_empty(), "用例 {} 应给出拒绝原因", u);
        }
    }

    /// S-1：公网 IP 直连通过（用 IP 字面量避免单测依赖 DNS；真实域名由无头实网测试覆盖）
    #[test]
    fn validate_url_allows_public() {
        for u in ["https://8.8.8.8/v1", "http://1.1.1.1/", "https://93.184.216.34/v2"] {
            assert!(validate_url(u).is_ok(), "{} 应通过", u);
        }
    }

    /// S-1：role_overrides.base_url 旁路封死——内网地址/空串一律 Validation，合法覆盖正常通过
    #[test]
    fn validate_request_rejects_role_override_internal_url() {
        fn good() -> PipelineRequest {
            PipelineRequest {
                mode: Mode::ModeB,
                user_input: "雨天".into(),
                model: "m".into(),
                api_key: "k".into(),
                base_url: "https://8.8.8.8/v1".into(),
                extra: None,
                original_lyrics: None,
                role_overrides: None,
                thinking: false,
                refine_targets: None,
                generation: None,
                run_id: None,
            }
        }
        use crate::errors::ErrorKind;
        use std::collections::HashMap;
        // 合法覆盖通过
        let mut r = good();
        let mut map = HashMap::new();
        map.insert(PipelineRole::Emotion, RoleApiOverride { model: None, api_key: None, base_url: Some("https://1.1.1.1/v1".into()) });
        r.role_overrides = Some(map);
        assert!(validate_request(&r, None).is_ok());
        // 覆盖指向内网 → 拒绝且报出角色名
        let mut map = HashMap::new();
        map.insert(PipelineRole::Emotion, RoleApiOverride { model: None, api_key: None, base_url: Some("http://127.0.0.1:9000/v1".into()) });
        r.role_overrides = Some(map);
        let e = validate_request(&r, None).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(e.message.contains("情感分析师"), "报错应带角色名：{}", e.message);
        // 覆盖为空串 → 拒绝
        let mut map = HashMap::new();
        map.insert(PipelineRole::Lyricist, RoleApiOverride { model: None, api_key: None, base_url: Some("   ".into()) });
        r.role_overrides = Some(map);
        assert_eq!(validate_request(&r, None).unwrap_err().kind, ErrorKind::Validation);
        // 覆盖非 http(s) → 拒绝
        let mut map = HashMap::new();
        map.insert(PipelineRole::Host, RoleApiOverride { model: None, api_key: None, base_url: Some("ftp://x.com".into()) });
        r.role_overrides = Some(map);
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
    /// deprecated——保留解析兼容一个版本（旧请求 extra 仍生效），新请求走 original_lyrics。
    pub extra: Option<String>,
    /// Mode C 原歌词独立字段（替代 extra 的字符串拼接协议）。
    /// 旧前端无此字段 → None（serde default），后端回退读 extra。
    #[serde(default)]
    pub original_lyrics: Option<String>,
    /// 角色级 API 覆盖：某角色配了就用配的，没配的字段 fallback 全局
    #[serde(default)]
    pub role_overrides: Option<HashMap<PipelineRole, RoleApiOverride>>,
    /// 思考模式：开启后按模型能力路由表注入厂商思考参数（旧前端无此字段 → 默认关闭）
    #[serde(default)]
    pub thinking: bool,
    /// 增量优化目标角色（None = 全量，旧行为；Some(空) 也视为全量，防前端误传）。
    /// 旧前端无此字段 → None（serde default）。
    #[serde(default)]
    pub refine_targets: Option<Vec<PipelineRole>>,
    /// 生成参数覆盖（缺省走内置默认；旧前端无此字段 → None）。
    #[serde(default)]
    pub generation: Option<GenerationConfig>,
    /// 任务归属 id（前端生成传入；缺省后端在 with_timeout 内生成）。
    #[serde(default)]
    pub run_id: Option<String>,
}

/// 生成参数（全字段可选，缺省=现行硬编码值，零行为变化）。
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
    /// max_tokens（缺省 30000，上限钳制 30000，与 MAX_TOKENS_CAP 对齐；Q5：此前 32000 可发出，与统一上限打架）
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(30000).min(30000)
    }
    /// 扩展：范围校验（temperature 0~2，max_tokens 1000~30000）
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
            if !(1000..=30000).contains(&m) {
                return Err(AppError::new(
                    ErrorKind::Validation,
                    format!("max_tokens 越界（{}，允许 1000~30000）", m),
                ));
            }
        }
        Ok(())
    }
}

impl PipelineRequest {
    /// 取 Mode C 原歌词统一入口——新字段优先，旧 extra 回退（兼容旧前端/旧请求）。
    pub fn original_lyrics_text(&self) -> Option<&str> {
        self.original_lyrics
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.extra.as_deref().filter(|s| !s.trim().is_empty()))
    }
}

/// URL 准入校验（S-1 SSRF 防线，纯函数供三入口复用：validate_request / test_api / 无头 test_config）。
/// 四层：真实解析 → 仅 http/https → 拒绝 userinfo（user[:pass]@host 变形）→
/// host 解析出全部 IP 逐一拒绝环回/内网/链路本地/组播/未指定/广播/CGNAT/文档段/唯一本地。
/// 域名走阻塞 DNS（桌面端一次性校验可接受）；解析失败视为不可验证，按拒绝处理（fail closed）。
pub fn validate_url(raw: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("URL 无法解析（{}）", e))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("URL 协议仅允许 http/https（当前 {}）", other)),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL 不允许携带用户名/密码（@ 形态）".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL 缺少主机名".to_string())?
        .to_string();
    if host.eq_ignore_ascii_case("localhost") || host.to_lowercase().ends_with(".localhost") {
        return Err("URL 主机不允许指向 localhost".to_string());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let ips: Vec<std::net::IpAddr> = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => {
            use std::net::ToSocketAddrs;
            (host.as_str(), port)
                .to_socket_addrs()
                .map_err(|e| format!("URL 主机解析失败，无法验证安全性（{}）", e))?
                .map(|s| s.ip())
                .collect()
        }
    };
    for ip in ips {
        if is_forbidden_ip(ip) {
            return Err(format!("URL 解析到内网/环回/保留地址（{}），已拒绝", ip));
        }
    }
    Ok(())
}

/// 内网/保留 IP 判定（纯函数可测；手写区间避免 std 未稳定的 is_global/is_unique_local）
fn is_forbidden_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, c, d] = v4.octets();
            v4.is_unspecified()             // 0.0.0.0
                || v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()           // 10/8、172.16/12、192.168/16
                || v4.is_link_local()        // 169.254/16（含云元数据 169.254.169.254）
                || v4.is_multicast()         // 224/4
                || a == 0                    // 0.0.0.0/8 本网络
                || (a == 100 && (64..=127).contains(&b)) // 100.64/10 CGNAT 共享段
                || (a == 192 && b == 0 && c == 2)        // 192.0.2/24 文档段
                || (a == 198 && b == 51 && c == 100)     // 198.51.100/24 文档段
                || (a == 203 && b == 0 && c == 113)      // 203.0.113/24 文档段
                || (a == 198 && (18..=19).contains(&b))  // 198.18/15 基准测试段
                || (a == 255 && b == 255 && c == 255 && d == 255) // 广播
        }
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped（::ffff:a.b.c.d）拆出内层按 IPv4 判定，防六代壳套四代内网
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_forbidden_ip(std::net::IpAddr::V4(v4));
            }
            let seg = v6.segments();
            v6.is_loopback()                    // ::1
                || v6.is_unspecified()          // ::
                || (seg[0] & 0xfe00) == 0xfc00  // fc00::/7 唯一本地
                || (seg[0] & 0xffc0) == 0xfe80  // fe80::/10 链路本地
                || (seg[0] & 0xff00) == 0xff00  // ff00::/8 组播
        }
    }
}

/// 请求准入校验（后端兜底——前端 InputPanel 保留快速反馈，后端为准入闸门）。
/// 限额：user_input ≤20000 字符、原歌词 ≤20000、feedback ≤2000；
/// base_url 双层校验（http(s) 前缀 + validate_url host 解析）且覆盖全部 role_overrides.base_url；
/// model/api_key 去空白后非空。
/// 失败返回 Validation kind（预留正式启用），前端 errText 原样展示。
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
    // S-1 新规则：host 级校验第二道（真实解析，拒绝环回/内网/保留地址），全局入口
    validate_url(url).map_err(|m| {
        AppError::new(ErrorKind::Validation, format!("API 地址不合规：{}，请在设置中检查", m))
    })?;
    // S-1：role_overrides.base_url 是独立第二入口，与全局同闸（旧规则完全不校验，属 SSRF 旁路）
    for (role, ov) in req.role_overrides.iter().flatten() {
        if let Some(ov_url) = ov.base_url.as_deref() {
            let ov_trim = ov_url.trim();
            if ov_trim.is_empty() {
                return Err(AppError::new(
                    ErrorKind::Validation,
                    format!("{} 角色的 API 地址为空，请填写完整或清空该角色覆盖", role.name()),
                ));
            }
            if !(ov_trim.starts_with("http://") || ov_trim.starts_with("https://")) {
                return Err(AppError::new(
                    ErrorKind::Validation,
                    format!("{} 角色的 API 地址非法（必须 http(s) 开头），请在设置中检查", role.name()),
                ));
            }
            validate_url(ov_trim).map_err(|m| {
                AppError::new(
                    ErrorKind::Validation,
                    format!("{} 角色的 API 地址不合规：{}，请在设置中检查", role.name(), m),
                )
            })?;
        }
    }
    // 生成参数范围校验（缺省跳过）
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
/// 统一包 envelope 传输（PipelineEnvelope { run_id, event }），事件本体无 run_id 字段。
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
    /// 用户取消（前端"停止"按钮）——与 Failed 区别：不标红，只回到空闲
    Cancelled,
    /// 单次调用的 token 用量（前端累计展示，不阻塞流程）
    StepUsage { role: PipelineRole, prompt_tokens: u32, completion_tokens: u32 },
}

/// 事件信封——run_id 归属 + 事件本体（前端按 run_id 过滤，替代旧纯 token 补丁的后端原生支持）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineEnvelope {
    pub run_id: String,
    pub event: PipelineEvent,
}

impl PipelineEnvelope {
    pub fn new(run_id: impl Into<String>, event: PipelineEvent) -> Self {
        Self { run_id: run_id.into(), event }
    }
}
