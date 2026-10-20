//! SongPlan 校验门（BLUEPRINT_v2 §2.3 / P1-4）——终稿的**唯一**硬门。
//!
//! 输入只有结构化字段：门只数 `sections[].instruments.len()`、只读 `energy`，不再对文本做
//! 关键词启发式（b02-root-cause.md 结论 1：文本口径 8/11 例出错）。
//! 阈值全部来自 `rules`（数字单源，提示词插值同源 → 上游告知 = 下游校验）与 `suno_profile`。

use std::collections::BTreeMap;

use super::render_suno::{render, style_tags};
use super::suno_profile::{SUNO_EXCLUDE_MAX, SUNO_LYRICS_MAX, SUNO_STYLE_MAX, SUNO_STYLE_TAGS_MAX, SUNO_STYLE_TAGS_MIN, VARIETY_DEFAULT};
use super::{SectionKind, SongPlan, SCHEMA_VERSION};
use crate::rules;

fn lim(l: super::suno_profile::Limit) -> usize {
    l.value as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 不过门：触发审改重试，耗尽后 validation_passed=false
    Hard,
    /// 提示：随 AuditResult 透出，不打回
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub code: &'static str,
    /// JSON pointer（指向 SongPlan 字段，审改重试据此定位）
    pub path: String,
    pub message: String,
    pub severity: Severity,
}

impl Issue {
    fn hard(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, path: path.into(), message: message.into(), severity: Severity::Hard }
    }
    fn warn(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, path: path.into(), message: message.into(), severity: Severity::Warn }
    }
    /// 面向模型/用户的一行（`[code] path: message`）
    pub fn line(&self) -> String {
        format!("[{}] {}: {}", self.code, if self.path.is_empty() { "/" } else { &self.path }, self.message)
    }
}

/// 模式相关输入（全部为结构化或用户原文，不含模型自由文本）
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidateCtx<'a> {
    /// mode_a / mode_b / mode_c / mode_d
    pub mode: &'a str,
    /// 讨论收敛后的方案（转写保真基准；None = 不比对）
    pub converged: Option<&'a SongPlan>,
    /// Mode C 原歌词（逐行填词基准）
    pub original_lyrics: Option<&'a str>,
}

pub fn hard_issues(issues: &[Issue]) -> Vec<&Issue> {
    issues.iter().filter(|i| i.severity == Severity::Hard).collect()
}

pub fn passed(issues: &[Issue]) -> bool {
    issues.iter().all(|i| i.severity != Severity::Hard)
}

pub fn validate(p: &SongPlan, ctx: &ValidateCtx<'_>) -> Vec<Issue> {
    let mut out = Vec::new();
    check_budget(p, &mut out);
    check_settings(p, &mut out);
    check_polarity(p, &mut out);
    check_structure(p, &mut out);
    check_vocal_mix_coverage(p, &mut out);
    if matches!(ctx.mode, "mode_a" | "mode_b") {
        check_arc(p, &mut out);
    }
    if ctx.mode != "mode_c" {
        if let Some(base) = ctx.converged {
            check_fidelity(base, p, &mut out);
        }
    }
    check_style_and_headers(p, ctx.mode, &mut out);
    check_params(p, ctx.mode, &mut out);
    check_lyric_lines(p, ctx.mode, &mut out);
    if ctx.mode == "mode_d" {
        check_douyin(p, &mut out);
    }
    let rendered = render(p);
    if ctx.mode == "mode_c" {
        if let Some(orig) = ctx.original_lyrics {
            for m in crate::commands::validator::validate_lyric_fill(orig, &rendered.lyrics).issues {
                out.push(Issue::hard("lyric_fill", "/sections", m));
            }
        }
    }
    out
}

/// §2.3 `validate_vocal_mix_coverage`（P1-5）：global.vocal / global.mix 只引用 vocal_mix 词表 id，
/// 且各至少 1 个对应类别。词表缺失（回滚=删表）→ 跳过（不制造无法满足的门）。
pub fn check_vocal_mix_coverage(p: &SongPlan, out: &mut Vec<Issue>) {
    check_vocal_mix_coverage_with(crate::knowledge::shared_knowledge(), p, out)
}

