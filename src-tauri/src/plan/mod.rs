//! SongPlan IR（v3 架构契约）——唯一定义点。
//!
//! 流程：直通创作（Stage S0/S1）→ `PlanIr`（schema `plan_ir.v1`）→ 专业审视与定稿（Stage S2）
//! → `SongPlan`（schema `song_plan.v1`）→ `validate`（物理硬门 + 去机器味诊断）→ `render_suno`（诚实编译器）。
//! 参照全局/段落 × 正向/负向四象限与能力契约，只将 Suno 真实可响应的声学指令渲染到四栏交付包。
//!
//! 字段约束（国产模型结构化输出稳定性）：
//! - 枚举化（`SectionKind`、`VocalGender`、`LyricLang`、`PlotType`、`SectionMotion`），不让模型自由写类别；
//! - `LyricLang` 为歌词语言**唯一定义点**（§3.8 IPC `output_lang` 将 `pub use` 本类型，禁止另立 `LyricsLang`）；
//! - 无嵌套可选对象以外的多态；全部字段在 schema 中 required（Option → 可为 null），
//!   `additionalProperties:false`——与 OpenAI `json_schema strict` 兼容（schema.rs 统一后处理）。

pub mod douyin;
pub mod render_suno;
pub mod schema;
pub mod style_lang;
pub mod suno_profile;
pub mod validate;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 当前 SongPlan / PlanIR 结构版本
pub const SCHEMA_VERSION: u32 = 1;

/// 歌曲展开模型（默认优先态度型或情境型，破除“每首歌都必须从头讲一个起承转合长故事”的套路）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlotType {
    /// 态度型：围绕一种立场、脾气或情绪切片展开（默认首选）
    Attitudinal,
    /// 情境型：围绕一个具体瞬间、空间现场展开（默认首选）
    Situational,
    /// 叙事型：有时间推进或因果转折的故事
    Narrative,
    /// 白描枚举型：用平行意象拼贴立意
    Expository,
}

/// 创作立意五问锚点（动笔前确立：展开模型、一句话口语真话、对谁说、在什么时刻、核心锚点物）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SongIntent {
    pub plot_type: PlotType,
    /// 一句话口语真话（禁修辞、禁押韵，整首歌只说透这一件事）
    pub one_thing: String,
    /// 对谁说（具体关系与姿态，禁写“所有人/听众”）
    pub to_whom: String,
    /// 在什么时刻说（具体时间、空间光影或身体状态）
    pub at_what_moment: String,
    /// 核心锚点物（可拍照的具体物件，全曲围绕它展开）
    pub anchor_object: String,
    /// 全局声律与格律统筹蓝图（全曲主韵部、同部位邻韵微跳位置与主副歌句式字数网格）
    #[serde(default)]
    pub rhyme_plan: Option<String>,
}

/// 段落动静职能（Push=推进失衡段，Stop=落点稳定段）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionMotion {
    /// 推进段：制造失衡、悬念与张力（如 Verse / Pre-Chorus / Bridge）
    Push,
    /// 落点段：对称、稳定、情绪释放或收束（如 Chorus / Hook / Outro）
    Stop,
}

