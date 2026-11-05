//! SongPlan 校验门（v3 物理契约硬门 + 去机器味诊断）——终稿的**唯一**校验入口。
//!
//! 输入只有结构化字段：门只读 `SongPlan` 结构化属性，不再对渲染文本做脆弱正则猜解。
//! 阈值全部来自 `rules`（数字单源，提示词插值同源 → 上游告知 = 下游校验）与 `suno_profile`。
//! 区分两层：
//! - `Severity::Hard`：平台物理上限、字段极性、语言契约、Mode C 等字对齐、Mode D 完播结构硬门；
//! - `Severity::Warn`：去机器味审美诊断（空洞形容词、能量单调无回落、末遍副歌雷同、抽象词堆砌、十三辙单调/高音闭口韵等），以及带 `exemptions` 显式创作理由豁免的配器/参数弹性项。

use std::collections::BTreeMap;

use super::render_suno::{render, style_tags};
use super::suno_profile::{
    SUNO_EXCLUDE_MAX, SUNO_LYRICS_MAX, SUNO_STYLE_MAX, SUNO_STYLE_TAGS_MAX, SUNO_STYLE_TAGS_MIN,
    VARIETY_DEFAULT,
};
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
    pub(crate) fn hard(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            severity: Severity::Hard,
        }
    }
    pub(crate) fn warn(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            severity: Severity::Warn,
        }
    }
    /// 当 `exempted=true` 时降级为 Warn（保留诊断可见性但不打回），否则为 Hard。
    pub(crate) fn hard_or_exempt(
        exempted: bool,
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        if exempted {
            Self::warn(code, path, message)
        } else {
            Self::hard(code, path, message)
        }
    }
    /// 面向模型/用户的一行（`[code] path: message`）
    pub fn line(&self) -> String {
        format!(
            "[{}] {}: {}",
            self.code,
            if self.path.is_empty() {
                "/"
            } else {
                &self.path
            },
            self.message
        )
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
    issues
        .iter()
        .filter(|i| i.severity == Severity::Hard)
        .collect()
}

pub fn passed(issues: &[Issue]) -> bool {
    issues.iter().all(|i| i.severity != Severity::Hard)
}

fn has_exemption(p: &SongPlan) -> bool {
    p.exemptions.iter().any(|e| !e.trim().is_empty())
}

pub fn validate(p: &SongPlan, ctx: &ValidateCtx<'_>) -> Vec<Issue> {
    let mut out = Vec::new();
    check_budget(p, &mut out);
    check_settings(p, &mut out);
    check_polarity(p, &mut out);
    check_structure(p, ctx.mode, &mut out);
    check_vocal_mix_coverage(p, &mut out);
    out.extend(validate_lang(p));
    if matches!(ctx.mode, "mode_a" | "mode_b") {
        check_arc(p, &mut out);
    }
    check_style_and_headers(p, ctx.mode, &mut out);
    out.extend(super::style_lang::validate_style_english(p));
    check_params(p, ctx.mode, &mut out);
    check_lyric_lines(p, ctx.mode, &mut out);
    check_anti_ai_tells(p, ctx.mode, &mut out);
    if ctx.mode == "mode_d" {
        check_douyin(p, &mut out);
    }
    check_prosody_and_rhyme(p, ctx, &mut out);
    if ctx.mode != "mode_c" {
        if let Some(base) = ctx.converged {
            // 仅当收敛初稿 base 的歌词本身无任何声韵/字数/语言/Hook 硬伤（或为 Mode A 锁定用户原词）时，
            // 才执行逐行保真锁；若初稿歌词存在押韵散乱、脱韵、同音挤韵或字数违例，必须允许定稿人重写违规歌词行！
            let base_needs_lyric_rewrite = if ctx.mode == "mode_a" {
                false
            } else {
                let mut base_issues = Vec::new();
                base_issues.extend(validate_lang(base));
                check_lyric_lines(base, ctx.mode, &mut base_issues);
                if ctx.mode == "mode_d" {
                    check_douyin(base, &mut base_issues);
                }
                let base_ctx = ValidateCtx {
                    mode: ctx.mode,
                    converged: None,
                    original_lyrics: ctx.original_lyrics,
                };
                check_prosody_and_rhyme(base, &base_ctx, &mut base_issues);
                base_issues.iter().any(|i| i.severity == Severity::Hard)
            };
            if !base_needs_lyric_rewrite {
                check_fidelity(base, p, &mut out);
            }
        }
    }
    let rendered = render(p);
    if ctx.mode == "mode_c" {
        if let Some(orig) = ctx.original_lyrics {
            for m in crate::commands::validator::validate_lyric_fill(orig, &rendered.lyrics).issues
            {
                out.push(Issue::hard("lyric_fill", "/sections", m));
            }
            // P1-8 / C-03：韵脚咨询保留（2 行极小样本走 Warn；≥3 行真实曲目由 check_prosody_and_rhyme 硬门拦截）
            out.extend(advise_rhyme(orig, &rendered.lyrics));
        }
    }
    out
}

// ---------------------------------------------------------------------------
/// P1-8（C-03）：Mode C 韵脚咨询——对照原词押韵位置，用拼音韵母（十三辙近似）检查新词。
/// 仅 `Severity::Warn`：多音字取 `lazy_pinyin` 首读，误判只报告不打回（BLUEPRINT 风险项）。
pub fn advise_rhyme(original: &str, new_lyrics: &str) -> Vec<Issue> {
    let orig = extract_c_lyric_lines(original);
    let neu = extract_c_lyric_lines(new_lyrics);
    if orig.len() < 2 || neu.is_empty() {
        return Vec::new();
    }
    let n = orig.len().min(neu.len());
    let of: Vec<Option<String>> = orig[..n].iter().map(|l| line_rhyme_key(l)).collect();
    let nf: Vec<Option<String>> = neu[..n].iter().map(|l| line_rhyme_key(l)).collect();

    // 原词中押韵的行对（同辙）；新词对应行对若不在同辙 → 告警
    let mut out = Vec::new();
    let mut reported = 0usize;
    const CAP: usize = 6;
    for i in 0..n {
        for j in (i + 1)..n {
            let (Some(a), Some(b)) = (&of[i], &of[j]) else {
                continue;
            };
            if a != b {
                continue; // 原词此处不押，不要求新词押
            }
            match (&nf[i], &nf[j]) {
                (Some(x), Some(y)) if x == y => {}
                (Some(x), Some(y)) => {
                    reported += 1;
                    if reported <= CAP {
                        out.push(Issue::warn(
                            "rhyme_advise",
                            format!("/sections/lines/{i}+{j}"),
                            format!(
                                "韵脚提示：原词第 {}/{} 行同辙「{}」，新词为「{}」/「{}」（多音字取首读，仅提示）",
                                i + 1,
                                j + 1,
                                a,
                                x,
                                y
                            ),
                        ));
                    }
                }
                _ => {
                    reported += 1;
                    if reported <= CAP {
                        out.push(Issue::warn(
                            "rhyme_advise",
                            format!("/sections/lines/{i}+{j}"),
                            format!(
                                "韵脚提示：原词第 {}/{} 行同辙「{}」，新词对应行缺汉字韵脚（仅提示）",
                                i + 1,
                                j + 1,
                                a
                            ),
                        ));
                    }
                }
            }
        }
    }
    if reported > CAP {
        out.push(Issue::warn(
            "rhyme_advise",
            "/sections",
            format!(
                "韵脚提示：另有 {} 处押韵位置可能漂移（已省略明细）",
                reported - CAP
            ),
        ));
    }
    out
}

/// 与 `validate_lyric_fill` 同口径抽取歌词行（去标签/说明/包装行）。
fn extract_c_lyric_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('[')
                && !l.starts_with('#')
                && !l.starts_with('|')
                && !l.starts_with("**")
                && !l.starts_with("Style")
                && !l.starts_with("风格")
                && !l.starts_with("参数")
                && !l.starts_with("Weirdness")
                && !l.starts_with("【声学制作偏好：")
                && !crate::commands::validator::is_bare_package_line(l)
        })
        .collect()
}

fn last_cjk(s: &str) -> Option<char> {
    s.chars().rev().find(|c| {
        let u = *c as u32;
        (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u)
    })
}

/// 抽取行内全部汉字（用于多音字词尾语境消歧与尾词排比判定）
fn cjk_chars(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| {
            let u = *c as u32;
            (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u)
        })
        .collect()
}

/// 常见词尾多音字语境消歧：返回消歧后的无调拼音（如 睡觉→jiao、感觉→jue、远行→xing、银行→hang）。
fn contextual_pinyin_plain(line: &str, ch: char) -> Option<String> {
    use pinyin::ToPinyin;
    let cjks = cjk_chars(line);
    let prev = if cjks.len() >= 2 {
        Some(cjks[cjks.len() - 2])
    } else {
        None
    };
    let overriden = match (prev, ch) {
        (Some('睡' | '午' | '困'), '觉') => Some("jiao"),
        (Some(_), '觉') => Some("jue"),
        (Some('音' | '奏' | '声' | '配' | '民' | '器' | '弦'), '乐') => Some("yue"),
        (Some(_), '乐') => Some("le"),
        (Some('银' | '外' | '内' | '排' | '本' | '改' | '同'), '行') => Some("hang"),
        (_, '行') => Some("xing"),
        (Some('归' | '偿' | '退' | '奉' | '往'), '还') => Some("huan"),
        (_, '还') => Some("hai"),
        (Some('睡' | '点' | '燃' | '找' | '猜' | '够'), '着') => Some("zhao"),
        (Some('执' | '衣' | '穿' | '沉' | '附' | '落'), '着') => Some("zhuo"),
        (Some('非' | '总' | '须' | '必'), '得') => Some("dei"),
        (Some('成' | '首' | '古' | '国' | '帝' | '魔' | '旧'), '都') => Some("du"),
        (_, '都') => Some("dou"),
        (Some('星'), '宿') => Some("xiu"),
        (Some('一' | '模'), '模') => Some("mu"),
        _ => None,
    };
    if let Some(p) = overriden {
        return Some(p.to_string());
    }
    let plain = ch.to_pinyin()?.plain().to_lowercase();
    if plain.is_empty() {
        None
    } else {
        Some(plain)
    }
}

/// 行尾汉字 → (尾字, 完整无调拼音音节, 十三辙名称)。
/// 公开行尾声韵分析接口（供集成测试与诊断打印：返回 (尾字, 无调拼音, 十三辙, 发音部位家族)）。
pub fn analyze_line_prosody(line: &str) -> Option<(char, String, &'static str, &'static str)> {
    let (ch, py, fam) = line_tail_prosody(line)?;
    Some((ch, py, fam, rhyme_super_group(fam)))
}

fn line_tail_prosody(line: &str) -> Option<(char, String, &'static str)> {
    let ch = last_cjk(line)?;
    let plain = contextual_pinyin_plain(line, ch)?;
    let fam = rhyme_family(extract_mandarin_final(&plain));
    Some((ch, plain, fam))
}

/// 行尾汉字 → 十三辙键；非汉字/无拼音 → None。
fn line_rhyme_key(line: &str) -> Option<String> {
    let (_, _, fam) = line_tail_prosody(line)?;
    Some(fam.to_string())
}

/// 汉语拼音韵母提取（修正 ye→ie、j/q/x/y + u/ue/uan/un→v/ve/van/vn，避免将「夜」误判为梭波或将「雨/去」误判为姑苏）。
fn extract_mandarin_final(plain: &str) -> &str {
    match plain {
        "ye" => return "ie",
        "yu" | "ju" | "qu" | "xu" => return "v",
        "yue" | "jue" | "que" | "xue" => return "ve",
        "yuan" | "juan" | "quan" | "xuan" => return "van",
        "yun" | "jun" | "qun" | "xun" => return "vn",
        "zhi" | "chi" | "shi" | "ri" | "zi" | "ci" | "si" => return "i",
        "er" => return "er",
        _ => {}
    }
    const INITIALS: &[&str] = &[
        "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
        "r", "z", "c", "s", "y", "w",
    ];
    for i in INITIALS {
        if let Some(rest) = plain.strip_prefix(i) {
            if !rest.is_empty() {
                return rest;
            }
        }
    }
    plain
}

