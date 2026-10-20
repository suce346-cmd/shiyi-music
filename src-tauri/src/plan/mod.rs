//! SongPlan IR（BLUEPRINT_v2 §2.1，P1-1）——唯一定义点。
//!
//! 流程：Host → `PlanIr`（schema `plan_ir.v1`）→ 讨论修订 → Auditor → `SongPlan`（schema `song_plan.v1`）
//! → `validate`（P1-4）→ `render_suno`（P1-4）。
//! 参照 ElevenLabs composition plan 的“全局/段落 × 正向/负向”四象限，只渲染到 Suno。
//!
//! 字段约束（国产模型结构化输出稳定性，P1-1 风险项）：
//! - 枚举化（`SectionKind`、`VocalGender`、`LyricLang`），不让模型自由写类别；
//! - `LyricLang` 为歌词语言**唯一定义点**（§3.8 IPC `output_lang` 将 `pub use` 本类型，禁止另立 `LyricsLang`）；
//! - 无嵌套可选对象以外的多态；全部字段在 schema 中 required（Option → 可为 null），
//!   `additionalProperties:false`——与 OpenAI `json_schema strict` 兼容（schema.rs 统一后处理）。

pub mod render_suno;
pub mod schema;
pub mod suno_profile;
pub mod validate;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 当前 SongPlan / PlanIR 结构版本
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SongPlan {
    /// 固定为 1
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub title: Option<String>,
    pub global: GlobalStyle,
    pub sections: Vec<Section>,
    pub settings: SunoSettings,
    pub mode_meta: ModeMeta,
    /// 创作说明（不渲染给 Suno）
    pub notes: Option<String>,
}

/// Style 六层的全局部分（渲染为 Suno Style 栏，恒英文——§3.7）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobalStyle {
    /// 1 genre / era
    pub genre: Vec<String>,
    #[schemars(range(min = 40, max = 220))]
    pub bpm: Option<u16>,
    pub key: Option<String>,
    pub meter: Option<String>,
    /// 2 vocal（引用 vocal_mix.csv id，P1-5）
    pub vocal: Vec<String>,
    /// 3 instrumentation & performance
    pub instruments: Vec<String>,
    /// 4 arrangement / energy arc
    pub arc: Option<String>,
    /// 5 production & mix（引用 vocal_mix.csv id，P1-5）
    pub mix: Vec<String>,
    /// → Suno Exclude 栏
    pub negative: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Hook,
    PostChorus,
    Bridge,
    Interlude,
    Drop,
    Breakdown,
    Outro,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub kind: SectionKind,
    /// 显示标签，如 "Verse 1"
    pub label: String,
    /// 段内正向 cue → `[Label | cue]` 或 `(cue)`（渲染语法由 P2 探针定，render_suno 单点切换）
    pub positive_local: Vec<String>,
    /// 段内负向 cue（Suno 无段落级 Exclude：渲染为正向改写或并入全局 Exclude，规则见 render_suno）
    pub negative_local: Vec<String>,
    /// 1–10
    #[schemars(range(min = 1, max = 10))]
    pub energy: Option<u8>,
    /// 段落在场乐器（只放乐器；环境声/人声描述不入此字段——B-02 结论 1，配器门只数 len()）
    pub instruments: Vec<String>,
    /// 极简段显式声明（B-02 结论 2：仅 intro/outro/bridge/interlude 且 energy ≤2 可声明，P1-4 校验）
    pub minimal: bool,
    /// 仅 D 模式必填（时间预算校验）
    #[schemars(range(min = 1, max = 600))]
    pub target_seconds: Option<u16>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VocalGender {
    Male,
    Female,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SunoSettings {
    /// 0–100
    #[schemars(range(max = 100))]
    pub weirdness: u8,
    /// 0–100
    #[schemars(range(max = 100))]
    pub style_influence: u8,
    /// 0–100；默认 0（suno_profile::VARIETY_DEFAULT）
    #[schemars(range(max = 100))]
    pub variety: u8,
    pub vocal_gender: Option<VocalGender>,
    /// 0–100；仅上传音频时有意义
    #[schemars(range(max = 100))]
    pub audio_influence: Option<u8>,
}

/// 歌词与 notes 语言（D13：zh/en 两值；Style 恒英文与此无关）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LyricLang {
    Zh,
    En,
}

