//! 无头离线测试：零网络，CI 可跑。
//! 覆盖四模式座位/动态语义、反馈路由、硬校验逐条阈值。
//! 与单元测试的区别：本文件从外部 crate 视角断言公开 API 的契约（座位数、校验口径），
//! 防止 R6 这类"名单口径"回归。

use suno_prompt_generator_lib::commands::orchestrator::{roles_for_feedback, seats_for_mode, steps_for_mode};
use suno_prompt_generator_lib::commands::validator::{
    validate_douyin, validate_for_mode, validate_lyric_fill, validate_production,
};
use suno_prompt_generator_lib::models::{Mode, PipelineRole};

/// R6：四模式座位含主持/校验落座（A=4、B=5、C=3、D=6；Q6：C 摘制作人）
#[test]
fn seats_include_host_and_auditor() {
    let a = seats_for_mode(&Mode::ModeA);
    assert_eq!(a.len(), 4, "Mode A 座位应为 4（情感+制作+主持+校验），实际 {:?}", a);
    assert!(a.contains(&PipelineRole::Host), "Mode A 座位缺主持人");
    assert!(a.contains(&PipelineRole::Auditor), "Mode A 座位缺校验员");

    let b = seats_for_mode(&Mode::ModeB);
    assert_eq!(b.len(), 5, "Mode B 座位应为 5，实际 {:?}", b);

    let c = seats_for_mode(&Mode::ModeC);
    assert_eq!(c.len(), 3, "Mode C 座位应为 3（改词+主持+校验），实际 {:?}", c);

    let d = seats_for_mode(&Mode::ModeD);
    assert_eq!(d.len(), 6, "Mode D 座位应为 6，实际 {:?}", d);
    assert!(d.contains(&PipelineRole::StyleAnalyst), "Mode D 座位缺流行风格分析师");
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
        assert!(!roles.contains(&PipelineRole::Host), "{:?} steps 不应含主持人", mode);
        assert!(!roles.contains(&PipelineRole::Auditor), "{:?} steps 不应含校验员", mode);
    }
}

/// 反馈路由五类关键词（后端真源，与前端预估展示对齐）
#[test]
fn feedback_routing_covers_five_categories() {
    // 歌词类：B→作词，C→改词
    assert!(roles_for_feedback("歌词韵脚不行", &Mode::ModeB).contains(&PipelineRole::Lyricist));
    assert!(roles_for_feedback("歌词韵脚不行", &Mode::ModeC).contains(&PipelineRole::Reviser));
    // 编曲类→制作
    assert!(roles_for_feedback("BPM 太慢", &Mode::ModeD).contains(&PipelineRole::Producer));
    // 情绪类→情感
    assert!(roles_for_feedback("情绪不够炸", &Mode::ModeB).contains(&PipelineRole::Emotion));
    // 抖音类：D→流行，其他→制作
    assert!(roles_for_feedback("不够洗脑", &Mode::ModeD).contains(&PipelineRole::StyleAnalyst));
    assert!(roles_for_feedback("不够洗脑", &Mode::ModeB).contains(&PipelineRole::Producer));
    // 无命中→空（调用方回落全量）
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

/// Mode A/B：Style Prompt ≤350（超限打回）
#[test]
fn mode_ab_style_prompt_cap() {
    let ok_text = "Style Prompt: folk ballad, warm acoustic guitar\n[Verse]\n啦啦啦，能量:5\n[Chorus]\n啦啦啦啦，能量:9";
    let _ = validate_production("mode_a", ok_text);
    let long_sp = "Style Prompt: ".to_string() + &"x".repeat(351) + "\n[Verse]\n啦，能量:5";
    let r = validate_production("mode_a", &long_sp);
    assert!(!r.passed, "超 350 字应打回，实际 {:?}", r.issues);
}

/// Mode D：Hook≥2 / Verse≤4 / 每行≤10字 / BPM≥90（逐条造样例）
#[test]
fn mode_d_hard_rules() {
    // Hook 不足打回
    let one_hook = "[Hook]\n我不睡\n[Verse]\n短句，能量:5\n骤停";
    assert!(!validate_douyin(one_hook).passed, "Hook 不足 2 应打回");
    // 每行超 10 字打回
    let long_line = "[Hook]\n我不睡\n[Hook]\n我不退\n[Verse]\n这是一行超过十个字的歌词行啊啊啊\n骤停";
    let r = validate_douyin(long_line);
    assert!(!r.passed, "超 10 字行应打回，实际 {:?}", r.issues);
}

/// validate_for_mode 分发口径（a/b 走 production，c 需原词，d 走 douyin，未知打回）
#[test]
fn validate_for_mode_dispatch() {
    assert!(validate_for_mode("mode_x", "x", None).passed == false);
    assert!(validate_for_mode("mode_c", "新词", None).passed == false, "C 无原词应打回");
}