fn check_vocal_mix_coverage_with(kb: &crate::knowledge::KnowledgeBase, p: &SongPlan, out: &mut Vec<Issue>) {
    let Some(rows) = crate::knowledge::vocal_mix_entries(kb) else { return };
    for (path, field, cat) in [("/global/vocal", &p.global.vocal, "vocal"), ("/global/mix", &p.global.mix, "mix")] {
        let mut hit = 0usize;
        for id in field {
            match rows.iter().find(|e| e.id == id.trim()) {
                Some(e) if e.category == cat => hit += 1,
                Some(e) => out.push(Issue::hard(
                    "vocal_mix_category",
                    path,
                    format!("「{id}」属 {} 类，不能填在 {cat} 字段", e.category),
                )),
                None => out.push(Issue::hard(
                    "vocal_mix_unknown_id",
                    path,
                    format!("「{id}」不是 vocal_mix 词表 id（可用：{}）", ids_of(&rows, cat)),
                )),
            }
        }
        if hit == 0 {
            out.push(Issue::hard(
                "vocal_mix_coverage",
                path,
                format!("至少引用 1 个 {cat} 词表 id（可用：{}）", ids_of(&rows, cat)),
            ));
        }
    }
}

fn ids_of(rows: &[crate::knowledge::VocalMixEntry], cat: &str) -> String {
    rows.iter().filter(|e| e.category == cat).map(|e| e.id.as_str()).collect::<Vec<_>>().join("/")
}

fn check_budget(p: &SongPlan, out: &mut Vec<Issue>) {
    if p.schema_version != SCHEMA_VERSION {
        out.push(Issue::hard("schema_version", "/schema_version", format!("须为 {SCHEMA_VERSION}")));
    }
    let r = render(p);
    let style_len = r.style.chars().count();
    if style_len > lim(SUNO_STYLE_MAX) {
        out.push(Issue::hard("style_too_long", "/global", format!("Style {style_len} 字符 > 上限 {}", lim(SUNO_STYLE_MAX))));
    }
    let lyr_len = r.lyrics.chars().count();
    if lyr_len > lim(SUNO_LYRICS_MAX) {
        out.push(Issue::hard("lyrics_too_long", "/sections", format!("Lyrics {lyr_len} 字符 > 上限 {}", lim(SUNO_LYRICS_MAX))));
    }
    let ex_len = r.exclude.chars().count();
    if ex_len > lim(SUNO_EXCLUDE_MAX) {
        out.push(Issue::hard("exclude_too_long", "/global/negative", format!("Exclude {ex_len} 字符 > 上限 {}", lim(SUNO_EXCLUDE_MAX))));
    }
    let n = style_tags(p).len();
    if n < lim(SUNO_STYLE_TAGS_MIN) || n > lim(SUNO_STYLE_TAGS_MAX) {
        out.push(Issue::warn(
            "style_tag_count",
            "/global",
            format!("Style 标签 {n} 个，建议 {}–{}", lim(SUNO_STYLE_TAGS_MIN), lim(SUNO_STYLE_TAGS_MAX)),
        ));
    }
}

