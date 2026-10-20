//! 大型实测矩阵（用户指令 2026-09-09）：四模式 × 10 提示词，40 次真实 API 调用。
//! 目的：产品维度的分布分析（编曲规范性/多样性/合理性/千篇一律/韵脚/Hook 质量），
//! 不是单次 pass/fail。逐例落盘 JSONL（target/large-test/results.jsonl），
//! 单例失败记录后继续（批量测试不中断）。凭据只从环境变量读，不进仓库。
//!
//! 运行：SHIYI_TEST_API_KEY=... SHIYI_TEST_BASE_URL=... SHIYI_TEST_MODEL=... \
//!   cargo test --test large_scale_matrix -- --ignored --nocapture
//! 提示词池冻结于 docs/qa/test-prompts.md（测试前确定，杜绝看结果选输入）。

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::Listener;

use suno_prompt_generator_lib::commands::orchestrator;
use suno_prompt_generator_lib::models::{Mode, PipelineRequest};

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
    if let Err(m) = suno_prompt_generator_lib::models::validate_url(&base_url) {
        eprintln!("SHIYI_TEST_BASE_URL 不合规，跳过: {}", m);
        return None;
    }
    Some(TestConfig { base_url, api_key, model })
}

fn make_request(cfg: &TestConfig, mode: Mode, user_input: &str, original_lyrics: Option<&str>) -> PipelineRequest {
    let tag = match_mode(&mode);
    PipelineRequest { structured_caps: None,
        mode,
        output_lang: "zh".into(),
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
        run_id: Some(format!("lsm-{}-{}", tag, std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))),
        // #12 确认门：矩阵为无人值守压测，必须**保持关闭**（开则每轮停 5 分钟等确认）
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

struct Case {
    mode: Mode,
    prompt_id: &'static str,
    input: &'static str,
    original_lyrics: Option<&'static str>,
    timeout: Duration,
}

fn d(min: u64) -> Duration { Duration::from_secs(min * 60) }

/// 冻结的测试矩阵（docs/qa/test-prompts.md）。带 ' 后缀 = 同题重跑（一致性/千篇一律检验）。
fn matrix() -> Vec<Case> {
    let d_pool: Vec<(&str, &str)> = vec![
        ("P1", "深夜加班打工人的心酸和坚持，要炸，要洗脑循环"),
        ("P2", "夏夜暴雨后巷口的烧烤摊，烟火气加一点孤独感"),
        ("P3", "健身房撸铁逆袭的热血，重拍炸裂，荷尔蒙爆棚"),
        ("P4", "早八人挤地铁的麻木与自嘲，鬼畜循环，魔性上头"),
        ("P3'", "健身房撸铁逆袭的热血，重拍炸裂，荷尔蒙爆棚"),
        ("P4'", "早八人挤地铁的麻木与自嘲，鬼畜循环，魔性上头"),
        ("P1''", "深夜加班打工人的心酸和坚持，要炸，要洗脑循环"),
        ("P2''", "夏夜暴雨后巷口的烧烤摊，烟火气加一点孤独感"),
    ];
    // D 池补足 10：加两个新题
    let d_extra: Vec<(&str, &str)> = vec![
        ("P11", "毕业季天台大合唱，青春散场又热血，大合唱氛围"),
        ("P12", "赛博朋克夜市霓虹下的独行侠，电子迷幻，冷酷拽感"),
    ];
    let a_pool: Vec<(&str, &str)> = vec![
        ("P1", "深夜加班打工人的心酸和坚持，要炸，要洗脑循环"),
        ("P5", "江南梅雨季的青石板巷，油纸伞下欲说还休的告别，国风叙事"),
        ("P6", "疫情后第一次回家过年，站台父子无言的拥抱，温暖催泪"),
        ("P7", "深海孤独症：社会性死亡后的自我放逐，电子冷感，意识流"),
        ("P10", "老房子拆迁前夜，三代人记忆的墙面剥落，民谣叙事"),
        ("P5'", "江南梅雨季的青石板巷，油纸伞下欲说还休的告别，国风叙事"),
        ("P6'", "疫情后第一次回家过年，站台父子无言的拥抱，温暖催泪"),
        ("P1'", "深夜加班打工人的心酸和坚持，要炸，要洗脑循环"),
        ("P10'", "老房子拆迁前夜，三代人记忆的墙面剥落，民谣叙事"),
        ("P7'", "深海孤独症：社会性死亡后的自我放逐，电子冷感，意识流"),
    ];
    let b_pool: Vec<(&str, &str)> = vec![
        ("P1", "深夜加班打工人的心酸和坚持，要炸，要洗脑循环"),
        ("P2", "夏夜暴雨后巷口的烧烤摊，烟火气加一点孤独感"),
        ("P5", "江南梅雨季的青石板巷，油纸伞下欲说还休的告别，国风叙事"),
        ("P6", "疫情后第一次回家过年，站台父子无言的拥抱，温暖催泪"),
        ("P8", "高考落榜生在天台看日出，从失落走向释怀，摇滚燃向"),
        ("P9", "异地恋第七年，视频里笑着说没事挂了电话哭，城市流行苦情"),
        ("P10", "老房子拆迁前夜，三代人记忆的墙面剥落，民谣叙事"),
        ("P8'", "高考落榜生在天台看日出，从失落走向释怀，摇滚燃向"),
        ("P9'", "异地恋第七年，视频里笑着说没事挂了电话哭，城市流行苦情"),
        ("P2'", "夏夜暴雨后巷口的烧烤摊，烟火气加一点孤独感"),
    ];
    // C 模式：原歌词池（公版诗 + 自写，杜绝版权文本）× 新主题
    let c_pool: Vec<(&str, &str, &str)> = vec![
        ("C1", "床前明月光\n疑是地上霜\n举头望明月\n低头思故乡", "深夜加班的坚持与自我打气"),
        ("C2", "春眠不觉晓\n处处闻啼鸟", "都市晨跑人的元气清晨"),
        ("C3", "长亭外\n古道边\n芳草碧连天", "大学毕业十年的同学重逢"),
        ("C4", "我是一只小小鸟\n想飞呀飞呀飞不高", "小镇青年到大城市打拼"),
        ("C5", "晚风轻拂湖面\n柳枝梳理月色", "夏夜湖边散步的惬意"),
        ("C6", "城市的夜 灯火阑珊\n一个人走 影子作伴\n梦里的家 隔着海岸", "北漂第五年的孤独与倔强"),
        ("C7", "雨落青石巷\n伞下人成双", "雨天等一个人的心事"),
        ("C8", "风 吹过 老槐树\n蝉声 漫过 整个夏午\n外婆的蒲扇 摇着旧时光", "童年夏天怀念"),
        ("C9", "灯火阑珊处\n谁在等归人", "深夜末班车的疲惫归途"),
        ("C10", "海浪一遍遍\n把脚印还给沙滩\n我把心事还给海风", "海边放下的释怀"),
    ];
    let mut cases: Vec<Case> = Vec::new();
    for (pid, input) in d_pool.iter().chain(d_extra.iter()) {
        cases.push(Case { mode: Mode::ModeD, prompt_id: pid, input, original_lyrics: None, timeout: d(12) });
    }
    for (pid, input) in a_pool.iter() {
        cases.push(Case { mode: Mode::ModeA, prompt_id: pid, input, original_lyrics: None, timeout: d(8) });
    }
    for (pid, input) in b_pool.iter() {
        cases.push(Case { mode: Mode::ModeB, prompt_id: pid, input, original_lyrics: None, timeout: d(8) });
    }
    for (pid, lyrics, theme) in c_pool.iter() {
        cases.push(Case { mode: Mode::ModeC, prompt_id: pid, input: theme, original_lyrics: Some(lyrics), timeout: d(8) });
    }
    cases
}

/// 逐例落盘（追加+立即 flush，中断不丢已完成样本）
fn append_result(path: &str, line: &str) {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("open results");
    writeln!(f, "{}", line).expect("write results");
    f.flush().ok();
}

#[tokio::test]
#[ignore]
async fn large_scale_matrix() {
    let Some(cfg) = test_config() else {
        eprintln!("缺少 SHIYI_TEST_* 环境变量，大型实测跳过");
        return;
    };
    let dir = results_dir();
    std::fs::create_dir_all(&dir).expect("create dir");
    let results_path = dir.join("results.jsonl").to_string_lossy().into_owned();
    let cases = matrix();
    // LSM_ONLY：调试过滤器——只跑 prompt_id 含该子串的用例（如 LSM_ONLY=P1）
    // LSM_CASES：精确用例选择——逗号分隔 "mode:prompt_id"（如 "d:P1,a:P5,c:C6"）
    // seq 取全矩阵稳定编号（过滤前），保证断点续跑与过滤器不冲突
    let only = std::env::var("LSM_ONLY").unwrap_or_default();
    let lsm_cases = std::env::var("LSM_CASES").unwrap_or_default();
    let wanted: Vec<(String, String)> = lsm_cases
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let mut it = s.trim().splitn(2, ':');
            (
                it.next().unwrap_or("").trim().to_string(),
                it.next().unwrap_or("").trim().to_string(),
            )
        })
        .collect();
    let mut cases: Vec<(usize, Case)> = cases.into_iter().enumerate().map(|(i, c)| (i + 1, c)).collect();
    if !only.is_empty() {
        cases.retain(|(_, c)| c.prompt_id.contains(&only));
    }
    if !wanted.is_empty() {
        cases.retain(|(_, c)| {
            wanted
                .iter()
                .any(|(m, pid)| *m == match_mode(&c.mode) && *pid == c.prompt_id)
        });
    }
    // 断点续跑：已有 results.jsonl 里的 seq 视为完成，跳过（全矩阵稳定 seq）
    let mut done: std::collections::HashSet<usize> = std::collections::HashSet::new();
    if let Ok(lines) = std::fs::read_to_string(&results_path) {
        for l in lines.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
                if let Some(seq) = v.get("seq").and_then(|s| s.as_u64()) {
                    done.insert(seq as usize);
                }
            }
        }
    }
    let total = cases.len();
    let remaining = total - done.intersection(&(1..=total).collect()).count();
    println!("LARGE-SCALE-MATRIX total={} done={} remaining={} results={}", total, done.len(), remaining, results_path);

    for (seq, case) in cases.into_iter() {
        if done.contains(&seq) {
            println!("=== [{}/{}] {} {} SKIP(已完成) ===", seq, total, match_mode(&case.mode), case.prompt_id);
            continue;
        }
        let started = Instant::now();
        println!("\n=== [{}/{}] {} {} ===", seq, total, match_mode(&case.mode), case.prompt_id);
        // v2：挂 pipeline 事件监听——全链路中间态（角色修订/主持人整合/打回/审计/降级/用量）落盘
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let events_cl = events.clone();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _eid = handle.listen("pipeline", move |ev| {
            events_cl.lock().unwrap().push(ev.payload().to_string());
        });
        let req = make_request(&cfg, case.mode.clone(), case.input, case.original_lyrics);
        let (output, err_msg) = match orchestrator::run_pipeline_with_timeout(handle, req, case.timeout).await {
            Ok(text) => (Some(text), None),
            Err(e) => {
                println!("CASE-FAIL kind={:?} msg={}", e.kind, e.message);
                (None, Some(format!("{:?}: {}", e.kind, e.message)))
            }
        };
        let evs = events.lock().unwrap().clone();
        let (validation_passed, issues) = match &output {
            Some(_) => match measure_from_events(&evs) {
                Some((passed, issues)) => (Some(passed), issues),
                None => (Some(false), vec!["缺少 AuditResult 事件".into()]),
            },
            None => (None, Vec::new()),
        };
        println!("CASE-DONE {} {} validation={:?} issues={} events={} dur={}s",
            case.prompt_id, match_mode(&case.mode), validation_passed, issues.len(), evs.len(),
            started.elapsed().as_secs());
        let rec = json_line(&case, seq, &output, err_msg.as_deref(), validation_passed, &issues,
            started.elapsed().as_secs(), &evs);
        append_result(&results_path, &rec);
    }
    println!("\nLARGE-SCALE-MATRIX-COMPLETE");
}

