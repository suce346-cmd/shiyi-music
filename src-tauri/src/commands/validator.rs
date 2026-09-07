//! 代码校验器：对最终生成的文本做**确定性**硬校验。
//!
//! 不信任 LLM 的自检（同一模型自审有盲区），格式类规则由代码裁决。
//! 校验失败时返回具体问题清单供主持人重收口。
//!
//! ⚠️ 双源维护提醒：本文件的规则阈值引用 `crate::rules` 单源常量
//! （STYLE_PROMPT_MAX_CHARS/MIN_ENERGY_GAP/MIN_INSTRUMENT_GAP 等），
//! `roles.rs` 校验员 prompt 的格式规范通过 `rules::checklist` 同源拼接——
//! 改数值只改 `rules.rs`，改任一处必须跑单源一致单测，
//! 否则会出现「prompt 说合规、代码说不合规」的漂移。

// 能量解析唯一实现在 energy.rs（原先 validator/knowledge 各持一份拷贝）
use crate::energy::extract_energy_values;
use crate::rules;

/// 校验结果
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub passed: bool,
    pub issues: Vec<String>,
}

impl ValidationResult {
    fn ok() -> Self {
        Self { passed: true, issues: vec![] }
    }
    fn fail(issues: Vec<String>) -> Self {
        Self { passed: false, issues }
    }
}

/// 提取全部 `[...]` 结构标签（[Verse 1] / [Chorus] 等）
fn extract_section_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // 跳过说明行（含逗号的都是说明行，不是结构标签）
            if !trimmed.contains(',') {
                tags.push(trimmed.to_string());
            }
        }
    }
    tags
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
pub(crate) fn extract_style_prompt(text: &str) -> Option<String> {
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

/// 从说明行解析乐器数量（从尾部连续去掉空间/人声/力度尾项）
/// 说明行格式：[乐器1+行为, 乐器2+行为, ..., 空间/力度, 人声状态]
fn parse_section_instruments(line: &str) -> usize {
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let non_instrument_keywords = [
        "voice", "vocal", "人声", "声", "room", "空间", "hall", "大厅", "能量",
        "力度", "no voice", "无", "氛围", "atmosphere", "reverb", "混响",
    ];
    let mut count = parts.len();
    while count > 0 {
        if let Some(last) = parts.get(count - 1) {
            let l = last.to_lowercase();
            if non_instrument_keywords.iter().any(|k| l.contains(k)) {
                count -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    count
}

/// 提取全部（结构标签，说明行乐器数）对
fn extract_section_instrument_counts(text: &str) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut current_section: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') && !t.contains(',') {
            current_section = Some(t.to_string());
        } else if t.starts_with('[') && t.ends_with(']') && t.contains(',') {
            // 说明行：为该节配器数
            if let Some(section) = &current_section {
                result.push((section.clone(), parse_section_instruments(t)));
            }
        }
    }
    result
}

/// Mode A/B：校验生产方案（真硬校验）
pub fn validate_production(mode: &str, text: &str) -> ValidationResult {
    let mut issues = Vec::new();

    // 1. Style Prompt 长度 ≤ 单源上限
    match extract_style_prompt(text) {
        Some(sp) => {
            let len = sp.chars().count();
            if len > rules::STYLE_PROMPT_MAX_CHARS {
                issues.push(format!("Style Prompt 长度 {} 超限（> {}）", len, rules::STYLE_PROMPT_MAX_CHARS));
            }
        }
        None => issues.push("未找到 Style Prompt 字段".to_string()),
    }

    // 2. 结构标签必须存在（至少单源段落数）
    let tags = extract_section_tags(text);
    if tags.len() < rules::MIN_SECTION_TAGS {
        issues.push(format!("结构标签不足 {} 个（当前 {}）", rules::MIN_SECTION_TAGS, tags.len()));
    }

    // 3. 能量差 ≥ 单源下限（真解析数值，而非仅看关键词）
    let energy = extract_energy_values(text);
    if energy.is_empty() {
        issues.push("未发现能量标注（应包含 0-10 能量值）".to_string());
    } else {
        let min = energy.iter().min().copied().unwrap_or(0);
        let max = energy.iter().max().copied().unwrap_or(0);
        if max.saturating_sub(min) < rules::MIN_ENERGY_GAP {
            issues.push(format!(
                "能量差不足: 最弱 {} vs 最强 {}（要求差 >= {} 级）",
                min, max, rules::MIN_ENERGY_GAP
            ));
        }
    }

    // 4. 配器差 ≥ 单源下限 + 弱强段下限：解析每段说明行乐器数（Mode A/B 都校验）
    // Q2：此前只查差值与上限，弱段 1 件/强段 4 件可蒙混过关；现补 MIN_INSTRUMENT_WEAK/STRONG 两条下限。
    if mode == "mode_a" || mode == "mode_b" {
        let counts = extract_section_instrument_counts(text);
        if !counts.is_empty() {
            let min_count = counts.iter().map(|(_, c)| *c).min().unwrap_or(0);
            let max_count = counts.iter().map(|(_, c)| *c).max().unwrap_or(0);
            if max_count.saturating_sub(min_count) < rules::MIN_INSTRUMENT_GAP {
                issues.push(format!(
                    "配器差不足: 最少 {} 件 / 最多 {} 件（要求差 >= {} 件）",
                    min_count, max_count, rules::MIN_INSTRUMENT_GAP
                ));
            }
            if min_count < rules::MIN_INSTRUMENT_WEAK {
                issues.push(format!("最弱段配器 {} 件（要求 >= {} 件）", min_count, rules::MIN_INSTRUMENT_WEAK));
            }
            if max_count < rules::MIN_INSTRUMENT_STRONG {
                issues.push(format!("最强段配器 {} 件（要求 >= {} 件）", max_count, rules::MIN_INSTRUMENT_STRONG));
            }
            if max_count > rules::INSTRUMENT_MAX {
                issues.push(format!("配器过多: 最多 {} 件（要求 <= {}）", max_count, rules::INSTRUMENT_MAX));
            }
        } else {
            issues.push("未找到任何说明行（每段应含 [乐器1+行为, ...] 说明行）".to_string());
        }
        // K-3：Audio Influence=0 硬门（双向）——清单写了"Audio Influence=0"但旧校验不查，
        // 非 0 会让 Suno 匹配不存在的参考音频导致生成漂移（CSV audio_influence 行）。
        if let Some(msg) = check_audio_influence(text) {
            issues.push(msg);
        }
    }

    if issues.is_empty() { ValidationResult::ok() } else { ValidationResult::fail(issues) }
}

/// K-3：解析参数行 Audio Influence（Mode A/B 硬门）。双向：缺参数行 / 值非 0 / 无法解析都拦。
fn check_audio_influence(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("参数") && l.contains("Audio Influence"));
    let line = match line {
        Some(l) => l,
        None => return Some("缺参数行（末尾须输出 `参数: Weirdness=… | Style Influence=… | Audio Influence=0`）".to_string()),
    };
    let after = line.split("Audio Influence").nth(1).unwrap_or("");
    let num: String = after
        .trim_start()
        .trim_start_matches(['=', '：', ':'])
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    match num.parse::<f64>() {
        Ok(v) if v == 0.0 => None,
        Ok(v) => Some(format!("Audio Influence 须为 0（无参考音频，当前 {}）", v)),
        Err(_) => Some("参数行 Audio Influence 无法解析（应为 `Audio Influence=0`）".to_string()),
    }
}

