//! 文本侧校验助手（§5.1#5 退役后）。
//!
//! 终稿硬门唯一入口 = `plan::validate`。本模块保留仍被 IR 门调用的助手：
//! `validate_lyric_fill`（Mode C）、`style_prompt_length_issues`、`check_bpm_range`、
//! `desc_line_length_issues`（供 lyric_fill）、`is_bare_package_line`、字数/行型工具，
//! 以及 `extract_style_prompt`（无头观测）。
//!
//! 已删：`validate_for_mode` / `validate_production` / 文本 `validate_douyin` /
//! `param_range_issues` / `check_audio_influence` / `declared_minimal_sections`。

use crate::rules;

/// 校验结果
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub passed: bool,
    pub issues: Vec<String>,
}

impl ValidationResult {
    fn ok() -> Self {
        Self {
            passed: true,
            issues: vec![],
        }
    }
    fn fail(issues: Vec<String>) -> Self {
        Self {
            passed: false,
            issues,
        }
    }
}

/// 提取 Style Prompt 正文（冒号后）。
/// 支持 `**Style Prompt**：` / `Style Prompt: ` / `风格:` 三种写法——
/// 旧 orchestrator 行级提取器认得 "风格" 前缀（语义下沉至此，无行为回退）。
/// 返回冒号后正文供过短/BPM 校验使用，不再被 "Style Prompt**: " 标签前缀虚增长度。
///
/// 标签回退（B 实测根治）：模型偶发丢标签但按格式规范把风格写在第一行
/// （v0.5.0 实网 B 模式 3 连打回均因此）。无标签时回退取首个"像风格正文"的行——
/// 跳过结构标签/说明行（`[` 开头）、参数行、标题行；找不到则维持 None。
/// 回退严格优于现状：最坏情况是长度/信息块检查跑在一行错误文本上产生打回
/// （可重试），而现状是"未找到 Style Prompt 字段"直接死路。
pub fn extract_style_prompt(text: &str) -> Option<String> {
    let mut label_seen = false;
    for line in text.lines() {
        let t = line.trim().trim_start_matches("**").trim();
        if t.starts_with("Style Prompt") || t.starts_with("风格") {
            // 形如 "**Style Prompt**：..." 或 "Style Prompt: ..." 或 "风格：..."
            let body = t.split(['：', ':']).nth(1).unwrap_or("").trim();
            if !body.is_empty() {
                return Some(body.to_string());
            }
            label_seen = true;
        }
    }
    // 有标签但正文为空 → 维持"未找到"语义（不回退，避免误抓歌词行）
    if label_seen {
        return None;
    }
    // 标签回退（B 实测根治）：格式规范要求风格写在第一行，模型偶发丢标签但
    // 风格正文仍占第一行位置——仅当首个非空行本身像风格正文时回退提取；
    // 首行是结构标签/说明行/参数行/标题则视为无风格行（None → 走"未找到"打回）
    let first = text
        .lines()
        .map(|l| l.trim().trim_start_matches("**").trim())
        .find(|l| !l.is_empty())?;
    if first.starts_with('[')
        || first.starts_with("参数")
        || first.starts_with("Parameter")
        || first.starts_with('#')
    {
        return None;
    }
    Some(first.to_string())
}

/// 说明行长度门（终稿层单源，A/B/C/D 全模式共用）：上限 rules::DESC_LINE_MAX_CHARS。
/// 与 plan::validate 同读一个常量、共用同一个行型谓词（rules::is_desc_line）——
/// 上游输出约束与下游校验约束不再各写一套（R1 根因：上游零告知 + 下游 80 硬门 → 必然降级）。
/// 报前 3 条明细 + 总数汇总（与其他硬门同口径），文案给出可执行的修复方向。
pub fn desc_line_length_issues(text: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let mut over = 0usize;
    for line in text.lines().filter(|l| rules::is_desc_line(l)) {
        let t = line.trim();
        let n = t.chars().count();
        if n > rules::DESC_LINE_MAX_CHARS {
            over += 1;
            if over <= 3 {
                issues.push(format!(
                    "说明行超 {} 字符（{} 字符）: {}",
                    rules::DESC_LINE_MAX_CHARS,
                    n,
                    t.chars().take(40).collect::<String>()
                ));
            }
        }
    }
    if over > 0 {
        issues.push(format!(
            "说明行超限共 {} 行（上限 {} 字符，含方括号整行计）——须压缩到限内：保乐器+行为动词，删修饰词/重复空间描述；不得删乐器（配器 3-7 件与最强段 ≥5 件照常校验）",
            over,
            rules::DESC_LINE_MAX_CHARS
        ));
    }
    issues
}