/// C-11（P0-7）：结果目录相对工程（`<repo>/target/large-test`），不再写死某台机器的绝对路径。
fn results_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("target").join("large-test")
}

/// P1-3/4：测量口径 = 生产终稿门 `plan::validate`（经流水线 `AuditResult` 事件；
/// 离线 dry-run 直接调 validate）。不再走文本终稿门。
fn measure_from_events(events: &[String]) -> Option<(bool, Vec<String>)> {
    // 取最后一次 AuditResult
    for raw in events.iter().rev() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { continue };
        let ev = v.get("event").unwrap_or(&v);
        if ev.get("type").and_then(|t| t.as_str()) != Some("audit_result") {
            continue;
        }
        let pass = ev.get("pass").and_then(|p| p.as_bool()).unwrap_or(false);
        let findings = ev
            .get("findings")
            .and_then(|f| f.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        return Some((pass, findings));
    }
    None
}

fn measure_plan(case: &Case, plan: &suno_prompt_generator_lib::plan::SongPlan) -> (bool, Vec<String>) {
    use suno_prompt_generator_lib::plan::validate::{self, ValidateCtx};
    let ctx = ValidateCtx {
        mode: case.mode.to_str_name(),
        original_lyrics: case.original_lyrics,
        converged: None,
    };
    let issues = validate::validate(plan, &ctx);
    let lines: Vec<String> = issues.iter().map(|i| i.line()).collect();
    (validate::passed(&issues), lines)
}

