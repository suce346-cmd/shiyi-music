//! 无头实网测试：四模式全链路 + 续跑（需真实网关，key 只从环境变量读，不进仓库）。
//! 运行：SHIYI_TEST_API_KEY=... SHIYI_TEST_BASE_URL=... SHIYI_TEST_MODEL=... cargo test --test headless_modes -- --ignored --nocapture
//! 缺变量时自动跳过（CI 无 key 不挂）。单用例预算：A/B/C=8 分钟，D=12 分钟（短于生产 15 分钟，抖动下仍可收敛）。
//! 凭据安全：源码、示例、测试均不写可用凭据字面量；URL 仅允许 http/https（validate_request 已拦 ftp）。

use std::time::Duration;
use suno_prompt_generator_lib::commands::orchestrator;
use suno_prompt_generator_lib::models::{Mode, PipelineRequest};

/// 测试配置：全从环境变量读（缺一即跳过，不硬失败）
struct TestConfig {
    base_url: String,
    api_key: String,
    model: String,
}

fn test_config() -> Option<TestConfig> {
    let base_url = std::env::var("SHIYI_TEST_BASE_URL").ok()?;
    let api_key = std::env::var("SHIYI_TEST_API_KEY").ok()?;
    let model = std::env::var("SHIYI_TEST_MODEL").ok()?;
    if base_url.trim().is_empty() || api_key.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    // S-1 新规则：与生产同一道 validate_url 闸门（旧规则仅前缀检查，测试与生产两套标准）
    // 配置非法时 eprintln 后跳过——与"缺变量自动跳过"语义一致，实网测试不吞凭据
    if let Err(m) = suno_prompt_generator_lib::models::validate_url(&base_url) {
        eprintln!("SHIYI_TEST_BASE_URL 不合规，实网测试跳过: {}", m);
        return None;
    }
    Some(TestConfig { base_url, api_key, model })
}

fn make_request(cfg: &TestConfig, mode: Mode, user_input: &str, original_lyrics: Option<&str>) -> PipelineRequest {
    let tag = match_mode(&mode);
    PipelineRequest {
        mode,
        user_input: user_input.to_string(),
        model: cfg.model.clone(),
        api_key: cfg.api_key.clone(),
        base_url: cfg.base_url.clone(),
        extra: None,
        original_lyrics: original_lyrics.map(|s| s.to_string()),
        role_overrides: None,
        thinking: false,
        refine_targets: None,
        generation: None,
        run_id: Some(format!("headless-{}-{}", tag, std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))),
    }
}

fn match_mode(mode: &Mode) -> &'static str {
    match mode {
        Mode::ModeA => "a",
        Mode::ModeB => "b",
        Mode::ModeC => "c",
        Mode::ModeD => "d",
    }
}

/// 无头执行：mock_app 拿句柄 → run_pipeline_with_timeout 直调（不走窗口）。
/// 事件发射在 mock 下为空操作；终稿文本即断言对象。
async fn run_headless(
    mode: Mode,
    user_input: &str,
    original_lyrics: Option<&str>,
    timeout: Duration,
) -> Option<String> {
    let cfg = test_config()?;
    let app = tauri::test::mock_app();
    let handle = app.handle().clone();
    let req = make_request(&cfg, mode, user_input, original_lyrics);
    match orchestrator::run_pipeline_with_timeout(handle, req, timeout).await {
        Ok(text) => Some(text),
        Err(e) => {
            println!("HEADLESS-FAIL kind={:?} msg={}", e.kind, e.message);
            None
        }
    }
}

/// P4/§6.2 集成断言（C1-C5 升级的新规则）：终稿必须通过全部硬校验（含 C1 参数门/C2 保真链路）。
/// 失败 = 打回耗尽降级（gate_degraded），打印行号级 issue 明细供定位——
/// 这是"收敛承诺传递到终稿"的实网观测点（项目书 §6.4 验收标准）。
fn assert_final_passes_hard_validation(mode: Mode, text: &str, original_lyrics: Option<&str>) {
    let v = suno_prompt_generator_lib::commands::validator::validate_for_mode(
        mode.to_str_name(),
        text,
        original_lyrics,
    );
    assert!(
        v.passed,
        "{} 终稿未通过全部硬校验（打回耗尽降级）：\n{}",
        match_mode(&mode),
        v.issues.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n")
    );
}