/// Mode C 字数计数：去空白 + 去标点（Q2：与 prompts.rs“标点不计入字数”同口径；
/// 此前仅去空白，带问号/感叹号的行会被误报字数不符）。
fn count_lyric_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace() && !is_lyric_punct(*c)).count()
}

/// 歌词标点表（半角标点 + 中日韩常用标点；断句空格已由空白分支处理）。
fn is_lyric_punct(c: char) -> bool {
    c.is_ascii_punctuation() || matches!(c, '，' | '。' | '？' | '！' | '、' | '；' | '：' | '「' | '」' | '『' | '』' | '（' | '）' | '《' | '》' | '〈' | '〉' | '…' | '"' | '\'')
}

/// 裸包装行判定（Q2收窄：此前仅“半角逗号+>15字”，合法歌词长句可被吞；
/// 现加关键词双条件：含配器/空间/人声/能量词才算包装行）。
fn is_bare_package_line(l: &str) -> bool {
    let n = l.chars().filter(|c| !c.is_whitespace()).count();
    if !(l.contains(',') && n > 15) {
        return false;
    }
    let ll = l.to_lowercase();
    ["voice", "vocal", "人声", "room", "空间", "hall", "能量", "energy", "混响", "reverb", "氛围", "atmosphere", "guitar", "piano", "cello", "drums", "bass", "synth"].iter().any(|k| ll.contains(k))
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
        let actual: Vec<usize> = new_lines.iter().take(n).map(|l| count_lyric_chars(l)).collect();
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

    if issues.is_empty() { ValidationResult::ok() } else { ValidationResult::fail(issues) }
}

