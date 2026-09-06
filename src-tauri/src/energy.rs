//! 能量标注解析：validator 硬校验与 knowledge 按需注入共用。
//! 历史上 validator 与 knowledge 各持一份逐字符扫描拷贝（漂移温床），现统一至此。
//! 改动解析规则只改这里——两处消费方行为由各自存量测试锁定，漂移会被测试抓住。

/// 提取全部能量数值（0-10，只取含 能量/energy 标记的行）。
/// 段号跳过：数字前紧跟的字母串是结构词（Verse/Chorus 等）→ 跳过；
/// 英文 "energy 8" 格式中 8 前的字母串是 "energy" 本身 → 属于能量值，必须提取；
/// 中文标记（"能量:3"/"能量 3"）后数字前是标点/中文 → 提取。
pub(crate) fn extract_energy_values(text: &str) -> Vec<u32> {
    let mut values = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        if !lower.contains("能量") && !lower.contains("energy") {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                // 段号 vs 能量值判定：取数字前「最后一个词」（跳过空白后收集连续字母）。
                // "Verse 1" → 词为 Verse（段号，跳过）；"energy 8" → 词为 energy（能量值，提取）；
                // "能量:3"/"能量 3" → 数字前是标点/中文，词为空（提取）
                let mut letters = String::new();
                let mut k = start;
                while k > 0 && chars[k - 1].is_whitespace() {
                    k -= 1;
                }
                while k > 0 && chars[k - 1].is_ascii_alphabetic() {
                    letters.insert(0, chars[k - 1]);
                    k -= 1;
                }
                if let Ok(v) = token.parse::<u32>() {
                    let skip_as_section_no =
                        !letters.is_empty() && !letters.eq_ignore_ascii_case("energy");
                    if v <= 10 && !skip_as_section_no {
                        values.push(v);
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    values
}

/// 能量范围 (min, max)；无能量标注返回 None。
/// 兼容四种标注格式：能量:X / 能量 X / energy:X / energy X（与前端 parseEnergy 同格式族），
/// 只取含标记的行；区间值（如 energy 3-4）取两端各算一个值（min/max 覆盖）。
pub(crate) fn plan_energy_range(text: &str) -> Option<(u32, u32)> {
    let vals = extract_energy_values(text);
    match (vals.iter().min(), vals.iter().max()) {
        (Some(&a), Some(&b)) => Some((a, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四种标注格式（能量:/能量 /energy:/energy）全部兼容，min/max 正确
    #[test]
    fn plan_energy_range_supports_four_formats() {
        let plan = "Intro 能量:2，Verse 能量 4，Chorus energy:7，Final energy 9";
        assert_eq!(plan_energy_range(plan), Some((2, 9)));
        // 无能量标注 → None
        assert_eq!(plan_energy_range("BPM 120 深夜民谣"), None);
    }

    /// 段号不误算（"Verse 1" 的 1 跳过）、单值区间、大写形态兼容
    #[test]
    fn skips_section_numbers_and_accepts_case_variants() {
        assert_eq!(
            plan_energy_range("能量轨迹：Verse 1 energy 3，Pre-Chorus 2 energy 8"),
            Some((3, 8))
        );
        assert_eq!(plan_energy_range("说明行 能量:5"), Some((5, 5)));
        assert_eq!(
            plan_energy_range("能量轨迹：Verse 1 Energy 6，Chorus ENERGY 9"),
            Some((6, 9))
        );
    }

    /// 裸数值列表语义（validator 硬校验用）：只取标记行内的 0-10 数值
    #[test]
    fn extract_values_from_marked_lines_only() {
        assert_eq!(
            extract_energy_values("能量轨迹：Verse 1 能量 3，Chorus 能量 8\nBPM 120 速度很快"),
            vec![3, 8]
        );
        assert!(extract_energy_values("没有标记的行 3 和 8").is_empty());
    }
}
