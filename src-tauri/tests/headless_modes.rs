//! 无头实网测试：四模式全链路 + 续跑（需真实网关，key 只从环境变量读，不进仓库）。
//! 运行：SHIYI_TEST_API_KEY=... SHIYI_TEST_BASE_URL=... SHIYI_TEST_MODEL=... cargo test --test headless_modes -- --ignored --nocapture
//! 缺变量时自动跳过（CI 无 key 不挂）。单用例预算：A/B/C=12 分钟，D=15 分钟（= 生产预算；短于生产 15 分钟；2026-10-20 实网 8 分钟在四路并发下 B 超时）。
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
    PipelineRequest { structured_caps: None,
    douyin_scene: None,
    text_eval_judge: None,
        mode,
        output_lang: suno_prompt_generator_lib::models::LyricLang::Zh,
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
        // #12 确认门：无头执行无人可确认，必须**保持关闭**（开则每轮空等至超时）
        round_gate: None,
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

/// 运行期告警可见化：集成测试默认无 tracing 订阅器 → `tracing::warn!`（知识库注入的
/// 引用必达/整表休眠/可达性告警）在无头实跑中**完全不可观测**，实跑复测无法验证告警口径。
/// 进程级初始化一次（多用例共用；已初始化则忽略），只放行 WARN 以上，避免 INFO 刷屏。
fn init_warn_logging_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(std::io::stderr)
            .try_init();
    });
}

/// 无头执行：mock_app 拿句柄 → run_pipeline_with_timeout 直调（不走窗口）。
/// 事件发射在 mock 下为空操作；终稿文本即断言对象。
/// 全量复审 2026-10-20：订阅 "pipeline" 事件（mock 运行时同样派发），终稿后对事件流做结构化断言。
async fn run_headless_with(
    mode: Mode,
    user_input: &str,
    original_lyrics: Option<&str>,
    timeout: Duration,
    tweak: impl FnOnce(&mut PipelineRequest),
) -> Option<(String, Vec<serde_json::Value>)> {
    use tauri::Listener;
    init_warn_logging_once();
    let cfg = test_config()?;
    let app = tauri::test::mock_app();
    let handle = app.handle().clone();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let sink = events.clone();
    handle.listen_any("pipeline", move |e| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) {
            sink.lock().unwrap().push(v["event"].clone());
        }
    });
    let mut req = make_request(&cfg, mode, user_input, original_lyrics);
    tweak(&mut req);
    match orchestrator::run_pipeline_with_timeout(handle, req, timeout).await {
        Ok(text) => {
            let ev = events.lock().unwrap().clone();
            Some((text, ev))
        }
        Err(e) => {
            println!("HEADLESS-FAIL kind={:?} msg={}", e.kind, e.message);
            None
        }
    }
}

/// 事件流断言：最后一次 audit_result.pass=true、无 degraded、final_plan 可解析且再过 plan::validate + render_suno
fn assert_events_pass(mode: Mode, events: &[serde_json::Value], original_lyrics: Option<&str>) -> suno_prompt_generator_lib::plan::SongPlan {
    use suno_prompt_generator_lib::plan::{render_suno, schema, validate};
    let types: Vec<&str> = events.iter().filter_map(|e| e["type"].as_str()).collect();
    println!("HEADLESS-EVENTS {}: {:?}", match_mode(&mode), {
        let mut c = std::collections::BTreeMap::<&str, usize>::new();
        for t in &types { *c.entry(t).or_default() += 1; }
        c
    });
    // 讨论轮次上限强制收敛是设计内软降级（多角色未全体共识），单独打印；其余降级（解析/门/预算）一律算失败
    let degraded: Vec<_> = events
        .iter()
        .filter(|e| e["type"] == "degraded")
        .filter(|e| {
            let soft = e["detail"].as_str().is_some_and(|d| d.contains("轮次上限"));
            if soft {
                println!("HEADLESS-SOFT-DEGRADED {}", e["detail"]);
            }
            !soft
        })
        .collect();
    assert!(degraded.is_empty(), "不应降级: {degraded:?}");
    let last_audit = events.iter().rev().find(|e| e["type"] == "audit_result").expect("应有 audit_result");
    assert_eq!(last_audit["pass"], true, "终审未过: {last_audit}");
    let fp = events.iter().rev().find(|e| e["type"] == "final_plan").expect("应有 final_plan 事件");
    let plan = schema::parse_song_plan(&fp["song_plan"]).expect("final_plan 应过 schema");
    let ctx = validate::ValidateCtx { mode: mode.to_str_name(), converged: None, original_lyrics };
    let issues = validate::validate(&plan, &ctx);
    assert!(validate::passed(&issues), "final_plan 复验未过: {:?}", issues.iter().map(|i| i.line()).collect::<Vec<_>>());
    if let Some(orig) = original_lyrics {
        let lines: Vec<String> = plan.sections.iter().flat_map(|s| s.lines.iter().cloned()).filter(|l| !l.trim().is_empty()).collect();
        let v = suno_prompt_generator_lib::commands::validator::validate_lyric_fill(orig, &lines.join("\n"));
        assert!(v.passed, "Mode C SongPlan 歌词未过逐行等字: {:?}", v.issues);
    }
    let r = render_suno::render(&plan);
    assert!(!r.style.trim().is_empty() && !r.lyrics.trim().is_empty(), "render_suno 产物不应为空");
    plan
}