/// 十三辙发音部位五大家族（用于“全局韵脚统一 + 允许同部位邻韵轻微跳动”判据）：
/// - 前鼻音家族(-n)：言前(an/ian/uan/üan) ↔ 人辰(en/in/un/ün)
/// - 后鼻音家族(-ng)：江阳(ang/iang/uang) ↔ 中东(eng/ing/ong/iong)
/// - 开口复合家族(a系)：发花(a/ia/ua) ↔ 怀来(ai/uai) ↔ 遥条(ao/iao)
/// - 齐撮前高家族(i/e系)：衣期(i/ü) ↔ 乜斜(ie/üe) ↔ 灰堆(ei/ui)
/// - 圆唇后元音家族(o/u系)：梭波(o/e/uo) ↔ 姑苏(u) ↔ 油求(ou/iu)
fn rhyme_super_group(fam: &str) -> &'static str {
    match fam {
        "言前" | "人辰" => "前鼻音(-n:言前/人辰)",
        "江阳" | "中东" => "后鼻音(-ng:江阳/中东)",
        "发花" | "怀来" | "遥条" => "开口韵(a系:发花/怀来/遥条)",
        "衣期" | "乜斜" | "灰堆" => "齐撮韵(i/e系:衣期/乜斜/灰堆)",
        "梭波" | "姑苏" | "油求" => "圆唇韵(o/u系:梭波/姑苏/油求)",
        _ => "其他",
    }
}

/// 十三辙归并（开口/闭口/鼻韵母严格区分）。
fn rhyme_family(finals: &str) -> &'static str {
    let f = finals.trim();
    match f {
        "a" | "ia" | "ua" => "发花",
        "o" | "e" | "uo" => "梭波",
        "ie" | "ue" | "ve" | "üe" | "er" => "乜斜",
        "u" => "姑苏",
        "i" | "v" | "ü" => "衣期",
        "ai" | "uai" => "怀来",
        "ei" | "ui" => "灰堆",
        "ao" | "iao" => "遥条",
        "ou" | "iu" => "油求",
        "an" | "ian" | "uan" | "van" | "üan" => "言前",
        "en" | "in" | "un" | "vn" | "ün" => "人辰",
        "ang" | "iang" | "uang" => "江阳",
        "eng" | "ing" | "ong" | "iong" => "中东",
        _ => "其他",
    }
}