/// 离线 dry-run：路径相对工程、测量与生产终稿门同源（无网络，常规 cargo test 运行）
#[test]
fn offline_dry_run_paths_and_measurement() {
    let dir = results_dir();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert!(dir.starts_with(&repo), "结果目录必须位于工程内: {}", dir.display());
    // 运行期解析出的目录本身是绝对路径（Mac 上必在 /Users 下），不能按前缀判"写死"；
    // 写死与否只看源码（下一条扫描）——旧断言在 box 上恒绿、在 Mac 上恒红（Mac 门首跑发现）。
    let src = include_str!("large_scale_matrix.rs");
    let hardcoded = ["/Users/", "hakayi"].concat();
    assert_eq!(src.matches(hardcoded.as_str()).count(), 0, "源码不得出现机器绝对路径");
    assert!(src.contains("measure_from_events"), "测量必须走 AuditResult 事件");
    assert!(src.contains("measure_plan"), "离线测量必须走 plan::validate");
    // 禁止再接线旧文本终稿门（拼接点缀，避免本断言字符串自命中）
    let banned = ["orchestrator::", "collect_hard_issues("].concat();
    assert_eq!(src.matches(banned.as_str()).count(), 0, "矩阵不得再调旧文本终稿门");
    // 离线：空 sections 的最小方案应被硬门拒绝
    let cases = matrix();
    assert!(!cases.is_empty());
    let mut bad = suno_prompt_generator_lib::plan::fixtures::minimal_plan();
    bad.sections.clear();
    for c in &cases {
        let (passed, issues) = measure_plan(c, &bad);
        assert!(!passed && !issues.is_empty(), "{} {} 空段落方案应被硬门拒绝", match_mode(&c.mode), c.prompt_id);
    }
}