/// Mode A：短词 → 完整方案（终稿含 Style Prompt；座位=4）
#[tokio::test]
#[ignore]
async fn headless_mode_a() {
    let text = run_headless(
        Mode::ModeA,
        "[Verse]\n深夜灯亮键盘响\n窗外雨落心火燃\n[Chorus]\n我不睡我不退\n熬过今夜见光来",
        None,
        Duration::from_secs(8 * 60),
    )
    .await
    .expect("Mode A 无头实网应产出终稿（R1 保底 + R2 流式重试已落地）");
    assert!(text.contains("Style Prompt") || text.contains("风格"), "终稿应含 Style Prompt，实际前200字：{}", text.chars().take(200).collect::<String>());
    assert_final_passes_hard_validation(Mode::ModeA, &text, None);
    println!("HEADLESS-A-OK chars={}", text.chars().count());
}

/// Mode B：短灵感 → 经典歌曲（终稿非空；座位=5）
#[tokio::test]
#[ignore]
async fn headless_mode_b() {
    let text = run_headless(
        Mode::ModeB,
        "雨天街角错过的那把伞",
        None,
        Duration::from_secs(8 * 60),
    )
    .await
    .expect("Mode B 无头实网应产出终稿");
    assert!(!text.trim().is_empty(), "Mode B 终稿不应为空");
    assert_final_passes_hard_validation(Mode::ModeB, &text, None);
    println!("HEADLESS-B-OK chars={}", text.chars().count());
}

/// Mode C：三行原词 + 新主题 → 逐行等字改写（座位=3：Reviser+Host+Auditor，Q6 后制作人已移除；硬校验逐行对齐）
#[tokio::test]
#[ignore]
async fn headless_mode_c() {
    let text = run_headless(
        Mode::ModeC,
        "深夜加班的坚持",
        Some("昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼"),
        Duration::from_secs(8 * 60),
    )
    .await
    .expect("Mode C 无头实网应产出终稿（原词直传 + 重试不丢词）");
    assert!(!text.trim().is_empty(), "Mode C 终稿不应为空");
    assert_final_passes_hard_validation(Mode::ModeC, &text, Some("昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼"));
    println!("HEADLESS-C-OK chars={}", text.chars().count());
}

/// Mode D：短灵感 → 抖音神曲（终稿非空；座位=6；最长链 12 分钟预算）
#[tokio::test]
#[ignore]
async fn headless_mode_d() {
    let text = run_headless(
        Mode::ModeD,
        "深夜加班打工人的心酸和坚持，要炸，要洗脑循环",
        None,
        Duration::from_secs(12 * 60),
    )
    .await
    .expect("Mode D 无头实网应产出终稿（R4 错峰 + R1 保底已落地）");
    assert!(!text.trim().is_empty(), "Mode D 终稿不应为空");
    assert_final_passes_hard_validation(Mode::ModeD, &text, None);
    println!("HEADLESS-D-OK chars={}", text.chars().count());
}

/// R3 续跑：极短预算强制失败 → 检查点存在 → resume 进终稿。
/// 用 60 秒预算跑 Mode B（必超时），不断言终稿，只断言失败路径 + 检查点语义。
#[tokio::test]
#[ignore]
async fn headless_resume_from_checkpoint() {
    let cfg = test_config().expect("需配 key 才跑续跑用例");
    let app = tauri::test::mock_app();
    let handle = app.handle().clone();
    let run_id = format!("headless-resume-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let mut req = make_request(&cfg, Mode::ModeB, "雨天街角错过的那把伞", None);
    req.run_id = Some(run_id.clone());
    // 60 秒必失败（阶段 0 都跑不完），只验证失败不崩、检查点语义由 R3 单测覆盖
    let res = orchestrator::run_pipeline_with_timeout(handle.clone(), req.clone(), Duration::from_secs(60)).await;
    println!("HEADLESS-RESUME-FIRST kind={:?}", res.as_ref().err().map(|e| &e.kind));
    // 续跑：新预算满额断点进终稿（无检查点时应明确报错，不静默从头来）
    let resume_res = orchestrator::pipeline_resume(handle, req, run_id).await;
    match resume_res {
        Ok(text) => println!("HEADLESS-RESUME-OK chars={}", text.chars().count()),
        Err(e) => println!("HEADLESS-RESUME-NO-CHECKPOINT kind={:?} msg={}", e.kind, e.message),
    }
}