/// 刻意不对称与留白设计（打破 AI 四平八稳的对称模具）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AsymmetryPlan {
    /// 破格短句或进副歌前的留白呼吸句
    #[serde(default)]
    pub short_line: Option<String>,
    /// 故意不押韵的转折刺点句
    #[serde(default)]
    pub deliberate_unrhymed: Option<String>,
    /// 末遍副歌相对于首遍副歌的热点词/人声微变说明
    #[serde(default)]
    pub chorus_variation: Option<String>,
    /// 终局留白：核心痛点如何悬置而不强行圆满升华
    #[serde(default)]
    pub withholding: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SongPlan {
    /// 固定为 1
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub title: Option<String>,
    /// 创作立意五问锚点（不直接渲染给 Suno，驱动词曲决策）
    #[serde(default)]
    pub intent: Option<SongIntent>,
    pub global: GlobalStyle,
    pub sections: Vec<Section>,
    pub settings: SunoSettings,
    pub mode_meta: ModeMeta,
    /// 刻意不对称与留白设计（不直接渲染给 Suno，驱动去机器味设计）
    #[serde(default)]
    pub asymmetry: Option<AsymmetryPlan>,
    /// 显式创作豁免理由（如“民谣清唱全曲仅吉他”“故意用空泛词表现失语”），用于软诊断豁免
    #[serde(default)]
    pub exemptions: Vec<String>,
    /// 创作说明（精简决策备忘，不渲染给 Suno）
    pub notes: Option<String>,
    /// 发音/多音字/同音替代提示（纯文本；默认 null=关闭）
    #[serde(default)]
    pub pronunciation_hints: Option<String>,
    /// 可复用 Persona/声音指纹描述（纯文本；默认 null=关闭）
    #[serde(default)]
    pub persona: Option<String>,
    /// 专辑/系列一致性上下文（纯文本；默认 null=关闭）
    #[serde(default)]
    pub series_context: Option<String>,
}

/// Style 全局部分（渲染为 Suno Style 栏，恒英文——§3.7）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobalStyle {
    /// 0 置顶情绪与声学质地（Suno 权重最高位，如 "intimate melancholic, late-night warmth"）
    #[serde(default)]
    pub mood: Option<String>,
    /// 1 genre / era
    pub genre: Vec<String>,
    #[schemars(range(min = 40, max = 220))]
    pub bpm: Option<u16>,
    pub key: Option<String>,
    pub meter: Option<String>,
    /// 2 vocal：支持 vocal_mix 词表 id（如 V-02，渲染自动展开）或具象英文人声描述短语
    pub vocal: Vec<String>,
    /// 3 instrumentation & performance
    pub instruments: Vec<String>,
    /// 贯穿全曲的器乐记忆点/动机钩子（如 "recurring 4-note muted guitar ostinato"）
    #[serde(default)]
    pub arrangement_hook: Option<String>,
    /// 4 arrangement / energy arc
    pub arc: Option<String>,
    /// 5 production & mix：支持 vocal_mix 词表 id（如 M-01，渲染自动展开）或具象英文混音描述短语
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
    /// 段落动静职能（push=推进失衡，stop=落点稳定）
    #[serde(default)]
    pub motion: Option<SectionMotion>,
    /// 段内正向 cue（局部变化量 Section Delta：奏法变化、人声姿态、空间切换）
    pub positive_local: Vec<String>,
    /// 段内负向 cue（Suno 无段落级 Exclude：渲染为正向改写如 X drop out）
    pub negative_local: Vec<String>,
    /// 本段减法/抽空事件（如 "drums and bass drop out on first 2 bars"）
    #[serde(default)]
    pub subtraction: Option<String>,
    /// 1–10
    #[schemars(range(min = 1, max = 10))]
    pub energy: Option<u8>,
    /// 段落在场乐器（只放乐器名称）
    pub instruments: Vec<String>,
    /// 极简段显式声明（intro/outro/bridge/interlude/verse 等剥离段）
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
    /// 0–100；仅参考音频模式有意义（默认 null/0；`reference_audio=true` 时允许非 0）
    #[schemars(range(max = 100))]
    pub audio_influence: Option<u8>,
    /// 是否启用参考音频模式（默认 false → Audio Influence 须 0/null）
    #[serde(default)]
    pub reference_audio: bool,
}

/// 歌词与 notes 语言（D13：zh/en 两值；Style 恒英文与此无关）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LyricLang {
    Zh,
    En,
}

/// 各模式专属字段（§2.3）。D 模式字段见 `douyin`（P1-7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeMeta {
    pub lyrics_lang: LyricLang,
    /// D 模式必填；其他模式显式 null（P1-7；schema required + null，与 notes 同口径）
    #[serde(default)]
    pub douyin: Option<douyin::DouyinModeMeta>,
}

