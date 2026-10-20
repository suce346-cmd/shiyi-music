//! D 模式（抖音）场景与 V2 规则（BLUEPRINT_v2 §6，P1-7）。
//!
//! 场景枚举为本文件单源；前端经 `get_douyin_meta` 读取，禁止在 TS 手抄 BPM 区间。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Section, SectionKind, SongPlan};
use crate::plan::validate::Issue;

// ---------------------------------------------------------------------------
// DOUYIN_V2_* 常量（rules 命名空间约定；本模块为定义点，rules 可 re-export）
// ---------------------------------------------------------------------------

/// Intro 上限秒（冷开例外：无 Intro 或 ≤ 本值后紧接 Hook）
pub const DOUYIN_V2_INTRO_MAX_SEC: u16 = 3;
/// Hook 落地时间上限（秒）
pub const DOUYIN_V2_HOOK_LAND_SEC: u16 = 10;
/// 核心 Hook 句数区间
pub const DOUYIN_V2_HOOK_LINES_MIN: usize = 2;
pub const DOUYIN_V2_HOOK_LINES_MAX: usize = 3;
/// 核心 Hook 合计字数上限（去空白）
pub const DOUYIN_V2_HOOK_CHARS_MAX: usize = 20;
/// Hook 全曲最少重复次数（按规范化正文计）
pub const DOUYIN_V2_HOOK_REPEAT_MIN: usize = 2;
/// 主歌上限行数（可省）
pub const DOUYIN_V2_VERSE_MAX_LINES: usize = 4;
/// 每行字数上限
pub const DOUYIN_V2_LINE_MAX_CHARS: usize = 12;
/// 指定可剪片段时长（秒）
pub const DOUYIN_V2_CLIP_SECONDS: u16 = 15;
/// Exclude 必含项（小写比对）
pub const DOUYIN_V2_EXCLUDE: &[&str] = &["long intro", "slow build", "fade out"];
/// 抖音 BPM 下限 = 全场景 BPM 带最低值（emotion 80；由 `bpm_min_is_scene_floor` 锁定）
pub const DOUYIN_V2_BPM_MIN: u32 = 80;
/// 成曲建议时长（§6.2：用户裁定 60–90 秒）——`${DOUYIN_V2_DURATION_RANGE}`
pub const DOUYIN_V2_DURATION_SEC_MIN: u32 = 60;
pub const DOUYIN_V2_DURATION_SEC_MAX: u32 = 90;
/// Hook 循环遍数上限（下限 = `DOUYIN_V2_HOOK_REPEAT_MIN`）——`${DOUYIN_V2_HOOK_LOOP_RANGE}`
pub const DOUYIN_V2_HOOK_LOOP_MAX: usize = 4;
/// 情绪弧线禁用句式（人设与 mode_d prompt 同源）
pub const DOUYIN_V2_ARC_FORBID_PHRASE: &str = "from A to B";
/// 动态走向能量窗口：全程高位 / 高开骤停（软指引，非硬门）
pub const DOUYIN_V2_ENERGY_HIGH_MIN: u32 = 7;
pub const DOUYIN_V2_ENERGY_HIGH_MAX: u32 = 9;
pub const DOUYIN_V2_ENERGY_ABRUPT_MIN: u32 = 8;
pub const DOUYIN_V2_ENERGY_ABRUPT_MAX: u32 = 10;
/// 参数区间（provisional，待 P2 探针；叙事型例外：≥2 叙事段且写明'叙事型'方可回落 A/B 区间）
pub const DOUYIN_V2_WEIRD_MIN: u32 = 12;
pub const DOUYIN_V2_WEIRD_MAX: u32 = 20;
pub const DOUYIN_V2_STYLE_MIN: u32 = 85;
pub const DOUYIN_V2_STYLE_MAX: u32 = 95;
/// 逐段动态参考表（设计方向示例，非阈值；表内 `${DOUYIN_V2_INTRO_MAX}` 为嵌套占位符）
pub const DOUYIN_V2_DYNAMIC_TABLE: &str = "· Intro/前${DOUYIN_V2_INTRO_MAX}秒：全程高位=8，3-4件，直接拉满；高开骤停=8，3-4件，直接拉满；先压后炸=3-4，1-2件，制造反差\n· Hook：全程高位=9，3-4件，持续高压；高开骤停=9，3-4件，持续高压；先压后炸=9-10，5件，突然爆发\n· Verse：全程高位=8，3件，不冷却；高开骤停=8，3件，不冷却；先压后炸=7，3件，保持热度\n· Hook重复：全程高位=9，4件，加层；高开骤停=9，4件，加层；先压后炸=10，5-6件，最炸\n· Bridge/反差：全程高位=7，2件，稍剥离；高开骤停=—；先压后炸=8，3件，再次推\n· 最后Hook：全程高位=10，4件，最炸；高开骤停=10，4件，骤停前一拍最炸；先压后炸=10，5-6件，炸完即停";
/// 混音词表：手机扬声器友好（P1-7 与 §6.2）
pub const DOUYIN_V2_MIX_PHONE_SPEAKER: &str = "M-06";
/// 混音词表：人声贴唱（vocal-forward）
pub const DOUYIN_V2_MIX_VOCAL_FORWARD: &str = "M-01";