fn check_settings(p: &SongPlan, out: &mut Vec<Issue>) {
    if usize::from(p.settings.variety) != lim(VARIETY_DEFAULT) {
        out.push(Issue::hard("variety", "/settings/variety", format!("Variety 须为 {}", lim(VARIETY_DEFAULT))));
    }
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

fn is_negative_phrase(s: &str) -> bool {
    let l = norm(s);
    l.starts_with("no ") || l.starts_with("without ") || l.starts_with("不要") || l.starts_with("无")
}

/// 极性：负向词不得出现在正向层；正向层不得写否定句（应进 negative / negative_local）
fn check_polarity(p: &SongPlan, out: &mut Vec<Issue>) {
    let g = &p.global;
    let positive: Vec<(&str, &String)> = g
        .genre
        .iter()
        .map(|x| ("/global/genre", x))
        .chain(g.vocal.iter().map(|x| ("/global/vocal", x)))
        .chain(g.instruments.iter().map(|x| ("/global/instruments", x)))
        .chain(g.mix.iter().map(|x| ("/global/mix", x)))
        .collect();
    for (path, v) in &positive {
        if is_negative_phrase(v) {
            out.push(Issue::hard("polarity", *path, format!("正向层出现否定项「{v}」，应移入 /global/negative")));
        }
        if g.negative.iter().any(|n| norm(n) == norm(v)) {
            out.push(Issue::hard("polarity", *path, format!("「{v}」同时出现在正向层与 /global/negative")));
        }
    }
    for n in &g.negative {
        if is_negative_phrase(n) {
            out.push(Issue::warn("polarity", "/global/negative", format!("Exclude 项「{n}」不需要否定词，直接写要排除的元素")));
        }
    }
    for (i, s) in p.sections.iter().enumerate() {
        for (j, c) in s.positive_local.iter().enumerate() {
            if is_negative_phrase(c) {
                out.push(Issue::hard("polarity", format!("/sections/{i}/positive_local/{j}"), format!("否定 cue「{c}」应移入 negative_local")));
            }
        }
        for (j, c) in s.instruments.iter().enumerate() {
            if is_negative_phrase(c) {
                out.push(Issue::hard("polarity", format!("/sections/{i}/instruments/{j}"), format!("instruments 只放在场乐器，「{c}」应移入 negative_local")));
            }
        }
    }
}

fn kind_allows_minimal(k: SectionKind) -> bool {
    matches!(k, SectionKind::Intro | SectionKind::Outro | SectionKind::Bridge | SectionKind::Interlude)
}

fn kind_is_chorus_class(k: SectionKind) -> bool {
    matches!(k, SectionKind::Chorus | SectionKind::Hook | SectionKind::PostChorus | SectionKind::Drop)
}

fn check_structure(p: &SongPlan, out: &mut Vec<Issue>) {
    if p.sections.len() < rules::MIN_SECTION_TAGS {
        out.push(Issue::hard("section_count", "/sections", format!("段落 {} 个 < 下限 {}", p.sections.len(), rules::MIN_SECTION_TAGS)));
    }
    for (i, s) in p.sections.iter().enumerate() {
        if s.label.trim().is_empty() {
            out.push(Issue::hard("section_label", format!("/sections/{i}/label"), "段名不能为空"));
        }
        if s.minimal {
            let energy_ok = s.energy.is_some_and(|e| u32::from(e) <= rules::MINIMAL_ENERGY_MAX);
            if !kind_allows_minimal(s.kind) || !energy_ok {
                out.push(Issue::hard(
                    "minimal_misuse",
                    format!("/sections/{i}/minimal"),
                    format!("极简段只允许 intro/outro/bridge/interlude 且 energy ≤{}", rules::MINIMAL_ENERGY_MAX),
                ));
            }
        }
        let distinct: std::collections::BTreeSet<String> = s.instruments.iter().map(|x| norm(x)).collect();
        if distinct.len() != s.instruments.len() {
            out.push(Issue::warn("instrument_dup", format!("/sections/{i}/instruments"), "段内乐器重复"));
        }
        for x in &s.instruments {
            if !p.global.instruments.iter().any(|g| norm(g) == norm(x)) {
                out.push(Issue::warn(
                    "instrument_not_global",
                    format!("/sections/{i}/instruments"),
                    format!("「{x}」未登记在 /global/instruments"),
                ));
            }
        }
    }
}

/// Style 长度门（清单承诺 ≤STYLE_PROMPT_MAX / A·B·D ≥STYLE_PROMPT_MIN，与提示词插值同源）+ 段头长度门
fn check_style_and_headers(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    let r = render(p);
    for m in crate::commands::validator::style_prompt_length_issues(mode, &r.style) {
        out.push(Issue::hard("style_length", "/global", m));
    }
    for (i, s) in p.sections.iter().enumerate() {
        let cues = super::render_suno::section_cues(s).join(", ");
        let n = s.label.trim().chars().count() + cues.chars().count() + 5;
        if n > rules::DESC_LINE_MAX_CHARS {
            out.push(Issue::hard(
                "header_too_long",
                format!("/sections/{i}/positive_local"),
                format!("「{}」段头 {n} 字符 > 上限 {}——压缩 cue（保留乐器，删修饰）", s.label, rules::DESC_LINE_MAX_CHARS),
            ));
        }
    }
}

fn narrative_sections(p: &SongPlan) -> usize {
    p.sections.iter().filter(|s| matches!(s.kind, SectionKind::Verse | SectionKind::PreChorus | SectionKind::Bridge)).count()
}

/// 参数区间（A=弧线区间并集 / B=专属区间 / D=抖音区间+叙事型例外）、Audio Influence、BPM
fn check_params(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    let w = u32::from(p.settings.weirdness);
    let s = u32::from(p.settings.style_influence);
    match mode {
        "mode_a" => {
            let (wlo, whi, slo, shi) = rules::arc_param_union();
            if !(wlo..=whi).contains(&w) || !(slo..=shi).contains(&s) {
                out.push(Issue::hard("param_range", "/settings", format!("Weirdness={w}/Style Influence={s} 越出弧线区间并集 {wlo}-{whi}/{slo}-{shi}")));
            }
        }
        "mode_b" => {
            if !(rules::MODE_B_WEIRD_MIN..=rules::MODE_B_WEIRD_MAX).contains(&w) {
                out.push(Issue::hard("param_range", "/settings/weirdness", format!("Weirdness={w}（B 专属区间 {}-{}）", rules::MODE_B_WEIRD_MIN, rules::MODE_B_WEIRD_MAX)));
            }
            if !(rules::MODE_B_STYLE_MIN..=rules::MODE_B_STYLE_MAX).contains(&s) {
                out.push(Issue::hard("param_range", "/settings/style_influence", format!("Style Influence={s}（B 专属区间 {}-{}）", rules::MODE_B_STYLE_MIN, rules::MODE_B_STYLE_MAX)));
            }
        }
        "mode_d" => {
            let in_douyin = (rules::DOUYIN_WEIRD_MIN..=rules::DOUYIN_WEIRD_MAX).contains(&w) && (rules::DOUYIN_STYLE_MIN..=rules::DOUYIN_STYLE_MAX).contains(&s);
            let narrative = narrative_sections(p) >= rules::NARRATIVE_SECTIONS_MIN && p.notes.as_deref().is_some_and(|n| n.contains("叙事型"));
            if !in_douyin && !narrative {
                out.push(Issue::hard(
                    "param_range",
                    "/settings",
                    format!(
                        "Weirdness={w}/Style Influence={s}（抖音区间 {}-{}/{}-{}；仅当含≥{}个叙事段且 notes 写明'叙事型'方可回落 A/B 区间）",
                        rules::DOUYIN_WEIRD_MIN, rules::DOUYIN_WEIRD_MAX, rules::DOUYIN_STYLE_MIN, rules::DOUYIN_STYLE_MAX, rules::NARRATIVE_SECTIONS_MIN
                    ),
                ));
            }
        }
        _ => {}
    }
    if matches!(mode, "mode_a" | "mode_b") && p.settings.audio_influence.is_some_and(|a| a != 0) {
        out.push(Issue::hard("audio_influence", "/settings/audio_influence", "无参考音频，Audio Influence 须为 0 或 null"));
    }
    if let Some(b) = p.global.bpm {
        if let Some(m) = crate::commands::validator::check_bpm_range(mode, u32::from(b)) {
            out.push(Issue::hard("bpm", "/global/bpm", m));
        }
    } else if mode == "mode_d" {
        out.push(Issue::hard("bpm", "/global/bpm", format!("抖音模式须给 BPM（≥{}）", rules::DOUYIN_BPM_MIN)));
    }
}

/// 歌词行：A/B 禁 `/` `、` 断句；D 每行 ≤DOUYIN_LINE_MAX 字（和音虚词不计）
fn check_lyric_lines(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    for (i, s) in p.sections.iter().enumerate() {
        for (j, l) in s.lines.iter().enumerate() {
            let path = format!("/sections/{i}/lines/{j}");
            if matches!(mode, "mode_a" | "mode_b") && (l.contains('/') || l.contains('、')) {
                out.push(Issue::hard("break_mark", path.clone(), format!("禁用断句符号（/ 或 、），改用单空格：{l}")));
            }
            if mode == "mode_d" {
                let n = crate::commands::validator::count_line_chars_excluding_vocables(l, |c| c.is_ascii_punctuation());
                if n > rules::DOUYIN_LINE_MAX_CHARS {
                    out.push(Issue::hard("line_too_long", path, format!("{n} 字 > 上限 {}：{l}", rules::DOUYIN_LINE_MAX_CHARS)));
                }
            }
        }
    }
}

/// D 模式结构门（原 `validate_douyin` 迁 IR）：Hook 次数、Verse 行数、结尾骤停
fn check_douyin(p: &SongPlan, out: &mut Vec<Issue>) {
    let hooks = p.sections.iter().filter(|s| s.kind == SectionKind::Hook || s.label.to_lowercase().contains("hook")).count();
    if hooks < rules::HOOK_MIN_COUNT {
        out.push(Issue::hard("hook_count", "/sections", format!("Hook {hooks} 次 < 下限 {}", rules::HOOK_MIN_COUNT)));
    }
    for (i, s) in p.sections.iter().enumerate() {
        let n = s.lines.iter().filter(|l| !l.trim().is_empty()).count();
        if s.kind == SectionKind::Verse && n > rules::VERSE_MAX_LINES {
            out.push(Issue::hard("verse_lines", format!("/sections/{i}/lines"), format!("「{}」{n} 行 > 上限 {}", s.label, rules::VERSE_MAX_LINES)));
        }
    }
    let has_stop = p.sections.last().is_some_and(|s| {
        s.positive_local.iter().chain(s.negative_local.iter()).any(|c| {
            let l = c.to_lowercase();
            l.contains("骤停") || l.contains("abrupt") || l.contains("cut off") || l.contains("all instruments cut") || l.contains("hard stop")
        })
    });
    if !has_stop {
        out.push(Issue::hard("douyin_stop", "/sections", "末段须在 positive_local 标注骤停（如 \"abrupt stop\" / \"all instruments cut\"）"));
    }
}

/// 段下限（b02-root-cause.md 结论 2–4）：声明极简 1 / Chorus 类 3 / 其余 2
pub fn section_floor(s: &super::Section) -> usize {
    if s.minimal && kind_allows_minimal(s.kind) {
        rules::MIN_INSTRUMENT_MINIMAL
    } else if kind_is_chorus_class(s.kind) {
        rules::MIN_INSTRUMENT_CHORUS
    } else {
        rules::MIN_INSTRUMENT_CORE
    }
}

fn check_arc(p: &SongPlan, out: &mut Vec<Issue>) {
    if p.sections.is_empty() {
        return;
    }
    let mut energies = Vec::new();
    for (i, s) in p.sections.iter().enumerate() {
        match s.energy {
            Some(e) => energies.push((i, u32::from(e))),
            None => out.push(Issue::hard("energy_missing", format!("/sections/{i}/energy"), "A/B 模式每段须标能量 1–10")),
        }
    }
    if let (Some(min), Some(max)) = (energies.iter().map(|x| x.1).min(), energies.iter().map(|x| x.1).max()) {
        if max - min < rules::MIN_ENERGY_GAP {
            out.push(Issue::hard("energy_gap", "/sections", format!("能量差 {} < 下限 {}", max - min, rules::MIN_ENERGY_GAP)));
        }
    }
    let global_n = p.global.instruments.iter().map(|x| norm(x)).collect::<std::collections::BTreeSet<_>>().len();
    if global_n > rules::INSTRUMENT_MAX {
        out.push(Issue::hard("instrument_max", "/global/instruments", format!("全曲乐器 {global_n} 件 > 上限 {}", rules::INSTRUMENT_MAX)));
    }
    for (i, s) in p.sections.iter().enumerate() {
        let n = s.instruments.len();
        let floor = section_floor(s);
        if n < floor {
            out.push(Issue::hard("instrument_floor", format!("/sections/{i}/instruments"), format!("「{}」{n} 件 < 段下限 {floor}", s.label)));
        }
        if n > rules::INSTRUMENT_MAX {
            out.push(Issue::hard("instrument_max", format!("/sections/{i}/instruments"), format!("「{}」{n} 件 > 上限 {}", s.label, rules::INSTRUMENT_MAX)));
        }
    }
    // 最强段 = 能量最高段（并列取件数最多者）；最弱段 = 能量最低段（并列取件数最少者）
    let pick = |want_max: bool| {
        energies
            .iter()
            .map(|&(i, e)| (i, e, p.sections[i].instruments.len()))
            .reduce(|a, b| {
                let better = if want_max { (b.1, b.2) > (a.1, a.2) } else { (b.1, std::cmp::Reverse(b.2)) < (a.1, std::cmp::Reverse(a.2)) };
                if better { b } else { a }
            })
    };
    if let (Some(strong), Some(weak)) = (pick(true), pick(false)) {
        if strong.2 < rules::MIN_INSTRUMENT_STRONG {
            out.push(Issue::hard(
                "instrument_strong",
                format!("/sections/{}/instruments", strong.0),
                format!("最强段「{}」{} 件 < 下限 {}", p.sections[strong.0].label, strong.2, rules::MIN_INSTRUMENT_STRONG),
            ));
        }
        if strong.0 != weak.0 && strong.2 < weak.2 + rules::MIN_INSTRUMENT_GAP {
            out.push(Issue::hard(
                "instrument_gap",
                "/sections",
                format!("最强段 {} 件 − 最弱段 {} 件 < 配器差下限 {}", strong.2, weak.2, rules::MIN_INSTRUMENT_GAP),
            ));
        }
    }
}

fn lyric_multiset(p: &SongPlan) -> BTreeMap<String, i64> {
    let mut m = BTreeMap::new();
    for s in &p.sections {
        for l in &s.lines {
            let k: String = l.chars().filter(|c| !c.is_whitespace()).collect();
            if !k.is_empty() {
                *m.entry(k).or_insert(0) += 1;
            }
        }
    }
    m
}

/// 转写保真：终稿歌词行 multiset == 收敛方案歌词行 multiset（空白归一）
fn check_fidelity(base: &SongPlan, fin: &SongPlan, out: &mut Vec<Issue>) {
    let a = lyric_multiset(base);
    let b = lyric_multiset(fin);
    for (k, n) in &a {
        let m = b.get(k).copied().unwrap_or(0);
        if m < *n {
            out.push(Issue::hard("fidelity_missing", "/sections", format!("收敛方案歌词行缺失 ×{}：{k}", n - m)));
        }
    }
    for (k, n) in &b {
        let m = a.get(k).copied().unwrap_or(0);
        if *n > m {
            out.push(Issue::hard("fidelity_added", "/sections", format!("终稿新增方案外歌词行 ×{}：{k}", n - m)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::fixtures::minimal_plan;
    use crate::plan::{Section, SectionKind};

    fn ctx(mode: &str) -> ValidateCtx<'_> {
        ValidateCtx { mode, converged: None, original_lyrics: None }
    }

    fn codes(v: &[Issue]) -> Vec<&'static str> {
        hard_issues(v).iter().map(|i| i.code).collect()
    }

    fn sec(kind: SectionKind, energy: u8, n: usize, minimal: bool) -> Section {
        Section {
            kind,
            label: format!("{kind:?}"),
            positive_local: vec![],
            negative_local: vec![],
            energy: Some(energy),
            instruments: ["guzheng", "cello", "dizi", "pipa", "erhu", "piano", "drums"][..n].iter().map(|s| s.to_string()).collect(),
            minimal,
            target_seconds: None,
            lines: vec![],
        }
    }

    fn ab_plan(sections: Vec<Section>) -> SongPlan {
        let mut p = minimal_plan();
        p.global.instruments = ["guzheng", "cello", "dizi", "pipa", "erhu", "piano", "drums"].iter().map(|s| s.to_string()).collect();
        p.sections = sections;
        p.settings.weirdness = 25;
        p.settings.style_influence = 80;
        p
    }

    #[test]
    fn minimal_fixture_passes_mode_b() {
        let mut p = minimal_plan();
        p.global.instruments.extend(["dizi".into(), "pipa".into()]);
        p.settings.style_influence = 80;
        let v = validate(&p, &ctx("mode_b"));
        assert!(passed(&v), "{v:?}");
    }

    #[test]
    fn variety_nonzero_is_hard() {
        let mut p = minimal_plan();
        p.settings.variety = 10;
        assert!(codes(&validate(&p, &ctx("mode_d_none"))).contains(&"variety"));
    }

    #[test]
    fn polarity_negative_in_positive_layer() {
        let mut p = minimal_plan();
        p.global.instruments.push("no drums".into());
        p.sections[1].positive_local.push("without bass".into());
        let c = codes(&validate(&p, &ctx("x")));
        assert_eq!(c.iter().filter(|c| **c == "polarity").count(), 2, "{c:?}");
    }

    /// b02 结论 2：极简段 1 件放行（仅限 kind 白名单 + energy ≤2）
    #[test]
    fn minimal_intro_one_instrument_passes_but_verse_cannot_claim_minimal() {
        let ok = ab_plan(vec![sec(SectionKind::Intro, 1, 1, true), sec(SectionKind::Verse, 4, 2, false), sec(SectionKind::Chorus, 8, 4, false)]);
        assert!(passed(&validate(&ok, &ctx("mode_a"))), "{:?}", validate(&ok, &ctx("mode_a")));
        let bad = ab_plan(vec![sec(SectionKind::Verse, 1, 1, true), sec(SectionKind::Chorus, 8, 4, false)]);
        let c = codes(&validate(&bad, &ctx("mode_a")));
        assert!(c.contains(&"minimal_misuse") && c.contains(&"instrument_floor"), "{c:?}");
        let loud = ab_plan(vec![sec(SectionKind::Intro, 3, 1, true), sec(SectionKind::Chorus, 8, 4, false)]);
        assert!(codes(&validate(&loud, &ctx("mode_a"))).contains(&"minimal_misuse"));
    }

    /// b02 结论 3/4：核心段 2 件放行；摇滚四件套最强 4 件放行；Chorus 类 2 件拦
    #[test]
    fn core_floor_two_strong_floor_four_chorus_floor_three() {
        let ok = ab_plan(vec![sec(SectionKind::Verse, 3, 2, false), sec(SectionKind::Chorus, 7, 4, false)]);
        assert!(passed(&validate(&ok, &ctx("mode_b"))));
        let weak_strong = ab_plan(vec![sec(SectionKind::Verse, 3, 2, false), sec(SectionKind::Chorus, 7, 3, false)]);
        let c = codes(&validate(&weak_strong, &ctx("mode_b")));
        assert!(c.contains(&"instrument_strong"), "{c:?}");
        let thin_chorus = ab_plan(vec![sec(SectionKind::Intro, 1, 1, true), sec(SectionKind::Chorus, 5, 2, false), sec(SectionKind::Chorus, 8, 4, false)]);
        assert!(codes(&validate(&thin_chorus, &ctx("mode_b"))).contains(&"instrument_floor"));
    }

    #[test]
    fn energy_gap_and_instrument_gap_and_cap() {
        let flat = ab_plan(vec![sec(SectionKind::Verse, 5, 2, false), sec(SectionKind::Chorus, 6, 4, false)]);
        assert!(codes(&validate(&flat, &ctx("mode_a"))).contains(&"energy_gap"));
        let no_gap = ab_plan(vec![sec(SectionKind::Verse, 2, 3, false), sec(SectionKind::Chorus, 8, 4, false)]);
        assert!(codes(&validate(&no_gap, &ctx("mode_a"))).contains(&"instrument_gap"));
        let mut over = ab_plan(vec![sec(SectionKind::Verse, 2, 2, false), sec(SectionKind::Chorus, 8, 7, false)]);
        over.global.instruments.push("bass".into());
        over.sections[1].instruments.push("bass".into());
        let c = codes(&validate(&over, &ctx("mode_a")));
        assert_eq!(c.iter().filter(|c| **c == "instrument_max").count(), 2, "{c:?}");
    }

    /// 门不对 C/D 模式做 A/B 弧线约束
    #[test]
    fn arc_rules_only_for_ab() {
        let p = ab_plan(vec![sec(SectionKind::Verse, 5, 1, false), sec(SectionKind::Chorus, 5, 1, false)]);
        assert!(!codes(&validate(&p, &ctx("mode_a"))).is_empty());
        assert!(codes(&validate(&p, &ctx("mode_c"))).iter().all(|c| !c.starts_with("instrument") && !c.starts_with("energy")));
    }

    #[test]
    fn fidelity_multiset_detects_missing_and_added() {
        let base = minimal_plan();
        let mut fin = minimal_plan();
        fin.sections[1].lines = vec!["雨落 长街".into(), "新加一句".into()];
        let v = validate(&fin, &ValidateCtx { mode: "mode_b", converged: Some(&base), original_lyrics: None });
        let c = codes(&v);
        assert!(c.contains(&"fidelity_missing") && c.contains(&"fidelity_added"), "{c:?}");
        assert!(!v.iter().any(|i| i.message.contains("雨落长街") && i.code == "fidelity_missing"), "空白归一");
        // Mode C 不跑方案保真（以原歌词为基准）
        let v = validate(&fin, &ValidateCtx { mode: "mode_c", converged: Some(&base), original_lyrics: None });
        assert!(!codes(&v).iter().any(|c| c.starts_with("fidelity")));
    }

    #[test]
    fn mode_c_lyric_fill_runs_on_rendered_lyrics() {
        let mut p = minimal_plan();
        p.sections[1].lines = vec!["一二三".into()];
        let v = validate(&p, &ValidateCtx { mode: "mode_c", converged: None, original_lyrics: Some("[Verse]\n雨落长街湿\n灯火未眠时") });
        assert!(codes(&v).contains(&"lyric_fill"), "{v:?}");
    }

    #[test]
    fn params_by_mode() {
        let mut p = minimal_plan();
        p.settings.weirdness = 50;
        p.settings.style_influence = 50;
        assert!(codes(&validate(&p, &ctx("mode_b"))).contains(&"param_range"));
        assert!(codes(&validate(&p, &ctx("mode_a"))).contains(&"param_range"));
        p.settings.weirdness = 15;
        p.settings.style_influence = 90;
        assert!(!codes(&validate(&p, &ctx("mode_d"))).contains(&"param_range"));
        p.settings.audio_influence = Some(20);
        assert!(codes(&validate(&p, &ctx("mode_b"))).contains(&"audio_influence"));
    }

    fn douyin_plan() -> SongPlan {
        let mut p = minimal_plan();
        p.global.bpm = Some(120);
        p.settings.weirdness = 15;
        p.settings.style_influence = 90;
        let mut hook = sec(SectionKind::Hook, 8, 3, false);
        hook.lines = vec!["一刀切下去".into()];
        let mut end = hook.clone();
        end.positive_local = vec!["abrupt stop".into()];
        p.sections = vec![hook, end];
        p
    }

    #[test]
    fn douyin_structure_ported_to_ir() {
        let p = douyin_plan();
        assert!(passed(&validate(&p, &ctx("mode_d"))), "{:?}", validate(&p, &ctx("mode_d")));
        let mut bad = douyin_plan();
        bad.global.bpm = Some(80);
        bad.sections[1].positive_local.clear();
        bad.sections[1].kind = SectionKind::Outro;
        bad.sections[1].label = "Outro".into();
        bad.sections[0].lines = vec!["这一行歌词明显超过了十二个字的上限".into()];
        let c = codes(&validate(&bad, &ctx("mode_d")));
        for want in ["bpm", "douyin_stop", "hook_count", "line_too_long"] {
            assert!(c.contains(&want), "缺 {want}: {c:?}");
        }
    }

    #[test]
    fn break_marks_and_header_length() {
        let mut p = minimal_plan();
        p.sections[1].lines[0] = "雨落/长街".into();
        p.sections[1].positive_local = vec!["x".repeat(rules::DESC_LINE_MAX_CHARS)];
        let c = codes(&validate(&p, &ctx("mode_b")));
        assert!(c.contains(&"break_mark") && c.contains(&"header_too_long"), "{c:?}");
    }

    #[test]
    fn budget_limits() {
        let mut p = minimal_plan();
        p.sections[1].lines = vec!["字".repeat(lim(SUNO_LYRICS_MAX) + 1)];
        p.global.negative = vec!["x".repeat(lim(SUNO_EXCLUDE_MAX) + 1)];
        let c = codes(&validate(&p, &ctx("x")));
        assert!(c.contains(&"lyrics_too_long") && c.contains(&"exclude_too_long"), "{c:?}");
    }

    #[test]
    fn vocal_mix_coverage_gate() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        let run = |p: &SongPlan| {
            let mut v = Vec::new();
            check_vocal_mix_coverage_with(&kb, p, &mut v);
            v.into_iter().map(|i| i.code).collect::<Vec<_>>()
        };
        let ok = minimal_plan();
        assert!(run(&ok).is_empty(), "fixture 引用合法 id");
        let mut p = minimal_plan();
        p.global.vocal.clear();
        assert_eq!(run(&p), vec!["vocal_mix_coverage"]);
        let mut p = minimal_plan();
        p.global.mix = vec!["warm mix".into()];
        let c = run(&p);
        assert!(c.contains(&"vocal_mix_unknown_id") && c.contains(&"vocal_mix_coverage"), "{c:?}");
        let mut p = minimal_plan();
        p.global.vocal = vec!["M-01".into()];
        let c = run(&p);
        assert!(c.contains(&"vocal_mix_category") && c.contains(&"vocal_mix_coverage"), "{c:?}");
        // 回滚=删表：门跳过
        let mut v = Vec::new();
        check_vocal_mix_coverage_with(&crate::knowledge::KnowledgeBase::default(), &p, &mut v);
        assert!(v.is_empty());
        // 接线：validate 主入口必须调用覆盖门
        let mut p = minimal_plan();
        p.global.mix.clear();
        let all = validate(&p, &ValidateCtx { mode: "mode_b", converged: None, original_lyrics: None });
        assert!(all.iter().any(|i| i.code == "vocal_mix_coverage"), "validate 未接线覆盖门");
    }

}