fn json_line(case: &Case, seq: usize, output: &Option<String>, err: Option<&str>,
             validation_passed: Option<bool>, issues: &[String], dur_secs: u64, events: &[String]) -> String {
    let mode_str = match_mode(&case.mode);
    let mut s = String::from("{");
    s.push_str(&format!("\"seq\":{},\"mode\":\"{}\",\"prompt_id\":\"{}\",\"dur_secs\":{},\"ts\":{}",
        seq, mode_str, case.prompt_id, dur_secs,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)));
    s.push_str(&format!(",\"input\":{}", json_escape(case.input)));
    if let Some(l) = case.original_lyrics {
        s.push_str(&format!(",\"original_lyrics\":{}", json_escape(l)));
    }
    match (output, err) {
        (Some(text), _) => {
            s.push_str(&format!(",\"ok\":true,\"output\":{}", json_escape(text)));
            s.push_str(&format!(",\"validation_passed\":{}", validation_passed.unwrap_or(false)));
            s.push_str(&format!(",\"issues\":{}", json_array(issues)));
        }
        (None, Some(e)) => {
            s.push_str(&format!(",\"ok\":false,\"error\":{}", json_escape(e)));
        }
        (None, None) => { s.push_str(",\"ok\":false"); }
    }
    // v2：全链路事件（envelope JSON 字符串数组——StepDone 携带角色修订全文，Retry/AuditResult/Degraded/StepUsage 齐）
    let ev_strs: Vec<String> = events.iter().map(|e| json_escape(e)).collect();
    s.push_str(&format!(",\"events\":[{}]", ev_strs.join(",")));
    s.push('}');
    s
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_array(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|i| json_escape(i)).collect();
    format!("[{}]", inner.join(","))
}
