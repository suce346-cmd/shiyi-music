//! P1-13 文本评估旁路测试。
//!
//! 离线：解析/校准/默认关闭/20 条池（进常规 cargo test）。
//! 在线全矩阵：SHIYI_TEXT_EVAL=1 + SHIYI_TEST_* 凭据，且勿写钥匙串。
//!   cargo test --test text_eval -- --ignored --nocapture

use suno_prompt_generator_lib::text_eval::{
    aggregate_report, agreement_rate, calibration_trusted, judge_enabled, parse_judge_response,
    FROZEN_PROMPTS_20, DIMENSIONS,
};

#[test]
fn default_off_never_enabled() {
    assert!(!judge_enabled(None));
    assert!(!judge_enabled(Some(false)));
}

#[test]
fn frozen_prompts_match_doc_count() {
    assert_eq!(FROZEN_PROMPTS_20.len(), 20);
    let doc = include_str!("../../docs/qa/text-eval-prompts-20.md");
    for p in FROZEN_PROMPTS_20 {
        assert!(doc.contains(p), "文档缺提示词: {p}");
    }
}

#[test]
fn parse_and_aggregate_smoke() {
    let body = format!(
        r#"{{"dimensions":[{}]}}"#,
        DIMENSIONS
            .iter()
            .map(|id| format!(r#"{{"id":"{id}","score":1,"note":""}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let s = parse_judge_response(&body).expect("parse");
    assert_eq!(s.total, 7);
    let r = aggregate_report(&[s]);
    assert_eq!(r.n, 1);
    assert!(r.small_sample);
}

#[test]
fn calibration_gate_matches_r10() {
    assert!(!calibration_trusted(40, 0.799));
    assert!(calibration_trusted(40, 0.80));
    let a = parse_judge_response(&full_scores(2)).unwrap();
    let b = parse_judge_response(&full_scores(2)).unwrap();
    assert!((agreement_rate(&a, &b) - 1.0).abs() < 1e-9);
}

fn full_scores(v: u8) -> String {
    format!(
        r#"{{"dimensions":[{}]}}"#,
        DIMENSIONS
            .iter()
            .map(|id| format!(r#"{{"id":"{id}","score":{v},"note":""}}"#))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// 在线占位：未设 SHIYI_TEXT_EVAL 时忽略；开启后由操作者在本机跑矩阵（不进 CI）。
#[tokio::test]
#[ignore = "需 SHIYI_TEXT_EVAL=1 与 SHIYI_TEST_*；成本高，旁路"]
async fn online_matrix_requires_explicit_env() {
    if std::env::var("SHIYI_TEXT_EVAL").ok().as_deref() != Some("1") {
        eprintln!("skip: SHIYI_TEXT_EVAL!=1");
        return;
    }
    let key = std::env::var("SHIYI_TEST_API_KEY").expect("SHIYI_TEST_API_KEY");
    let base = std::env::var("SHIYI_TEST_BASE_URL").expect("SHIYI_TEST_BASE_URL");
    let model = std::env::var("SHIYI_TEST_MODEL").expect("SHIYI_TEST_MODEL");
    assert!(!key.is_empty() && !base.is_empty() && !model.is_empty());
    // 全量 20×4×2 留给操作手册；此处只断言 opt-in 环境齐备
    eprintln!("text_eval online env ok; run full matrix via dedicated script when ready");
}
