//! RULE-STYLE-ENGLISH（BLUEPRINT_v2 §3.7 / D12）
//! Style/Exclude 恒英文；仅 allow 表登记的中文术语可保留。不读 output_lang。

use super::validate::Issue;
use super::SongPlan;

/// allow 表行（`knowledge/style_cjk_allow.csv`）。空表 = 任何 CJK token 均违规（P2-S12 前缺省）。
#[derive(Debug, Clone)]
pub struct AllowRow {
    pub term_cn: String,
    pub suggest_en: String,
    pub probe_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleTokenOk {
    /// 纯非 CJK（或空）
    Ok,
    /// 含 CJK 且命中 allow
    AllowedCjk { term: String },
    /// 含 CJK 未登记
    Violation { token: String },
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{3000}'..='\u{303F}'
        )
    })
}

/// 解析内嵌 CSV（跳过空行；表头必含 term_cn）。
pub fn load_allow_rows(csv: &str) -> Vec<AllowRow> {
    let mut rows = Vec::new();
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return rows;
    };
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let idx = |name: &str| cols.iter().position(|c| *c == name);
    let i_term = idx("term_cn");
    let i_en = idx("suggest_en");
    let i_probe = idx("probe_id");
    let (Some(i_term), Some(i_en), Some(i_probe)) = (i_term, i_en, i_probe) else {
        return rows;
    };
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        let get = |i: usize| parts.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let term = get(i_term);
        if term.is_empty() {
            continue;
        }
        rows.push(AllowRow {
            term_cn: term,
            suggest_en: get(i_en),
            probe_id: get(i_probe),
        });
    }
    rows
}

fn embedded_allow() -> Vec<AllowRow> {
    load_allow_rows(include_str!("../../knowledge/style_cjk_allow.csv"))
}

/// 单 token 判定（token = Style 标签块或 Exclude 逗号项，已 trim）。
pub fn style_token_ok(token: &str) -> StyleTokenOk {
    let t = token.trim();
    if t.is_empty() || !has_cjk(t) {
        return StyleTokenOk::Ok;
    }
    for row in embedded_allow() {
        if t == row.term_cn || t.contains(&row.term_cn) {
            return StyleTokenOk::AllowedCjk {
                term: row.term_cn,
            };
        }
    }
    StyleTokenOk::Violation {
        token: t.to_string(),
    }
}

/// 收集 Style（`style_tags`）与 Exclude（`global.negative`）及段内 cue 中的违规。
pub fn validate_style_english(p: &SongPlan) -> Vec<Issue> {
    let mut out = Vec::new();
    for (i, tag) in super::render_suno::style_tags(p).iter().enumerate() {
        if let StyleTokenOk::Violation { token } = style_token_ok(tag) {
            out.push(Issue::hard(
                "style_not_english",
                format!("/global/style_tags/{i}"),
                format!("Style 标签含未登记中文「{token}」（§3.7；仅 allow 表可保留）"),
            ));
        }
    }
    for (i, n) in p.global.negative.iter().enumerate() {
        if let StyleTokenOk::Violation { token } = style_token_ok(n) {
            out.push(Issue::hard(
                "exclude_not_english",
                format!("/global/negative/{i}"),
                format!("Exclude 含未登记中文「{token}」（§3.7）"),
            ));
        }
    }
    for (si, s) in p.sections.iter().enumerate() {
        for (j, c) in s.positive_local.iter().enumerate() {
            if let StyleTokenOk::Violation { token } = style_token_ok(c) {
                out.push(Issue::hard(
                    "style_not_english",
                    format!("/sections/{si}/positive_local/{j}"),
                    format!("段内正向 cue 含未登记中文「{token}」（§3.7）"),
                ));
            }
        }
        for (j, c) in s.negative_local.iter().enumerate() {
            if let StyleTokenOk::Violation { token } = style_token_ok(c) {
                out.push(Issue::hard(
                    "style_not_english",
                    format!("/sections/{si}/negative_local/{j}"),
                    format!("段内负向 cue 含未登记中文「{token}」（§3.7）"),
                ));
            }
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::fixtures::minimal_plan;
    use crate::plan::validate::ValidateCtx;
    use crate::plan::LyricLang;

    #[test]
    fn pure_english_ok() {
        assert_eq!(style_token_ok("clear diction"), StyleTokenOk::Ok);
        assert_eq!(style_token_ok("90 BPM"), StyleTokenOk::Ok);
    }

    #[test]
    fn cjk_without_allow_is_violation() {
        match style_token_ok("戏腔") {
            StyleTokenOk::Violation { token } => assert_eq!(token, "戏腔"),
            other => panic!("expected Violation, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_cjk_in_style() {
        let mut p = minimal_plan();
        p.global.genre = vec!["民谣".into()];
        let issues = validate_style_english(&p);
        assert!(
            issues.iter().any(|i| i.code == "style_not_english"),
            "{issues:?}"
        );
    }

    #[test]
    fn style_english_rule_independent_of_output_lang() {
        let mut zh = minimal_plan();
        zh.mode_meta.lyrics_lang = LyricLang::Zh;
        let mut en = zh.clone();
        en.mode_meta.lyrics_lang = LyricLang::En;
        en.sections[1].lines = vec!["rain on the street".into(), "lights still on".into()];
        let rz = crate::plan::render_suno::render(&zh);
        let re = crate::plan::render_suno::render(&en);
        assert_eq!(rz.style, re.style, "Style must ignore lyric language");
        assert_eq!(rz.exclude, re.exclude);
        let iz = crate::plan::validate::validate(
            &zh,
            &ValidateCtx {
                mode: "mode_b",
                converged: None,
                original_lyrics: None,
            },
        );
        let ie = crate::plan::validate::validate(
            &en,
            &ValidateCtx {
                mode: "mode_b",
                converged: None,
                original_lyrics: None,
            },
        );
        let pick = |v: &[Issue]| -> Vec<String> {
            v.iter()
                .filter(|i| i.code == "style_not_english" || i.code == "exclude_not_english")
                .map(|i| i.line())
                .collect()
        };
        assert_eq!(pick(&iz), pick(&ie));
    }

    #[test]
    fn style_cjk_allow_rows_have_probe_id() {
        for row in embedded_allow() {
            assert!(
                !row.probe_id.is_empty(),
                "allow 行「{}」缺 probe_id（须 P2-S12 实测后入表）",
                row.term_cn
            );
        }
    }
}