/// O5：歌词行禁用断句符号检出（`/` 斜杠、`、` 顿号——Suno 会读出或忽略）。
/// 只查歌词行（行型解析），说明行/标签/参数行不查。
fn forbidden_break_marks(text: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if is_lyric_line(t) && (t.contains('/') || t.contains('、')) {
            issues.push(format!(
                "第{}行使用了禁用断句符号（/ 或 、——Suno 会读出或忽略），请改用单空格断句: {}",
                i + 1,
                t
            ));
        }
    }
    issues
}

/// 和音虚词单源（用户拍板 2026-09-19）：纯英文虚词 token 不计入歌词行字数。
/// 根治："(ooh~) 一半是你 一半是我" 剥标点后 ooh 三字母被计入=11 字误判超限。
/// 扩充规则：只收公认无实义的拟声/和音词，小写匹配；实义词（baby 等）不得收入。
const LYRIC_VOCABLES: [&str; 9] = ["ooh", "oh", "yeah", "ah", "la", "hey", "woah", "woo", "hmm"];

/// 通用歌词行计数：按空白分词 → 每 token 剥标点 → 剔除和音虚词（纯 ASCII 字母且
/// 小写 ∈ LYRIC_VOCABLES）→ 汇总字数。分词在剥标点之前，保证 "(ooh~)" 整体识别为虚词。
pub(crate) fn count_line_chars_excluding_vocables(s: &str, is_punct: fn(char) -> bool) -> usize {
    s.split_whitespace()
        .filter_map(|t| {
            let stripped: String = t
                .chars()
                .filter(|c| !is_punct(*c) && !c.is_whitespace())
                .collect();
            if stripped.is_empty() {
                return None;
            }
            let lower = stripped.to_ascii_lowercase();
            let is_vocable = stripped.bytes().all(|b| b.is_ascii_alphabetic())
                && LYRIC_VOCABLES.contains(&lower.as_str());
            if is_vocable {
                None
            } else {
                Some(stripped.chars().count())
            }
        })
        .sum()
}

/// Mode C 字数计数：去空白 + 去标点 + 去和音虚词（Q2：与 prompts.rs“标点不计入字数”同口径；
/// 此前仅去空白，带问号/感叹号的行会被误报字数不符）。
fn count_lyric_chars(s: &str) -> usize {
    count_line_chars_excluding_vocables(s, is_lyric_punct)
}

/// 歌词标点表（半角标点 + 中日韩常用标点；断句空格已由空白分支处理）。
fn is_lyric_punct(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '？'
                | '！'
                | '、'
                | '；'
                | '：'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '《'
                | '》'
                | '〈'
                | '〉'
                | '…'
                | '"'
                | '\''
        )
}

/// 裸包装行判定（Q2收窄：此前仅“半角逗号+>15字”，合法歌词长句可被吞；
/// 现加关键词双条件：含配器/空间/人声/能量词才算包装行）。
pub(crate) fn is_bare_package_line(l: &str) -> bool {
    let n = l.chars().filter(|c| !c.is_whitespace()).count();
    if !(l.contains(',') && n > 15) {
        return false;
    }
    let ll = l.to_lowercase();
    [
        "voice",
        "vocal",
        "人声",
        "room",
        "空间",
        "hall",
        "能量",
        "energy",
        "混响",
        "reverb",
        "氛围",
        "atmosphere",
        "guitar",
        "piano",
        "cello",
        "drums",
        "bass",
        "synth",
    ]
    .iter()
    .any(|k| ll.contains(k))
}

/// 歌词行判定的排除前缀表（单源；供 `forbidden_break_marks` / lyric_fill 行型）。
/// '（' 与「注：」「结构归类」开头的方案元信息行不算歌词；`<<<` 形态（历史分节标记）同样排除。
/// 终稿行型：结构标签 `[X]`（无逗号）、说明行 `[乐器, ...]`（含逗号）、歌词行（其余）。
const LYRIC_LINE_EXCLUDED_PREFIXES: &[&str] = &[
    "[",
    "#",
    "-",
    "*",
    "（",
    "注：",
    "结构归类",
    "Style",
    "风格",
    "参数",
    "Weirdness",
    "<<<",
];