/// Mode D：校验抖音神曲
pub fn validate_douyin(text: &str) -> ValidationResult {
    let mut issues = Vec::new();
    let tags = extract_section_tags(text);

    // 1. 至少单源 Hook 次数
    let hook_count = tags.iter().filter(|t| t.contains("Hook")).count();
    if hook_count < rules::HOOK_MIN_COUNT {
        issues.push(format!("Hook 出现 {} 次（要求 >= {}）", hook_count, rules::HOOK_MIN_COUNT));
    }

    // 2. Verse 不超过单源行数（逐段结算——旧实现每遇新 Verse 重置计数，
    //    多段 Verse 只检查了最后一段，前面段落超行漏检）
    let lines: Vec<&str> = text.lines().collect();
    let mut verse_counts: Vec<(String, usize)> = Vec::new();
    let mut current_verse: Option<String> = None;
    let mut verse_line_count = 0usize;
    for line in lines.iter() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') && !t.contains(',') {
            // 遇到任何结构标签：先结算进行中的 Verse 段
            if let Some(label) = current_verse.take() {
                verse_counts.push((label, verse_line_count));
                verse_line_count = 0;
            }
            if t.contains("Verse") {
                current_verse = Some(t.trim_matches(|c| c == '[' || c == ']').to_string());
            }
        } else if current_verse.is_some() && !t.is_empty() && !(t.starts_with('[') && t.ends_with(']')) {
            // 排除说明行与裸包装行（Q2收窄判定，防吞真歌词）
            if !is_bare_package_line(t) {
                verse_line_count += 1;
            }
        }
    }
    // 文本结束：结算最后一个 Verse 段
    if let Some(label) = current_verse.take() {
        verse_counts.push((label, verse_line_count));
    }
    for (label, count) in &verse_counts {
        if *count > rules::VERSE_MAX_LINES {
            issues.push(format!("{} 行数 {} 超限（要求 <= {}）", label, count, rules::VERSE_MAX_LINES));
        }
    }

    // 3. 结尾骤停标记：全文包含 骤停 / abruptly / cut / [Drop] 结尾等
    let lower = text.to_lowercase();
    let has_stop = lower.contains("骤停")
        || lower.contains("abruptly")
        || lower.contains("cut off")
        || lower.contains("all instruments cut");
    if !has_stop {
        issues.push("缺少骤停标记（结尾应一刀切）".to_string());
    }

    // 3b. 说明行 ≤ 单源上限（Q2：此前仅 prompt/checklist 声称，代码零执行；现补硬门）。
    for line in text.lines().map(|l| l.trim()).filter(|l| l.starts_with('[') && l.ends_with(']') && l.contains(',')) {
        let n = line.chars().count();
        if n > rules::DOUYIN_DESC_LINE_MAX_CHARS {
            issues.push(format!("说明行超 {} 字符（{} 字符）: {}", rules::DOUYIN_DESC_LINE_MAX_CHARS, n, line.chars().take(30).collect::<String>()));
            break;
        }
    }

    // 4. 每行歌词 ≤ 单源字数（排除结构标签/说明行/Style Prompt/参数行）
    let lyric_lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with('['))
        .filter(|l| !l.starts_with('#'))
        .filter(|l| !l.starts_with('-'))
        .filter(|l| !l.starts_with('*'))
        .filter(|l| !l.starts_with("Style"))
        .filter(|l| !l.starts_with("风格"))
        .filter(|l| !l.starts_with("参数"))
        .filter(|l| !l.starts_with("Weirdness"))
        // 裸 Style Prompt/裸说明行识别（Q2收窄判定；无逗号超长行仍视为歌词行，由下方字数门报错）
        .filter(|l| !is_bare_package_line(l))
        .collect();
    let mut overlong = 0usize;
    for line in lyric_lines {
        // 去掉半角标点与空白（断句空格不计入字数，历史修复）
        let chars: String = line
            .chars()
            .filter(|c| !c.is_ascii_punctuation() && !c.is_whitespace())
            .collect();
        let count = chars.chars().count();
        if count > rules::DOUYIN_LINE_MAX_CHARS {
            overlong += 1;
            if overlong <= 3 {
                issues.push(format!("歌词行超 {} 字（{} 字）: {}", rules::DOUYIN_LINE_MAX_CHARS, count, line));
            }
        }
    }
    if overlong > 3 {
        issues.push(format!("共 {} 行歌词超 {} 字", overlong, rules::DOUYIN_LINE_MAX_CHARS));
    }

    if issues.is_empty() { ValidationResult::ok() } else { ValidationResult::fail(issues) }
}

/// 检查 BPM 是否满足模式要求（对齐原指令 prompts.rs：仅 mode_d 明确 "BPM>=90"（:736）；
/// mode_a/b 的调性节奏块（:94/:1009）无区间要求——不设代码区间，避免误杀原指令合法输出，
/// BPM 合理性由制作人讨论轮审查兜底）
pub fn check_bpm_range(mode: &str, bpm: u32) -> Option<String> {
    if mode == "mode_d" && bpm < rules::DOUYIN_BPM_MIN {
        Some(format!("BPM {} 低于抖音模式要求 {}（原指令 BPM>=90）", bpm, rules::DOUYIN_BPM_MIN))
    } else {
        None
    }
}