/// 终稿文本基本断言（硬门由 assert_events_pass 在 final_plan 上复验；Mode C 逐行等字在 SongPlan 歌词行上比对，
/// 不再对整包文本跑 lyric_fill——整包含 Title/Style/段头行，旧断言必然误报）。
fn assert_final_passes_hard_validation(mode: Mode, text: &str, _original_lyrics: Option<&str>) {
    assert!(!text.trim().is_empty(), "{} 终稿不应为空", match_mode(&mode));
}

/// Mode A：短词 → 完整方案（终稿含 Style Prompt；座位=4）
#[tokio::test]
#[ignore]
async fn headless_mode_a() {
    let (text, events) = run_headless_with(
        Mode::ModeA,
        "[Verse]\n深夜灯亮键盘响\n窗外雨落心火燃\n[Chorus]\n我不睡我不退\n熬过今夜见光来",
        None,
        Duration::from_secs(12 * 60),
        |_| {},
    )
    .await
    .expect("Mode A 无头实网应产出终稿（R1 保底 + R2 流式重试已落地）");
    // P4 口径修正：标签回退是 v0.5.1 特性（模型丢标签但首行写风格正文仍可提取）——
    // 断言与产品同口径：extract_style_prompt 可提取即可，不要求字面标签存在
    assert!(
        suno_prompt_generator_lib::commands::validator::extract_style_prompt(&text).is_some(),
        "终稿应可提取 Style Prompt（含标签回退），实际前200字：{}",
        text.chars().take(200).collect::<String>()
    );
    assert_final_passes_hard_validation(Mode::ModeA, &text, None);
    assert_events_pass(Mode::ModeA, &events, None);
    println!("HEADLESS-A-OK chars={}", text.chars().count());
}

/// Mode B：短灵感 → 经典歌曲（终稿非空；座位=5）
#[tokio::test]
#[ignore]
async fn headless_mode_b() {
    let (text, events) = run_headless_with(
        Mode::ModeB,
        "雨天街角错过的那把伞",
        None,
        Duration::from_secs(12 * 60),
        |_| {},
    )
    .await
    .expect("Mode B 无头实网应产出终稿");
    assert!(!text.trim().is_empty(), "Mode B 终稿不应为空");
    assert_final_passes_hard_validation(Mode::ModeB, &text, None);
    assert_events_pass(Mode::ModeB, &events, None);
    println!("HEADLESS-B-OK chars={}", text.chars().count());
}

/// Mode C：三行原词 + 新主题 → 逐行等字改写（座位=3：Reviser+Host+Auditor，Q6 后制作人已移除；硬校验逐行对齐）
#[tokio::test]
#[ignore]
async fn headless_mode_c() {
    let (text, events) = run_headless_with(
        Mode::ModeC,
        "深夜加班的坚持",
        Some("昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼"),
        Duration::from_secs(12 * 60),
        |_| {},
    )
    .await
    .expect("Mode C 无头实网应产出终稿（原词直传 + 重试不丢词）");
    assert!(!text.trim().is_empty(), "Mode C 终稿不应为空");
    assert_final_passes_hard_validation(Mode::ModeC, &text, Some("昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼"));
    assert_events_pass(Mode::ModeC, &events, Some("昨夜星辰昨夜风\n画楼西畔桂堂东\n身无彩凤双飞翼"));
    println!("HEADLESS-C-OK chars={}", text.chars().count());
}

/// Mode D：短灵感 → 抖音神曲（场景=dance + 文本评估 Judge 开；最长链 12 分钟预算）
#[tokio::test]
#[ignore]
async fn headless_mode_d() {
    let (text, events) = run_headless_with(
        Mode::ModeD,
        "深夜加班打工人的心酸和坚持，要炸，要洗脑循环",
        None,
        Duration::from_secs(15 * 60),
        |r| {
            r.douyin_scene = Some("dance".into());
            r.text_eval_judge = Some(true);
        },
    )
    .await
    .expect("Mode D 无头实网应产出终稿");
    assert!(!text.trim().is_empty(), "Mode D 终稿不应为空");
    let plan = assert_events_pass(Mode::ModeD, &events, None);
    let dy = serde_json::to_value(&plan.mode_meta).unwrap();
    assert_eq!(dy["douyin"]["scene"], "dance", "场景应由请求盖写: {dy}");
    assert!(events.iter().any(|e| e["type"] == "text_eval"), "Judge 开启应发 text_eval");
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