/// 歌词行判定（单源）：排除空行/结构标签/说明行/元信息/Style Prompt/参数行/裸包装行。
/// 无逗号超长行仍视为歌词行（由字数门报错，Q2收窄判定口径）。
fn is_lyric_line(l: &str) -> bool {
    !l.is_empty()
        && !LYRIC_LINE_EXCLUDED_PREFIXES
            .iter()
            .any(|p| l.starts_with(p))
        && !is_bare_package_line(l)
}

/// Mode C：校验填词（字数对齐）
pub fn validate_lyric_fill(original: &str, new: &str) -> ValidationResult {
    let mut issues = Vec::new();

    let orig_lines: Vec<&str> = original
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('[') && !l.starts_with('#'))
        .collect();
    let new_lines: Vec<&str> = new
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('[')
                && !l.starts_with('#')
                && !l.starts_with('|')
                // 过滤标准包的包装行（**Style Prompt**/参数/说明行），只比对歌词行
                && !l.starts_with("**")
                && !l.starts_with("Style")
                && !l.starts_with("风格")
                && !l.starts_with("参数")
                && !l.starts_with("Weirdness")
        })
        // 裸说明行识别（Q2收窄：逗号+长度+关键词三条件，防吞真歌词）
        .filter(|l| !is_bare_package_line(l))
        .collect();

    if orig_lines.is_empty() || new_lines.is_empty() {
        return ValidationResult::fail(vec!["原歌词或新歌词为空".to_string()]);
    }
    // O5：C 模式同禁断句符号（校验清单同源）
    issues.extend(forbidden_break_marks(new));
    // R1：C 模式同样过说明行长度门（下游此前只有方案侧信封门在拦、终稿侧无门，
    // 上游 CHECKLIST_C 又零告知——两侧口径与告知现已同源）
    issues.extend(desc_line_length_issues(new));

    // 字数逐行对比：前 N 行（N=原歌词行数）逐行等字数（去空白+去标点，Q2 与 prompt 同口径）。
    // 允许尾部 ≤2 行收尾（如 Outro 一句）；超过报"多余歌词行"。
    let n = orig_lines.len();
    let max_lines = n.min(new_lines.len());
    let mut mismatches = 0usize;
    for i in 0..max_lines {
        let oc = count_lyric_chars(orig_lines[i]);
        let nc = count_lyric_chars(new_lines[i]);
        if oc != nc {
            mismatches += 1;
            if mismatches <= 3 {
                issues.push(format!(
                    "第 {} 行字数不符: 原 {} 字 vs 新 {} 字（原文: {}）",
                    i + 1,
                    oc,
                    nc,
                    orig_lines[i]
                ));
            }
        }
    }
    if mismatches > 0 {
        if mismatches > 3 {
            issues.push(format!("共 {} 行字数不符", mismatches));
        }
        // 打回信息可执行化：期望 vs 实际逐行对照，auditor 按数字逐行对齐
        let expect: Vec<usize> = orig_lines.iter().map(|l| count_lyric_chars(l)).collect();
        let actual: Vec<usize> = new_lines
            .iter()
            .take(n)
            .map(|l| count_lyric_chars(l))
            .collect();
        issues.push(format!(
            "字数对照——原歌词每行期望字数: {:?}；当前新歌词每行实际字数: {:?}；请逐行调整至期望字数（用同字数的短句替换，宁短勿长）",
            expect, actual
        ));
    }
    if new_lines.len() < n {
        issues.push(format!(
            "歌词行数不足: 原 {} 行 vs 新 {} 行",
            n,
            new_lines.len()
        ));
    } else if new_lines.len() > n + rules::LYRIC_FILL_TAIL_ALLOW {
        issues.push(format!(
            "歌词行数过多: 原 {} 行 vs 新 {} 行（仅允许尾部 ≤2 行收尾）",
            n,
            new_lines.len()
        ));
    }

    if issues.is_empty() {
        ValidationResult::ok()
    } else {
        ValidationResult::fail(issues)
    }
}

/// 检查 BPM 是否满足模式要求（对齐原指令 prompts.rs：仅 mode_d 明确 "BPM>=90"（:736）；
/// mode_a/b 的调性节奏块（:94/:1009）无区间要求——不设代码区间，避免误杀原指令合法输出，
/// BPM 合理性由制作人讨论轮审查兜底）
pub fn check_bpm_range(mode: &str, bpm: u32) -> Option<String> {
    if mode == "mode_d" && bpm < rules::DOUYIN_V2_BPM_MIN {
        Some(format!(
            "BPM {} 低于抖音模式要求 {}（原指令 BPM>=90）",
            bpm,
            rules::DOUYIN_V2_BPM_MIN
        ))
    } else {
        None
    }
}