/// 抖音场景（单源；BPM 区间来源 §6.1 Produsso E3）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DouyinScene {
    /// 情绪治愈 80–100
    Emotion,
    /// 日常 Vlog 100–120（默认）
    Daily,
    /// 舞蹈卡点 120–140
    Dance,
    /// 剧情反转 90–110
    Story,
    /// 国风 90–130
    Guofeng,
}

impl DouyinScene {
    pub const ALL: [DouyinScene; 5] = [
        DouyinScene::Emotion,
        DouyinScene::Daily,
        DouyinScene::Dance,
        DouyinScene::Story,
        DouyinScene::Guofeng,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DouyinScene::Emotion => "emotion",
            DouyinScene::Daily => "daily",
            DouyinScene::Dance => "dance",
            DouyinScene::Story => "story",
            DouyinScene::Guofeng => "guofeng",
        }
    }

    pub fn parse(s: &str) -> Option<DouyinScene> {
        match s.trim().to_ascii_lowercase().as_str() {
            "emotion" => Some(DouyinScene::Emotion),
            "daily" => Some(DouyinScene::Daily),
            "dance" => Some(DouyinScene::Dance),
            "story" => Some(DouyinScene::Story),
            "guofeng" => Some(DouyinScene::Guofeng),
            _ => None,
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            DouyinScene::Emotion => "情绪治愈",
            DouyinScene::Daily => "日常 Vlog",
            DouyinScene::Dance => "舞蹈卡点",
            DouyinScene::Story => "剧情反转",
            DouyinScene::Guofeng => "国风",
        }
    }

    /// BPM 闭区间 [min, max]
    pub fn bpm_band(self) -> (u16, u16) {
        match self {
            DouyinScene::Emotion => (80, 100),
            DouyinScene::Daily => (100, 120),
            DouyinScene::Dance => (120, 140),
            DouyinScene::Story => (90, 110),
            DouyinScene::Guofeng => (90, 130),
        }
    }

    pub fn default_scene() -> DouyinScene {
        DouyinScene::Daily
    }
}

/// 场景来源（后端写入，不让模型选）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneSource {
    User,
    Default,
}

/// 可剪 15 秒片段声明
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DouyinClip {
    pub start_section: String,
    pub end_section: String,
    #[schemars(range(min = 15, max = 15))]
    pub seconds: u16,
}

/// SongPlan.mode_meta.douyin（D 模式必填；其他模式 null）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DouyinModeMeta {
    pub scene: DouyinScene,
    pub scene_source: SceneSource,
    pub clip: DouyinClip,
    /// 核心 Hook 正文（2–3 句合并前的主记忆点，可与某段 lines 重合）
    pub hook_line: String,
    /// true = 结尾回到 Hook 首字；false = 硬停 [End]
    pub loop_tail: bool,
}