/// v3 汉语声律与押韵硬门（Mode B 写词 / Mode C 改词 / Mode D 抖音）：
/// 1. section_rhyme_missing（Hard）：有词段落必须形成十三辙押韵闭环，且收尾合句（或偶句）必须入韵；
/// 2. rhyme_homophone_collision（Hard）：同段内非复沓/非排比句严禁同音字挤韵（如 jian/jian、tong/tong）或同字重韵；
/// 3. mode_c_rhyme_scheme & mode_c_copied_rhyme_char（Hard）：Mode C 必须严格对齐原词押韵行对，且严禁直接照搬原词尾字；
/// 4. prosody_meter_imbalance（Hard）：同段对偶句（非显式声明的 short_line）字数律动不得失衡断裂。
fn check_prosody_and_rhyme(p: &SongPlan, ctx: &ValidateCtx<'_>, out: &mut Vec<Issue>) {
    if p.mode_meta.lyrics_lang != super::LyricLang::Zh {
        return;
    }
    if !matches!(ctx.mode, "mode_b" | "mode_c" | "mode_d") {
        return;
    }
    let total_lines: usize = p
        .sections
        .iter()
        .map(|s| s.lines.iter().filter(|l| !l.trim().is_empty()).count())
        .sum();
    // 保留 2 行极简测试夹具（minimal_plan / advise_rhyme_known_samples）的独立门测试语义；
    // 任何真实生成的歌曲或 ≥3 行的改词/写词/抖音作品全部强制过声律硬门。
    if total_lines < 3 {
        return;
    }
    let exempted = has_exemption(p);
    if p.sections.len() >= 3 && total_lines >= 6 {
        if let Some(it) = &p.intent {
            if it.anchor_object.trim().is_empty()
                || it.rhyme_plan.as_deref().unwrap_or("").trim().is_empty()
            {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "intent_missing",
                    "/intent",
                    "intent.anchor_object（核心锚点物）与 intent.rhyme_plan（全局十三辙声律与句式网格统筹）不得为空",
                ));
            }
        }
    }
    let unrhymed_allowance = p
        .asymmetry
        .as_ref()
        .and_then(|a| a.deliberate_unrhymed.as_deref())
        .unwrap_or("");
    let short_line_allowance = p
        .asymmetry
        .as_ref()
        .and_then(|a| a.short_line.as_deref())
        .unwrap_or("");

    for (si, s) in p.sections.iter().enumerate() {
        let non_empty: Vec<(usize, &str)> = s
            .lines
            .iter()
            .enumerate()
            .map(|(idx, l)| (idx, l.trim()))
            .filter(|(_, l)| !l.is_empty())
            .collect();
        if non_empty.len() < 2 {
            continue;
        }
        let mut prosodies: Vec<(usize, &str, char, String, &'static str)> = Vec::new();
        for (li, l) in &non_empty {
            if let Some((ch, plain, fam)) = line_tail_prosody(l) {
                prosodies.push((*li, *l, ch, plain, fam));
            }
        }
        if prosodies.len() < 2 {
            continue;
        }

        let mut fam_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (_, _, _, _, fam) in &prosodies {
            *fam_counts.entry(*fam).or_insert(0) += 1;
        }
        let max_fam_count = fam_counts.values().copied().max().unwrap_or(0);
        let rhymed_lines_count: usize = fam_counts.values().copied().filter(|&c| c >= 2).sum();

        if matches!(ctx.mode, "mode_b" | "mode_d") {
            let n = prosodies.len();
            let (_, last_text, _, _, last_fam) = prosodies.last().unwrap();
            let last_is_deliberate = !unrhymed_allowance.is_empty()
                && (unrhymed_allowance.contains(*last_text)
                    || last_text
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                        .chars()
                        .take(4)
                        .all(|c| unrhymed_allowance.contains(c)));
            // 计算偶句（第 2、4、6 句）与末句加权主导韵部 dom_fam
            let mut weighted_fams: BTreeMap<&'static str, usize> = BTreeMap::new();
            for (k, (_, _, _, _, fam)) in prosodies.iter().enumerate() {
                let w = if k % 2 == 1 || k + 1 == n { 2 } else { 1 };
                *weighted_fams.entry(*fam).or_insert(0) += w;
            }
            let dom_fam = weighted_fams
                .iter()
                .max_by_key(|(f, c)| (**c, **f == *last_fam))
                .map(|(f, _)| *f)
                .unwrap_or(*last_fam);
            let last_rhymes = (*last_fam == dom_fam && fam_counts.get(dom_fam).copied().unwrap_or(0) >= 2) || last_is_deliberate;
            let even_lines: Vec<&'static str> = prosodies
                .iter()
                .enumerate()
                .filter(|(k, _)| k % 2 == 1)
                .map(|(_, (_, _, _, _, fam))| *fam)
                .collect();
            let even_in_dom = even_lines.iter().filter(|&&f| f == dom_fam).count();
            let even_ok = even_lines.is_empty() || even_in_dom * 2 >= even_lines.len() || last_is_deliberate;
            let density_ok = if n == 2 {
                !(kind_is_chorus_class(s.kind) || ctx.mode == "mode_d")
                    || max_fam_count >= 2
                    || last_is_deliberate
            } else if n == 3 {
                max_fam_count >= 2 && (last_rhymes || fam_counts.get(prosodies[1].4).copied().unwrap_or(0) >= 2)
            } else {
                rhymed_lines_count * 2 >= n && last_rhymes && even_ok
            };

            const WEAK_CADENCE_PARTICLES: &[char] = &['的', '吗', '呢', '吧', '啊', '呀', '啦', '呗'];
            let (_, _, last_ch, _, _) = prosodies.last().unwrap();
            if ctx.mode == "mode_b" && WEAK_CADENCE_PARTICLES.contains(last_ch) && !last_is_deliberate {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "rhyme_weak_particle",
                    format!("/sections/{si}/lines"),
                    format!(
                        "「{}」末句以轻声虚词「{}」收尾——收束落韵句必须以有分量的实词（名词/动词/形容词）入韵，严禁拿轻声虚词充当韵脚",
                        s.label, last_ch
                    ),
                ));
            }
            if !density_ok {
                let detail: Vec<String> = prosodies
                    .iter()
                    .enumerate()
                    .map(|(k, (_, _, ch, py, fam))| format!("第{}行「{}」({}/{})", k + 1, ch, py, fam))
                    .collect();
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "section_rhyme_missing",
                    format!("/sections/{si}/lines"),
                    format!(
                        "「{}」段落缺乏十三辙押韵闭环或末句脱韵（当前尾字：{}）——必须统一本段主韵部（如偶句与末句同押江阳/言前/中东/怀来/遥条/发花/灰堆等同一十三辙），重写脱韵行",
                        s.label,
                        detail.join("、")
                    ),
                ));
            }
        }

        for a_idx in 0..prosodies.len() {
            for b_idx in (a_idx + 1)..prosodies.len() {
                let (_, line_a, ch_a, ref py_a, fam_a) = prosodies[a_idx];
                let (li_b, line_b, ch_b, ref py_b, fam_b) = prosodies[b_idx];
                if fam_a != fam_b || py_a != py_b {
                    continue;
                }
                let cjk_a = cjk_chars(line_a);
                let cjk_b = cjk_chars(line_b);
                if cjk_a == cjk_b {
                    continue;
                }
                if cjk_a.len() >= 2
                    && cjk_b.len() >= 2
                    && cjk_a[cjk_a.len() - 2..] == cjk_b[cjk_b.len() - 2..]
                {
                    continue;
                }
                if ctx.mode == "mode_c" {
                    if let Some(orig_text) = ctx.original_lyrics {
                        let orig_lines = extract_c_lyric_lines(orig_text);
                        if orig_lines
                            .iter()
                            .filter_map(|ol| line_tail_prosody(ol))
                            .filter(|(_, opy, _)| opy == py_a)
                            .count()
                            >= 2
                        {
                            continue;
                        }
                    }
                }
                let reason = if ch_a == ch_b {
                    format!("同字重韵（两行均以「{ch_a}」收尾）")
                } else {
                    format!("同音挤韵（「{ch_a}」与「{ch_b}」同音均为 {py_a}）")
                };
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "rhyme_homophone_collision",
                    format!("/sections/{si}/lines/{li_b}"),
                    format!(
                        "「{}」第 {} 行与第 {} 行出现{}——同段押韵必须使用不同声母的同辙字（当前为{}辙），请把第 {} 行句尾词换为其他声母的{}辙字",
                        s.label,
                        a_idx + 1,
                        b_idx + 1,
                        reason,
                        fam_a,
                        b_idx + 1,
                        fam_a
                    ),
                ));
            }
        }

        if matches!(ctx.mode, "mode_b" | "mode_d") && non_empty.len() >= 2 {
            let lens: Vec<(usize, &str, usize)> = non_empty
                .iter()
                .filter(|(_, l)| {
                    short_line_allowance.is_empty() || !short_line_allowance.contains(*l)
                })
                .map(|(li, l)| {
                    let c = crate::commands::validator::count_line_chars_excluding_vocables(l, |ch| {
                        ch.is_ascii_punctuation() || "，。！？；：、“”‘’（）—…·".contains(ch)
                    });
                    (*li, *l, c)
                })
                .filter(|(_, _, c)| *c > 0)
                .collect();
            if lens.len() >= 2 {
                let min_l = lens.iter().map(|x| x.2).min().unwrap_or(0);
                let max_l = lens.iter().map(|x| x.2).max().unwrap_or(0);
                let max_gap = 3;
                if max_l.saturating_sub(min_l) > max_gap {
                    out.push(Issue::hard_or_exempt(
                        exempted,
                        "prosody_meter_imbalance",
                        format!("/sections/{si}/lines"),
                        format!(
                            "「{}」句式律动失衡：最长句 {} 字 vs 最短句 {} 字（差 {} 字 > {} 字上限，且未在 asymmetry.short_line 声明破格短句）——请统一本段对偶句字数与节奏切分（如 7+7、8+8 或 6+6）",
                            s.label,
                            max_l,
                            min_l,
                            max_l - min_l,
                            max_gap
                        ),
                    ));
                }
            }
        }
    }

    // ---- 5：全局韵脚统筹与跨段同职能锁死硬门（Mode B / Mode D，有词段落 ≥3 段时生效）----
    if matches!(ctx.mode, "mode_b" | "mode_d") {
        struct SecDom<'a> {
            idx: usize,
            kind: SectionKind,
            label: &'a str,
            dom_fam: &'static str,
            group: &'static str,
            line_cnt: usize,
            avg_chars: usize,
        }
        let mut sec_dom: Vec<SecDom<'_>> = Vec::new();
        for (si, s) in p.sections.iter().enumerate() {
            let lines: Vec<&str> = s.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            if lines.len() < 2 {
                continue;
            }
            let mut fc: BTreeMap<&'static str, usize> = BTreeMap::new();
            let mut last_f: Option<&'static str> = None;
            let line_cnt = lines.len();
            for (li, l) in lines.iter().enumerate() {
                if let Some((_, _, fam)) = line_tail_prosody(l) {
                    // 中文词格律：偶句（第 2、4、6 句）与末句是落韵支柱，赋予 2 倍权重识别段落主韵（精准支持 XAXA / ABCB 隔句押韵）
                    let weight = if li % 2 == 1 || li + 1 == line_cnt { 2 } else { 1 };
                    *fc.entry(fam).or_insert(0) += weight;
                    last_f = Some(fam);
                }
            }
            // 取出现次数最多的十三辙作为本段主韵部（并列时取末句收尾韵部）
            let dom_fam = fc
                .iter()
                .max_by_key(|(f, c)| (**c, Some(**f) == last_f))
                .map(|(f, _)| *f)
                .unwrap_or("其他");
            let total_chars: usize = lines
                .iter()
                .map(|l| {
                    crate::commands::validator::count_line_chars_excluding_vocables(l, |ch| {
                        ch.is_ascii_punctuation() || "，。！？；：、“”‘’（）—…·".contains(ch)
                    })
                })
                .sum();
            let avg_chars = total_chars / lines.len().max(1);
            sec_dom.push(SecDom {
                idx: si,
                kind: s.kind,
                label: s.label.as_str(),
                dom_fam,
                group: rhyme_super_group(dom_fam),
                line_cnt: lines.len(),
                avg_chars,
            });
        }

        if sec_dom.len() >= 3 {
            // G1: 全曲韵部发散硬门（global_rhyme_scatter）
            // 规则：全曲核心主韵部最多 2 个（一韵到底，或主歌A+副歌B双核）；
            // 若出现第 3 个韵部（如 Bridge/Pre-Chorus 微跳），必须属于已出现的同一发音部位家族（同部位邻韵微跳）；
            // 整首歌跨发音部位大类绝对不得超过 2 个，且不同十三辙总数不得超过 3 个。
            let distinct_fams: std::collections::BTreeSet<&'static str> =
                sec_dom.iter().map(|x| x.dom_fam).collect();
            let distinct_groups: std::collections::BTreeSet<&'static str> =
                sec_dom.iter().map(|x| x.group).collect();
            let non_bridge_fams: std::collections::BTreeSet<&'static str> = sec_dom
                .iter()
                .filter(|x| x.kind != SectionKind::Bridge)
                .map(|x| x.dom_fam)
                .collect();

            if distinct_fams.len() > 3 || distinct_groups.len() > 2 || non_bridge_fams.len() > 2 {
                let summary: Vec<String> = sec_dom
                    .iter()
                    .map(|x| format!("{}[{}·{}]", x.label, x.dom_fam, x.group))
                    .collect();
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "global_rhyme_scatter",
                    "/sections",
                    format!(
                        "全曲各段韵脚严重跳脱、缺乏全局统筹（当前各段主韵：{}，共跨 {} 个十三辙 / {} 个发音部位家族）——全曲必须遵循全局韵部架构：①「一韵到底+邻韵微跳」（全曲统一 1 个主韵部，仅 Bridge/导歌可在同一发音部位家族内微跳邻韵，如言前↔人辰、江阳↔中东）或 ②「主副双韵锁死」（全曲主歌统一 A 韵、副歌统一 B 韵，非 Bridge 段最多 2 个十三辙，全曲含 Bridge 最多 3 个十三辙且不超过 2 个发音部位家族）",
                        summary.join(" → "),
                        distinct_fams.len(),
                        distinct_groups.len()
                    ),
                ));
            }

            // G2: 同职能段落跨段韵部锁死（homologous_section_rhyme_drift）
            // 0. 流行曲式双循环结构门（Mode B）：有 ≥2 段主歌时，Verse 1 与 Verse 2 之间必须有 Chorus/Hook（禁止 V1→V2 连堆在首遍副歌前），且全曲至少包含 2 遍普通 Chorus（或 Chorus + Final Chorus 共 ≥2–3 遍）
            if ctx.mode == "mode_b" {
                let verse_indices: Vec<usize> = sec_dom
                    .iter()
                    .filter(|x| x.kind == SectionKind::Verse)
                    .map(|x| x.idx)
                    .collect();
                let chorus_indices: Vec<usize> = sec_dom
                    .iter()
                    .filter(|x| matches!(x.kind, SectionKind::Chorus | SectionKind::Hook))
                    .map(|x| x.idx)
                    .collect();
                if verse_indices.len() >= 2 {
                    let v1 = verse_indices[0];
                    let v2 = verse_indices[1];
                    let has_chorus_between = chorus_indices.iter().any(|&c| c > v1 && c < v2);
                    if !has_chorus_between {
                        out.push(Issue::hard_or_exempt(
                            exempted,
                            "song_form_imbalance",
                            "/sections",
                            "曲式结构失衡：Verse 1 与 Verse 2 直接连写，中间缺少第一遍 Chorus——标准流行曲式必须遵循「Verse 1 → (Pre-Chorus 1) → Chorus 1 → Verse 2 → (Pre-Chorus 2) → Chorus 2 → Bridge → Final Chorus → Outro」双循环结构".to_string(),
                        ));
                    }
                }
            }

            // ① 所有 Chorus / Hook 段必须 100% 锁定同一个十三辙主韵部
            let choruses: Vec<_> = sec_dom
                .iter()
                .filter(|x| matches!(x.kind, SectionKind::Chorus | SectionKind::Hook))
                .collect();
            if choruses.len() >= 2 {
                let first_c_fam = choruses[0].dom_fam;
                for c in choruses.iter().skip(1) {
                    if c.dom_fam != first_c_fam {
                        out.push(Issue::hard_or_exempt(
                            exempted,
                            "homologous_section_rhyme_drift",
                            format!("/sections/{}/lines", c.idx),
                            format!(
                                "副歌跨段跑韵：「{}」主韵为「{}」辙，但「{}」跳到了「{}」辙——全曲所有 Chorus/Hook 必须锁死同一个十三辙主韵部",
                                choruses[0].label, first_c_fam, c.label, c.dom_fam
                            ),
                        ));
                    }
                }
            }

            // ② 所有 Pre-Chorus 导歌段必须锁死同一发音部位家族，不得各自乱跳
            let pre_choruses: Vec<_> = sec_dom
                .iter()
                .filter(|x| x.kind == SectionKind::PreChorus)
                .collect();
            if pre_choruses.len() >= 2 {
                let pc0 = pre_choruses[0];
                for pc in pre_choruses.iter().skip(1) {
                    if pc.group != pc0.group {
                        out.push(Issue::hard_or_exempt(
                            exempted,
                            "homologous_section_rhyme_drift",
                            format!("/sections/{}/lines", pc.idx),
                            format!(
                                "导歌跨段韵脚跳脱：「{}」主韵为「{}」({})，但「{}」跳到了毫无亲缘的「{}」({})——Pre-Chorus 1 与 Pre-Chorus 2 唱同一套导歌旋律，必须锁死同一十三辙（或同发音部位邻韵）",
                                pc0.label, pc0.dom_fam, pc0.group, pc.label, pc.dom_fam, pc.group
                            ),
                        ));
                    }
                }
            }

            // ③ 所有 Verse 段必须锁死同一十三辙（或最多在同一发音部位邻韵家族内微跳） + G3 句式网格对齐
            let verses: Vec<_> = sec_dom
                .iter()
                .filter(|x| x.kind == SectionKind::Verse)
                .collect();
            if verses.len() >= 2 {
                let v0 = verses[0];
                for v in verses.iter().skip(1) {
                    if v.group != v0.group {
                        out.push(Issue::hard_or_exempt(
                            exempted,
                            "homologous_section_rhyme_drift",
                            format!("/sections/{}/lines", v.idx),
                            format!(
                                "主歌跨段韵脚跳脱：「{}」主韵为「{}」({})，但「{}」跳到了毫无亲缘的「{}」({})——Verse 1 与 Verse 2 唱同一套主歌旋律，必须保持同一十三辙韵部（或仅在同一发音部位邻韵内微调）",
                                v0.label, v0.dom_fam, v0.group, v.label, v.dom_fam, v.group
                            ),
                        ));
                    }
                    if v.line_cnt.abs_diff(v0.line_cnt) > 1 || v.avg_chars.abs_diff(v0.avg_chars) > 2 {
                        out.push(Issue::hard_or_exempt(
                            exempted,
                            "homologous_section_meter_drift",
                            format!("/sections/{}/lines", v.idx),
                            format!(
                                "主歌跨段格律失配：「{}」为 {} 行（均长 {} 字），而「{}」为 {} 行（均长 {} 字）——同旋律主歌段落的行数与句式字数网格必须对齐",
                                v0.label, v0.line_cnt, v0.avg_chars, v.label, v.line_cnt, v.avg_chars
                            ),
                        ));
                    }
                }
            }
        }
    }

    if ctx.mode == "mode_c" {
        if let Some(orig_raw) = ctx.original_lyrics {
            let orig_lines = extract_c_lyric_lines(orig_raw);
            let new_lines: Vec<String> = p
                .sections
                .iter()
                .flat_map(|s| s.lines.iter().map(|l| l.trim().to_string()))
                .filter(|l| !l.is_empty())
                .collect();
            let n = orig_lines.len().min(new_lines.len());
            if n >= 3 {
                let orig_p: Vec<Option<(char, String, &'static str)>> =
                    orig_lines[..n].iter().map(|l| line_tail_prosody(l)).collect();
                let new_p: Vec<Option<(char, String, &'static str)>> =
                    new_lines[..n].iter().map(|l| line_tail_prosody(l)).collect();

                const PARTICLES: &[char] = &['的', '了', '吗', '呢', '吧', '啊', '呀', '着', '过'];
                for k in 0..n {
                    if let (Some((och, _, _)), Some((nch, _, fam))) = (&orig_p[k], &new_p[k]) {
                        if och == nch
                            && !PARTICLES.contains(och)
                            && cjk_chars(&orig_lines[k]) != cjk_chars(&new_lines[k])
                        {
                            out.push(Issue::hard_or_exempt(
                                exempted,
                                "mode_c_copied_rhyme_char",
                                "/sections",
                                format!(
                                    "改词第 {} 行尾字「{}」直接照搬了原词对应行尾字（原句「{}」→ 新句「{}」）——填词必须彻底替换为新主题下的其他{}辙字，严禁留用原词韵字",
                                    k + 1,
                                    nch,
                                    orig_lines[k],
                                    new_lines[k],
                                    fam
                                ),
                            ));
                        }
                    }
                }

                let mut mismatch_count = 0usize;
                for i in 0..n {
                    for j in (i + 1)..n {
                        let (Some((_, _, ofam_i)), Some((_, _, ofam_j))) = (&orig_p[i], &orig_p[j]) else {
                            continue;
                        };
                        if ofam_i != ofam_j {
                            continue;
                        }
                        if let (Some((nch_i, npy_i, nfam_i)), Some((nch_j, npy_j, nfam_j))) =
                            (&new_p[i], &new_p[j])
                        {
                            if nfam_i != nfam_j {
                                mismatch_count += 1;
                                if mismatch_count <= 4 {
                                    out.push(Issue::hard_or_exempt(
                                        exempted,
                                        "mode_c_rhyme_scheme",
                                        "/sections",
                                        format!(
                                            "改词韵脚失配：原词第 {}/{} 行同押「{}」辙，但新词第 {} 行尾字「{}」({}[{}]) 与第 {} 行尾字「{}」({}[{}]) 不同辙——必须保持原词押韵行位置同辙对齐",
                                            i + 1,
                                            j + 1,
                                            ofam_i,
                                            i + 1,
                                            nch_i,
                                            npy_i,
                                            nfam_i,
                                            j + 1,
                                            nch_j,
                                            npy_j,
                                            nfam_j
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 是否为具象的多词英文人声/混音描述（至少 3 个英文单词，非 V-xx/M-xx ID 形态）。
fn is_descriptive_english_phrase(s: &str) -> bool {
    let t = s.trim();
    if t.starts_with("V-") || t.starts_with("M-") || t.starts_with("v-") || t.starts_with("m-") {
        return false;
    }
    let word_count = t
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|w| w.chars().any(|c| c.is_ascii_alphabetic()))
        .count();
    word_count >= 3
}

/// §2.3 `validate_vocal_mix_coverage`：global.vocal / global.mix 支持引用 vocal_mix 词表 id，
/// 或书写 ≥3 词的具象英文人声/混音短语，且各至少 1 个有效项。词表缺失（回滚=删表）→ 跳过。
pub fn check_vocal_mix_coverage(p: &SongPlan, out: &mut Vec<Issue>) {
    check_vocal_mix_coverage_with(crate::knowledge::shared_knowledge(), p, out)
}

fn check_vocal_mix_coverage_with(
    kb: &crate::knowledge::KnowledgeBase,
    p: &SongPlan,
    out: &mut Vec<Issue>,
) {
    let Some(rows) = crate::knowledge::vocal_mix_entries(kb) else {
        return;
    };
    for (path, field, cat) in [
        ("/global/vocal", &p.global.vocal, "vocal"),
        ("/global/mix", &p.global.mix, "mix"),
    ] {
        let mut hit = 0usize;
        for id in field {
            let trimmed = id.trim();
            match rows.iter().find(|e| e.id == trimmed) {
                Some(e) if e.category == cat => hit += 1,
                Some(e) => out.push(Issue::hard(
                    "vocal_mix_category",
                    path,
                    format!("「{id}」属 {} 类，不能填在 {cat} 字段", e.category),
                )),
                None if is_descriptive_english_phrase(trimmed) => {
                    hit += 1;
                }
                None => out.push(Issue::hard(
                    "vocal_mix_unknown_id",
                    path,
                    format!(
                        "「{id}」不是有效的 vocal_mix 词表 id（可用：{}），亦非 ≥3 词的具象英文描述短语",
                        ids_of(&rows, cat)
                    ),
                )),
            }
        }
        if hit == 0 {
            out.push(Issue::hard(
                "vocal_mix_coverage",
                path,
                format!(
                    "至少引用 1 个 {cat} 词表 id（可用：{}）或填写 ≥3 词的具象英文描述",
                    ids_of(&rows, cat)
                ),
            ));
        }
    }
}

fn ids_of(rows: &[crate::knowledge::VocalMixEntry], cat: &str) -> String {
    rows.iter()
        .filter(|e| e.category == cat)
        .map(|e| e.id.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn check_budget(p: &SongPlan, out: &mut Vec<Issue>) {
    if p.schema_version != SCHEMA_VERSION {
        out.push(Issue::hard(
            "schema_version",
            "/schema_version",
            format!("须为 {SCHEMA_VERSION}"),
        ));
    }
    let r = render(p);
    let style_len = r.style.chars().count();
    if style_len > lim(SUNO_STYLE_MAX) {
        out.push(Issue::hard(
            "style_too_long",
            "/global",
            format!("Style {style_len} 字符 > 上限 {}", lim(SUNO_STYLE_MAX)),
        ));
    }
    let lyr_len = r.lyrics.chars().count();
    if lyr_len > lim(SUNO_LYRICS_MAX) {
        out.push(Issue::hard(
            "lyrics_too_long",
            "/sections",
            format!("Lyrics {lyr_len} 字符 > 上限 {}", lim(SUNO_LYRICS_MAX)),
        ));
    }
    let ex_len = r.exclude.chars().count();
    if ex_len > lim(SUNO_EXCLUDE_MAX) {
        out.push(Issue::hard(
            "exclude_too_long",
            "/global/negative",
            format!("Exclude {ex_len} 字符 > 上限 {}", lim(SUNO_EXCLUDE_MAX)),
        ));
    }
    let n = style_tags(p).len();
    if n < lim(SUNO_STYLE_TAGS_MIN) || n > lim(SUNO_STYLE_TAGS_MAX) {
        out.push(Issue::warn(
            "style_tag_count",
            "/global",
            format!(
                "Style 标签 {n} 个，建议 {}–{}",
                lim(SUNO_STYLE_TAGS_MIN),
                lim(SUNO_STYLE_TAGS_MAX)
            ),
        ));
    }
}

fn check_settings(p: &SongPlan, out: &mut Vec<Issue>) {
    if usize::from(p.settings.variety) != lim(VARIETY_DEFAULT) {
        out.push(Issue::hard(
            "variety",
            "/settings/variety",
            format!("Variety 须为 {}", lim(VARIETY_DEFAULT)),
        ));
    }
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

fn is_negative_phrase(s: &str) -> bool {
    let l = norm(s);
    l.starts_with("no ")
        || l.starts_with("without ")
        || l.starts_with("不要")
        || l.starts_with("无")
}

/// 极性：负向词不得出现在正向层；正向层不得写否定句（应进 negative / negative_local）
fn check_polarity(p: &SongPlan, out: &mut Vec<Issue>) {
    let g = &p.global;
    let mut positive: Vec<(&str, &String)> = g
        .genre
        .iter()
        .map(|x| ("/global/genre", x))
        .chain(g.vocal.iter().map(|x| ("/global/vocal", x)))
        .chain(g.instruments.iter().map(|x| ("/global/instruments", x)))
        .chain(g.mix.iter().map(|x| ("/global/mix", x)))
        .collect();
    if let Some(m) = &g.mood {
        positive.push(("/global/mood", m));
    }
    if let Some(h) = &g.arrangement_hook {
        positive.push(("/global/arrangement_hook", h));
    }
    for (path, v) in &positive {
        if is_negative_phrase(v) {
            out.push(Issue::hard(
                "polarity",
                *path,
                format!("正向层出现否定项「{v}」，应移入 /global/negative"),
            ));
        }
        if g.negative.iter().any(|n| norm(n) == norm(v)) {
            out.push(Issue::hard(
                "polarity",
                *path,
                format!("「{v}」同时出现在正向层与 /global/negative"),
            ));
        }
    }
    for n in &g.negative {
        if is_negative_phrase(n) {
            out.push(Issue::warn(
                "polarity",
                "/global/negative",
                format!("Exclude 项「{n}」不需要否定词，直接写要排除的元素"),
            ));
        }
    }
    for (i, s) in p.sections.iter().enumerate() {
        for (j, c) in s.positive_local.iter().enumerate() {
            if is_negative_phrase(c) {
                out.push(Issue::hard(
                    "polarity",
                    format!("/sections/{i}/positive_local/{j}"),
                    format!("否定 cue「{c}」应移入 negative_local"),
                ));
            }
        }
        for (j, c) in s.instruments.iter().enumerate() {
            if is_negative_phrase(c) {
                out.push(Issue::hard(
                    "polarity",
                    format!("/sections/{i}/instruments/{j}"),
                    format!("instruments 只放在场乐器，「{c}」应移入 negative_local"),
                ));
            }
        }
    }
}

fn kind_allows_minimal(k: SectionKind) -> bool {
    matches!(
        k,
        SectionKind::Intro | SectionKind::Outro | SectionKind::Bridge | SectionKind::Interlude
    )
}

fn kind_is_chorus_class(k: SectionKind) -> bool {
    matches!(
        k,
        SectionKind::Chorus | SectionKind::Hook | SectionKind::PostChorus | SectionKind::Drop
    )
}

fn check_structure(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    let exempted = has_exemption(p);
    if p.sections.is_empty()
        || (matches!(mode, "mode_a" | "mode_b") && p.sections.len() < rules::MIN_SECTION_TAGS)
    {
        out.push(Issue::hard(
            "section_count",
            "/sections",
            format!(
                "段落 {} 个 < 下限 {}",
                p.sections.len(),
                rules::MIN_SECTION_TAGS
            ),
        ));
    }
    for (i, s) in p.sections.iter().enumerate() {
        if s.label.trim().is_empty() {
            out.push(Issue::hard(
                "section_label",
                format!("/sections/{i}/label"),
                "段名不能为空",
            ));
        }
        if s.minimal {
            let max_e = if s.kind == SectionKind::Bridge {
                rules::MINIMAL_ENERGY_MAX + 1
            } else {
                rules::MINIMAL_ENERGY_MAX
            };
            let energy_ok = s
                .energy
                .is_some_and(|e| u32::from(e) <= max_e);
            if !kind_allows_minimal(s.kind) || !energy_ok {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "minimal_misuse",
                    format!("/sections/{i}/minimal"),
                    format!(
                        "极简段只允许 intro/outro/bridge/interlude 且 energy ≤{}",
                        rules::MINIMAL_ENERGY_MAX
                    ),
                ));
            }
        }
        let distinct: std::collections::BTreeSet<String> =
            s.instruments.iter().map(|x| norm(x)).collect();
        if distinct.len() != s.instruments.len() {
            out.push(Issue::warn(
                "instrument_dup",
                format!("/sections/{i}/instruments"),
                "段内乐器重复",
            ));
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
                format!(
                    "「{}」段头 {n} 字符 > 上限 {}——须再删 ≥{} 字符（段头 = 标签 + positive_local + instruments + energy N 拼接）：压缩 positive_local 短语，保留核心变化量，删冗余修饰",
                    s.label,
                    rules::DESC_LINE_MAX_CHARS,
                    n - rules::DESC_LINE_MAX_CHARS
                ),
            ));
        }
    }
}

fn narrative_sections(p: &SongPlan) -> usize {
    p.sections
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SectionKind::Verse | SectionKind::PreChorus | SectionKind::Bridge
            )
        })
        .count()
}

/// 参数区间（A=弧线区间并集 / B=专属区间 / D=抖音区间+叙事型例外；exemptions 非空时可豁免为 Warn）、Audio Influence、BPM
fn check_params(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    let exempted = has_exemption(p);
    let w = u32::from(p.settings.weirdness);
    let s = u32::from(p.settings.style_influence);
    match mode {
        "mode_a" => {
            let (wlo, whi, slo, shi) = rules::arc_param_union();
            if !(wlo..=whi).contains(&w) || !(slo..=shi).contains(&s) {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "param_range",
                    "/settings",
                    format!(
                        "Weirdness={w}/Style Influence={s} 越出弧线区间并集 {wlo}-{whi}/{slo}-{shi}"
                    ),
                ));
            }
        }
        "mode_b" => {
            if !(rules::MODE_B_WEIRD_MIN..=rules::MODE_B_WEIRD_MAX).contains(&w) {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "param_range",
                    "/settings/weirdness",
                    format!(
                        "Weirdness={w}（B 专属区间 {}-{}）",
                        rules::MODE_B_WEIRD_MIN,
                        rules::MODE_B_WEIRD_MAX
                    ),
                ));
            }
            if !(rules::MODE_B_STYLE_MIN..=rules::MODE_B_STYLE_MAX).contains(&s) {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "param_range",
                    "/settings/style_influence",
                    format!(
                        "Style Influence={s}（B 专属区间 {}-{}）",
                        rules::MODE_B_STYLE_MIN,
                        rules::MODE_B_STYLE_MAX
                    ),
                ));
            }
        }
        "mode_d" => {
            let in_douyin = (rules::DOUYIN_V2_WEIRD_MIN..=rules::DOUYIN_V2_WEIRD_MAX).contains(&w)
                && (rules::DOUYIN_V2_STYLE_MIN..=rules::DOUYIN_V2_STYLE_MAX).contains(&s);
            let narrative = narrative_sections(p) >= rules::NARRATIVE_SECTIONS_MIN
                && p.notes.as_deref().is_some_and(|n| n.contains("叙事型"));
            if !in_douyin && !narrative {
                out.push(Issue::hard_or_exempt(
                    exempted,
                    "param_range",
                    "/settings",
                    format!(
                        "Weirdness={w}/Style Influence={s}（抖音区间 {}-{}/{}-{}；仅当含≥{}个叙事段且 notes 写明'叙事型'方可回落 A/B 区间）",
                        rules::DOUYIN_V2_WEIRD_MIN, rules::DOUYIN_V2_WEIRD_MAX, rules::DOUYIN_V2_STYLE_MIN, rules::DOUYIN_V2_STYLE_MAX, rules::NARRATIVE_SECTIONS_MIN
                    ),
                ));
            }
        }
        _ => {}
    }
    // P1-9 / M-12：默认关闭参考音频 → 非 0 仍硬拦；reference_audio=true 时允许
    if matches!(mode, "mode_a" | "mode_b")
        && p.settings.audio_influence.is_some_and(|a| a != 0)
        && !p.settings.reference_audio
    {
        out.push(Issue::hard(
            "audio_influence",
            "/settings/audio_influence",
            "未开启 reference_audio 时 Audio Influence 须为 0 或 null（P1-9：可选参考音频模式）",
        ));
    }
    if let Some(b) = p.global.bpm {
        if let Some(m) = crate::commands::validator::check_bpm_range(mode, u32::from(b)) {
            out.push(Issue::hard("bpm", "/global/bpm", m));
        }
    } else if mode == "mode_d" {
        out.push(Issue::hard(
            "bpm",
            "/global/bpm",
            format!("抖音模式须给 BPM（≥{}）", rules::DOUYIN_V2_BPM_MIN),
        ));
    }
}

/// 歌词行：A/B 禁 `/` `、` 断句；全模式禁将 `[...]` 结构标签或圆括号 `(舞台/器乐指示)` 塞进歌词行；D 每行 ≤DOUYIN_V2_LINE_MAX 字（和音虚词不计）
fn check_lyric_lines(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    const STAGE_PAREN_KEYWORDS: &[&str] = &[
        "独奏",
        "间奏",
        "前奏",
        "尾奏",
        "伴奏",
        "停顿",
        "骤停",
        "渐弱",
        "渐强",
        "旁白",
        "念白",
        "低语",
        "气声",
        "升调",
        "鼓点",
        "吉他",
        "钢琴",
        "弦乐",
        "贝斯",
        "唢呐",
        "古筝",
        "二胡",
        "笛子",
        "合成器",
        "混响",
        "抽空",
        "solo",
        "instrumental",
        "break",
        "drums",
        "guitar",
        "piano",
        "bass",
        "synth",
        "strings",
        "whisper",
        "spoken",
        "fade",
        "drop",
        "cut",
        "pause",
        "silence",
        "reverb",
        "chorus",
        "verse",
        "bridge",
        "intro",
        "outro",
    ];
    for (i, s) in p.sections.iter().enumerate() {
        for (j, l) in s.lines.iter().enumerate() {
            let path = format!("/sections/{i}/lines/{j}");
            let trimmed = l.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed != "[spoken]" {
                out.push(Issue::hard(
                    "lyric_bracket_leak",
                    path.clone(),
                    format!("歌词行内不得混入结构标签或方括号说明行（应填在 label / positive_local）：{l}"),
                ));
            }
            // v6.2 硬门：Suno 会把歌词框圆括号 (...) 内的舞台指示/配器动作直接当成歌词唱出来！
            // 仅允许简短和声回声（如 "(oh yeah)" / "(不退)" / "(x2)"），严禁整行圆括号说明或含器乐/舞台调度词。
            if matches!(mode, "mode_b" | "mode_c" | "mode_d") {
                let norm_paren = trimmed.replace('（', "(").replace('）', ")");
                if let (Some(open), Some(close)) = (norm_paren.find('('), norm_paren.rfind(')')) {
                    if close > open {
                        let inner = norm_paren[open + 1..close].trim().to_lowercase();
                        let whole_line_paren =
                            norm_paren.starts_with('(') && norm_paren.ends_with(')');
                        let is_repeat_mark = inner.starts_with('x')
                            && inner[1..].chars().all(|c| c.is_ascii_digit());
                        let has_stage_kw = STAGE_PAREN_KEYWORDS
                            .iter()
                            .any(|kw| inner.contains(kw));
                        if has_stage_kw || (whole_line_paren && !is_repeat_mark && inner.chars().count() > 4) {
                            out.push(Issue::hard(
                                "lyric_paren_direction",
                                path.clone(),
                                format!(
                                    "歌词行内严禁混入圆括号舞台/配器指示「{l}」（Suno v6 会把圆括号文字当成歌词直接唱出！请将配器/奏法/人声指示移入本段 positive_local 或 subtraction，歌词行只留真正要唱的字词）"
                                ),
                            ));
                        }
                    }
                }
            }
            if matches!(mode, "mode_a" | "mode_b") && (l.contains('/') || l.contains('、')) {
                out.push(Issue::hard(
                    "break_mark",
                    path.clone(),
                    format!("禁用断句符号（/ 或 、），请直接分行或自然连写：{l}"),
                ));
            }
            if mode == "mode_d" {
                let n = crate::commands::validator::count_line_chars_excluding_vocables(l, |c| {
                    c.is_ascii_punctuation()
                });
                if n > rules::DOUYIN_V2_LINE_MAX_CHARS {
                    out.push(Issue::hard(
                        "line_too_long",
                        path,
                        format!("{n} 字 > 上限 {}：{l}", rules::DOUYIN_V2_LINE_MAX_CHARS),
                    ));
                }
            }
        }
    }
}

/// v3/v6.2 去机器味与平台防坑诊断（Anti-AI-Tell & Suno v6 Guardrails）
fn check_anti_ai_tells(p: &SongPlan, mode: &str, out: &mut Vec<Issue>) {
    let exempted = has_exemption(p);

    // A1: 空洞形容词拦截 + 跨界伪视觉词拦截 + 真实艺人名泄漏拦截
    const EMPTY_ADJS: &[&str] = &[
        "epic",
        "emotional",
        "beautiful",
        "amazing",
        "masterpiece",
        "perfect",
        "louder",
    ];
    const VISUAL_HALLUCINATIONS: &[&str] = &[
        "4k",
        "8k",
        "1080p",
        "uhd",
        "widescreen",
        "color palette",
        "lossless video",
    ];
    const BLOCKED_ARTISTS: &[&str] = &[
        "taylor swift",
        "the weeknd",
        "daft punk",
        "ed sheeran",
        "billie eilish",
        "ariana grande",
        "kendrick lamar",
        "eminem",
        "travis scott",
        "kanye west",
        "post malone",
        "bruno mars",
        "coldplay",
        "imagine dragons",
        "linkin park",
        "metallica",
        "nirvana",
        "radiohead",
        "pink floyd",
        "hans zimmer",
        "blackpink",
        "newjeans",
        "stray kids",
        "jay chou",
        "eason chan",
        "jj lin",
    ];
    let mut style_fields: Vec<(String, &str)> = Vec::new();
    if let Some(m) = &p.global.mood {
        style_fields.push(("/global/mood".into(), m.as_str()));
    }
    for (i, g) in p.global.genre.iter().enumerate() {
        style_fields.push((format!("/global/genre/{i}"), g.as_str()));
    }
    for (i, inst) in p.global.instruments.iter().enumerate() {
        style_fields.push((format!("/global/instruments/{i}"), inst.as_str()));
    }
    if let Some(a) = &p.global.arc {
        style_fields.push(("/global/arc".into(), a.as_str()));
    }
    for (si, s) in p.sections.iter().enumerate() {
        for (pi, c) in s.positive_local.iter().enumerate() {
            style_fields.push((format!("/sections/{si}/positive_local/{pi}"), c.as_str()));
        }
    }
    for (path, text) in style_fields {
        let lower = text.to_ascii_lowercase();
        for word in text.split(|c: char| !c.is_ascii_alphabetic()) {
            let wl = word.to_ascii_lowercase();
            if EMPTY_ADJS.contains(&wl.as_str()) {
                out.push(Issue::warn(
                    "style_empty_adjective",
                    path.clone(),
                    format!("含空洞修饰词「{word}」（Suno 无法将其转化为声学参数，建议改为具体奏法、音色或空间描述）"),
                ));
                break;
            }
        }
        for vh in VISUAL_HALLUCINATIONS {
            let hit = if vh.contains(' ') {
                lower.contains(vh)
            } else {
                lower
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|tok| tok == *vh)
            };
            if hit {
                out.push(Issue::warn(
                    "style_visual_hallucination",
                    path.clone(),
                    format!(
                        "含绘画/视频跨界伪提示词「{vh}」（Suno 声学模型无法解析视觉分辨率或画幅词，请改用 M-04/M-05 等分轨均衡与母带动态标签）"
                    ),
                ));
                break;
            }
        }
        let has_like_pattern =
            lower.contains("in the style of ") || lower.contains("sounds like ");
        let matched_artist = BLOCKED_ARTISTS.iter().copied().find(|a| lower.contains(a));
        if has_like_pattern || matched_artist.is_some() {
            let culprit = matched_artist.unwrap_or("in the style of / sounds like");
            out.push(Issue::hard_or_exempt(
                exempted,
                "style_artist_name_leak",
                path.clone(),
                format!(
                    "风格或段落提示含真实艺人引用「{culprit}」——Suno v6 会拦截真实艺人名并在后台强制重写整个 Style 框！请将其拆解为四维声学特征（年代流派 + 音域与声带颗粒度 + 标志性律动配器 + 混音空间），严禁直接写艺人名"
                ),
            ));
        }
    }

    // A1b: 参考音频模式参数联动诊断（Cover / Voice 模式下 Variety 须为 0 且 Audio Influence 建议 70-85）
    if p.settings.reference_audio {
        let ai = p.settings.audio_influence.unwrap_or(0);
        if p.settings.variety != 0 || ai < 65 {
            out.push(Issue::warn(
                "reference_audio_slider_mismatch",
                "/settings",
                format!(
                    "已开启 reference_audio（当前 Variety={}, Audio Influence={}）：Suno v6 下翻唱/锁声线建议保持 Variety=0 且将 Audio Influence 设为 70–85，否则原曲旋律或克隆声线易漂移",
                    p.settings.variety, ai
                ),
            ));
        }
    }

    // A2: 能量单调递增无回落检查（仅 A/B 模式且 ≥4 段）
    if matches!(mode, "mode_a" | "mode_b") && p.sections.len() >= 4 {
        let energies: Vec<u8> = p.sections.iter().filter_map(|s| s.energy).collect();
        if energies.len() == p.sections.len() {
            let non_decreasing = energies.windows(2).all(|w| w[0] <= w[1]);
            let has_growth = energies.first() < energies.last();
            let has_sub = p
                .sections
                .iter()
                .any(|s| s.minimal || s.subtraction.as_ref().is_some_and(|x| !x.trim().is_empty()));
            if non_decreasing && has_growth && !has_sub && !exempted {
                out.push(Issue::warn(
                    "energy_monotonic",
                    "/sections",
                    "能量曲线全程单调递增无回落：建议在高潮前设置至少一处能量回落或减法抽空（subtraction），制造动态反差",
                ));
            }
        }
    }

    // A3: 末遍副歌完全雷同检查（≥2 个有词 Chorus 段，首末 Chorus 的词、局部 cue 与配器完全一致）
    let choruses: Vec<&super::Section> = p
        .sections
        .iter()
        .filter(|s| s.kind == SectionKind::Chorus && !s.lines.is_empty())
        .collect();
    if choruses.len() >= 2 {
        let first = choruses.first().unwrap();
        let last = choruses.last().unwrap();
        let has_var_note = p
            .asymmetry
            .as_ref()
            .and_then(|a| a.chorus_variation.as_ref())
            .is_some_and(|v| !v.trim().is_empty());
        if first.lines == last.lines
            && first.positive_local == last.positive_local
            && first.instruments == last.instruments
            && last.subtraction.is_none()
            && !has_var_note
            && !exempted
        {
            out.push(Issue::warn(
                "chorus_static_repeat",
                "/sections",
                "末遍副歌与首遍副歌的词、局部提示与配器完全雷同：建议在末遍副歌做热点词微变、八度和声叠加、半音升调（key modulation up a half-step）或前两拍抽空反差",
            ));
        }
    }

    // A4 & A5 & A6: 中文歌词抽象套话、比喻自解（Class 2）、励志套语（Class 5）、主副歌交界回声（Verse-Chorus Echo）与声律检查
    if p.mode_meta.lyrics_lang == super::LyricLang::Zh && !exempted {
        const ABSTRACT_CLICHES: &[&str] = &[
            "时光", "岁月", "远方", "星辰", "余生", "救赎", "宿命", "深渊", "彼岸", "永恒",
        ];
        const CLICHE_ESCALATIONS: &[&str] = &[
            "冲破枷锁",
            "逆风飞翔",
            "重写命运",
            "照亮黑暗",
            "撕碎沉默",
            "打破宿命",
            "穿越黑暗",
        ];
        const METAPHOR_DECODE_MARKERS: &[&str] = &[
            "就像我们的",
            "代表我们的",
            "说明我们",
            "意味着我们",
            "正如同我们",
            "象征着我们",
        ];
        let mut hit_cliches: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut hit_escalations: std::collections::BTreeSet<&str> =
            std::collections::BTreeSet::new();
        for (si, s) in p.sections.iter().enumerate() {
            for (li, l) in s.lines.iter().enumerate() {
                for &cl in ABSTRACT_CLICHES {
                    if l.contains(cl) {
                        hit_cliches.insert(cl);
                    }
                }
                for &esc in CLICHE_ESCALATIONS {
                    if l.contains(esc) {
                        hit_escalations.insert(esc);
                    }
                }
                for &md in METAPHOR_DECODE_MARKERS {
                    if l.contains(md) {
                        out.push(Issue::warn(
                            "lyric_metaphor_decoded",
                            format!("/sections/{si}/lines/{li}"),
                            format!(
                                "歌词出现「比喻自解/破喻」句式（含「{md}」）：写物即写情，请直接保留具象动作或物象，删掉向听众解释比喻含义的说明文"
                            ),
                        ));
                        break;
                    }
                }
            }
        }
        if hit_cliches.len() >= 2 {
            let list: Vec<&str> = hit_cliches.into_iter().collect();
            out.push(Issue::warn(
                "lyric_abstract_cliche",
                "/sections",
                format!(
                    "歌词出现高频抽象套话词（{}）：建议落实到可拍照的具体锚点物（anchor_object）与动作细节",
                    list.join("、")
                ),
            ));
        }
        if !hit_escalations.is_empty() {
            let list: Vec<&str> = hit_escalations.into_iter().collect();
            out.push(Issue::warn(
                "lyric_cliche_escalation",
                "/sections",
                format!(
                    "歌词出现 AI 高频励志口号套语（{}）：建议改为人物在具体场景下的真实动作或克制独白（LC-18）",
                    list.join("、")
                ),
            ));
        }
        // 主副歌交界回声检查（Verse-Chorus Echo）：主歌/导歌末句不应与紧随的副歌首句同字撞韵或提前剧透副歌核心句
        for win in p.sections.windows(2) {
            let prev = &win[0];
            let next = &win[1];
            if matches!(prev.kind, SectionKind::Verse | SectionKind::PreChorus)
                && kind_is_chorus_class(next.kind)
            {
                let prev_last = prev.lines.iter().rev().map(|s| s.trim()).find(|s| !s.is_empty());
                let next_first = next.lines.iter().map(|s| s.trim()).find(|s| !s.is_empty());
                if let (Some(pl), Some(nf)) = (prev_last, next_first) {
                    let cjk_p = cjk_chars(pl);
                    let cjk_n = cjk_chars(nf);
                    if !cjk_p.is_empty() && !cjk_n.is_empty() && cjk_p != cjk_n {
                        if let (Some(ch_p), Some(ch_n)) = (last_cjk(pl), last_cjk(nf)) {
                            if ch_p == ch_n {
                                out.push(Issue::warn(
                                    "verse_chorus_echo",
                                    "/sections",
                                    format!(
                                        "「{}」末句与紧随的「{}」首句均以同一个「{ch_p}」字收尾（主副歌交界同字回声会削弱副歌进入时的爆发反差，建议将导歌/主歌末句换为同辙其他字做蓄力铺垫）",
                                        prev.label, next.label
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        // 检查非复沓句中是否存在同一个汉字被反复当作韵脚 ≥4 次（全曲统一押同一十三辙是优良全局统筹，但不能反复拿同一个字凑韵）
        let mut unique_lines: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut tail_char_counts: BTreeMap<char, usize> = BTreeMap::new();
        for s in &p.sections {
            for l in &s.lines {
                let norm: String = cjk_chars(l).into_iter().collect();
                if norm.is_empty() || !unique_lines.insert(norm) {
                    continue;
                }
                if let Some(ch) = last_cjk(l) {
                    *tail_char_counts.entry(ch).or_insert(0) += 1;
                }
            }
        }
        for (ch, count) in &tail_char_counts {
            if *count >= 4 {
                out.push(Issue::warn(
                    "rhyme_char_overuse",
                    "/sections",
                    format!(
                        "全曲不同歌词行中有 {count} 行重复以同一个「{ch}」字收尾：全局统押同一十三辙时，建议更换同辙内的不同汉字以丰富咬字层次"
                    ),
                ));
            }
        }
        // 检查最高能量的 Chorus/Hook 末行是否落闭口韵（衣期/姑苏）
        if let Some((idx, peak_sec)) = p
            .sections
            .iter()
            .enumerate()
            .filter(|(_, s)| kind_is_chorus_class(s.kind) && !s.lines.is_empty())
            .max_by_key(|(_, s)| s.energy.unwrap_or(5))
        {
            if let Some(last_line) = peak_sec.lines.last() {
                if let Some(rk) = line_rhyme_key(last_line) {
                    if matches!(rk.as_str(), "衣期" | "姑苏") {
                        out.push(Issue::warn(
                            "peak_closed_vowel",
                            format!("/sections/{idx}/lines"),
                            format!(
                                "高潮段「{}」收尾句落闭口韵（{rk}辙），高音易憋嗓：建议改用发花/江阳/怀来/遥条/言前等开口洪亮韵部",
                                peak_sec.label
                            ),
                        ));
                    }
                }
            }
        }
    }
}

/// D 模式结构门（原 `validate_douyin` 迁 IR）：Hook 次数、Verse 行数、结尾骤停
fn check_douyin(p: &SongPlan, out: &mut Vec<Issue>) {
    crate::plan::douyin::validate_douyin_v2(p, out);
}

/// 段下限（配器门归因结论）：声明极简 1 / Chorus 类 3 / 其余 2
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
    let exempted = has_exemption(p);
    let mut energies = Vec::new();
    for (i, s) in p.sections.iter().enumerate() {
        match s.energy {
            Some(e) => energies.push((i, u32::from(e))),
            None => out.push(Issue::hard(
                "energy_missing",
                format!("/sections/{i}/energy"),
                "A/B 模式每段须标能量 1–10",
            )),
        }
    }
    if let (Some(min), Some(max)) = (
        energies.iter().map(|x| x.1).min(),
        energies.iter().map(|x| x.1).max(),
    ) {
        if max - min < rules::MIN_ENERGY_GAP {
            out.push(Issue::hard_or_exempt(
                exempted,
                "energy_gap",
                "/sections",
                format!("能量差 {} < 下限 {}", max - min, rules::MIN_ENERGY_GAP),
            ));
        }
    }
    let global_n = p
        .global
        .instruments
        .iter()
        .map(|x| norm(x))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if global_n > rules::INSTRUMENT_MAX {
        out.push(Issue::hard(
            "instrument_max",
            "/global/instruments",
            format!("全曲乐器 {global_n} 件 > 上限 {}", rules::INSTRUMENT_MAX),
        ));
    }
    for (i, s) in p.sections.iter().enumerate() {
        let n = s.instruments.len();
        let floor = section_floor(s);
        if n < floor {
            out.push(Issue::hard_or_exempt(
                exempted,
                "instrument_floor",
                format!("/sections/{i}/instruments"),
                format!("「{}」{n} 件 < 段下限 {floor}", s.label),
            ));
        }
        if n > rules::INSTRUMENT_MAX {
            out.push(Issue::hard(
                "instrument_max",
                format!("/sections/{i}/instruments"),
                format!("「{}」{n} 件 > 上限 {}", s.label, rules::INSTRUMENT_MAX),
            ));
        }
    }
    // 最强段 = 能量最高段（并列取件数最多者）；最弱段 = 能量最低段（并列取件数最少者）
    let pick = |want_max: bool| {
        energies
            .iter()
            .map(|&(i, e)| (i, e, p.sections[i].instruments.len()))
            .reduce(|a, b| {
                let better = if want_max {
                    (b.1, b.2) > (a.1, a.2)
                } else {
                    (b.1, std::cmp::Reverse(b.2)) < (a.1, std::cmp::Reverse(a.2))
                };
                if better {
                    b
                } else {
                    a
                }
            })
    };
    if let (Some(strong), Some(weak)) = (pick(true), pick(false)) {
        if strong.2 < rules::MIN_INSTRUMENT_STRONG {
            out.push(Issue::hard_or_exempt(
                exempted,
                "instrument_strong",
                format!("/sections/{}/instruments", strong.0),
                format!(
                    "最强段「{}」{} 件 < 下限 {}",
                    p.sections[strong.0].label,
                    strong.2,
                    rules::MIN_INSTRUMENT_STRONG
                ),
            ));
        }
        if strong.0 != weak.0 && strong.2 < weak.2 + rules::MIN_INSTRUMENT_GAP {
            out.push(Issue::hard_or_exempt(
                exempted,
                "instrument_gap",
                "/sections",
                format!(
                    "最强段 {} 件 − 最弱段 {} 件 < 配器差下限 {}",
                    strong.2,
                    weak.2,
                    rules::MIN_INSTRUMENT_GAP
                ),
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
            out.push(Issue::hard(
                "fidelity_missing",
                "/sections",
                format!("收敛方案歌词行缺失 ×{}：{k}", n - m),
            ));
        }
    }
    for (k, n) in &b {
        let m = a.get(k).copied().unwrap_or(0);
        if *n > m {
            out.push(Issue::hard(
                "fidelity_added",
                "/sections",
                format!("终稿新增方案外歌词行 ×{}：{k}", n - m),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
/// §2.3 / §3.8 `validate_lang`：歌词行语言 ↔ `mode_meta.lyrics_lang`（= 请求 output_lang，后端盖写）。
/// 只读 `sections[].lines`；Style/Exclude 归 §3.7（`style_lang::validate_style_english`），本门不读。
/// - En：任一歌词行含 CJK → Hard（逐行定位）。
/// - Zh：全曲无一行含 CJK → Hard（英文和声/拟声行允许混入，整首英文不允许）。
pub fn validate_lang(p: &SongPlan) -> Vec<Issue> {
    let is_cjk = |c: char| {
        matches!(
            c,
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
        )
    };
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut cjk_lines = 0usize;
    for (i, s) in p.sections.iter().enumerate() {
        for (j, l) in s.lines.iter().enumerate() {
            let l = l.trim();
            if !l.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            total += 1;
            let has = l.chars().any(is_cjk);
            if has {
                cjk_lines += 1;
            }
            if has && p.mode_meta.lyrics_lang == super::LyricLang::En {
                out.push(Issue::hard(
                    "lyrics_lang_mismatch",
                    format!("/sections/{i}/lines/{j}"),
                    format!("歌词语言=en，但该行含中文「{l}」"),
                ));
            }
        }
    }
    if p.mode_meta.lyrics_lang == super::LyricLang::Zh && total > 0 && cjk_lines == 0 {
        out.push(Issue::hard(
            "lyrics_lang_mismatch",
            "/sections",
            "歌词语言=zh，但全曲歌词无中文行",
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::fixtures::minimal_plan;
    use crate::plan::{Section, SectionKind};

    fn ctx(mode: &str) -> ValidateCtx<'_> {
        ValidateCtx {
            mode,
            converged: None,
            original_lyrics: None,
        }
    }

    fn codes(v: &[Issue]) -> Vec<&'static str> {
        hard_issues(v).iter().map(|i| i.code).collect()
    }

    fn sec(kind: SectionKind, energy: u8, n: usize, minimal: bool) -> Section {
        Section {
            kind,
            label: format!("{kind:?}"),
            motion: None,
            positive_local: vec![],
            negative_local: vec![],
            subtraction: None,
            energy: Some(energy),
            instruments: ["guzheng", "cello", "dizi", "pipa", "erhu", "piano", "drums"][..n]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            minimal,
            target_seconds: None,
            lines: vec![],
        }
    }

    fn ab_plan(sections: Vec<Section>) -> SongPlan {
        let mut p = minimal_plan();
        p.global.instruments = ["guzheng", "cello", "dizi", "pipa", "erhu", "piano", "drums"]
            .iter()
            .map(|s| s.to_string())
            .collect();
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
        let ok = ab_plan(vec![
            sec(SectionKind::Intro, 1, 1, true),
            sec(SectionKind::Verse, 4, 2, false),
            sec(SectionKind::Chorus, 8, 4, false),
        ]);
        assert!(
            passed(&validate(&ok, &ctx("mode_a"))),
            "{:?}",
            validate(&ok, &ctx("mode_a"))
        );
        let bad = ab_plan(vec![
            sec(SectionKind::Verse, 1, 1, true),
            sec(SectionKind::Chorus, 8, 4, false),
        ]);
        let c = codes(&validate(&bad, &ctx("mode_a")));
        assert!(
            c.contains(&"minimal_misuse") && c.contains(&"instrument_floor"),
            "{c:?}"
        );
        let loud = ab_plan(vec![
            sec(SectionKind::Intro, 3, 1, true),
            sec(SectionKind::Chorus, 8, 4, false),
        ]);
        assert!(codes(&validate(&loud, &ctx("mode_a"))).contains(&"minimal_misuse"));
    }

    /// b02 结论 3/4：核心段 2 件放行；摇滚四件套最强 4 件放行；Chorus 类 2 件拦
    #[test]
    fn core_floor_two_strong_floor_four_chorus_floor_three() {
        let ok = ab_plan(vec![
            sec(SectionKind::Verse, 3, 2, false),
            sec(SectionKind::Chorus, 7, 4, false),
        ]);
        assert!(passed(&validate(&ok, &ctx("mode_b"))));
        let weak_strong = ab_plan(vec![
            sec(SectionKind::Verse, 3, 2, false),
            sec(SectionKind::Chorus, 7, 3, false),
        ]);
        let c = codes(&validate(&weak_strong, &ctx("mode_b")));
        assert!(c.contains(&"instrument_strong"), "{c:?}");
        let thin_chorus = ab_plan(vec![
            sec(SectionKind::Intro, 1, 1, true),
            sec(SectionKind::Chorus, 5, 2, false),
            sec(SectionKind::Chorus, 8, 4, false),
        ]);
        assert!(codes(&validate(&thin_chorus, &ctx("mode_b"))).contains(&"instrument_floor"));
    }

    #[test]
    fn energy_gap_and_instrument_gap_and_cap() {
        let flat = ab_plan(vec![
            sec(SectionKind::Verse, 5, 2, false),
            sec(SectionKind::Chorus, 6, 4, false),
        ]);
        assert!(codes(&validate(&flat, &ctx("mode_a"))).contains(&"energy_gap"));
        let no_gap = ab_plan(vec![
            sec(SectionKind::Verse, 2, 3, false),
            sec(SectionKind::Chorus, 8, 4, false),
        ]);
        assert!(codes(&validate(&no_gap, &ctx("mode_a"))).contains(&"instrument_gap"));
        let mut over = ab_plan(vec![
            sec(SectionKind::Verse, 2, 2, false),
            sec(SectionKind::Chorus, 8, 7, false),
        ]);
        over.global.instruments.push("bass".into());
        over.sections[1].instruments.push("bass".into());
        let c = codes(&validate(&over, &ctx("mode_a")));
        assert_eq!(
            c.iter().filter(|c| **c == "instrument_max").count(),
            2,
            "{c:?}"
        );
    }

    /// 门不对 C/D 模式做 A/B 弧线约束
    #[test]
    fn arc_rules_only_for_ab() {
        let p = ab_plan(vec![
            sec(SectionKind::Verse, 5, 1, false),
            sec(SectionKind::Chorus, 5, 1, false),
        ]);
        assert!(!codes(&validate(&p, &ctx("mode_a"))).is_empty());
        assert!(codes(&validate(&p, &ctx("mode_c")))
            .iter()
            .all(|c| !c.starts_with("instrument") && !c.starts_with("energy")));
    }

    #[test]
    fn fidelity_multiset_detects_missing_and_added() {
        let base = minimal_plan();
        let mut fin = minimal_plan();
        fin.sections[1].lines = vec!["雨落 长街".into(), "新加一句".into()];
        let v = validate(
            &fin,
            &ValidateCtx {
                mode: "mode_b",
                converged: Some(&base),
                original_lyrics: None,
            },
        );
        let c = codes(&v);
        assert!(
            c.contains(&"fidelity_missing") && c.contains(&"fidelity_added"),
            "{c:?}"
        );
        assert!(
            !v.iter()
                .any(|i| i.message.contains("雨落长街") && i.code == "fidelity_missing"),
            "空白归一"
        );
        // Mode C 不跑方案保真（以原歌词为基准）
        let v = validate(
            &fin,
            &ValidateCtx {
                mode: "mode_c",
                converged: Some(&base),
                original_lyrics: None,
            },
        );
        assert!(!codes(&v).iter().any(|c| c.starts_with("fidelity")));
    }

    #[test]
    fn mode_c_lyric_fill_runs_on_rendered_lyrics() {
        let mut p = minimal_plan();
        p.sections[1].lines = vec!["一二三".into()];
        let v = validate(
            &p,
            &ValidateCtx {
                mode: "mode_c",
                converged: None,
                original_lyrics: Some("[Verse]\n雨落长街湿\n灯火未眠时"),
            },
        );
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
        p.global.bpm = Some(110);
        p.global.negative = vec!["long intro".into(), "slow build".into(), "fade out".into()];
        p.global.mix = vec!["M-01".into(), "M-06".into()];
        p.settings.weirdness = 15;
        p.settings.style_influence = 90;
        let hook_line = "今夜不退";
        p.sections = vec![
            sec(SectionKind::Hook, 8, 3, false),
            sec(SectionKind::Verse, 4, 2, false),
            sec(SectionKind::Hook, 8, 3, false),
            sec(SectionKind::Hook, 8, 1, false),
        ];
        p.sections[0].label = "Hook".into();
        p.sections[0].target_seconds = Some(8);
        p.sections[0].lines = vec![hook_line.into(), hook_line.into(), "干就干脆".into()];
        p.sections[1].label = "Verse".into();
        p.sections[1].target_seconds = Some(8);
        p.sections[1].lines = vec!["冷风吹过长街".into(), "霓虹照亮黑夜".into()];
        p.sections[2].label = "Hook 2".into();
        p.sections[2].target_seconds = Some(8);
        p.sections[2].lines = vec![hook_line.into(), hook_line.into(), "干就干脆".into()];
        p.sections[3].label = "Hook 3".into();
        p.sections[3].target_seconds = Some(8);
        p.sections[3].lines = vec![hook_line.into()];
        p.sections[3].positive_local = vec!["abrupt stop".into()];
        p.mode_meta.douyin = Some(crate::plan::douyin::DouyinModeMeta {
            scene: crate::plan::douyin::DouyinScene::Daily,
            scene_source: crate::plan::douyin::SceneSource::Default,
            clip: crate::plan::douyin::DouyinClip {
                start_section: "Hook".into(),
                end_section: "Hook".into(),
                seconds: 15,
            },
            hook_line: format!("{hook_line}\n干就干脆"),
            loop_tail: true,
        });
        p
    }

    #[test]
    fn douyin_structure_ported_to_ir() {
        let p = douyin_plan();
        assert!(
            passed(&validate(&p, &ctx("mode_d"))),
            "{:?}",
            validate(&p, &ctx("mode_d"))
        );
        let mut bad = douyin_plan();
        bad.global.bpm = Some(80);
        bad.mode_meta.douyin = None;
        bad.sections[0].lines = vec!["这一行歌词明显超过了十二个字的上限".into()];
        let c = codes(&validate(&bad, &ctx("mode_d")));
        for want in ["douyin_meta_missing", "line_too_long"] {
            assert!(c.contains(&want), "缺 {want}: {c:?}");
        }
    }

    #[test]
    fn break_marks_and_header_length() {
        let mut p = minimal_plan();
        p.sections[1].lines[0] = "雨落/长街".into();
        p.sections[1].positive_local = vec!["x".repeat(rules::DESC_LINE_MAX_CHARS)];
        let c = codes(&validate(&p, &ctx("mode_b")));
        assert!(
            c.contains(&"break_mark") && c.contains(&"header_too_long"),
            "{c:?}"
        );
    }

    /// §5.1#5 变异覆盖：style_length 经 check_style_and_headers 接线（关闭则红）
    #[test]
    fn style_length_wired_through_plan_validate() {
        let mut p = minimal_plan();
        p.global.genre = vec!["folk".into()];
        p.global.bpm = None;
        p.global.key = None;
        p.global.meter = None;
        p.global.vocal.clear();
        p.global.instruments.clear();
        p.global.arc = None;
        p.global.mix.clear();
        let c = codes(&validate(&p, &ctx("mode_b")));
        assert!(c.contains(&"style_length"), "{c:?}");
    }

    /// P1-9 / M-12：reference_audio 默认关；开启后允许 Audio Influence ≠ 0
    #[test]
    fn reference_audio_allows_nonzero_influence() {
        let mut p = minimal_plan();
        p.settings.audio_influence = Some(40);
        p.settings.reference_audio = false;
        assert!(codes(&validate(&p, &ctx("mode_b"))).contains(&"audio_influence"));
        p.settings.reference_audio = true;
        assert!(!codes(&validate(&p, &ctx("mode_b"))).contains(&"audio_influence"));
    }

    /// P1-9：发音/Persona/系列字段默认关闭（None）不产生硬问题；有内容仍只是文本层
    #[test]
    fn p1_9_text_advisories_default_off() {
        let p = minimal_plan();
        assert!(
            p.pronunciation_hints.is_none() && p.persona.is_none() && p.series_context.is_none()
        );
        assert!(!p.settings.reference_audio);
        let mut p = minimal_plan();
        p.pronunciation_hints = Some("行(háng) 勿读 xíng".into());
        p.persona = Some("breathy female, close-mic, consistent across album".into());
        p.series_context = Some("album: 雨季三部曲; shared vocal persona".into());
        assert!(
            passed(&validate(&p, &ctx("mode_b")))
                || !codes(&validate(&p, &ctx("mode_b")))
                    .iter()
                    .any(|c| c.starts_with("persona")),
            "{:?}",
            validate(&p, &ctx("mode_b"))
        );
    }

    /// P1-8：已知押韵样本同辙不告警；破坏押韵位置则 Warn 且不致 passed=false
    #[test]
    fn advise_rhyme_known_samples() {
        let orig = "[Verse]\n天上的星星不说话\n地上的孩子想妈妈\n";
        let good = "[Verse]\n海边的灯火不说话\n夜里的孩子想妈妈\n";
        let w = advise_rhyme(orig, good);
        assert!(w.is_empty(), "同辙保留不应告警: {w:?}");
        let bad = "[Verse]\n海边的灯火不说话\n夜里的孩子在跑步\n";
        let w = advise_rhyme(orig, bad);
        assert!(
            w.iter()
                .any(|i| i.code == "rhyme_advise" && i.severity == Severity::Warn),
            "{w:?}"
        );
        let mut p = minimal_plan();
        p.sections[0].lines = vec!["海边的灯火不说话".into()];
        p.sections[1].lines = vec!["夜里的孩子在跑步".into()];
        let rendered =
            crate::plan::render_suno::to_package_text(&crate::plan::render_suno::render(&p));
        let v = validate(
            &p,
            &ValidateCtx {
                mode: "mode_c",
                converged: None,
                original_lyrics: Some(orig),
            },
        );
        let _ = rendered;
        assert!(
            passed(&v) || v.iter().any(|i| i.code == "lyric_fill"),
            "字数可能硬红，但韵脚不得单独打回"
        );
        assert!(
            v.iter()
                .all(|i| i.code != "rhyme_advise" || i.severity == Severity::Warn),
            "韵脚必须是 Warn"
        );
    }

    #[test]
    fn advise_rhyme_polyphone_reports_only() {
        let orig = "大路朝天各自走\n小河有水慢慢流\n";
        let neu = "大路朝天各自行\n小河有水慢慢游\n";
        let w = advise_rhyme(orig, neu);
        assert!(w.iter().all(|i| i.severity == Severity::Warn));
    }

    #[test]
    fn budget_limits() {
        let mut p = minimal_plan();
        p.sections[1].lines = vec!["字".repeat(lim(SUNO_LYRICS_MAX) + 1)];
        p.global.negative = vec!["x".repeat(lim(SUNO_EXCLUDE_MAX) + 1)];
        let c = codes(&validate(&p, &ctx("x")));
        assert!(
            c.contains(&"lyrics_too_long") && c.contains(&"exclude_too_long"),
            "{c:?}"
        );
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
        // v3 新增：允许 ≥3 词的具象英文人声与混音描述直接过门
        let mut custom = minimal_plan();
        custom.global.vocal = vec!["husky baritone, close-mic breath, slight voice crack".into()];
        custom.global.mix = vec!["dry close-mic bedroom tape warmth".into()];
        assert!(run(&custom).is_empty(), "具象英文短语应放行");
        let mut p = minimal_plan();
        p.global.vocal.clear();
        assert_eq!(run(&p), vec!["vocal_mix_coverage"]);
        let mut p = minimal_plan();
        p.global.mix = vec!["warm mix".into()];
        let c = run(&p);
        assert!(
            c.contains(&"vocal_mix_unknown_id") && c.contains(&"vocal_mix_coverage"),
            "{c:?}"
        );
        let mut p = minimal_plan();
        p.global.vocal = vec!["M-01".into()];
        let c = run(&p);
        assert!(
            c.contains(&"vocal_mix_category") && c.contains(&"vocal_mix_coverage"),
            "{c:?}"
        );
        // 回滚=删表：门跳过
        let mut v = Vec::new();
        check_vocal_mix_coverage_with(&crate::knowledge::KnowledgeBase::default(), &p, &mut v);
        assert!(v.is_empty());
        // 接线：validate 主入口必须调用覆盖门
        let mut p = minimal_plan();
        p.global.mix.clear();
        let all = validate(
            &p,
            &ValidateCtx {
                mode: "mode_b",
                converged: None,
                original_lyrics: None,
            },
        );
        assert!(
            all.iter().any(|i| i.code == "vocal_mix_coverage"),
            "validate 未接线覆盖门"
        );
    }

    #[test]
    fn validate_lang_two_states() {
        use crate::plan::LyricLang;
        let mut p = minimal_plan();
        assert!(validate_lang(&p).is_empty(), "zh 中文歌词应过");
        p.sections[1].lines.push("oh yeah".into());
        assert!(validate_lang(&p).is_empty(), "zh 允许英文和声行");
        p.mode_meta.lyrics_lang = LyricLang::En;
        let v = validate_lang(&p);
        assert!(
            v.iter()
                .any(|i| i.code == "lyrics_lang_mismatch" && i.path == "/sections/1/lines/0"),
            "{v:?}"
        );
        p.sections[1].lines = vec!["rain on the street".into()];
        assert!(validate_lang(&p).is_empty(), "en 英文歌词应过");
        p.mode_meta.lyrics_lang = LyricLang::Zh;
        assert!(
            codes(&validate(&p, &ctx("mode_b"))).contains(&"lyrics_lang_mismatch"),
            "zh 全英文须经 validate 打回"
        );
    }

    #[test]
    fn validate_lang_ignores_style() {
        use crate::plan::LyricLang;
        let mut p = minimal_plan();
        p.mode_meta.lyrics_lang = LyricLang::En;
        p.sections[1].lines = vec!["rain".into()];
        p.global.genre = vec!["民谣".into()];
        assert!(validate_lang(&p).is_empty(), "Style 中文不归 validate_lang（归 §3.7）");
    }

    #[test]
    fn v3_anti_ai_tells_and_exemptions_work() {
        let mut p = ab_plan(vec![
            sec(SectionKind::Verse, 2, 1, false),
            sec(SectionKind::PreChorus, 4, 1, false),
            sec(SectionKind::Chorus, 6, 1, false),
            sec(SectionKind::Chorus, 8, 1, false),
        ]);
        p.global.mood = Some("epic emotional masterpiece".into());
        p.sections[2].lines = vec!["走过漫长时光与远方".into(), "寻找宿命的星辰".into()];
        p.sections[3].lines = p.sections[2].lines.clone();
        // 未声明 exemptions 时：配器不足报 Hard，同时检出空洞形容词/能量单调/副歌雷同/抽象套话 Warn
        let issues = validate(&p, &ctx("mode_b"));
        assert!(!passed(&issues));
        let warn_codes: Vec<&str> = issues
            .iter()
            .filter(|i| i.severity == Severity::Warn)
            .map(|i| i.code)
            .collect();
        for w in [
            "style_empty_adjective",
            "energy_monotonic",
            "chorus_static_repeat",
            "lyric_abstract_cliche",
        ] {
            assert!(warn_codes.contains(&w), "应检出软诊断 {w}: {warn_codes:?}");
        }
        // 声明 exemptions 后：极简独奏编制的配器门槛自动降级为 Warn，不再硬打回
        p.exemptions = vec!["独立民谣独奏编制，全曲仅一把古筝".into()];
        let issues_exempt = validate(&p, &ctx("mode_b"));
        assert!(
            passed(&issues_exempt),
            "声明 exemptions 后应放行极简编制: {issues_exempt:?}"
        );
    }

    #[test]
    fn v3_prosody_and_rhyme_hard_gates_work() {
        // 1. Mode B: 段落末句脱韵 -> section_rhyme_missing (Hard) + 同音挤韵 -> rhyme_homophone_collision (Hard) + 句长失衡 -> prosody_meter_imbalance (Hard)
        let mut p = minimal_plan();
        p.sections[1].lines = vec![
            "箱子敞着口蹲在客厅中间".into(),
            "灯泡一闪一闪数着还剩几件".into(),
            "杯子裹进你留下的旧毛衣".into(),
            "咖啡洒在".into(),
        ];
        let issues = validate(&p, &ctx("mode_b"));
        assert!(
            issues.iter().any(|i| i.code == "rhyme_homophone_collision" && i.severity == Severity::Hard),
            "同音挤韵（间 jian / 件 jian）必须触发 Hard 打回: {issues:?}"
        );
        assert!(
            issues.iter().any(|i| i.code == "section_rhyme_missing" && i.severity == Severity::Hard),
            "末句脱韵必须触发 Hard 打回: {issues:?}"
        );
        assert!(
            issues.iter().any(|i| i.code == "prosody_meter_imbalance" && i.severity == Severity::Hard),
            "句长极差过大且未声明 short_line 必须触发 Hard 打回: {issues:?}"
        );

        // 2. Mode C: 照搬原词尾字 + 同音挤韵 -> 全部 Hard 打回
        let orig = "昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼\n心有灵犀一点通";
        let mut pc = minimal_plan();
        pc.sections[0].lines = vec![];
        pc.sections[1].lines = vec![
            "凌晨机房灯微红".into(),
            "两块屏幕光不同".into(),
            "没开口却同频率".into(),
            "屏幕两端一点通".into(),
        ];
        let issues_c = validate(
            &pc,
            &ValidateCtx {
                mode: "mode_c",
                converged: None,
                original_lyrics: Some(orig),
            },
        );
        assert!(
            issues_c.iter().any(|i| i.code == "mode_c_copied_rhyme_char" && i.severity == Severity::Hard),
            "改词直接照搬原词尾字「通」必须触发 Hard 打回: {issues_c:?}"
        );
        assert!(
            issues_c.iter().any(|i| i.code == "rhyme_homophone_collision" && i.severity == Severity::Hard),
            "改词同音挤韵（同 tong / 通 tong）必须触发 Hard 打回: {issues_c:?}"
        );

        // 3. 全局韵部乱跳（Verse 1 江阳 -> Pre-Chorus 怀来 -> Chorus 言前 -> Verse 2 遥条）-> global_rhyme_scatter + homologous_section_rhyme_drift (Hard)
        let mut pg = minimal_plan();
        pg.sections = vec![
            Section {
                kind: SectionKind::Verse,
                label: "Verse 1".into(),
                motion: None,
                positive_local: vec!["close room".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(3),
                instruments: vec!["guzheng".into(), "cello".into()],
                minimal: false,
                target_seconds: None,
                lines: vec!["纸箱没封口靠在白墙".into(), "胶带垂下来晃着微光".into()],
            },
            Section {
                kind: SectionKind::PreChorus,
                label: "Pre-Chorus".into(),
                motion: None,
                positive_local: vec!["close room".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(5),
                instruments: vec!["guzheng".into(), "cello".into()],
                minimal: false,
                target_seconds: None,
                lines: vec!["胶带撕开又粘回来".into(), "箱底露出旧毛衣的白".into()],
            },
            Section {
                kind: SectionKind::Chorus,
                label: "Chorus 1".into(),
                motion: None,
                positive_local: vec!["wide hall".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(8),
                instruments: vec!["guzheng".into(), "cello".into(), "dizi".into(), "pipa".into()],
                minimal: false,
                target_seconds: None,
                lines: vec!["这间房我扫了三遍".into(), "灰还卡在指甲尖".into()],
            },
            Section {
                kind: SectionKind::Verse,
                label: "Verse 2".into(),
                motion: None,
                positive_local: vec!["close room".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(4),
                instruments: vec!["guzheng".into(), "cello".into()],
                minimal: false,
                target_seconds: None,
                lines: vec!["胶带在手里绕了两遭".into(), "又怕它压到最底下的票".into()],
            },
        ];
        let issues_g = validate(&pg, &ctx("mode_b"));
        assert!(
            issues_g.iter().any(|i| i.code == "global_rhyme_scatter" && i.severity == Severity::Hard),
            "四段跨四个毫无亲缘的十三辙必须触发 global_rhyme_scatter Hard 打回: {issues_g:?}"
        );
        assert!(
            issues_g.iter().any(|i| i.code == "homologous_section_rhyme_drift" && i.severity == Severity::Hard),
            "Verse 1(江阳) 与 Verse 2(遥条) 跨段跑韵必须触发 homologous_section_rhyme_drift Hard 打回: {issues_g:?}"
        );
    }

    #[test]
    fn v6_2_open_source_skill_gates_work() {
        // 1. 歌词行混入圆括号舞台/器乐指示 -> lyric_paren_direction (Hard)
        let mut p = minimal_plan();
        p.sections[1].lines = vec![
            "雨落长街(吉他独奏)".into(),
            "灯火未眠".into(),
        ];
        let issues = validate(&p, &ctx("mode_b"));
        assert!(
            issues.iter().any(|i| i.code == "lyric_paren_direction" && i.severity == Severity::Hard),
            "歌词行内混入圆括号器乐/舞台指示必须触发 Hard 打回: {issues:?}"
        );

        // 2. 风格栏混入真实艺人名 + 8K 伪视觉词 + 参考音频参数失配 + 比喻自解 + 励志口号 + 主副歌交界回声
        let mut p2 = minimal_plan();
        p2.global.mood = Some("in the style of Taylor Swift, 8K lossless video".into());
        p2.settings.reference_audio = true;
        p2.settings.variety = 20;
        p2.settings.audio_influence = Some(30);
        p2.sections = vec![
            Section {
                kind: SectionKind::Verse,
                label: "Verse 1".into(),
                motion: None,
                positive_local: vec!["close room".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(3),
                instruments: vec!["guzheng".into(), "cello".into()],
                minimal: false,
                target_seconds: None,
                lines: vec![
                    "茶杯在桌上早就凉透".into(),
                    "就像我们的爱已走不眠".into(),
                ],
            },
            Section {
                kind: SectionKind::Chorus,
                label: "Chorus 1".into(),
                motion: None,
                positive_local: vec!["wide hall".into()],
                negative_local: vec![],
                subtraction: None,
                energy: Some(8),
                instruments: vec!["guzheng".into(), "cello".into(), "dizi".into(), "pipa".into()],
                minimal: false,
                target_seconds: None,
                lines: vec![
                    "冲破枷锁在这长夜不眠".into(),
                    "逆风飞翔直到海角天边".into(),
                ],
            },
        ];
        let issues2 = validate(&p2, &ctx("mode_b"));
        let codes2: Vec<&str> = issues2.iter().map(|i| i.code).collect();
        for expected in [
            "style_artist_name_leak",
            "style_visual_hallucination",
            "reference_audio_slider_mismatch",
            "lyric_metaphor_decoded",
            "lyric_cliche_escalation",
            "verse_chorus_echo",
        ] {
            assert!(
                codes2.contains(&expected),
                "应检出 v6.2 诊断/硬门 {expected}，实际: {codes2:?}"
            );
        }
    }
}