/// 检查 Style Prompt 是否过短（极端缺失提示）。
/// 对齐原指令 prompts.rs：信息块"选填，不需要填满所有块"（:102）、允许创造新流派（:85）——
/// 不设流派/乐器/人声词表强制（词表必然误杀原指令允许的合法流派，如喜剧/emo/自创方向），
/// 信息块完整性由制作人讨论轮审查（prompt 维度 1）把关。
pub fn check_style_prompt_blocks(style_prompt: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let chars = style_prompt.chars().count();
    if chars < rules::STYLE_PROMPT_MIN_CHARS {
        issues.push(format!("Style Prompt 过短（{} 字符），缺少信息块", chars));
    }
    issues
}

/// 按模式分发校验
pub fn validate_for_mode(mode: &str, text: &str, extra: Option<&str>) -> ValidationResult {
    match mode {
        "mode_a" | "mode_b" => validate_production(mode, text),
        "mode_c" => {
            match extra {
                Some(original) => validate_lyric_fill(original, text),
                None => ValidationResult::fail(vec!["Mode C 校验需要原歌词（extra 参数缺失）".to_string()]),
            }
        }
        "mode_d" => validate_douyin(text),
        _ => ValidationResult::fail(vec![format!("未知模式: {}", mode)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_prompt_too_long_fails() {
        let long_prompt = format!("**Style Prompt**: {}", "a,".repeat(200));
        let r = validate_production("mode_a", &long_prompt);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("350")));
    }

    /// 标签回退（B 实测根治）：模型丢标签但首行写风格正文时仍能提取——
    /// v0.5.0 实网 B 模式 3 连打回均因"未找到 Style Prompt 字段"
    #[test]
    fn style_prompt_fallback_first_line_without_label() {
        let text = "潮湿室内民谣, 男中音气声低语, 钢琴与尼龙弦吉他\n[Intro]\n[felt piano, room tone, 能量:2]\n雨声 先落下来";
        assert_eq!(
            extract_style_prompt(text).as_deref(),
            Some("潮湿室内民谣, 男中音气声低语, 钢琴与尼龙弦吉他"),
        );
        // 带标签的文本标签优先（回退不劫持正常路径）
        let labeled = "Style Prompt: 深夜室内民谣\n[Intro]\n[felt piano]";
        assert_eq!(extract_style_prompt(labeled).as_deref(), Some("深夜室内民谣"));
    }

    /// 回退边界：首行是结构标签/说明行/参数行时不误提取（维持 None → 走"未找到"打回）
    #[test]
    fn style_prompt_fallback_skips_structural_lines() {
        let section_first = "[Intro]\n[felt piano, room tone, 能量:2]\n雨声 先落下来";
        assert_eq!(extract_style_prompt(section_first), None);
        let params_first = "参数: Weirdness=25 | Style Influence=80 | Audio Influence=0\n[Intro]";
        assert_eq!(extract_style_prompt(params_first), None);
    }

    #[test]
    fn production_with_no_energy_fails() {
        let text = "**Style Prompt**: dark indie folk 60bpm\n[Verse 1]\n[acoustic guitar, cello]\n歌词";
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("能量")));
    }

    #[test]
    fn lyric_fill_mismatch_detected() {
        let original = "我们 很早前 就 谋过面\n我一直 带给你 麻烦不断";
        let new = "我们 很早前 就 谋过面\n我一直 带给你 麻烦不断啊";
        let r = validate_lyric_fill(original, new);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("字数不符")));
    }

    #[test]
    fn lyric_fill_matching_passes() {
        let original = "我们 很早前 就 谋过面\n我一直 带给你 麻烦不断";
        let new = "你们 很晚后 也 想过我\n她总是 送给你 快乐满满";
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// 断句空格差异不计入字数（去空白对齐）
    #[test]
    fn lyric_fill_whitespace_diff_passes() {
        let original = "我们 很早前 就 谋过面";
        let new = "你们很晚后也想过我"; // 无断句空格，去空白后字数一致
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// 标准提示词包的包装行（Style Prompt/参数/结构标签/说明行）不参与字数比对
    #[test]
    fn lyric_fill_ignores_package_wrapper_lines() {
        let original = "我们 很早前 就 谋过面\n我一直 带给你 麻烦不断";
        let new = "Style Prompt: 抒情流行, 80BPM\n[Verse 1]\n[钢琴+弦乐, 温暖空间]\n你们很晚后也想过我\n她总是送给你快乐满满\n参数: Weirdness=20 | Style Influence=80 | Audio Influence=0";
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    #[test]
    fn douyin_requires_two_hooks() {
        let text = "[Hook]\n我 真的 会谢\n[Verse]\n一行\n[Chorus]\n我 真的 会谢\n[Drop]";
        let r = validate_douyin(text);
        // 只有 2 个 Hook 标签（[Hook] + [Chorus]？不，Chorus 不算 Hook）
        assert!(!r.passed);
    }

    #[test]
    fn douyin_valid_passes() {
        let text = "[Hook]\n我 真的 会谢\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Verse]\n一行\n[Hook]\n[all instruments cut abruptly]";
        let r = validate_douyin(text);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    #[test]
    fn mode_c_requires_extra() {
        let r = validate_for_mode("mode_c", "some lyrics", None);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("原歌词")));
    }

    #[test]
    fn section_tag_extraction_skips_description_lines() {
        let text = "[Verse 1]\n[acoustic guitar, cello]\n歌词\n[Chorus]";
        let tags = extract_section_tags(text);
        assert_eq!(tags, vec!["[Verse 1]", "[Chorus]"]);
    }

    #[test]
    fn unknown_mode_fails() {
        let r = validate_for_mode("mode_x", "text", None);
        assert!(!r.passed);
    }

    /// 多段 Verse 逐段结算——第一段超行必须被检出（旧实现只查最后一段，漏检）
    #[test]
    fn douyin_multi_verse_each_section_checked() {
        // Verse 1 六行超限 + Verse 2 两行合规：旧实现只看最后一段会放行
        let text = "[Hook]\n我 真的 会谢\n[Verse]\n这一行\n这一行\n这一行\n这一行\n这一行\n这一行\n[Hook]\n我 真的 会谢\n[Verse]\n一行\n一行\n[Hook]\n[all instruments cut abruptly]";
        let r = validate_douyin(text);
        assert!(!r.passed, "Verse 1 六行必须被检出: {:?}", r.issues);
        assert!(
            r.issues.iter().any(|i| i.contains("Verse") && i.contains("6")),
            "issue 应带段名与行数: {:?}",
            r.issues
        );
    }

    /// 回归：两段 Verse 各 4 行（合法）不得误报
    #[test]
    fn douyin_multi_verse_legal_passes() {
        let text = "[Hook]\n我 真的 会谢\n[Verse]\n一\n二\n三\n四\n[Hook]\n我 真的 会谢\n[Verse]\n一\n二\n三\n四\n[Hook]\n[all instruments cut abruptly]";
        let r = validate_douyin(text);
        assert!(!r.issues.iter().any(|i| i.contains("超限")), "4 行 Verse 不应报超限: {:?}", r.issues);
    }

    /// 一份合规的 Mode A 产出应能通过（正样：弱段 3 件、强段 5 件，满足单段 3-7 下限）
    #[test]
    fn production_valid_passes() {
        let text = r#"**Style Prompt**: dark indie folk 60BPM F#小调
[Verse 1]
[acoustic guitar fingerpicked, cello soft pads, brushed drums keep time, intimate room]
我们 很早前 就 谋过面
[Chorus]
[acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses, wide hall]
我梦过 你的未来
能量轨迹：Verse 1 能量 3，Chorus 能量 8
参数: Weirdness=25 | Style Influence=80 | Audio Influence=0"#;
        let r = validate_production("mode_a", text);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// K-3：Audio Influence 非 0 必须被拦（旧规则清单写了但不查）
    #[test]
    fn production_audio_influence_nonzero_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, cello soft pads, brushed drums keep time, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses, wide hall]
能量轨迹：Verse 1 能量 3，Chorus 能量 8
参数: Weirdness=25 | Style Influence=80 | Audio Influence=30"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("Audio Influence")), "issues: {:?}", r.issues);
    }

    /// K-3：缺参数行同样被拦（双向门）
    #[test]
    fn production_audio_param_line_missing_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, cello soft pads, brushed drums keep time, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses, wide hall]