/// 前端元数据（BPM 区间 + 标签，禁止 TS 手抄）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DouyinSceneInfo {
    pub id: String,
    pub label_zh: String,
    pub bpm_min: u16,
    pub bpm_max: u16,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DouyinMeta {
    pub scenes: Vec<DouyinSceneInfo>,
    pub default_scene: String,
    pub intro_max_sec: u16,
    pub hook_land_sec: u16,
    pub hook_chars_max: usize,
    pub clip_seconds: u16,
    pub exclude_required: Vec<String>,
    pub mix_phone_speaker: String,
    pub mix_vocal_forward: String,
}

#[tauri::command]
pub fn get_douyin_meta() -> DouyinMeta {
    DouyinMeta {
        scenes: DouyinScene::ALL
            .iter()
            .map(|s| {
                let (lo, hi) = s.bpm_band();
                DouyinSceneInfo {
                    id: s.as_str().into(),
                    label_zh: s.label_zh().into(),
                    bpm_min: lo,
                    bpm_max: hi,
                    is_default: *s == DouyinScene::default_scene(),
                }
            })
            .collect(),
        default_scene: DouyinScene::default_scene().as_str().into(),
        intro_max_sec: DOUYIN_V2_INTRO_MAX_SEC,
        hook_land_sec: DOUYIN_V2_HOOK_LAND_SEC,
        hook_chars_max: DOUYIN_V2_HOOK_CHARS_MAX,
        clip_seconds: DOUYIN_V2_CLIP_SECONDS,
        exclude_required: DOUYIN_V2_EXCLUDE.iter().map(|s| (*s).into()).collect(),
        mix_phone_speaker: DOUYIN_V2_MIX_PHONE_SPEAKER.into(),
        mix_vocal_forward: DOUYIN_V2_MIX_VOCAL_FORWARD.into(),
    }
}

/// 推荐结果（纯函数，不调 LLM）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSuggestion {
    pub scene: String,
    pub score: i32,
    pub reason: String,
}

#[tauri::command]
pub fn suggest_douyin_scenes(user_input: String) -> Vec<SceneSuggestion> {
    suggest_scenes(&user_input)
        .into_iter()
        .map(|(s, score, reason)| SceneSuggestion {
            scene: s.as_str().into(),
            score,
            reason,
        })
        .collect()
}

/// 关键词表（嵌入；与 knowledge 组切换无关）
fn keyword_table() -> Vec<(DouyinScene, &'static str, i32)> {
    include_str!("data/douyin_scene_keywords.csv")
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut p = l.splitn(3, ',');
            let scene = DouyinScene::parse(p.next()?)?;
            let kw = p.next()?;
            let w: i32 = p.next()?.parse().ok()?;
            Some((scene, kw, w))
        })
        .collect()
}