/// 各模式专属字段（§2.3）。D 模式字段（场景/片段/Hook）随 P1-7 加入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeMeta {
    pub lyrics_lang: LyricLang,
}

/// Host 阶段产物（schema `plan_ir.v1`）：与 SongPlan 同形——讨论轮对它做结构化 patch（P1-3），
/// Auditor 定稿为 SongPlan。同形避免两套类型间的转换层与漂移；区别只在阶段与校验强度
/// （PlanIR 不跑 Suno 预算门，SongPlan 必须全门通过）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PlanIr(pub SongPlan);

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// 最小合法 SongPlan（测试共用）
    pub(crate) fn minimal_plan() -> SongPlan {
        SongPlan {
            schema_version: SCHEMA_VERSION,
            title: Some("雨巷".into()),
            global: GlobalStyle {
                genre: vec!["chinese folk ballad".into()],
                bpm: Some(72),
                key: None,
                meter: Some("4/4".into()),
                vocal: vec!["vocal_breathy_close".into()],
                instruments: vec!["guzheng".into(), "cello".into()],
                arc: Some("sparse intro to full final chorus".into()),
                mix: vec!["mix_warm_intimate".into()],
                negative: vec!["edm drops".into()],
            },
            sections: vec![
                Section {
                    kind: SectionKind::Intro,
                    label: "Intro".into(),
                    positive_local: vec!["rain ambience".into()],
                    negative_local: vec![],
                    energy: Some(1),
                    instruments: vec!["guzheng".into()],
                    minimal: true,
                    target_seconds: None,
                    lines: vec![],
                },
                Section {
                    kind: SectionKind::Chorus,
                    label: "Chorus".into(),
                    positive_local: vec!["full, soaring".into()],
                    negative_local: vec!["no drums".into()],
                    energy: Some(8),
                    instruments: vec!["guzheng".into(), "cello".into(), "dizi".into(), "pipa".into()],
                    minimal: false,
                    target_seconds: None,
                    lines: vec!["雨落长街".into(), "灯火未眠".into()],
                },
            ],
            settings: SunoSettings {
                weirdness: 30,
                style_influence: 70,
                variety: 0,
                vocal_gender: Some(VocalGender::Female),
                audio_influence: None,
            },
            mode_meta: ModeMeta { lyrics_lang: LyricLang::Zh },
            notes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::minimal_plan;
    use super::*;

    #[test]
    fn plan_round_trips_json() {
        let p = minimal_plan();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["sections"][0]["kind"], "intro");
        assert_eq!(v["sections"][1]["kind"], "chorus");
        assert_eq!(v["mode_meta"]["lyrics_lang"], "zh");
        let back: SongPlan = serde_json::from_value(v).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn plan_ir_is_transparent() {
        let p = minimal_plan();
        let ir = PlanIr(p.clone());
        assert_eq!(serde_json::to_value(&ir).unwrap(), serde_json::to_value(&p).unwrap());
    }

    #[test]
    fn unknown_fields_rejected() {
        let mut v = serde_json::to_value(minimal_plan()).unwrap();
        v["global"]["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SongPlan>(v).is_err());
    }

    #[test]
    fn section_kind_wire_names() {
        let names: Vec<String> = [
            SectionKind::PreChorus,
            SectionKind::PostChorus,
            SectionKind::Breakdown,
        ]
        .iter()
        .map(|k| serde_json::to_value(k).unwrap().as_str().unwrap().to_string())
        .collect();
        assert_eq!(names, ["pre_chorus", "post_chorus", "breakdown"]);
    }
}