能量轨迹：Verse 1 能量 3，Chorus 能量 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("缺参数行")), "issues: {:?}", r.issues);
    }

    /// L-3：弱段 2 件必须被揪出（单段下限统一为 3 后，旧正样"2 件"不再合法）
    #[test]
    fn production_weak_two_instruments_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, cello soft pads, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses, wide hall]
能量轨迹：Verse 1 能量 3，Chorus 能量 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains(&format!("要求 >= {} 件", rules::MIN_INSTRUMENT_WEAK))), "issues: {:?}", r.issues);
    }

    /// Q2：最弱段 1 件必须被揪出（此前只查差值可蒙混）
    #[test]
    fn production_weak_floor_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses, wide hall]
能量轨迹：Verse 1 能量 3，Chorus 能量 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("最弱段配器")), "issues: {:?}", r.issues);
    }

    /// Q2：最强段 4 件必须被揪出（差值够、上限够也不行）
    #[test]
    fn production_strong_floor_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, cello soft pads, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello bowing, warm piano, light drums, wide hall]
歌词行
能量轨迹：Verse 1 能量 3，Chorus 能量 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("最强段配器")), "issues: {:?}", r.issues);
    }

    /// Q2：Mode D 说明行超 80 字必须被揪出
    #[test]
    fn douyin_desc_line_over_80_fails() {
        let long_desc = format!("[{}, wide hall, aggressive male voice]", "acoustic guitar strummed, cello dark bowing, warm piano cushions, light drums, deep bass pulses");
        let text = format!("[Hook]\n我 真的 会谢\n我 真的 会谢\n[Hook]\n{}\n我 真的 会谢\n[Verse]\n一行\n[Hook]\n[all instruments cut abruptly]", long_desc);
        let r = validate_douyin(&text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("说明行超")), "issues: {:?}", r.issues);
    }

    /// Q2：标点不计字数——带问号的对齐行不应误报
    #[test]
    fn lyric_fill_punct_ignored() {
        let original = "我们 很早前 就 谋过面？\n我一直 带给你 麻烦不断！";
        let new = "你们 很晚后 也 想过我\n她总是 送给你 快乐满满";
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// Q2：含半角逗号但无关键词的长歌词行不得被吞
    #[test]
    fn lyric_fill_long_line_with_comma_kept() {
        let original = "这是一个很长很长的歌词行，中间有个逗号但没有乐器词";
        let new = "这是另一句很长很长歌词行，中间有个逗号但没有配器词";
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// 能量差不足（<3 级）应被揪出
    #[test]
    fn production_energy_gap_too_small_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar fingerpicked, intimate room]
歌词行
[Chorus]
[acoustic guitar strummed, cello bowing]
歌词行
能量轨迹：Verse 1 能量 5，Chorus 能量 6"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("能量差")));
    }

    /// 配器差不足（<2 件）应被揪出
    #[test]
    fn production_instrument_gap_too_small_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Verse 1]
