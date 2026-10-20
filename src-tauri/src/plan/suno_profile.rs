//! RULE-SUNO-PROFILE（BLUEPRINT_v2 §3.3）：Suno 平台限制的唯一定义点。
//! 每个限制必须带事实来源（`Fact`）；P2 用户探针结果回填为 `UserProbed`。
//! 前端经 `get_suno_meta` 读取，不在 TS 抄。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fact {
    /// 公开资料测得（来源 + 日期）
    Measured { source: &'static str, date: &'static str },
    /// 用户在自己账号上探针确认（§7）
    UserProbed { date: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Limit {
    pub name: &'static str,
    pub value: u32,
    pub fact: Fact,
}

const RESEARCH: &str = "docs/research/2026-10-19-suno-v6-and-vocal-mix-scan.md（公开资料扫描；上限/标签区间待 §7 S1–S3/S4 用户探针确认 → UserProbed）";
const DATE: &str = "2026-10-20";

/// Style 栏字符上限
pub const SUNO_STYLE_MAX: Limit = Limit { name: "style_max_chars", value: 1000, fact: Fact::Measured { source: RESEARCH, date: DATE } };
/// Lyrics 栏字符上限
pub const SUNO_LYRICS_MAX: Limit = Limit { name: "lyrics_max_chars", value: 5000, fact: Fact::Measured { source: RESEARCH, date: DATE } };
/// Exclude 栏字符上限
pub const SUNO_EXCLUDE_MAX: Limit = Limit { name: "exclude_max_chars", value: 1000, fact: Fact::Measured { source: RESEARCH, date: DATE } };
/// Style 标签数区间
pub const SUNO_STYLE_TAGS_MIN: Limit = Limit { name: "style_tags_min", value: 8, fact: Fact::Measured { source: RESEARCH, date: DATE } };
pub const SUNO_STYLE_TAGS_MAX: Limit = Limit { name: "style_tags_max", value: 15, fact: Fact::Measured { source: RESEARCH, date: DATE } };
/// Variety 默认值（§6.1：短视频 Hook 稳定性取 0）
pub const VARIETY_DEFAULT: Limit = Limit { name: "variety_default", value: 0, fact: Fact::Measured { source: RESEARCH, date: DATE } };

/// 全部登记的限制（`get_suno_meta` 与守护测试共用）
pub const ALL_LIMITS: &[Limit] = &[
    SUNO_STYLE_MAX,
    SUNO_LYRICS_MAX,
    SUNO_EXCLUDE_MAX,
    SUNO_STYLE_TAGS_MIN,
    SUNO_STYLE_TAGS_MAX,
    VARIETY_DEFAULT,
];

// 编译期不变量：标签区间有序
const _: () = assert!(SUNO_STYLE_TAGS_MIN.value <= SUNO_STYLE_TAGS_MAX.value);

#[derive(Debug, Clone, Serialize)]
pub struct SunoMeta {
    pub limits: Vec<Limit>,
}

/// 前端读取 Suno 限制（单源）
#[tauri::command]
pub fn get_suno_meta() -> SunoMeta {
    SunoMeta { limits: ALL_LIMITS.to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §3.3 守护：每个限制都有非空事实来源；名称唯一；本文件所有 `Limit` 常量都已登记进 ALL_LIMITS
    #[test]
    fn suno_profile_every_limit_has_fact() {
        let mut names = std::collections::HashSet::new();
        for l in ALL_LIMITS {
            assert!(names.insert(l.name), "重复限制名 {}", l.name);
            match l.fact {
                Fact::Measured { source, date } => assert!(!source.is_empty() && !date.is_empty(), "{}", l.name),
                Fact::UserProbed { date } => assert!(!date.is_empty(), "{}", l.name),
            }
        }
        let src = include_str!("suno_profile.rs");
        let needle = [": Limit = ", "Limit {"].concat();
        let declared = src.matches(needle.as_str()).count();
        assert_eq!(declared, ALL_LIMITS.len(), "有 Limit 常量未登记进 ALL_LIMITS");
    }

    #[test]
    fn suno_meta_wire_shape() {
        let v = serde_json::to_value(get_suno_meta()).unwrap();
        let first = &v["limits"][0];
        assert_eq!(first["name"], "style_max_chars");
        assert_eq!(first["value"], 1000);
        assert_eq!(first["fact"]["kind"], "measured");
    }
}
