//! 无头离线测试：零网络，CI 可跑。
//! 覆盖四模式座位/动态语义、反馈路由、硬校验助手（§5.1#5 后终稿门 = plan::validate）。
//! 与单元测试的区别：本文件从外部 crate 视角断言公开 API 的契约（座位数、校验口径），
//! 防止 R6 这类"名单口径"回归。

use suno_prompt_generator_lib::commands::orchestrator::{
    roles_for_feedback, seats_for_mode, steps_for_mode,
};
use suno_prompt_generator_lib::commands::validator::{
    style_prompt_length_issues, validate_lyric_fill,
};
use suno_prompt_generator_lib::models::{Mode, PipelineRole};
use suno_prompt_generator_lib::plan::fixtures::minimal_plan;
use suno_prompt_generator_lib::plan::validate::{validate, ValidateCtx};
use suno_prompt_generator_lib::plan::SectionKind;
use suno_prompt_generator_lib::rules;

/// R6：四模式座位含主持/校验落座（A=4、B=5、C=3、D=6；Q6：C 摘制作人）
#[test]
fn seats_include_host_and_auditor() {
    let a = seats_for_mode(&Mode::ModeA);
    assert_eq!(
        a.len(),
        4,
        "Mode A 座位应为 4（情感+制作+主持+校验），实际 {:?}",
        a
    );
    assert!(a.contains(&PipelineRole::Host), "Mode A 座位缺主持人");
    assert!(a.contains(&PipelineRole::Auditor), "Mode A 座位缺校验员");

    let b = seats_for_mode(&Mode::ModeB);
    assert_eq!(b.len(), 5, "Mode B 座位应为 5，实际 {:?}", b);

    let c = seats_for_mode(&Mode::ModeC);
    assert_eq!(
        c.len(),
        3,
        "Mode C 座位应为 3（改词+主持+校验），实际 {:?}",
        c
    );

    let d = seats_for_mode(&Mode::ModeD);
    assert_eq!(d.len(), 6, "Mode D 座位应为 6，实际 {:?}", d);
    assert!(
        d.contains(&PipelineRole::StyleAnalyst),
        "Mode D 座位缺流行风格分析师"
    );
}

/// steps 语义不变：讨论轮只跑动态角色（A=2、B=3、C=1、D=4，不含主持/校验；Q6：C 只留改词）
#[test]
fn steps_exclude_host_and_auditor() {
    assert_eq!(steps_for_mode(&Mode::ModeA).len(), 2);
    assert_eq!(steps_for_mode(&Mode::ModeB).len(), 3);
    assert_eq!(steps_for_mode(&Mode::ModeC).len(), 1);
    assert_eq!(steps_for_mode(&Mode::ModeD).len(), 4);
    for mode in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
        let roles: Vec<_> = steps_for_mode(&mode).iter().map(|s| s.role).collect();
        assert!(
            !roles.contains(&PipelineRole::Host),
            "{:?} steps 不应含主持人",
            mode
        );
        assert!(
            !roles.contains(&PipelineRole::Auditor),
            "{:?} steps 不应含校验员",
            mode
        );
    }
}

/// 反馈路由五类关键词（后端真源，与前端预估展示对齐）
#[test]
fn feedback_routing_covers_five_categories() {
    assert!(roles_for_feedback("歌词韵脚不行", &Mode::ModeB).contains(&PipelineRole::Lyricist));
    assert!(roles_for_feedback("歌词韵脚不行", &Mode::ModeC).contains(&PipelineRole::Reviser));
    assert!(roles_for_feedback("BPM 太慢", &Mode::ModeD).contains(&PipelineRole::Producer));
    assert!(roles_for_feedback("情绪不够炸", &Mode::ModeB).contains(&PipelineRole::Emotion));
    assert!(roles_for_feedback("不够洗脑", &Mode::ModeD).contains(&PipelineRole::StyleAnalyst));
    assert!(roles_for_feedback("不够洗脑", &Mode::ModeB).contains(&PipelineRole::Producer));
    assert!(roles_for_feedback("随便改改", &Mode::ModeB).is_empty());
}

/// Mode C：逐行等字数对齐（等字通过，不等打回）
#[test]
fn mode_c_line_alignment() {
    let original = "昨夜星辰昨夜风\n画楼西畔桂堂东";
    let aligned = "今朝有酒今朝醉\n明日愁来明日愁";
    let r = validate_lyric_fill(original, aligned);
    assert!(r.passed, "等字数应通过，实际 {:?}", r.issues);
    let misaligned = "今朝有酒今朝醉啊啊\n明日愁来明日愁";
    let r2 = validate_lyric_fill(original, misaligned);
    assert!(!r2.passed, "不等字数应打回");
}

/// Mode A/B：Style Prompt ≤350（超限打回）——助手单测；IR 接线见 plan::validate
#[test]
fn mode_ab_style_prompt_cap() {
    let long = "x".repeat(rules::STYLE_PROMPT_MAX_CHARS + 1);
    let issues = style_prompt_length_issues("mode_a", &long);
    assert!(!issues.is_empty(), "超 350 字应打回，实际 {:?}", issues);
}

/// Mode D：行长硬门经 plan::validate（文本 validate_douyin 已退役）
#[test]
fn mode_d_hard_rules_via_plan_validate() {
    let mut p = minimal_plan();
    p.sections[0].kind = SectionKind::Hook;
    p.sections[0].label = "Hook".into();
    p.sections[0].lines = vec!["这一行歌词明显超过了十二个字的上限啦".into()];
    let v = validate(
        &p,
        &ValidateCtx {
            mode: "mode_d",
            converged: None,
            original_lyrics: None,
        },
    );
    let codes: Vec<_> = v.iter().map(|i| i.code).collect();
    assert!(
        codes.contains(&"line_too_long")
            || codes
                .iter()
                .any(|c| c.contains("douyin") || c.contains("line")),
        "超长行应被 IR 门打回: {codes:?} / {v:?}"
    );
}

/// §5.1#5：未知模式 / Mode C 无原词不再经 validate_for_mode；C 无原词时 lyric_fill 不接线
#[test]
fn mode_c_without_original_skips_lyric_fill_gate() {
    let p = minimal_plan();
    let v = validate(
        &p,
        &ValidateCtx {
            mode: "mode_c",
            converged: None,
            original_lyrics: None,
        },
    );
    assert!(
        !v.iter().any(|i| i.code == "lyric_fill"),
        "无原词不应产生 lyric_fill 硬问题: {v:?}"
    );
}