[acoustic guitar, cello, room]
歌词行
[Chorus]
[acoustic guitar, cello, piano, hall]
歌词行
能量轨迹：Verse 1 能量 3，Chorus 能量 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("配器差")));
    }

    /// 配器超过 7 件应被揪出
    #[test]
    fn production_instrument_overload_fails() {
        let text = r#"**Style Prompt**: dark indie folk
[Chorus]
[guitar, piano, cello, drums, bass, strings, brass, flute, sax, room]
歌词行
能量轨迹：Verse 能量 3，Chorus 能量 9"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("配器过多")));
    }

    /// 每行歌词 >10 字应被 Mode D 揪出
    #[test]
    fn douyin_line_over_10_chars_fails() {
        let text = "[Hook]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Verse]\n这一行歌词实在是太长了超过了十个字\n[Hook]\n[all instruments cut abruptly]";
        let r = validate_douyin(text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("超 10 字")));
    }

    /// 能量值解析：只在含能量标记的行提取
    #[test]
    fn energy_values_extracted_only_from_marked_lines() {
        let text = "能量轨迹：Verse 1 能量 3，Chorus 能量 8\nBPM 120 速度很快";
        let values = extract_energy_values(text);
        assert_eq!(values, vec![3, 8]);
    }

    /// 能量格式兼容：英文无冒号 "energy 8" 必须提取（旧逻辑误判为段号跳过）
    #[test]
    fn energy_values_extract_english_no_colon() {
        let text = "能量轨迹：Verse 1 energy 3，Chorus energy 8";
        let values = extract_energy_values(text);
        assert_eq!(values, vec![3, 8], "energy 后无冒号的值必须提取");
    }

    /// 段号跳过仍生效：结构词后数字不提取（"Verse 1" 的 1 不是能量）
    #[test]
    fn energy_values_still_skip_section_numbers() {
        let text = "能量轨迹：Verse 1 能量 5，Pre-Chorus 2 能量 9";
        let values = extract_energy_values(text);
        assert_eq!(values, vec![5, 9], "Verse/Pre-Chorus 的段号必须跳过");
    }

    /// 大小写兼容：Energy/ENERGY 大写形态同样提取（eq_ignore_ascii_case）
    #[test]
    fn energy_values_case_insensitive() {
        let text = "能量轨迹：Verse 1 Energy 4，Chorus ENERGY 8";
        let values = extract_energy_values(text);
        assert_eq!(values, vec![4, 8], "Energy 大写形态必须提取");
    }

    /// Mode C：原歌词含 >15 字长行（无逗号）时不得被当说明行过滤（字数对齐仍然生效）
    #[test]
    fn lyric_fill_long_line_without_comma_not_filtered() {
        let original = "这是一句非常非常长的歌词行没有任何逗号超过十五个字";
        let new = "这是一句很长的歌词行没有加逗号超过十五个字整";
        let r = validate_lyric_fill(original, new);
        // 字数不一致（15 vs 14）→ 应报字数不符；但不得报"歌词行数不足/未找到"（长行未被误过滤）
        assert!(r.issues.iter().any(|i| i.contains("字数不符")), "应报字数不符: {:?}", r.issues);
        assert!(!r.issues.iter().any(|i| i.contains("行数不足")), "长行被误过滤: {:?}", r.issues);
    }

    /// 说明行乐器数解析：去人声/空间尾项
    #[test]
    fn section_instrument_count_parses() {
        assert_eq!(parse_section_instruments("acoustic guitar, cell, warm bass, intimate room, voice hesitant"), 3);
        assert_eq!(parse_section_instruments("piano and cello only"), 1); // 无逗号 = 1 组
    }

    // ---- 边界分支补测（覆盖率）----

    #[test]
    fn style_prompt_missing_is_issue() {
        let text = "[Verse 1]\n说明行\n歌词\n[Chorus]\n能量轨迹: 3 vs 8";
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("未找到 Style Prompt")));
    }

    #[test]
    fn no_section_line_is_issue() {
        let text = r#"**Style Prompt**: x
[Verse 1]
歌词行
[Chorus]
能量轨迹: 3 vs 8"#;
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("未找到任何说明行")));
    }

    #[test]
    fn lyric_fill_empty_original_fails() {
        let r = validate_lyric_fill("", "新歌词");
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("为空")));
    }

    #[test]
    fn lyric_fill_line_count_mismatch_detected() {
        let original = "第一行\n第二行";
        let new = "第一行";
        let r = validate_lyric_fill(original, new);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("行数不足")));
    }

    /// 允许尾部 ≤2 行收尾（如 Outro 一句），前 N 行仍逐行对齐
    #[test]
    fn lyric_fill_trailing_short_tail_passes() {
        let original = "异乡 的 夜 格外 长\n想念 故乡 的 月亮\n妈妈 做的 饭菜 香\n梦里 回到 她 身旁";
        let new = "工位窗外雪纷扬\n想起母亲的灶膛\n腊肉香肠挂满梁\n梦里回到她身旁\n车票攥紧在手上"; // 4 对齐 + 1 收尾
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    /// 超过 2 行尾部 → 报"行数过多"
    #[test]
    fn lyric_fill_too_many_extra_lines_fails() {
        let original = "第一行\n第二行";
        let new = "一行新\n二行新\n尾一\n尾二\n尾三";
        let r = validate_lyric_fill(original, new);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("行数过多")));
    }

    /// 裸说明行（无方括号）不被计入歌词行
    #[test]
    fn lyric_fill_ignores_bare_description_lines() {
        let original = "我们 很早前 就 谋过面\n我一直 带给你 麻烦不断";
        let new = "felt piano 低音区铺垫孤独, 空间空旷, 人声低沉叙事\n你们很晚后也想过我\n她总是送给你快乐满满";
        let r = validate_lyric_fill(original, new);
        assert!(r.passed, "issues: {:?}", r.issues);
    }

    #[test]
    fn lyric_fill_many_mismatches_condensed() {
        // 超过 3 处字数不符 → 汇总提示
        let original = "一\n二\n三\n四\n五";
        let new = "十一\n十二\n十三\n十四\n十五";
        let r = validate_lyric_fill(original, new);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("共") || i.contains("行")));
    }

    #[test]
    fn douyin_many_overlong_condensed() {
        let mut text = String::from("[Hook]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Verse]\n");
        for _ in 0..5 {
            text.push_str("这一行真的超级无敌巨长超过了十个字\n");
        }
        text.push_str("[Hook]\n[all instruments cut abruptly]\n能量轨迹行");
        let r = validate_douyin(&text);
        assert!(!r.passed);
    }

    /// 超过 3 处超长行 → 汇总提示（不逐条报）
    #[test]
    fn douyin_many_overlong_condensed_msg() {
        let mut text = String::from("[Hook]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n\n[Hook]\n[all instruments cut abruptly]\n");
        for i in 0..6 {
            text.push_str(&format!("第{}行实在是太长太长了超过了十个字限制呀\n", i));
        }
        let r = validate_douyin(&text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("超 10 字")));
    }

    // ---- 最后 4 行边界 ----

    /// Style Prompt 冒号后为空 → 视为未找到（跳过空 body）
    #[test]
    fn style_prompt_empty_body_is_missing() {
        let text = "**Style Prompt**:\n[Verse 1]\n[a, b, room]\n歌词\n[Chorus]\n能量轨迹: 3 vs 8";
        let r = validate_production("mode_a", text);
        assert!(!r.passed);
        assert!(r.issues.iter().any(|i| i.contains("未找到 Style Prompt")));
    }

    /// 配器尾项全为非乐器词（无 break 场景：连续空间词）
    #[test]
    fn section_instrument_count_all_non_instrument() {
        // 全部为空间/人声词 → 0
        assert_eq!(parse_section_instruments("intimate room, reverb, voice"), 0);
        // 正常混合 → 非乐器词在尾部被剥掉
        assert_eq!(parse_section_instruments("cello, piano, warm hall, voice soft"), 2);
    }

    /// mode_c 带 extra 走正常校验路径
    #[test]
    fn mode_c_with_extra_runs_lyric_fill() {
        let original = "我们 很早前 就 谋过面";
        let good = "你们 很晚后 也 想过我";
        let bad = "我们 很早前 就 谋过面啊";
        let r_ok = validate_for_mode("mode_c", good, Some(original));
        assert!(r_ok.passed, "issues: {:?}", r_ok.issues);
        let r_bad = validate_for_mode("mode_c", bad, Some(original));
        assert!(!r_bad.passed);
    }

    #[test]
    fn bpm_check_only_mode_d() {
        // 对齐原指令：仅 mode_d 要求 BPM>=90（prompts.rs:736）；其他模式不设代码区间（原指令无要求）
        assert!(check_bpm_range("mode_d", 90).is_none(), "mode_d 90 应合法（BPM>=90）");
        assert!(check_bpm_range("mode_d", 89).is_some(), "mode_d 89 应报错");
        // mode_a/b 任意 BPM 不报（原指令无区间；合理性由制作人审查兜底）
        assert!(check_bpm_range("mode_a", 120).is_none(), "mode_a 120 不应报错（原指令无区间）");
        assert!(check_bpm_range("mode_b", 180).is_none(), "mode_b 180 不应报错（原指令无区间）");
        assert!(check_bpm_range("mode_c", 70).is_none());
    }

    #[test]
    fn style_prompt_short_detected() {
        let issues = check_style_prompt_blocks("民谣");
        assert!(issues.iter().any(|i| i.contains("过短")));
    }

    #[test]
    fn style_prompt_comedy_allowed() {
        // 对齐原指令：喜剧/搞笑是合法流派（prompts.rs:713"喜剧/整活方向"）、可创造新方向（:85）
        // 信息块选填（:102）——只要不短就不报
        let issues = check_style_prompt_blocks("喜剧整活, 130BPM, 滑稽滑音合成器, 弹拨贝斯, 密集军鼓, 痞气半说半唱, 拥挤商场混响");
        assert!(issues.is_empty(), "喜剧流派不应误报: {:?}", issues);
    }

    #[test]
    fn style_prompt_emo_allowed() {
        let issues = check_style_prompt_blocks("emo破碎, 110BPM, 延迟吉他, 氛围垫, 稀疏鼓, 近麦气声");
        assert!(issues.is_empty(), "emo 流派不应误报: {:?}", issues);
    }

    /// 裸 Style Prompt/裸说明行（无 "Style Prompt:" 前缀、无方括号）不算歌词行（LLM 输出波动兜底）
    #[test]
    fn douyin_ignores_bare_style_prompt_line() {
        // (a) 含逗号 >15 字裸行被忽略（Style Prompt 裸行）
        let text = "黑色幽默喜剧, 120BPM 4/4拍, 滑音合成器+808底鼓+细碎hi-hat+人声采样, 30岁男声痞气半说半唱, 拥挤商场混响+窃窃私语底噪, 高开骤停两连击, 抖音神曲魔性\n[Hook]\n[suona blast, 808 slide bass, full energy, aggressive male voice]\n我 真的 会谢\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[all instruments cut abruptly]\n参数: Weirdness=15 | Style Influence=90 | Audio Influence=0";
        let r = validate_douyin(text);
        assert!(!r.issues.iter().any(|i| i.contains("超 10 字")), "issues: {:?}", r.issues);
        // (b) 含逗号 ≤15 字行仍按歌词检查（"妈妈，好吗" 4 字合法）
        let text2 = "[Hook]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Verse]\n妈妈，好吗\n[Hook]\n[all instruments cut abruptly]";
        let r2 = validate_douyin(text2);
        assert!(!r2.issues.iter().any(|i| i.contains("超 10 字")), "issues2: {:?}", r2.issues);
        // (c) 无逗号超长行仍报超 10 字（不被裸行规则豁免）
        let text3 = "[Hook]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Verse]\n这一行歌词实在是太长了超过了十个字\n[Hook]\n[all instruments cut abruptly]";
        let r3 = validate_douyin(text3);
        assert!(r3.issues.iter().any(|i| i.contains("超 10 字")), "issues3: {:?}", r3.issues);
    }

    /// Style Prompt 行 / 参数行不得计入歌词行（Mode D ≤10 字检查误杀修复）
    #[test]
    fn douyin_ignores_style_and_params_lines() {
        let text = "Style Prompt: 深夜室内民谣, 68 BPM 慢速 D小调, felt piano 与指弹吉他交织, 温暖木质氛围, 疲惫沙哑念白
[Verse 1]
[钢琴单音, 雨声采样, 空旷, 能量:2]
雨敲窗 一声一声慢

参数: Weirdness=25 | Style Influence=80 | Audio Influence=0";
        let r = validate_douyin(text);
        // 歌词行 4 字/6 字都不超长；Style/参数行不再被误计
        assert!(!r.issues.iter().any(|i| i.contains("超 10 字")), "issues: {:?}", r.issues);
}
}