/// 全模式 Style Prompt 长度门（上/下限同门，唯一执行者）。
///
/// - 单点接线：`plan::validate::check_style_and_headers`（四模式共用同一调用，无第二处判法）。
/// - 模式域单源：`rules::STYLE_PROMPT_MAX_MODES` / `STYLE_PROMPT_MIN_MODES`（注册表同读同两个常量，
///   杜绝"注册模式域 ≠ 执行模式域"漂移——本轮实测的 `douyin_bpm_min` 错挂执行者即此类）。
/// - 上游承诺对齐：CHECKLIST_A/B/C/D 均承诺上限（≤350）；下限（≥30）仅 A/B/D 承诺，
///   C 的 STYLE 节是"氛围/流派基调一句"、上游无下限告知——故下限不对 C 执行
///   （多执行即上游零告知的隐性降级，违反"上游输出约束 ↔ 下游校验约束"同域原则）。
/// - 不设流派/乐器/人声词表强制（原指令允许创造新流派），信息块完整性由制作人讨论轮把关。
pub fn style_prompt_length_issues(mode: &str, style_prompt: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let chars = style_prompt.chars().count();
    if rules::STYLE_PROMPT_MAX_MODES.contains(&mode) && chars > rules::STYLE_PROMPT_MAX_CHARS {
        issues.push(format!(
            "Style Prompt 长度 {} 超限（> {}）——须再删 ≥{} 字符（Style 由 global 字段拼出：删修饰词/重复标签/合并同义，保留流派+人声+核心乐器）",
            chars,
            rules::STYLE_PROMPT_MAX_CHARS,
            chars - rules::STYLE_PROMPT_MAX_CHARS
        ));
    }
    if rules::STYLE_PROMPT_MIN_MODES.contains(&mode) && chars < rules::STYLE_PROMPT_MIN_CHARS {
        issues.push(format!("Style Prompt 过短（{} 字符），缺少信息块", chars));
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_line_chars_exempts_vocables() {
        assert_eq!(count_lyric_chars("(ooh~) 一半是你 一半是我"), 8);
        assert_eq!(count_lyric_chars("oh oh yeah"), 0);
        assert_eq!(count_lyric_chars("baby 一半是你"), 8);
        assert!(count_lyric_chars("ooh你") > 0);
    }

    #[test]
    fn forbidden_break_marks_detected() {
        let text = "[Verse]\n雨落/长街\n[Chorus]\n心火、未灭";
        let issues = forbidden_break_marks(text);
        assert!(issues.len() >= 2, "{issues:?}");
    }

    #[test]
    fn style_prompt_too_long_fails() {
        let long = "x".repeat(rules::STYLE_PROMPT_MAX_CHARS + 1);
        let issues = style_prompt_length_issues("mode_a", &long);
        assert!(!issues.is_empty());
    }

    #[test]
    fn style_prompt_fallback_first_line_without_label() {
        let text = "folk ballad, warm acoustic guitar, soft vocal\n[Verse]\n啦啦啦";
        assert_eq!(
            extract_style_prompt(text).as_deref(),
            Some("folk ballad, warm acoustic guitar, soft vocal")
        );
    }

    #[test]
    fn style_prompt_fallback_skips_structural_lines() {
        assert!(extract_style_prompt("[Verse]\n啦啦啦").is_none());
    }

    #[test]
    fn lyric_fill_mismatch_detected() {
        let r = validate_lyric_fill("昨夜星辰昨夜风", "短");
        assert!(!r.passed);
    }

    #[test]
    fn lyric_fill_matching_passes() {
        let r = validate_lyric_fill("昨夜星辰昨夜风", "今朝有酒今朝醉");
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_whitespace_diff_passes() {
        let r = validate_lyric_fill("昨夜星辰昨夜风", "今 朝 有 酒 今 朝 醉");
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_ignores_package_wrapper_lines() {
        let original = "昨夜星辰昨夜风\n画楼西畔桂堂东";
        let neu = "Style Prompt: x\n今朝有酒今朝醉\n明日愁来明日愁\n参数: Weirdness=10";
        let r = validate_lyric_fill(original, neu);
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn desc_line_gate_shared_across_modes() {
        // is_desc_line 要求方括号 + 逗号
        let over = format!("[pad, {}]", "啊".repeat(rules::DESC_LINE_MAX_CHARS));
        let issues = desc_line_length_issues(&over);
        assert!(!issues.is_empty(), "{issues:?}");
        let prefix = "[pad, ";
        let exact_body_len = rules::DESC_LINE_MAX_CHARS - prefix.chars().count() - 1; // + ']'
        let exact = format!("{prefix}{}]", "啊".repeat(exact_body_len));
        assert_eq!(exact.chars().count(), rules::DESC_LINE_MAX_CHARS);
        assert!(
            desc_line_length_issues(&exact).is_empty(),
            "{} {}",
            exact.chars().count(),
            exact
        );
        assert!(!desc_line_length_issues(&format!("   {}", over)).is_empty());
        assert!(desc_line_length_issues("[Chorus]").is_empty());
        assert!(desc_line_length_issues(&format!("{}, 就是这样", "啊".repeat(300))).is_empty());
    }

    #[test]
    fn desc_line_gate_reports_capped_details() {
        let line = format!("[pad, {}]", "啊".repeat(rules::DESC_LINE_MAX_CHARS));
        let text = format!("{0}\n{0}\n{0}\n{0}", line);
        let issues = desc_line_length_issues(&text);
        assert!(issues.iter().any(|i| i.contains("共")), "{issues:?}");
    }

    #[test]
    fn lyric_fill_desc_line_over_limit_fails() {
        let original = "昨夜星辰昨夜风";
        let over = format!(
            "[pad, {}]\n今朝有酒今朝醉",
            "x".repeat(rules::DESC_LINE_MAX_CHARS)
        );
        let r = validate_lyric_fill(original, &over);
        assert!(!r.passed, "{:?}", r.issues);
        assert!(r.issues.iter().any(|i| i.contains("说明行")));
    }

    #[test]
    fn lyric_fill_punct_ignored() {
        let r = validate_lyric_fill("昨夜星辰昨夜风", "今朝有酒，今朝醉！");
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_long_line_with_comma_kept() {
        // 无配器关键词的长逗号行仍算歌词（等字）
        let original = "春风又绿江南岸，明月何时照我还";
        let neu = "秋水共长天一色，落霞与孤鹜齐飞";
        let r = validate_lyric_fill(original, neu);
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_empty_original_fails() {
        assert!(!validate_lyric_fill("", "有词").passed);
    }

    #[test]
    fn lyric_fill_line_count_mismatch_detected() {
        let r = validate_lyric_fill("一行\n二行\n三行", "一行字\n二行字");
        assert!(!r.passed);
    }

    #[test]
    fn lyric_fill_trailing_short_tail_passes() {
        let original = "昨夜星辰昨夜风\n画楼西畔桂堂东";
        let neu = "今朝有酒今朝醉\n明日愁来明日愁\n收";
        let r = validate_lyric_fill(original, neu);
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_too_many_extra_lines_fails() {
        let original = "昨夜星辰昨夜风";
        let neu = "今朝有酒今朝醉\na\nb\nc";
        assert!(!validate_lyric_fill(original, neu).passed);
    }

    #[test]
    fn lyric_fill_ignores_bare_description_lines() {
        let original = "昨夜星辰昨夜风";
        let neu = "[felt piano, nylon guitar, warm room, energy:3]\n今朝有酒今朝醉";
        let r = validate_lyric_fill(original, neu);
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn lyric_fill_many_mismatches_condensed() {
        let original = "一行字数啊\n二行字数啊\n三行字数啊\n四行字数啊";
        let neu = "短\n短\n短\n短";
        let r = validate_lyric_fill(original, neu);
        assert!(!r.passed);
        assert!(r
            .issues
            .iter()
            .any(|i| i.contains("共") || i.contains("对照")));
    }

    #[test]
    fn lyric_fill_long_line_without_comma_not_filtered() {
        let long = "这是一行没有逗号但是很长的歌词用来确认不会被当成包装行过滤掉啊啊";
        let r = validate_lyric_fill(long, long);
        assert!(r.passed, "{:?}", r.issues);
    }

    #[test]
    fn style_prompt_empty_body_is_missing() {
        assert!(extract_style_prompt("Style Prompt:\n[Verse]\nx").is_none());
    }

    #[test]
    fn bpm_check_only_mode_d() {
        assert!(check_bpm_range("mode_a", 60).is_none());
        assert!(check_bpm_range("mode_d", rules::DOUYIN_V2_BPM_MIN).is_none());
        assert!(check_bpm_range("mode_d", rules::DOUYIN_V2_BPM_MIN - 1).is_some());
    }

    #[test]
    fn style_prompt_short_detected() {
        let issues = style_prompt_length_issues("mode_a", "民谣");
        assert!(issues.iter().any(|i| i.contains("过短")));
    }

    #[test]
    fn style_prompt_comedy_allowed() {
        let s = "喜剧整活, 130BPM, 滑稽滑音合成器, 弹拨贝斯, 密集军鼓, 痞气半说半唱, 拥挤商场混响";
        assert!(style_prompt_length_issues("mode_a", s).is_empty());
    }

    #[test]
    fn style_prompt_emo_allowed() {
        let s = "emo破碎, 110BPM, 延迟吉他, 氛围垫, 稀疏鼓, 近麦气声";
        assert!(style_prompt_length_issues("mode_a", s).is_empty());
    }

    #[test]
    fn style_prompt_length_gate_mode_domain_single_sourced() {
        let long = "x".repeat(rules::STYLE_PROMPT_MAX_CHARS + 1);
        for m in rules::STYLE_PROMPT_MAX_MODES {
            assert!(
                style_prompt_length_issues(m, &long)
                    .iter()
                    .any(|i| i.contains("350")
                        || i.contains(&rules::STYLE_PROMPT_MAX_CHARS.to_string())),
                "{m}"
            );
        }
        for m in rules::STYLE_PROMPT_MIN_MODES {
            assert!(
                style_prompt_length_issues(m, "民谣")
                    .iter()
                    .any(|i| i.contains("过短")),
                "{m}"
            );
        }
        assert!(
            style_prompt_length_issues("mode_c", "民谣").is_empty(),
            "C 无下限"
        );
    }

    #[test]
    fn style_prompt_length_boundary() {
        let exact = "x".repeat(rules::STYLE_PROMPT_MAX_CHARS);
        assert!(style_prompt_length_issues("mode_c", &exact).is_empty());
        let over = "x".repeat(rules::STYLE_PROMPT_MAX_CHARS + 1);
        assert!(!style_prompt_length_issues("mode_c", &over).is_empty());
        let min_exact = "x".repeat(rules::STYLE_PROMPT_MIN_CHARS);
        assert!(style_prompt_length_issues("mode_a", &min_exact).is_empty());
        let under = "x".repeat(rules::STYLE_PROMPT_MIN_CHARS - 1);
        assert!(!style_prompt_length_issues("mode_a", &under).is_empty());
    }

    /// 叙事型例外门槛：上游告知与常量同源（下游 IR 判定见 plan::validate / douyin）
    #[test]
    fn narrative_exception_threshold_is_single_sourced() {
        let n = rules::NARRATIVE_SECTIONS_MIN;
        let expect_role = format!("≥{} 个叙事段", n);
        let expect_checklist = format!("≥{}叙事段", n);
        let carriers: [(&str, String); 5] = [
            ("CHECKLIST_D", rules::checklist("mode_d")),
            (
                "auditor_review_prompt",
                rules::interpolate(crate::commands::roles::auditor_review_prompt()),
            ),
            (
                "auditor",
                rules::interpolate(crate::commands::roles::auditor().system_prompt),
            ),
            (
                "emotion",
                rules::interpolate(crate::commands::roles::emotion().system_prompt),
            ),
            (
                "producer",
                rules::interpolate(crate::commands::roles::producer().system_prompt),
            ),
        ];
        for (name, text) in &carriers {
            let expect = if *name == "CHECKLIST_D" {
                &expect_checklist
            } else {
                &expect_role
            };
            assert!(
                text.contains(expect.as_str()),
                "上游告知 {} 未含常量派生的门槛串 \"{}\"",
                name,
                expect
            );
        }
    }

    /// §5.1#5 无遗留：生产段不得再出现已删文本终稿门符号
    #[test]
    fn no_legacy_text_terminal_validators() {
        let src = include_str!("validator.rs");
        let cut = src.find("#[cfg(test)]").unwrap_or(src.len());
        let prod = &src[..cut];
        for banned in [
            "fn validate_for_mode",
            "fn validate_production",
            "fn validate_douyin",
            "fn check_audio_influence",
            "fn param_range_issues",
            "fn declared_minimal_sections",
            "collect_hard_issues(",
        ] {
            assert!(!prod.contains(banned), "生产段仍含已退役符号：{banned}");
        }
    }
}