/// 初稿阶段产物（schema `plan_ir.v1`）：与 SongPlan 同形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PlanIr(pub SongPlan);

pub mod fixtures {
    use super::*;

    /// 最小合法 SongPlan（测试共用）
    pub fn minimal_plan() -> SongPlan {
        SongPlan {
            schema_version: SCHEMA_VERSION,
            title: Some("雨巷".into()),
            intent: None,
            global: GlobalStyle {
                mood: None,
                genre: vec!["chinese folk ballad".into()],
                bpm: Some(72),
                key: None,
                meter: Some("4/4".into()),
                vocal: vec!["V-02".into()],
                instruments: vec!["guzheng".into(), "cello".into()],
                arrangement_hook: None,
                arc: Some("sparse intro to full final chorus".into()),
                mix: vec!["M-01".into()],
                negative: vec!["edm drops".into()],
            },
            sections: vec![
                Section {
                    kind: SectionKind::Intro,
                    label: "Intro".into(),
                    motion: None,
                    positive_local: vec!["rain ambience".into()],
                    negative_local: vec![],
                    subtraction: None,
                    energy: Some(1),
                    instruments: vec!["guzheng".into()],
                    minimal: true,
                    target_seconds: None,
                    lines: vec![],
                },
                Section {
                    kind: SectionKind::Chorus,
                    label: "Chorus".into(),
                    motion: None,
                    positive_local: vec!["full, soaring".into()],
                    negative_local: vec!["no drums".into()],
                    subtraction: None,
                    energy: Some(8),
                    instruments: vec![
                        "guzheng".into(),
                        "cello".into(),
                        "dizi".into(),
                        "pipa".into(),
                    ],
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
                reference_audio: false,
            },
            mode_meta: ModeMeta {
                lyrics_lang: LyricLang::Zh,
                douyin: None,
            },
            asymmetry: None,
            exemptions: vec![],
            notes: None,
            pronunciation_hints: None,
            persona: None,
            series_context: None,
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
        assert_eq!(
            serde_json::to_value(&ir).unwrap(),
            serde_json::to_value(&p).unwrap()
        );
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
        .map(|k| {
            serde_json::to_value(k)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(names, ["pre_chorus", "post_chorus", "breakdown"]);
    }

    #[test]
    fn v3_creative_fields_round_trip() {
        let mut p = minimal_plan();
        p.intent = Some(SongIntent {
            plot_type: PlotType::Situational,
            one_thing: "加班到凌晨看到便利店关灯才发觉你真走了".into(),
            to_whom: "刚搬走的合租恋人".into(),
            at_what_moment: "凌晨两点便利店冰柜嗡鸣时".into(),
            anchor_object: "半盒过期的关东煮".into(),
            rhyme_plan: Some("全曲统归言前辙(an/ian)，仅Bridge微跳同部位前鼻音人辰辙(en/in)后回归".into()),
        });
        p.global.mood = Some("intimate melancholic, late-night urban solitude".into());
        p.global.arrangement_hook = Some("recurring muted electric guitar 4-note ostinato".into());
        p.sections[0].motion = Some(SectionMotion::Push);
        p.sections[1].motion = Some(SectionMotion::Stop);
        p.sections[1].subtraction = Some("drums drop out on first bar".into());
        p.asymmetry = Some(AsymmetryPlan {
            short_line: Some("灯灭了".into()),
            deliberate_unrhymed: Some("收银台不再找零".into()),
            chorus_variation: Some("末遍副歌把'未眠'改为'已远'，叠加八度和声".into()),
            withholding: Some("始终没发出那条短信".into()),
        });
        p.exemptions = vec!["故意保留单句短句破格".into()];
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["intent"]["plot_type"], "situational");
        assert_eq!(v["sections"][0]["motion"], "push");
        assert_eq!(v["sections"][1]["motion"], "stop");
        let back: SongPlan = serde_json::from_value(v).unwrap();
        assert_eq!(back, p);
    }
}