/// 纯函数：返回得分前 3；无命中 → 仅默认 daily
pub fn suggest_scenes(user_input: &str) -> Vec<(DouyinScene, i32, String)> {
    let text = user_input.to_lowercase();
    let mut scores: std::collections::HashMap<DouyinScene, (i32, Vec<String>)> =
        std::collections::HashMap::new();
    for (scene, kw, w) in keyword_table() {
        if text.contains(&kw.to_lowercase()) {
            let e = scores.entry(scene).or_default();
            e.0 += w;
            e.1.push(kw.into());
        }
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .map(|(s, (score, kws))| (s, score, format!("命中 {}", kws.join("/"))))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    ranked.truncate(3);
    if ranked.is_empty() {
        vec![(
            DouyinScene::default_scene(),
            0,
            "无关键词命中，使用默认日常 Vlog".into(),
        )]
    } else {
        ranked
    }
}

/// 解析请求侧场景：Some(合法) → User；None/非法 → Default(daily)
pub fn resolve_request_scene(raw: Option<&str>) -> (DouyinScene, SceneSource) {
    match raw.and_then(DouyinScene::parse) {
        Some(s) => (s, SceneSource::User),
        None => (DouyinScene::default_scene(), SceneSource::Default),
    }
}

/// 后端盖写：把请求场景写入 SongPlan（不信任模型选的 scene）
pub fn apply_scene_to_plan(plan: &mut SongPlan, scene: DouyinScene, source: SceneSource) {
    let clip = DouyinClip {
        start_section: "Hook".into(),
        end_section: "Hook".into(),
        seconds: DOUYIN_V2_CLIP_SECONDS,
    };
    let existing = plan.mode_meta.douyin.take();
    let (hook_line, loop_tail, clip) = match existing {
        Some(d) => (d.hook_line, d.loop_tail, d.clip),
        None => (String::new(), true, clip),
    };
    plan.mode_meta.douyin = Some(DouyinModeMeta {
        scene,
        scene_source: source,
        clip,
        hook_line,
        loop_tail,
    });
}

fn norm_line(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_hook(s: &Section) -> bool {
    s.kind == SectionKind::Hook || s.label.to_lowercase().contains("hook")
}

fn is_chorus_or_hook(s: &Section) -> bool {
    matches!(s.kind, SectionKind::Hook | SectionKind::Chorus) || is_hook(s)
}

/// §6.2 / `validate_douyin`（P1-7）
pub fn validate_douyin_v2(p: &SongPlan, out: &mut Vec<Issue>) {
    let Some(meta) = p.mode_meta.douyin.as_ref() else {
        out.push(Issue::hard(
            "douyin_meta_missing",
            "/mode_meta/douyin",
            "D 模式须填写 mode_meta.douyin（scene/clip/hook_line/loop_tail）",
        ));
        return;
    };

    // BPM ∈ scene.bpm_band
    let (lo, hi) = meta.scene.bpm_band();
    match p.global.bpm {
        Some(b) if (lo..=hi).contains(&b) => {}
        Some(b) => out.push(Issue::hard(
            "douyin_bpm_band",
            "/global/bpm",
            format!(
                "BPM {b} 不在场景 {}（{}）区间 {lo}–{hi}",
                meta.scene.as_str(),
                meta.scene.label_zh()
            ),
        )),
        None => out.push(Issue::hard(
            "douyin_bpm_missing",
            "/global/bpm",
            format!("D 模式须填 BPM（场景 {} 区间 {lo}–{hi}）", meta.scene.as_str()),
        )),
    }

    // 结构：首段 Chorus/Hook 或 Intro≤DOUYIN_V2_INTRO_MAX_SEC 紧接 Hook
    if let Some(first) = p.sections.first() {
        if is_chorus_or_hook(first) {
            // 冷开 OK
        } else if first.kind == SectionKind::Intro {
            let sec = first.target_seconds.unwrap_or(u16::MAX);
            if sec > DOUYIN_V2_INTRO_MAX_SEC {
                out.push(Issue::hard(
                    "douyin_intro_max",
                    "/sections/0/target_seconds",
                    format!("Intro {sec}s > 上限 {DOUYIN_V2_INTRO_MAX_SEC}s（或改为冷开 Hook）"),
                ));
            }
            let second_ok = p.sections.get(1).is_some_and(is_chorus_or_hook);
            if !second_ok {
                out.push(Issue::hard(
                    "douyin_intro_hook",
                    "/sections/1",
                    "Intro 后须紧接 Hook/Chorus",
                ));
            }
        } else {
            out.push(Issue::hard(
                "douyin_cold_open",
                "/sections/0",
                "首段须为 Hook/Chorus（冷开）或短 Intro 紧接 Hook",
            ));
        }
    } else {
        out.push(Issue::hard("douyin_empty", "/sections", "D 模式至少一段"));
    }

    // Hook 落地 ≤DOUYIN_V2_HOOK_LAND_SEC：累加首段至首个 Hook 的 target_seconds
    if let Some(hi) = p.sections.iter().position(is_chorus_or_hook) {
        let land: u16 = p.sections[..hi]
            .iter()
            .map(|s| s.target_seconds.unwrap_or(0))
            .sum();
        if land > DOUYIN_V2_HOOK_LAND_SEC {
            out.push(Issue::hard(
                "douyin_hook_land",
                "/sections",
                format!("Hook 落地 {land}s > 上限 {DOUYIN_V2_HOOK_LAND_SEC}s"),
            ));
        }
    }

    // 核心 Hook：hook_line 2–3 句、≤20 字；全曲重复 ≥DOUYIN_V2_HOOK_REPEAT_MIN
    let hook_lines: Vec<&str> = meta
        .hook_line
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !(DOUYIN_V2_HOOK_LINES_MIN..=DOUYIN_V2_HOOK_LINES_MAX).contains(&hook_lines.len()) {
        out.push(Issue::hard(
            "douyin_hook_lines",
            "/mode_meta/douyin/hook_line",
            format!(
                "核心 Hook 须 {DOUYIN_V2_HOOK_LINES_MIN}–{DOUYIN_V2_HOOK_LINES_MAX} 句，当前 {}",
                hook_lines.len()
            ),
        ));
    }
    let hook_chars: usize = hook_lines.iter().map(|l| norm_line(l).chars().count()).sum();
    if hook_chars > DOUYIN_V2_HOOK_CHARS_MAX {
        out.push(Issue::hard(
            "douyin_hook_chars",
            "/mode_meta/douyin/hook_line",
            format!("核心 Hook 合计 {hook_chars} 字 > 上限 {DOUYIN_V2_HOOK_CHARS_MAX}"),
        ));
    }
    if let Some(primary) = hook_lines.first().map(|l| norm_line(l)) {
        if !primary.is_empty() {
            let reps = p
                .sections
                .iter()
                .flat_map(|s| s.lines.iter())
                .filter(|l| norm_line(l) == primary)
                .count();
            if reps < DOUYIN_V2_HOOK_REPEAT_MIN {
                out.push(Issue::hard(
                    "douyin_hook_repeat",
                    "/sections",
                    format!("Hook 主句全曲重复 {reps} < 下限 {DOUYIN_V2_HOOK_REPEAT_MIN}"),
                ));
            }
        }
    }

    // Verse ≤4 行；每行 ≤12 字
    for (i, s) in p.sections.iter().enumerate() {
        let n = s.lines.iter().filter(|l| !l.trim().is_empty()).count();
        if s.kind == SectionKind::Verse && n > DOUYIN_V2_VERSE_MAX_LINES {
            out.push(Issue::hard(
                "verse_lines",
                format!("/sections/{i}/lines"),
                format!("「{}」{n} 行 > 上限 {DOUYIN_V2_VERSE_MAX_LINES}", s.label),
            ));
        }
        for (j, line) in s.lines.iter().enumerate() {
            let c = norm_line(line).chars().count();
            if c > DOUYIN_V2_LINE_MAX_CHARS {
                out.push(Issue::hard(
                    "douyin_line_chars",
                    format!("/sections/{i}/lines/{j}"),
                    format!("「{}」第 {} 行 {c} 字 > 上限 {DOUYIN_V2_LINE_MAX_CHARS}", s.label, j + 1),
                ));
            }
        }
    }

    // clip 必填且 seconds=15；起止标签须存在
    if meta.clip.seconds != DOUYIN_V2_CLIP_SECONDS {
        out.push(Issue::hard(
            "douyin_clip_seconds",
            "/mode_meta/douyin/clip/seconds",
            format!("clip.seconds 须为 {DOUYIN_V2_CLIP_SECONDS}"),
        ));
    }
    let labels: std::collections::BTreeSet<_> = p.sections.iter().map(|s| s.label.as_str()).collect();
    for (path, lab) in [
        ("/mode_meta/douyin/clip/start_section", meta.clip.start_section.as_str()),
        ("/mode_meta/douyin/clip/end_section", meta.clip.end_section.as_str()),
    ] {
        if !labels.contains(lab) && !p.sections.iter().any(|s| s.label.eq_ignore_ascii_case(lab)) {
            out.push(Issue::hard(
                "douyin_clip_section",
                path,
                format!("clip 引用的段落「{lab}」不在 sections[].label 中"),
            ));
        }
    }

    // 结尾：loop_tail 或末段 End / 骤停
    let last = p.sections.last();
    let hard_end = last.is_some_and(|s| {
        s.kind == SectionKind::End
            || s.positive_local.iter().chain(s.negative_local.iter()).any(|c| {
                let l = c.to_lowercase();
                l.contains("骤停")
                    || l.contains("abrupt")
                    || l.contains("cut off")
                    || l.contains("all instruments cut")
                    || l.contains("hard stop")
            })
    });
    if !meta.loop_tail && !hard_end {
        out.push(Issue::hard(
            "douyin_ending",
            "/mode_meta/douyin/loop_tail",
            "须 loop_tail=true（回 Hook）或末段硬停/[End]",
        ));
    }

    // Exclude 必含
    let neg: Vec<String> = p.global.negative.iter().map(|n| n.to_lowercase()).collect();
    for req in DOUYIN_V2_EXCLUDE {
        if !neg.iter().any(|n| n.contains(req)) {
            out.push(Issue::hard(
                "douyin_exclude",
                "/global/negative",
                format!("Exclude 须含「{req}」"),
            ));
        }
    }

    // 混音：vocal-forward + phone-speaker
    for id in [DOUYIN_V2_MIX_VOCAL_FORWARD, DOUYIN_V2_MIX_PHONE_SPEAKER] {
        if !p.global.mix.iter().any(|m| m.trim() == id) {
            out.push(Issue::hard(
                "douyin_mix",
                "/global/mix",
                format!("D 模式 mix 须含词表 id {id}"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bpm_min_is_scene_floor() {
        let floor = super::DouyinScene::ALL.iter().map(|s| s.bpm_band().0 as u32).min().unwrap();
        assert_eq!(super::DOUYIN_V2_BPM_MIN, floor);
    }

    use super::*;
    use crate::plan::fixtures::minimal_plan;

    fn d_plan() -> SongPlan {
        let mut p = minimal_plan();
        p.global.bpm = Some(110);
        p.global.negative = vec![
            "long intro".into(),
            "slow build".into(),
            "fade out".into(),
        ];
        p.global.mix = vec!["M-01".into(), "M-06".into()];
        p.sections = vec![
            Section {
                kind: SectionKind::Hook,
                label: "Hook".into(),
                positive_local: vec!["punchy".into()],
                negative_local: vec![],
                energy: Some(8),
                instruments: vec!["808".into(), "synth".into()],
                minimal: false,
                target_seconds: Some(8),
                lines: vec!["干就完了".into(), "干就完了".into(), "今晚不睡".into()],
            },
            Section {
                kind: SectionKind::Verse,
                label: "Verse".into(),
                positive_local: vec!["thin".into()],
                negative_local: vec![],
                energy: Some(4),
                instruments: vec!["pad".into()],
                minimal: false,
                target_seconds: Some(8),
                lines: vec!["灯还亮着".into(), "窗外风大".into()],
            },
            Section {
                kind: SectionKind::Hook,
                label: "Hook 2".into(),
                positive_local: vec!["punchy".into()],
                negative_local: vec![],
                energy: Some(9),
                instruments: vec!["808".into(), "synth".into()],
                minimal: false,
                target_seconds: Some(8),
                lines: vec!["干就完了".into(), "干就完了".into(), "今晚不睡".into()],
            },
            Section {
                kind: SectionKind::Hook,
                label: "Hook 3".into(),
                positive_local: vec!["abrupt stop".into()],
                negative_local: vec![],
                energy: Some(9),
                instruments: vec!["808".into()],
                minimal: false,
                target_seconds: Some(8),
                lines: vec!["干就完了".into()],
            },
        ];
        p.mode_meta.douyin = Some(DouyinModeMeta {
            scene: DouyinScene::Daily,
            scene_source: SceneSource::Default,
            clip: DouyinClip {
                start_section: "Hook".into(),
                end_section: "Hook".into(),
                seconds: 15,
            },
            hook_line: "干就完了\n今晚不睡".into(),
            loop_tail: true,
        });
        p
    }

    #[test]
    fn douyin_scene_default_is_daily() {
        assert_eq!(DouyinScene::default_scene(), DouyinScene::Daily);
        assert_eq!(DouyinScene::Daily.bpm_band(), (100, 120));
    }

    #[test]
    fn suggest_scenes_keywords() {
        let r = suggest_scenes("今晚跳舞卡点热舞");
        assert_eq!(r[0].0, DouyinScene::Dance, "{r:?}");
        let r = suggest_scenes("完全无关的输入 xyz");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, DouyinScene::Daily);
    }

    #[test]
    fn user_decision_values_locked() {
        // 用户裁定 2026-10-20：成曲 60–90s、Intro≤3s、Hook 10s 内落地、Hook≥2、BPM 下限 80
        assert_eq!((DOUYIN_V2_DURATION_SEC_MIN, DOUYIN_V2_DURATION_SEC_MAX), (60, 90));
        assert_eq!(DOUYIN_V2_INTRO_MAX_SEC, 3);
        assert_eq!(DOUYIN_V2_HOOK_LAND_SEC, 10);
        assert_eq!(DOUYIN_V2_HOOK_REPEAT_MIN, 2);
        assert_eq!(DOUYIN_V2_BPM_MIN, 80);
        assert_eq!(crate::rules::HOOK_MIN_COUNT, 2);
        assert_eq!((crate::rules::HOOK_REPEAT_MIN, crate::rules::HOOK_REPEAT_MAX), (2, 4));
    }

    #[test]
    fn intro_and_hook_land_boundaries() {
        let codes = |intro: u16| {
            let mut p = d_plan();
            let mut s = p.sections[0].clone();
            s.kind = SectionKind::Intro;
            s.label = "Intro".into();
            s.lines.clear();
            s.target_seconds = Some(intro);
            p.sections.insert(0, s);
            let mut v = Vec::new();
            validate_douyin_v2(&p, &mut v);
            v.iter().map(|i| i.code).collect::<Vec<_>>()
        };
        assert!(!codes(3).contains(&"douyin_intro_max"), "Intro 3s 应过");
        assert!(codes(4).contains(&"douyin_intro_max"), "Intro 4s 应拦");
        assert!(!codes(10).contains(&"douyin_hook_land"), "Hook 10s 落地应过");
        assert!(codes(11).contains(&"douyin_hook_land"), "Hook 11s 落地应拦");
    }

    #[test]
    fn douyin_v2_happy_path_and_negatives() {
        let p = d_plan();
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.is_empty(), "{v:?}");

        let mut p = d_plan();
        p.mode_meta.douyin = None;
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_meta_missing"));

        let mut p = d_plan();
        p.global.bpm = Some(200);
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_bpm_band"));

        let mut p = d_plan();
        if let Some(m) = p.mode_meta.douyin.as_mut() {
            m.hook_line = "这是一句远远超过二十个汉字限制的超长钩子还要第二句也超".into();
        }
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_hook_chars"));

        let mut p = d_plan();
        if let Some(m) = p.mode_meta.douyin.as_mut() {
            m.clip.start_section = "NoSuch".into();
        }
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_clip_section"));

        let mut p = d_plan();
        // 冷开失败：Verse 开头
        p.sections[0].kind = SectionKind::Verse;
        p.sections[0].label = "Verse".into();
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_cold_open"));
    }

    #[test]
    fn douyin_bpm_band_enforced_and_scene_written_by_backend() {
        let mut p = d_plan();
        apply_scene_to_plan(&mut p, DouyinScene::Dance, SceneSource::User);
        let m = p.mode_meta.douyin.as_ref().unwrap();
        assert_eq!(m.scene, DouyinScene::Dance);
        assert_eq!(m.scene_source, SceneSource::User);
        // Dance 120–140；plan bpm 110 → 红
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_bpm_band"));
        p.global.bpm = Some(128);
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(!v.iter().any(|i| i.code == "douyin_bpm_band"), "{v:?}");
    }

    #[test]
    fn douyin_mix_requires_phone_speaker() {
        let mut p = d_plan();
        p.global.mix = vec!["M-01".into()];
        let mut v = Vec::new();
        validate_douyin_v2(&p, &mut v);
        assert!(v.iter().any(|i| i.code == "douyin_mix" && i.message.contains("M-06")), "{v:?}");
    }

    #[test]
    fn get_douyin_meta_lists_all_scenes() {
        let m = get_douyin_meta();
        assert_eq!(m.scenes.len(), 5);
        assert_eq!(m.default_scene, "daily");
        assert!(m.scenes.iter().any(|s| s.is_default && s.id == "daily"));
    }
}
