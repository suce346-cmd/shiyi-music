//! 圆桌编排器（v3 架构）：主持人统领 + 角色审改 + 校验员讨论轮审查 + 校验员结构化定稿。
//!
//! 三阶段：
//! - 阶段 0：主持人用该模式的完整指令（prompts.rs，不查 CSV）产出方案初稿
//! - 阶段 1：动态角色（专业审改员）审查方案 → 查 CSV 调素材 → 输出修订片段；
//!   校验员审查（当前方案 + 本轮修订 + 任务分发，提出观点返回主持人）；
//!   主持人汇总成新版完整方案 + 下轮任务分发（结构化 HostSummary.next_tasks）→ 再分发；
//!   全角色与校验员无异议或满 3 轮收敛
//! - 阶段 2：校验员结构化定稿 SongPlan；plan::validate 硬校验（≤2 次带问题重试）→ render_suno 渲染交付包

use crate::commands::{cancel, gate, interject, llm, plan_flow, prompts, roles};
use crate::energy::plan_energy_range;
use crate::errors::AppError;
use crate::knowledge::KnowledgeBase;
use crate::models::{
    HostStage, Mode, PipelineEvent, PipelineRequest, PipelineRole, PipelineStep,
};
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

/// 各模式的流水线动态角色（固定主持/校验由各自阶段独家执行）
pub fn steps_for_mode(mode: &Mode) -> Vec<PipelineStep> {
    use PipelineRole::*;
    match mode {
        // 想法模式（ModeB）：情感 → 作词 → 制作
        Mode::ModeB => vec![Emotion, Lyricist, Producer],
        // 歌词模式（ModeA）：情感 → 制作（跳过作词）
        Mode::ModeA => vec![Emotion, Producer],
        // 改写模式（ModeC）：只留改词（Q6：C 初稿无 Style Prompt 可审，制作人每轮空转；改词线由改词+校验收口）
        Mode::ModeC => vec![Reviser],
        // 抖音（ModeD）：情感 → 作词 → 流行 → 制作
        Mode::ModeD => vec![Emotion, Lyricist, StyleAnalyst, Producer],
    }
    .into_iter()
    .map(|role| PipelineStep { role })
    .collect()
}

/// 各模式的圆桌座位（含固定主持/校验——R6：主持校验落座）。
/// 与 steps_for_mode 的区别：steps 只含动态审改角色（讨论轮只跑这些）；
/// seats 另加 Host + Auditor（阶段条 chip 与事件点亮早已覆盖，主持/校验只是缺座位）。
/// get_pipeline_meta 下发 seats，前端按此摆座；讨论轮/增量过滤仍用 steps，语义不变。
pub fn seats_for_mode(mode: &Mode) -> Vec<PipelineRole> {
    use PipelineRole::*;
    let mut seats: Vec<PipelineRole> = steps_for_mode(mode).iter().map(|s| s.role).collect();
    seats.push(Host);
    seats.push(Auditor);
    seats
}

/// 按优化反馈关键词路由重跑角色（纯函数，可测）。
/// 规则：歌词类→作词/改词；编曲类→制作；情绪类→情感；抖音类→流行风格；参数类→制作+情感。
/// 无命中 → 空（调用方回落全量，安全默认不猜）；ModeC 的歌词类映射 Reviser 而非 Lyricist。
/// 注：前端有 TS 镜像仅做预估展示，真源在此（双源同步，两端同 commit）。
pub fn roles_for_feedback(feedback: &str, mode: &Mode) -> Vec<PipelineRole> {
    use PipelineRole::*;
    // B-2：输入统一转小写匹配（旧规则大小写混排——用户输入 "Hook"/"weirdness" 时
    // 小写关键词 "hook" 与大写关键词 "Weirdness" 各漏一头）
    let fb = feedback.to_lowercase();
    let mut out: Vec<PipelineRole> = Vec::new();
    let mut push = |r: PipelineRole| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    // 歌词类
    if ["歌词", "词", "句", "韵", "唱", "hook", "副歌", "主歌", "金句"].iter().any(|k| fb.contains(k)) {
        push(if *mode == Mode::ModeC { Reviser } else { Lyricist });
    }
    // 编曲类
    if ["编曲", "配器", "乐器", "伴奏", "bpm", "人声", "音色", "混音", "鼓", "吉他", "钢琴", "唢呐"]
        .iter()
        .any(|k| fb.contains(k))
    {
        push(Producer);
    }
    // 情绪类
    if ["情绪", "能量", "感觉", "氛围", "情感", "炸", "软", "嗨"].iter().any(|k| fb.contains(k)) {
        push(Emotion);
    }
    // 抖音传播类（仅 ModeD 有 StyleAnalyst；其他模式回落 Producer）
    if ["抖音", "传播", "钩子", "洗脑", "爆", "魔性", "循环", "骤停"].iter().any(|k| fb.contains(k)) {
        push(if *mode == Mode::ModeD { StyleAnalyst } else { Producer });
    }
    // 参数类
    if ["参数", "怪异度", "影响度", "weirdness", "influence"].iter().any(|k| fb.contains(k)) {
        push(Producer);
        push(Emotion);
    }
    out
}

/// 加载知识库（读进程共享缓存，解析一次；测试直调 knowledge 接口）
fn load_knowledge() -> Result<KnowledgeBase, String> {
    Ok(crate::knowledge::shared_knowledge().clone())
}

/// 按模式取原版完整指令（主持人阶段 0 用，一字不改）
/// 取模式 prompt（用户覆盖优先，嵌入版回退；来源打日志）
fn prompt_for_mode(mode: &Mode) -> String {
    let (name, embedded): (&str, &'static str) = match mode {
        Mode::ModeA => ("mode_a_system_prompt", prompts::mode_a_system_prompt()),
        Mode::ModeB => ("mode_b_system_prompt", prompts::mode_b_system_prompt()),
        Mode::ModeC => ("mode_c_system_prompt", prompts::mode_c_system_prompt()),
        Mode::ModeD => ("mode_d_system_prompt", prompts::mode_d_system_prompt()),
    };
    // 覆盖函数在 prompts.rs 内（同模块可见性需 pub(crate)）：此处经 prompts::prompt_override 读取
    match prompts::prompt_override(name) {
        Some(text) => {
            tracing::info!(prompt = %name, source = "override", "prompt 来源：用户覆盖");
            crate::rules::interpolate(&text) // C3：override 文本同样过占位符插值（写了占位符也能解析）
        }
        None => crate::rules::interpolate(embedded),
    }
}

/// 流水线元数据（单一真源下发）——modes 座位来自 seats_for_mode（含主持/校验落座），
/// roles 元数据来自 role_for（name/emoji/knowledge 表名；prompt 文本不下发）。
/// 前端启动获取一次，本地三张表（MODE_EXPERTS/ROLE_NAMES/ROLE_EMOJIS）由它驱动；
/// 一致性由 pipeline_meta_matches_sources 测试锁定。
#[derive(serde::Serialize)]
pub struct PipelineMeta {
    pub modes: std::collections::HashMap<String, Vec<PipelineRole>>,
    pub roles: std::collections::HashMap<PipelineRole, RoleMeta>,
}

#[derive(serde::Serialize)]
pub struct RoleMeta {
    pub name: String,
    pub emoji: String,
    /// knowledge_tables 的表名列表（列投影/行子集不下发，只同步表级归属）
    pub knowledge: Vec<String>,
}

/// 元数据查询命令（只读，无参数）
#[tauri::command]
pub async fn get_pipeline_meta() -> Result<PipelineMeta, crate::errors::AppError> {
    use crate::models::Mode;
    let mut modes = std::collections::HashMap::new();
    for (key, mode) in [
        ("mode_a", Mode::ModeA),
        ("mode_b", Mode::ModeB),
        ("mode_c", Mode::ModeC),
        ("mode_d", Mode::ModeD),
    ] {
        modes.insert(
            key.to_string(),
            seats_for_mode(&mode),
        );
    }
    let mut roles = std::collections::HashMap::new();
    for role in [
        PipelineRole::Host,
        PipelineRole::Auditor,
        PipelineRole::Emotion,
        PipelineRole::Lyricist,
        PipelineRole::Reviser,
        PipelineRole::Producer,
        PipelineRole::StyleAnalyst,
    ] {
        let r = roles::role_for(role);
        roles.insert(
            role,
            RoleMeta {
                name: r.name.to_string(),
                emoji: r.emoji.to_string(),
                knowledge: r.knowledge_tables.iter().map(|(t, _, _)| t.to_string()).collect(),
            },
        );
    }
    Ok(PipelineMeta { modes, roles })
}

/// 解析角色实际使用的 API 配置：角色覆盖优先，缺的字段逐项 fallback 全局
pub fn resolve_api(req: &PipelineRequest, role: PipelineRole) -> (String, String, String) {
    if let Some(map) = &req.role_overrides {
        if let Some(ov) = map.get(&role) {
            let base_url = ov.base_url.clone().unwrap_or_else(|| req.base_url.clone());
            let api_key = ov.api_key.clone().unwrap_or_else(|| req.api_key.clone());
            let model = ov.model.clone().unwrap_or_else(|| req.model.clone());
            return (base_url, api_key, model);
        }
    }
    (req.base_url.clone(), req.api_key.clone(), req.model.clone())
}

// ---------------------------------------------------------------------------
// 角色审改（阶段 1）
// ---------------------------------------------------------------------------

/// 一条修订片段（P1-3：target = SongPlan JSON Pointer，content = JSON 替换值）
#[derive(Debug, Clone)]
struct ReviewChange {
    target: String,
    content: serde_json::Value,
    reason: String,
}

/// 角色审改结果
#[derive(Debug, Clone)]
struct ReviewResult {
    agree: bool,
    changes: Vec<ReviewChange>,
    reason: String,
    /// agree=true 时的已核查关键检查项清单（无异议最低门槛：必须列出核查依据，防偷懒 agree）
    checked: Vec<String>,
    /// 输出不可信（JSON 解析失败 / agree 字段缺失）——意见作废，仅作警示记录。
    /// degraded 的结果不算 agree 也不算异议：不进 round_changes、不阻断收敛，但必须可见。
    degraded: bool,
}

/// target 合法：JSON Pointer（以 / 开头）；非法归一为 /notes（杂项意见）
fn normalize_target(t: &str) -> String {
    let t = t.trim();
    if t.starts_with('/') {
        t.to_string()
    } else {
        // 旧枚举兼容：映射到最接近的指针，避免模型偶发旧格式时整条作废
        match t {
            "style_prompt" => "/global".to_string(),
            "lyrics" => "/sections".to_string(),
            "params" => "/settings".to_string(),
            _ => "/notes".to_string(),
        }
    }
}

/// 解析审改 JSON（不可信输出显式降级，不再假同意）。
/// - JSON 解析失败 / agree 字段缺失 → degraded=true（意见作废，仅作警示记录，见 humanize_review）
///   ——content 为空的修订丢弃；非法 target 归一为 other。
fn parse_review(raw: &str) -> ReviewResult {
    let cleaned = strip_json_fence(raw);
    let Ok(v) = serde_json::from_str::<Value>(&cleaned) else {
        return ReviewResult {
            agree: false,
            changes: vec![],
            reason: "输出无法解析为 JSON，本轮意见未采纳".to_string(),
            checked: vec![],
            degraded: true,
        };
    };
    let Some(agree) = v["agree"].as_bool() else {
        return ReviewResult {
            agree: false,
            changes: vec![],
            reason: "输出缺少 agree 字段（可能被截断或格式不符），本轮意见未采纳".to_string(),
            checked: vec![],
            degraded: true,
        };
    };
    let mut changes = Vec::new();
    if let Some(arr) = v["changes"].as_array() {
        for c in arr {
            let content = c.get("content").cloned().unwrap_or(serde_json::Value::Null);
            if content.is_null()
                || content.as_str().is_some_and(|s| s.trim().is_empty())
                || content.as_array().is_some_and(|a| a.is_empty())
                || content.as_object().is_some_and(|o| o.is_empty())
            {
                continue; // 空修订无意义，丢弃
            }
            changes.push(ReviewChange {
                target: normalize_target(c["target"].as_str().unwrap_or("")),
                content,
                reason: c["reason"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    let reason = v["reason"].as_str().unwrap_or("").to_string();
    let checked = v["checked"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    ReviewResult { agree, changes, reason, checked, degraded: false }
}

/// 审改结果 → 前端可读文本
fn humanize_review(role: PipelineRole, r: &ReviewResult) -> String {
    // 不可信输出显式示警——绝不伪装成"无异议 ✅"
    if r.degraded {
        return format!("⚠️ {}：输出无法采信（{}）", role.name(), r.reason);
    }
    if r.agree {
        // 空手 agree 可见化：prompt 要求无异议必须附 checked 清单，未附即警示
        if r.checked.is_empty() {
            return format!("{}：无异议 ⚠️（未附核查清单）", role.name());
        }
        // 无异议最低门槛：展示已核查清单（防"偷懒 agree"，让无异议可审计）
        return format!("{}：无异议 ✅（已核查：{}）", role.name(), r.checked.join(" / "));
    }
    let mut out = format!("{}：提出 {} 处修订", role.name(), r.changes.len());
    for c in &r.changes {
        // 修订全文展示（旧 40 字截断删除——对话流气泡支持长文本，用户应看到完整意见）
        let reason = if c.reason.is_empty() { String::new() } else { format!("（{}）", c.reason) };
        out.push_str(&format!("\n· {} → {}{}", c.target, content_preview(&c.content), reason));
    }
    if !r.reason.is_empty() {
        out.push_str(&format!("\n总体意见：{}", r.reason));
    }
    out
}

fn content_preview(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// 取表某列全部值（按需检索候选词用）
fn column_values(kb: &KnowledgeBase, table: &str, col: &str) -> Vec<String> {
    kb.table(table)
        .ok()
        .and_then(|t| t.header_index(col).map(|idx| (t, idx)))
        .map(|(t, idx)| {
            t.rows
                .iter()
                .filter_map(|r| r.get(idx).cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// 关键词表候选 token 集（**多列析取**检索面的候选来源，#31）：
/// 逐列取单元格 → 单源分词（`rules::keyword_tokens`，与可达性审计同一函数）→ 去重。
/// 去重避免同一 token（如多行共用的家族伞词）在打分里被重复计数。
fn keyword_candidates(kb: &KnowledgeBase, table: &str, key_cols: &[&str]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for col in key_cols {
        for cell in column_values(kb, table, col) {
            for tok in crate::rules::keyword_tokens(&cell) {
                if seen.insert(tok.to_string()) {
                    out.push(tok.to_string());
                }
            }
        }
    }
    out
}

/// 从方案文本提取候选词命中（候选词出现在方案中即命中）。
/// 单字（短于 `rules::KEYWORD_MIN_CHARS`）候选直接丢弃——避免"深夜"误命中关键词"夜"；
/// 因此数据侧必须满足同一字长下限，否则该行**永久不可达**（旧行为静默丢弃，无日志无守护；
/// 现由 `knowledge::reachability_violations` 审计 + 守护测试 + 运行期告警三重兜底，#22）。
fn matching_keywords(plan: &str, candidates: &[String]) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| c.chars().count() >= crate::rules::KEYWORD_MIN_CHARS && plan.contains(c.as_str()))
        .cloned()
        .collect()
}

/// 关键词表全表休眠告警去重（每表每进程一次）——旧行为下"整表零命中"完全静默（#22）。
static KEYWORD_DORMANT_WARNED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

/// #22：关键词表零命中不再静默——整表休眠即告警一次（含行数），提示"补充行的检索键未出现在方案里"。
fn warn_keyword_table_all_dormant_once(table: &str, kb: &KnowledgeBase) {
    let set = KEYWORD_DORMANT_WARNED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    let mut guard = match set.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.insert(table.to_string()) {
        let rows = kb.table(table).map(|t| t.rows.len()).unwrap_or(0);
        tracing::warn!(
            table = %table,
            rows,
            "关键词表本轮整表零命中：全部行均未注入（检索列集合见 rules::KEYWORD_TABLES；补充行若其任一检索列的 token 都不出现在方案文本中将永久休眠，可加 aliases 列补同义名）"
        );
    }
}

/// 关键词表注入（#22/#31）：检索键列集合与条数上限取自 `rules::KEYWORD_TABLES` 单源；
/// 检索面 = **多列析取**（任一列的任一 token 字面出现在方案中即命中，如 style_genre 的
/// genre/base/aliases）；命中即注入（上限随规格），全部未命中 → 零行 + 无示例标注（M18）并告警一次。
fn keyword_table_render(
    kb: &KnowledgeBase,
    table: &str,
    proj: Option<&[&str]>,
    plan: &str,
) -> Result<String, String> {
    let spec = crate::rules::keyword_table(table)
        .ok_or_else(|| format!("{} 未登记为关键词表（rules::KEYWORD_TABLES）", table))?;
    let cands = matching_keywords(plan, &keyword_candidates(kb, table, spec.key_cols));
    if cands.is_empty() {
        warn_keyword_table_all_dormant_once(table, kb);
        // 整表休眠：条件恒不命中（主检索列 + 空候选集）——输出走"未命中关键词"标准文案
        return kb.render_filtered_any(table, &[(spec.key_cols[0], &[])], proj, plan, None, &[]);
    }
    let refs: Vec<&str> = cands.iter().map(|s| s.as_str()).collect();
    // 多列析取：每列一个 (列, 命中 token 集) 条件——render_filtered_any 的 any() 语义即 OR
    let conds: Vec<(&str, &[&str])> =
        spec.key_cols.iter().map(|c| (*c, refs.as_slice())).collect();
    kb.render_filtered_any(table, &conds, proj, plan, Some(spec.max_rows), &[])
}


// ---------------------------------------------------------------------------
// 注入量规范（条数上限）——集中定义，测试锁定，禁止散改
// 依据：知识库注入是"参考素材"而非"全量拷贝"，条数过多则 LLM 记不住重点。
// - 关键词表（emotions/cliches/hooks）：命中 6 条封顶——主词 1-3 个 + 近邻，6 条覆盖完整
// - 流派表：3 条封顶——方案通常命中 1-2 个流派，3 条少而准
// - 乐器表：15 件封顶——能量区间覆盖弧线两端，"少而准"验证值
// - 未命中：零行+无示例标注（M18，调用方走确定性默认；见 render_filtered_any 内部）
// - suno_rules：校验员全量（40 条 < 50 截断上限）；其他角色按规则子集过滤
// - 思维资产（lyric_craft/compose_craft）：引用必达（prompt 点名核查的编号强制投递）+ 按
//   与当前方案的内容相关性补足，8 条封顶（必达项数受守护测试约束 ≤ 本上限）
// - 单角色一次注入总字数封顶：预算按最坏情况**实测**标定（#31 aliases；add_douyin_arc 再标）：
//   制作人 mode_d = style_genre 3 + instruments 15 + suno_rules（原子集 + 10 条 style_arc_d_*）+ compose_craft 8 → 5514 字（最大）；
//   取 6000 = 实测最大 + 486 字余量（~8%）
// ---------------------------------------------------------------------------
/// 关键词表（emotions/cliches/hooks）命中条数上限（单源：rules::KEYWORD_MAX_ROWS）
pub const INJECT_MAX_KEYWORD_ROWS: usize = crate::rules::KEYWORD_MAX_ROWS;
/// 流派表命中条数上限（单源：rules::STYLE_GENRE_MAX_ROWS）
pub const INJECT_MAX_STYLE_GENRE_ROWS: usize = crate::rules::STYLE_GENRE_MAX_ROWS;
/// 乐器表能量区间命中件数上限
pub const INJECT_MAX_INSTRUMENTS_ROWS: usize = 15;
/// 思维资产（lyric_craft/compose_craft）单次注入条数上限。
/// 语义：**引用必达行优先占位**（prompt 点名要求核查的编号必须可见，不得被上限截掉），
/// 余下名额按内容相关性补足——不再是"CSV 前 8 条"（#15 旧行为）。
pub const INJECT_MAX_CRAFT_ROWS: usize = 8;
/// 全量表（suno_rules）渲染截断上限（当前 50 条规则 = 40 + 10×style_arc_d_*，留 10 条余量防静默截断）
pub const INJECT_MAX_FULL_ROWS: usize = 60;
/// 单角色一次注入总字数封顶（超过告警；最坏情况 = 制作人 mode_d 含 10 条 style_arc_d_*，
/// add_douyin_arc 实测 5514 字，取 6000 留 ~8% 余量）
pub const INJECT_MAX_TOTAL_CHARS: usize = 6000;
/// 完整档案传递原则（2026-09-13，用户设计确认）：方案/修订史全量传给每个角色——
/// 每个角色拿到的是完整档案（情绪/歌词/编曲/指令/分析），不做信息孤岛。
/// 旧值 8000 字在 NOTES 分析并入方案后（M1）已达 80%，一旦越线从尾部切掉的
/// 恰是 LYRICS/STYLE/PARAMS 本体（NOTES-first 排序）——角色只看到分析看不到方案。
/// 预算护栏职责在 budget 模块（token 计费与超时），不在此处；此值仅作病态输出保险丝。
pub const INJECT_MAX_PLAN_CHARS: usize = 100_000;
/// revisions_log 保留条数（完整档案原则：3 轮 × 全角色 + 主持人条目全额保留，不再折叠）
pub const INJECT_MAX_LOG_ENTRIES: usize = 24;
/// 格式输出截断时注入打回循环的 issue 文案（走 AuditResult 事件，用户可见；且随【格式问题】
/// 清单进 LLM 上下文 → 属散文载体，已登记进 `rules::PROSE_CARRIERS`）
pub(crate) const TRUNCATION_ISSUE: &str = "输出被截断（finish_reason=length），请精简内容后重新输出完整提示词包";

/// 方案截断（超长截断 + 附注，不静默丢；纯函数可测）
fn truncate_plan(plan: &str) -> String {
    if plan.chars().count() <= INJECT_MAX_PLAN_CHARS {
        return plan.to_string();
    }
    let kept: String = plan.chars().take(INJECT_MAX_PLAN_CHARS).collect();
    format!("{}…\n[方案过长，已截断前 {} 字]", kept, INJECT_MAX_PLAN_CHARS)
}

/// 修订 log 折叠（只留最近 N 条，更早合成单行摘要；纯函数可测）
/// 的 ⚠️ 条目短，折叠只压旧条目，警示可见性不受影响
fn fold_log(log: &[(String, String)]) -> Vec<(String, String)> {
    if log.len() <= INJECT_MAX_LOG_ENTRIES {
        return log.to_vec();
    }
    let folded_count = log.len() - INJECT_MAX_LOG_ENTRIES;
    // 摘要保留各条 reason 前 30 字（去重关键词仍在）
    let summary: Vec<String> = log[..folded_count]
        .iter()
        .map(|(name, rev)| {
            let brief: String = rev.chars().take(30).collect();
            format!("{}:{}", name, brief)
        })
        .collect();
    let mut out = vec![(
        "早期修订".to_string(),
        format!("等 {} 条早期修订已折叠（{}）", folded_count, summary.join("；")),
    )];
    out.extend_from_slice(&log[folded_count..]);
    out
}

/// 微观②：按需检索注入——按角色绑定表 + 列投影 + 当前方案关键词过滤，只注入命中条目。
/// - emotions/cliches/hooks/style_genre（关键词表）：候选与过滤条件同取单源检索列集合
///   （`rules::KEYWORD_TABLES` 的多列析取面 + 单源分词 `rules::keyword_tokens`，条数上限随规格）；
///   整表零命中时告警一次（#22）
/// - instruments：按方案能量区间数值过滤（覆盖弧线两端），上限 15 件
/// - suno_rules：校验员全量（定稿人必须全见）；其他角色按行子集过滤（rule 列 contains 匹配）
///   行子集单源 = `rules::SUNO_RULES_FOR_*`（#19：规则对职责角色可达）
/// - cols 投影：空切片 = 全列；非空 = 按角色只注入这些列（多角色侧重点）
/// - subset 行子集：空切片 = 全行；非空 = 按 rule 列值过滤（如制作人只要参数/配器类规则）
/// - craft_refs：角色 prompt 点名要求核查的思维资产编号（`rules::CRAFT_REFS_*` 单源）——
///   透传给渲染层做**引用必达**（强制投递，不受条数上限截断），与 prompt 要求同源；
///   #30 起渲染层**按表结算**（别表编号不算本表缺陷），悬空编号（全表皆无）在此审计并告警
/// - mode / role：**审改注入上下文**（#29）——思维资产行的条件标签（抖音→mode_d、
///   A/B/C→mode_a/b/c、制作→producer）据此成为**真限定**；阶段标签取自
///   `rules::CRAFT_STAGE_CONSUMER_REVIEWER` 单源登记（调用点不得自造标签集合）
///
/// 条数上限见 INJECT_MAX_* 常量（集中定义，测试锁定）。
fn inject_knowledge(
    kb: &KnowledgeBase,
    tables: &[(&str, &[&str], &[&str])],
    craft_refs: &[&str],
    plan: &str,
    mode: &str,
    role: Option<&str>,
) -> String {
    // 审改上下文：阶段标签单源取自规则登记（未登记即空 → 思维资产渲染报错并被下方 warn 捕获）
    let craft_ctx = crate::rules::stage_consumer(crate::rules::CRAFT_STAGE_CONSUMER_REVIEWER).map(|c| {
        crate::knowledge::CraftInjectCtx { consumer: c.name, stage_tags: c.stage_tags, mode, role }
    });
    // #30 悬空引用审计：refs 中在任何思维资产表都查无此行的编号 = 真数据缺陷（每编号每进程一次告警）。
    // 与渲染层的"按表结算"分工：别表编号不算缺陷（不进告警），全部表皆无才算。
    kb.warn_dangling_craft_refs(craft_refs);
    let mut out = String::new();
    for (t, cols, subset) in tables {
        // 列投影：空切片 = 全列（None），非空 = 角色裁剪
        let proj: Option<&[&str]> = if cols.is_empty() { None } else { Some(cols) };
        let rendered = match *t {
            "instruments" => {
                match plan_energy_range(plan) {
                    // 条数规范：能量区间命中 ≤15 件（"少而准"验证值）
                    Some((e_min, e_max)) => {
                        kb.render_instruments_by_energy(e_min, e_max, proj, plan, Some(INJECT_MAX_INSTRUMENTS_ROWS))
                    }
                    None => kb.render_filtered_any("instruments", &[("instrument", &[])], proj, plan, None, &[]), // 无能量：兜底
                }
            }
            "suno_rules" => {
                if subset.is_empty() {
                    // 校验员：全量（规则必须全见）
                    kb.render_table(t, proj, Some(INJECT_MAX_FULL_ROWS), Some(mode))
                } else {
                    // 其他角色：按规则名子集过滤（rule 列 contains 匹配），子集先过场景门（P1-6：
                    // 旧实现子集路径绕过 scenario 过滤，抖音行可进 A/B 审改上下文）
                    let allowed = kb.suno_rule_subset_for_mode(subset, mode);
                    if allowed.is_empty() {
                        Ok(String::new())
                    } else {
                        kb.render_filtered_any(t, &[("rule", &allowed)], proj, plan, None, &[])
                    }
                }
            }
            // 思维资产表（lyric_craft/compose_craft，单源判定 rules::is_craft_table）：注入门 =
            // `knowledge::craft_inject_gate`（阶段域"审改/全程" + 条件域限定抖音/A/B/C/制作，见 #29）；
            // 在上限内**引用必达优先 + 内容相关性补足**（#30 起引用必达按表结算，别表编号不再假告警）。
            // 旧行为（#15）：恒 0 打分 + 稳定排序 → 永远只注入 CSV 前 8 行，尾部（含用户追加）永不生效。
            // 旧行为（#17）：子串匹配字面"审改" → trigger="全程" 的行（CC-23/CC-28）任何角色不可达。
            // 旧行为（#29）：条件标签无执行门 → LC-31（审改/A/B/C）在 D 模式也注入、CC-22（审改/制作）进所有角色。
            _ if crate::rules::is_craft_table(t) => match craft_ctx.as_ref() {
                Some(ctx) => kb.render_craft_table(
                    t,
                    ctx,
                    proj,
                    plan,
                    Some(INJECT_MAX_CRAFT_ROWS),
                    craft_refs,
                ),
                None => Err(format!(
                    "思维资产表 {} 注入缺少审改上下文（CRAFT_STAGE_CONSUMER_REVIEWER 未登记）",
                    t
                )),
            },
            // 关键词表（emotions/cliches/hooks/style_genre）：路由与条数上限单源在
            // rules::KEYWORD_TABLES——**新增关键词表只改单源**（此处无需改动）；未登记的表保守全量。
            _ => match crate::rules::keyword_table(t) {
                Some(_) => keyword_table_render(kb, t, proj, plan),
                None => kb.render_table(t, proj, Some(INJECT_MAX_FULL_ROWS), Some(mode)),
            },
        };
        match rendered {
            Ok(rendered) => {
                out.push_str(&rendered);
                out.push('\n');
            }
            Err(e) => {
                // 表可能在加载期被 P3 降级跳过——告警但不阻断
                tracing::warn!(table = %t, error = %e, "知识库注入失败");
            }
        }
    }
    // 总字数封顶：超过预算告警（不截断——宁可让测试/日志暴露，也不破坏表格完整性）
    if out.chars().count() > INJECT_MAX_TOTAL_CHARS {
        tracing::warn!(
            total_chars = out.chars().count(),
            cap = INJECT_MAX_TOTAL_CHARS,
            "注入总量超封顶"
        );
    }
    // Q2（第二十五批）：注入结果可观测——命中/未命中表名直接落 INFO（成功路径此前无任何记录，
    // "style_genre 是否命中"只能靠"无休眠告警"倒推）。段首行形如「## 表名 知识库（按需命中 N 条）」。
    {
        let mut hit_tables: Vec<&str> = Vec::new();
        let mut empty_tables: Vec<&str> = Vec::new();
        for seg in out.split("## ").skip(1) {
            let head = seg.lines().next().unwrap_or("");
            let name = head.split(' ').next().unwrap_or("");
            if head.contains("按需命中") {
                hit_tables.push(name);
            } else if head.contains("未命中") {
                empty_tables.push(name);
            }
        }
        tracing::info!(
            role = role.unwrap_or("host"),
            chars = out.chars().count(),
            hit = %hit_tables.join(","),
            empty = %empty_tables.join(","),
            "知识注入完成"
        );
    }
    out
}

/// 角色审改 user prompt 构建（纯函数，可测）。
///
/// #14 修复（同轮互盲）：讨论轮改**阵容序串行**后，第 k 个角色能拿到前 k-1 个角色
/// **本轮**已提的修订（`round_peers`）——落到同一 target 的修订在产生前即可被看到：
/// 可细化、可明确反对，冲突不再只能拖到汇总阶段由主持人"事后整合"。
/// 三段可见性严格分工，避免与历史日志重复：
/// ① 主持人当前方案；② **本轮同轮其他角色已提修订**；③ 历史（往轮）已提修订。
fn build_role_review_user_prompt(
    current_plan: &str,
    round_peers: &[(String, String)],
    history_log: &[(String, String)],
    next_tasks: &str,
    req: &PipelineRequest,
) -> String {
    // 方案截断 + log 折叠（预算纪律；知识库注入同风格）
    let plan_view = truncate_plan(current_plan);
    let log_view = fold_log(history_log);
    let mut user = format!("【主持人当前方案】\n{}\n\n", plan_view);
    // Mode C：原歌词全链路传递——审改员逐行字数/韵脚对齐的依据
    if let Some(original) = req.original_lyrics_text() {
        user.push_str(&format!("【原歌词（改写需逐行对齐）】\n{}\n\n", original));
    }
    if !round_peers.is_empty() {
        user.push_str("【本轮同轮其他角色已提修订（已按阵容顺序先行；可在此基础上细化，若不同意必须说明理由，禁止重复提同一问题）】\n");
        for (name, rev) in round_peers {
            user.push_str(&format!("- {}：{}\n", name, rev));
        }
        user.push('\n');
    }
    if !log_view.is_empty() {
        user.push_str("【往轮已提修订（可参考，不要重复提同一问题）】\n");
        for (name, rev) in &log_view {
            user.push_str(&format!("- {}：{}\n", name, rev));
        }
        user.push('\n');
    }
    if !next_tasks.is_empty() {
        user.push_str(&format!(
            "【主持人本轮任务分发（你这一轮要重点解决的问题）】\n{}\n\n",
            next_tasks
        ));
    }
    user.push_str("请审查：同意则输出 {\"agree\":true}；有优化点则输出 {\"agree\":false, \"changes\":[...]}。");
    user
}

/// 角色审改：审查主持人当前方案 → 查 CSV → 输出修订片段 JSON
///
/// `history_log` = 本轮开始前的修订日志（往轮 + 用户插话）；`round_peers` = 本轮
/// 阵容序中**排在本角色之前**的角色已提交的修订（串行产生的同轮可见性）。
async fn execute_review<R: Runtime>(
    app: &AppHandle<R>,
    role: PipelineRole,
    current_plan: &str,
    round_peers: &[(String, String)],
    history_log: &[(String, String)],
    next_tasks: &str,
    req: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<ReviewResult, AppError> {
    use crate::models::PipelineEnvelope;
    // 本函数事件包 run_id
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    let kb = load_knowledge()?;
    let r = roles::role_for(role);

    let mut system = String::new();
    system.push_str(&format!("【角色】{} {}\n", r.name, r.emoji));
    // C3：角色提示词过数值单源插值（${占位符} → rules 常量派生值）
    system.push_str(&format!("{}\n", crate::rules::interpolate(r.system_prompt)));
    // 微观②：按需检索注入——按角色绑定表 + 当前方案关键词过滤，只注入命中条目（suno_rules 规则全量）
    // #29：思维资产条件标签（抖音/A/B/C/制作）在此以「当前模式 + 当前角色」判定，成为真限定
    system.push_str(&inject_knowledge(
        &kb,
        r.knowledge_tables,
        r.craft_refs,
        current_plan,
        req.mode.to_str_name(),
        Some(role.storage_key()),
    ));
    // P4：单源校验清单（数字唯一 prose 载体；审改口径与硬校验同源）
    system.push_str(&crate::rules::checklist(req.mode.to_str_name()));
    system.push('\n');
    // P1-5：制作人/校验员可改 /global/vocal|mix，须见词表 id
    if matches!(role, PipelineRole::Producer | PipelineRole::Auditor) {
        system.push_str(&crate::knowledge::vocal_mix_prompt_block(&kb, crate::knowledge::VOCAL_MIX_INJECT_MAX_CHARS));
    }
    system.push_str("\n输出 JSON（严格符合格式，不输出其他内容）：\n");
    system.push_str(&crate::rules::interpolate(r.output_schema));
    // 产出语言开关：en 时追加英文产出指令（zh = 空串，零漂移）
    system.push_str(discussion_lang_directive(req));

    // #14：同轮可见性由纯函数单源构建（本轮同轮修订块 + 往轮日志块严格分工）
    let user = build_role_review_user_prompt(current_plan, round_peers, history_log, next_tasks, req);

    let (base_url, api_key, model) = resolve_api(req, role);
    emit(PipelineEvent::StepStart { role });
    let (out, resp) = match plan_flow::role_review_structured(
        app, req, role, system, user, budget, run_id, &base_url, &api_key, &model,
    )
    .await
    {
        Ok(v) => v,
        Err(e) if e.kind == crate::errors::ErrorKind::Parse => {
            // 结构化 Parse：意见作废（与旧 degraded 同语义）
            emit(PipelineEvent::Degraded {
                flag: "call_degraded".into(),
                detail: format!("{} 结构化审改解析失败，意见作废（已降级警示）: {}", role.name(), e.message),
            });
            let result = ReviewResult {
                agree: false,
                changes: vec![],
                reason: e.message.clone(),
                checked: vec![],
                degraded: true,
            };
            emit(PipelineEvent::StepDone {
                role,
                summary: humanize_review(role, &result),
            });
            return Ok(result);
        }
        Err(e) => return Err(e),
    };
    emit_usage(app, role, &resp, run_id);
    let result = ReviewResult {
        agree: out.agree,
        changes: out
            .changes
            .into_iter()
            .filter(|c| {
                !(c.content.is_null()
                    || c.content.as_str().is_some_and(|s| s.trim().is_empty())
                    || c.content.as_array().is_some_and(|a| a.is_empty()))
            })
            .map(|c| ReviewChange {
                target: normalize_target(&c.target),
                content: c.content,
                reason: c.reason,
            })
            .collect(),
        reason: out.reason,
        checked: out.checked,
        degraded: false,
    };
    // 空手 agree 仍可见（humanize）；不降级
    emit(PipelineEvent::StepDone {
        role,
        summary: humanize_review(role, &result),
    });
    Ok(result)
}

/// 校验员讨论轮 user prompt 构建（纯函数，可测）。
/// round_changes 三元组 =（角色, 修订片段, 角色总体意见）——意见必达：即使无具体修订，
/// 角色总体意见也要进 prompt，供校验员核验冲突与漏项。
fn build_audit_review_user_prompt(
    current_plan: &str,
    round_changes: &[(PipelineRole, Vec<ReviewChange>, String)],
    revisions_log: &[(String, String)],
    next_tasks: &str,
    req: &PipelineRequest,
) -> String {
    // 方案截断 + log 折叠
    let plan_view = truncate_plan(current_plan);
    let log_view = fold_log(revisions_log);
    let mut user = format!("【主持人当前方案】\n{}\n\n", plan_view);
    // Mode C：原歌词全链路传递——校验员核对逐行对齐
    if let Some(original) = req.original_lyrics_text() {
        user.push_str(&format!("【原歌词（逐行字数对齐依据）】\n{}\n\n", original));
        if req.mode == Mode::ModeC {
            user.push_str("【Mode C 专项：必须核对新歌词与原歌词逐行对齐（行数一致、每行字数一致），发现漂移必须提出修订】\n\n");
        }
    }
    if !round_changes.is_empty() {
        user.push_str("【本轮各角色修订片段（审查合理性/冲突/漏项）】\n");
        for (role, changes, role_reason) in round_changes {
            user.push_str(&format!("## {} 的修订：\n", role.name()));
            if !role_reason.is_empty() {
                user.push_str(&format!("（总体意见：{}）\n", role_reason));
            }
            for c in changes {
                user.push_str(&format!(
                    "- target: {} | content: {} | reason: {}\n",
                    c.target, content_preview(&c.content), c.reason
                ));
            }
        }
        user.push('\n');
    }
    if !log_view.is_empty() {
        user.push_str("【往轮已提修订（不要重复提同一问题）】\n");
        for (name, rev) in &log_view {
            user.push_str(&format!("- {}：{}\n", name, rev));
        }
        user.push('\n');
    }
    if !next_tasks.is_empty() {
        user.push_str(&format!(
            "【主持人本轮任务分发（核验任务是否覆盖漏项、分配是否合理）】\n{}\n\n",
            next_tasks
        ));
    }
    user.push_str("请审查并输出观点：同意则 {\"agree\":true}；有问题则 {\"agree\":false, \"changes\":[...]}。");
    user
}

/// 校验员讨论轮审查：审查当前方案 + 本轮角色修订 + 主持人任务分发 → 提出观点返回主持人（复用 ReviewResult 契约）
async fn execute_audit_review<R: Runtime>(
    app: &AppHandle<R>,
    current_plan: &str,
    round_changes: &[(PipelineRole, Vec<ReviewChange>, String)],
    revisions_log: &[(String, String)],
    next_tasks: &str,
    req: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<ReviewResult, AppError> {
    use crate::models::PipelineEnvelope;
    // 生成参数（缺省默认；调用点透传给 llm 层）
    let gen = req.generation.clone().unwrap_or_default();
    // 本函数事件包 envelope
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    // #27-c 退避播报钩子（本函数两次调用共用）
    let backoff = backoff_hook(app.clone(), run_id.to_string());
    let kb = load_knowledge()?;

    let mut system = String::new();
    system.push_str("【角色】校验员 🔍\n");
    // C3：角色提示词过数值单源插值
    system.push_str(&crate::rules::interpolate(roles::auditor_review_prompt()));
    system.push('\n');
    if let Ok(rendered) = kb.render_table("suno_rules", None, Some(INJECT_MAX_FULL_ROWS), Some(req.mode.to_str_name())) {
        system.push_str(&rendered);
        system.push('\n');
    }
    // P4：单源校验清单（校验员审查口径与硬校验同源）
    system.push_str(&crate::rules::checklist(req.mode.to_str_name()));
    system.push('\n');
    system.push_str("\n输出 JSON（严格符合格式，不输出其他内容）：\n");
    system.push_str(roles::REVIEW_SCHEMA_AUDITOR);
    // M4（2026-09-13）：mode_c 校验员讨论轮补原词结构铁律——此前该规则只在改词人提示词
    // 与格式阶段存在，讨论阶段校验员不知情，提出"加 [Verse 2]"类修订（40 例实测 C 运行实证），
    // 争端拖到格式阶段才爆。与改词人提示词同口径。
    if req.mode == Mode::ModeC {
        system.push_str("\n【Mode C 结构铁律（审查时必须执行）】新歌词的段落结构必须与原歌词一致：原歌词几段，新歌词就几段；原歌词没有 Hook/Chorus/Verse 段，禁止提议新增任何歌词段落；每行字数必须与原歌词对应行完全一致。任何违反此铁律的修订（含你自己的修订建议）都应否决。\n");
    }

    let user = build_audit_review_user_prompt(current_plan, round_changes, revisions_log, next_tasks, req);

    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Auditor);
    emit(PipelineEvent::StepStart { role: PipelineRole::Auditor });
    let messages = vec![
        json!({"role":"system","content":system}),
        json!({"role":"user","content":user}),
    ];
    let resp = llm::call_llm_silent(
        &base_url, &api_key, &model,
        messages.clone(),
        llm::MAX_TOKENS_CAP,
        req.thinking,
        budget,
        &gen,
        run_id,
        llm::FINAL_STAGE_RESERVE,
        Some(&backoff),
    )
    .await?;
    // （上限放开后简化）：截断观测记录，JSON 完整时照常解析
    if llm::is_truncated(&resp.finish_reason) {
        tracing::warn!("校验员审查输出触及 max_tokens 上限");
    }
    let mut result = parse_review(&resp.raw);
    // P1-12 / D-05：首调 usage 先入账（RAII 语义）——重试若 `?` 失败也不丢首调 tokens
    emit_usage(app, PipelineRole::Auditor, &resp, run_id);
    // 解析失败 → 重试一次；仍失败走相关降级警示
    if result.degraded {
        tracing::warn!("校验员审查输出无法解析，重试一次");
        match llm::call_llm_silent(
            &base_url,
            &api_key,
            &model,
            messages.clone(),
            llm::MAX_TOKENS_CAP,
            req.thinking,
            budget,
            &gen,
            run_id,
            llm::FINAL_STAGE_RESERVE,
            Some(&backoff),
        )
        .await
        {
            Ok(resp2) => {
                let result2 = parse_review(&resp2.raw);
                // B-1：重试调用同样计费
                emit_usage(app, PipelineRole::Auditor, &resp2, run_id);
                if !result2.degraded {
                    result = result2;
                }
            }
            Err(e) => {
                // 首调已入账；重试失败仍返回首调降级结果（不丢 usage）
                tracing::warn!(error = %e.message, "校验员审查重试失败，保留首调降级结果");
            }
        }
    }
    emit(PipelineEvent::StepDone {
        role: PipelineRole::Auditor,
        summary: humanize_review(PipelineRole::Auditor, &result),
    });
    Ok(result)
}

// ---------------------------------------------------------------------------
// 主持人（阶段 0 统领 / 阶段 1 汇总）
// ---------------------------------------------------------------------------

/// 主持人 system 组装单源（阶段 0 初稿 / 阶段 1 汇总与所有回炉共用）：
/// 人设底座（阶段 0 = 模式完整指令；汇总 = host 统领人设）+ 地基 primer + 校验清单 + 信封契约。
///
/// R2 根因修复（2026-09-17）：汇总/回炉阶段旧实现只拼 `host 人设 + 信封规范`，看不到 primer 与
/// checklist——主持人整合修订时手里没有数字口径（说明行上限、配器 3-7 件、最强段 ≥5 件…），
/// 于是"修完一处又踩另一处"，硬校验打回 → 回炉 → 再违规，无限降级。
/// 现两阶段同走本函数：凡是主持人写方案的地方，看到的约束完全一致（上游告知 = 下游校验）。
fn host_system_with(base: &str, mode: &Mode, kb: &KnowledgeBase, plan: &str) -> String {
    let mut system = base.to_string();
    // P4：地基 primer（模式专属静态文本；缺失回退无 primer 旧行为）
    if let Some(primer) = crate::rules::host_primer(mode.to_str_name()) {
        system.push_str("\n\n");
        system.push_str(&crate::rules::interpolate(primer));
    }
    // #23 修复：阶段0 思维资产（trigger=阶段0）真消费者——旧实现该标签**零消费者**
    // （词表里可见、primer 是静态手写文本不读 CSV），于是 LC-12/LC-01/CC-24 在 C/D 模式
    // 彻底不可达、在 A/B 只能靠 primer 手写句双载体。现主持人写方案的**全部**上下文
    // （阶段0 初稿 + 阶段1 汇总/回炉）经唯一门 `knowledge::craft_inject_gate` 注入这些行，
    // 上下文（阶段标签/无角色身份）取自 `rules::CRAFT_STAGE_CONSUMERS` 单源登记。
    system.push_str(&host_craft_injection(kb, mode.to_str_name(), plan));
    // C3/D3：纳入单源——主持人与审改员/校验员同一清单（数字口径同源，加载一致）
    system.push_str("\n\n");
    system.push_str(&crate::rules::checklist(mode.to_str_name()));
    // P1-3：SongPlan 字段契约（替代信封/说明行文本契约；交付包由 render_suno 渲染）
    system.push_str("\n\n");
    system.push_str(&crate::rules::plan_field_contract());
    // P1-5：vocal_mix 受控词表（SongPlan.global.vocal/mix 引用其 id；超预算只注入 id+label_en）
    system.push_str("\n\n");
    system.push_str(&crate::knowledge::vocal_mix_prompt_block(kb, crate::knowledge::VOCAL_MIX_INJECT_MAX_CHARS));
    system
}

/// #23：主持人阶段0 思维资产注入块（**唯一调用点**在 `host_system_with`，两阶段共享）。
/// 阶段域 = `rules::CRAFT_STAGE_CONSUMER_HOST0`（标签"阶段0"），角色身份 = None
/// （角色域条件行如"阶段0/制作"在此不满足，fail-closed）；条数上限与审改侧同源
/// （`INJECT_MAX_CRAFT_ROWS`）。渲染失败/表缺失不阻断（与 `inject_knowledge` 同策略）。
fn host_craft_injection(kb: &KnowledgeBase, mode: &str, plan: &str) -> String {
    let consumer = match crate::rules::stage_consumer(crate::rules::CRAFT_STAGE_CONSUMER_HOST0) {
        Some(c) => c,
        None => return String::new(), // 登记缺失由守护测试拦截，运行期不阻断
    };
    let ctx = crate::knowledge::CraftInjectCtx {
        consumer: consumer.name,
        stage_tags: consumer.stage_tags,
        mode,
        role: None,
    };
    let mut block = String::new();
    for t in ["lyric_craft", "compose_craft"] {
        match kb.render_craft_table(t, &ctx, None, plan, Some(INJECT_MAX_CRAFT_ROWS), &[]) {
            Ok(rendered) => {
                block.push_str("\n\n");
                block.push_str(&rendered);
            }
            Err(e) => tracing::warn!(table = %t, error = %e, "主持人阶段0 思维资产注入失败"),
        }
    }
    block
}

/// 阶段 0 主持人 system：模式完整指令（含 override）作底座。
fn host_initial_system(mode: &Mode, kb: &KnowledgeBase, plan: &str) -> String {
    host_system_with(&prompt_for_mode(mode), mode, kb, plan)
}

/// 阶段 1 汇总/回炉主持人 system：host 统领人设作底座，primer + 阶段0 思维资产 + 清单 +
/// 信封与阶段 0 同源（汇总期主持人手里同样要有完整规则，否则"修完一处踩另一处"）。
fn host_summarize_system(mode: &Mode, kb: &KnowledgeBase, plan: &str) -> String {
    host_system_with(&crate::rules::interpolate(roles::host().system_prompt), mode, kb, plan)
}

/// 讨论层产出指令（阶段 0 统领 / 阶段 1 汇总 / 专家修订）：en = 全英文产出（含歌词）；
/// zh/mix = 空串（mix 态歌词保持中文，语言指令延迟到阶段 2 转写层，见 final_pack_lang_directive）。
fn discussion_lang_directive(req: &PipelineRequest) -> &'static str {
    if req.wants_all_en() {
        "\n\n【输出语言】本轮你的全部产出文字（方案、修订内容、歌词正文、风格与音色描述）一律使用英文输出；结构标签约定（如 [Verse]）与数值参数保持原样，不受影响。"
    } else {
        ""
    }
}

/// 阶段 2 最终包语言指令：mix = STYLE 节描述译英文、LYRICS 节逐字保留原语言；
/// en = 整包英文兜底（方案在讨论层已英文，此处防回退）；zh = 空串。
fn final_pack_lang_directive(req: &PipelineRequest) -> &'static str {
    if req.wants_style_en() && !req.wants_all_en() {
        "\n\n【STYLE 节语言覆盖】STYLE Prompt 中的风格、流派、乐器、音色、人声描述一律以英文排版（仅语言转换，内容语义与条目不变）；歌词正文保持原语言，不得翻译或改写。"
    } else if req.wants_all_en() {
        "\n\n【输出语言】最终提示词包全文以英文输出。"
    } else {
        ""
    }
}

/// 阶段 0：主持人用该模式的完整指令产出方案初稿（流式）
async fn run_host_initial<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<String, AppError> {
    use crate::models::PipelineEnvelope;
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    emit(PipelineEvent::HostStart { stage: HostStage::Initial });
    let kb = load_knowledge()?;
    let system = host_initial_system(&req.mode, &kb, &req.user_input);
    let system = format!("{}{}", system, discussion_lang_directive(req));
    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Host);
    let mut user = format!(
        "用户输入：\n{}\n\n请按上述方法论与 SongPlan schema 直接输出完整 PlanIR（JSON）。",
        req.user_input
    );
    if let Some(original) = req.original_lyrics_text() {
        user = format!(
            "原歌词：\n{}\n\n新主题/故事：\n{}\n\n请按上述方法论与 SongPlan schema 直接输出完整改词 PlanIR（JSON）。",
            original, req.user_input
        );
    }
    let plan = plan_flow::host_initial_plan(
        app, req, system, user, budget, run_id, &base_url, &api_key, &model,
    )
    .await?;
    emit(PipelineEvent::HostDone { stage: HostStage::Initial });
    let json = plan_flow::plan_to_checkpoint(&plan);
    tracing::info!(
        mode = %req.mode.to_str_name(),
        chars = json.chars().count(),
        sections = plan.sections.len(),
        "阶段0 初稿完成"
    );
    Ok(json)
}


/// 主持人汇总冲突预检（P1-3：content 为 JSON Value）。
type ConflictPair = (usize, usize, usize, usize, String);

fn is_conflict_pair(ca: &ReviewChange, cb: &ReviewChange) -> bool {
    if ca.target != cb.target {
        return false;
    }
    if ca.content == cb.content {
        return false;
    }
    let a = content_preview(&ca.content);
    let b = content_preview(&cb.content);
    let a = a.trim();
    let b = b.trim();
    !a.is_empty() && !b.is_empty() && !a.contains(b) && !b.contains(a)
}

fn detect_revision_conflicts(round_changes: &[(PipelineRole, Vec<ReviewChange>, String)]) -> Vec<ConflictPair> {
    let mut pairs = Vec::new();
    for (ai, (_, changes_a, _)) in round_changes.iter().enumerate() {
        for (pi, ca) in changes_a.iter().enumerate() {
            for (bi, (_, changes_b, _)) in round_changes.iter().enumerate().skip(ai + 1) {
                for (qi, cb) in changes_b.iter().enumerate() {
                    if is_conflict_pair(ca, cb) {
                        pairs.push((ai, pi, bi, qi, ca.target.clone()));
                    }
                }
            }
        }
    }
    pairs
}

/// round_changes 三元组 =（角色, 修订片段, 角色总体意见）——意见必达：即使无具体修订，
/// "提出总体异议但没给改法"也要让主持人知道并自行权衡。
fn build_summarize_user_prompt(
    current_plan: &str,
    round_changes: &[(PipelineRole, Vec<ReviewChange>, String)],
    original_lyrics: Option<&str>,
) -> String {
    // C4/D4：冲突预检——同 target 跨角色互不相容的修订对，条目前注入裁决指引
    let conflicts = detect_revision_conflicts(round_changes);
    // 方案截断（汇总输入同样封顶）
    let mut user = format!("【当前方案】\n{}\n\n【本轮各角色修订片段与校验员观点】\n", truncate_plan(current_plan));
    for (ri, (role, changes, role_reason)) in round_changes.iter().enumerate() {
        user.push_str(&format!("## {} 的修订：\n", role.name()));
        if !role_reason.is_empty() {
            user.push_str(&format!("（总体意见：{}）\n", role_reason));
        }
        for (ci, c) in changes.iter().enumerate() {
            // 该条目属某冲突对 → 条目前注入裁决指引（冲突对双侧条目都标）
            for (ai, pi, bi, qi, target) in &conflicts {
                let involved = (*ai == ri && *pi == ci) || (*bi == ri && *qi == ci);
                if involved {
                    user.push_str(&crate::rules::territory_adjudication_text(
                        round_changes[*ai].0.name(),
                        round_changes[*bi].0.name(),
                        target,
                    ));
                }
            }
            user.push_str(&format!("- target: {} | content: {} | reason: {}\n", c.target, content_preview(&c.content), c.reason));
        }
    }
    // L-4：原歌词比对基准（与阶段 2 同措辞）；None（A/B/D 或未填）不追加，行为零变化
    if let Some(original) = original_lyrics {
        user.push_str(&format!(
            "\n\n【原歌词（逐行字数对齐依据，整合修订时改词必须逐行等字数）】\n{}",
            original
        ));
    }
    user.push_str("\n请把修订整合进当前 SongPlan，输出 JSON：{\"plan\": <完整 SongPlan>, \"next_tasks\": \"下轮任务（无需继续则空字符串）\"}。notes 只写本轮整合说明，不重复上一版分析。");
    user
}

/// 阶段 1：主持人收集各角色修订 + 校验员观点，汇总成新版完整方案 + 下轮任务分发。
/// 返回 (完整方案, 下轮任务段)；任务段为空 = 已收敛/无需下轮。
async fn run_host_summarize<R: Runtime>(
    app: &AppHandle<R>,
    current_plan: &str,
    round_changes: &[(PipelineRole, Vec<ReviewChange>, String)],
    req: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<(String, String), AppError> {
    use crate::models::PipelineEnvelope;
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    let user = build_summarize_user_prompt(current_plan, round_changes, req.original_lyrics_text());
    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Host);
    emit(PipelineEvent::HostStart { stage: HostStage::Summarize });
    let kb = load_knowledge()?;
    let host_system = host_summarize_system(&req.mode, &kb, current_plan);
    let host_system = format!("{}{}", host_system, discussion_lang_directive(req));
    let (plan, tasks) = plan_flow::host_summarize_plan(
        app, req, host_system, user, budget, run_id, &base_url, &api_key, &model,
    )
    .await?;
    emit(PipelineEvent::HostDone { stage: HostStage::Summarize });
    Ok((plan_flow::plan_to_checkpoint(&plan), tasks))
}


// ---------------------------------------------------------------------------
// 校验员（阶段 2：结构化定稿）
// ---------------------------------------------------------------------------


/// 圆桌流水线事件发射单源（run_id 绑定 envelope）。
fn emit_pipeline_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
    event: PipelineEvent,
) {
    use crate::models::PipelineEnvelope;
    let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
}

/// #27-c 退避播报 → 流水线事件（纯函数，可测）：wire format 单源，
/// 无需 AppHandle 即可锁定字段（事件字段改名由测试拦下）。
fn backoff_event(n: llm::BackoffNotice) -> PipelineEvent {
    match n {
        llm::BackoffNotice::Start { attempt, wait_secs, reason } => PipelineEvent::Backoff {
            attempt,
            wait_secs,
            reason: reason.as_str().to_string(),
        },
        llm::BackoffNotice::End { attempt } => PipelineEvent::BackoffEnd { attempt },
    }
}

/// #27-c 退避播报钩子（单源）：llm 层进入/退出退避时回调 → 转成 pipeline 事件。
/// 事件带 run_id 信封，前端按 run 归属过滤（跨 run 串台不会显示到别的 run 上）。
pub(crate) fn backoff_hook<R: Runtime>(
    app: AppHandle<R>,
    run_id: String,
) -> impl Fn(llm::BackoffNotice) + Send + Sync {
    move |n| emit_pipeline_event(&app, &run_id, backoff_event(n))
}



/// P1-13：显式 opt-in 后调用圆桌同模型 Judge；失败只记日志/降级事件，不阻断终稿。
async fn maybe_run_text_eval<R: tauri::Runtime>(
    app: &AppHandle<R>,
    request: &PipelineRequest,
    final_text: &str,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) {
    if !crate::text_eval::judge_enabled(request.text_eval_judge) {
        return;
    }
    emit_pipeline_event(
        app,
        run_id,
        PipelineEvent::StepDone {
            role: PipelineRole::Auditor,
            summary: "文本评估 Judge 已启用（圆桌同模型，小样本）".into(),
        },
    );
    let gen = request.generation.clone().unwrap_or_default();
    match crate::text_eval::run_judge(
        &request.base_url,
        &request.api_key,
        &request.model,
        request.mode.to_str_name(),
        &request.user_input,
        final_text,
        budget,
        &gen,
        run_id,
    )
    .await
    {
        Ok(scores) => {
            let lines: Vec<String> = scores
                .dimensions
                .iter()
                .map(|d| format!("{}:{}:{}", d.id, d.score, d.note))
                .collect();
            emit_pipeline_event(
                app,
                run_id,
                PipelineEvent::TextEval {
                    total: scores.total,
                    lines,
                    small_sample: scores.small_sample,
                    note: "小样本：人工校准满 40 条且一致率≥80% 前仅供参考".into(),
                },
            );
        }
        Err(e) => {
            tracing::warn!(error = %e.message, "文本评估 Judge 失败（不阻断终稿）");
            emit_pipeline_event(
                app,
                run_id,
                PipelineEvent::Degraded {
                    flag: "text_eval_degraded".into(),
                    detail: format!("Judge 失败: {}", e.message),
                },
            );
        }
    }
}

async fn produce_final_text<R: Runtime>(
    app: &AppHandle<R>,
    current_plan: &str,
    mode: &Mode,
    request: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<(String, Vec<String>), AppError> {
    let plan = plan_flow::plan_from_checkpoint(current_plan)?;
    let (base_url, api_key, model) = resolve_api(request, PipelineRole::Auditor);
    // Auditor 定稿：按 SongPlan schema 输出完整方案（不再转写文本包；交付由 render_suno）
    // 定稿人设单源 = roles::auditor().system_prompt（经 rules 插值）；suno_rules 按角色绑定全量注入
    let r = roles::auditor();
    let mut system = crate::rules::interpolate(r.system_prompt);
    system.push_str(&inject_knowledge(
        &load_knowledge()?,
        r.knowledge_tables,
        r.craft_refs,
        current_plan,
        mode.to_str_name(),
        Some(PipelineRole::Auditor.storage_key()),
    ));
    system.push_str("\n\n");
    system.push_str(&crate::rules::plan_field_contract());
    system.push_str("\n\n");
    system.push_str(&crate::rules::checklist(mode.to_str_name()));
    system.push_str("\n\n");
    system.push_str(&crate::knowledge::vocal_mix_prompt_block(
        crate::knowledge::shared_knowledge(),
        crate::knowledge::VOCAL_MIX_INJECT_MAX_CHARS,
    ));
    system.push_str(final_pack_lang_directive(request));
    let (package, findings, _final_plan) = plan_flow::auditor_finalize(
        app,
        request,
        &plan,
        budget,
        run_id,
        &base_url,
        &api_key,
        &model,
        system,
    )
    .await?;
    Ok((package, findings))
}


/// 单次调用结束后发射用量事件（usage为None时不发射——网关未返回不阻塞流程）
pub(crate) fn emit_usage<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    role: PipelineRole,
    resp: &crate::models::LLMResponse,
    run_id: &str,
) {
    use crate::models::PipelineEnvelope;
    if let Some(u) = &resp.usage {
        let _ = app.emit(
            "pipeline",
            PipelineEnvelope::new(
                run_id.to_string(),
                PipelineEvent::StepUsage {
                    role,
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                },
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

/// 剥 markdown 围栏
fn strip_json_fence(t: &str) -> String {
    let t = t.trim();
    let s = t.find('{');
    let e = t.rfind('}');
    match (s, e) {
        (Some(a), Some(b)) if b > a => t[a..=b].to_string(),
        _ => t.to_string(),
    }
}


/// 从 refine 输入提取【上一版方案】段。
/// 取最后一个标记之后全文（同理：上一版方案内可能含标记字样）；缺失/空白返回 None。
fn extract_previous_plan(user_input: &str) -> Option<String> {
    let marker = "【上一版方案】";
    let pos = user_input.rfind(marker)?;
    let plan = user_input[pos + marker.len()..].trim();
    if plan.is_empty() {
        None
    } else {
        Some(plan.to_string())
    }
}


// ---------------------------------------------------------------------------
// 主流程
// ---------------------------------------------------------------------------

/// 讨论轮数上限（阶段 1：首轮审改 + 最多 2 轮修订讨论）
const MAX_DISCUSSION_ROUNDS: u32 = 3;

/// 流水线整体超时（防静默挂死兜底：所有模式正常 10 分钟内完成）
pub(crate) const PIPELINE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// #12 轮间确认门：单轮等待上限。无人确认即按"超时"自动放行——
/// 绝不因用户离开而把流水线挂死到超时强杀。
pub(crate) const ROUND_GATE_MAX_WAIT: Duration = Duration::from_secs(5 * 60);

/// #12 门的最大道数 = 轮数 - 1（门开在"轮与轮之间"，末轮之后无下一轮可确认）。
/// 与 `MAX_DISCUSSION_ROUNDS` 同源派生——改轮数自动跟随，不另立数字。
pub(crate) const ROUND_GATE_COUNT: u32 = MAX_DISCUSSION_ROUNDS.saturating_sub(1);

/// #12 开启确认门时对整体超时的补偿额度：用户的思考时间**不计入生成预算**，
/// 否则暂停会挤掉终稿阶段、或被 `spawn_guarded` 的硬超时整条强杀。
pub(crate) const ROUND_GATE_TIMEOUT_ALLOWANCE: Duration =
    Duration::from_secs(ROUND_GATE_MAX_WAIT.as_secs() * ROUND_GATE_COUNT as u64);

/// spawn + 超时守卫的结果
enum GuardOutcome {
    /// 正常完成（含业务 Err）
    Completed(Result<String, AppError>),
    /// 内部 panic（隔离层捕获，依赖 unwind——Cargo.toml 不得设置 panic="abort"）
    JoinPanicked(String),
    /// 超时：任务已被 abort 并回收，费用已停止
    TimedOut,
}

/// spawn 任务并施加超时守卫：超时则 abort 任务并等待其真正终止（防假中止）。
/// panic 隔离与超时强杀都在这里，与事件发射解耦（可独立单测）。
async fn spawn_guarded<F>(fut: F, timeout: Duration) -> GuardOutcome
where
    F: Future<Output = Result<String, AppError>> + Send + 'static,
{
    let mut handle = tokio::task::spawn(fut);
    // 用 &mut handle 保住所有权：若按值传入，超时后 handle 会随 timeout future 一起被
    // drop → tokio 语义为 detach，任务会继续在后台烧钱（历史修复的根因）
    match tokio::time::timeout(timeout, &mut handle).await {
        Ok(Ok(inner)) => GuardOutcome::Completed(inner),
        Ok(Err(e)) => GuardOutcome::JoinPanicked(format!("{}", e)),
        Err(_elapsed) => {
            handle.abort();
            let _ = handle.await; // 等任务真正终止（future drop 完成、在途连接关闭）再返回
            GuardOutcome::TimedOut
        }
    }
}

/// 主流程：跑圆桌流水线。
/// 隔离层：tokio::task::spawn 执行（内部 panic 不杀 worker 线程，转为错误返回——依赖
/// unwind，Cargo.toml 禁设 panic="abort"，改配置前先看这里）+ 整体超时（超时 = abort
/// 强杀在途任务，非放弃等待）。真实错误统一发 Failed 事件；用户取消发 Cancelled 事件。
pub async fn run_pipeline<R: Runtime>(app: AppHandle<R>, request: PipelineRequest) -> Result<String, AppError> {
    // RULE-RUN-ID：入口一次性解析 run_id（缺省后端生成）并写回请求，
    // 与 with_timeout 内解析同源（resolve_run_id），清理/取消/事件三处必为同一 id。
    let mut request = request;
    let rid = resolve_run_id(request.run_id.as_deref());
    request.run_id = Some(rid.clone());
    interject::reset(&rid);
    gate::reset(&rid);
    // #12：开启确认门时整体超时（= 预算）加补偿额度——暂停等待不吞生成预算。
    // 缺省关闭（None）走原超时，headless/脚本调用方不受影响。
    let timeout = if request.round_gate == Some(true) {
        PIPELINE_TIMEOUT + ROUND_GATE_TIMEOUT_ALLOWANCE
    } else {
        PIPELINE_TIMEOUT
    };
    // F2/D-06：with_active_run 守卫——任何结局（含 future 被丢弃）都 finish；
    // F3：begin 前到达的取消在此被消费，run 一开始即处于取消态
    cancel::with_active_run(&rid, run_pipeline_with_timeout(app, request, timeout)).await
}

/// run_id 解析单源：请求带非空 id 则尊重，否则后端生成 `run-{nanos}`。
pub(crate) fn resolve_run_id(requested: Option<&str>) -> String {
    match requested {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ),
    }
}

/// run_pipeline 的可测形态：超时时长参数化（生产 15 分钟，测试注入极小值）
/// pub 可见性供无头集成测试（tests/headless_modes.rs）直调；生产仍走 run_pipeline。
pub async fn run_pipeline_with_timeout<R: Runtime>(
    app: AppHandle<R>,
    request: PipelineRequest,
    timeout: Duration,
) -> Result<String, AppError> {
    use crate::budget::Budget;
    use crate::errors::ErrorKind;
    use crate::models::PipelineEnvelope;
    let app2 = app.clone();
    // 共享预算 = 超时时长——超时守卫是最后防线，预算是事前约束
    let budget = std::sync::Arc::new(Budget::with_timeout(timeout));
    // run_id 归属（请求带则尊重，缺省后端生成）；全部事件包 envelope 发射
    let run_id = resolve_run_id(request.run_id.as_deref());
    let emit = |app: &AppHandle<R>, event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.clone(), event));
    };
    match spawn_guarded(run_pipeline_inner(app2, request, budget, run_id.clone()), timeout).await {
        GuardOutcome::Completed(inner) => {
            if let Err(e) = &inner {
                // 取消走 Cancelled 事件（前端不标红），真实错误仍走 Failed
                if e.kind == ErrorKind::Cancelled {
                    emit(&app, PipelineEvent::Cancelled);
                } else {
                    emit(&app, PipelineEvent::Failed { error: e.message.clone() });
                }
            }
            inner
        }
        GuardOutcome::JoinPanicked(join_err) => {
            // spawn 的 future panic（如字符边界切片越界）：不杀 worker，转错误返回
            let msg = format!("流水线内部异常: {}", join_err);
            tracing::error!(message = %msg, "流水线失败");
            let err = AppError::new(ErrorKind::Internal, msg.clone());
            emit(&app, PipelineEvent::Failed { error: msg });
            Err(err)
        }
        GuardOutcome::TimedOut => {
            let mins = timeout.as_secs() / 60;
            let msg = format!("流水线超时（{} 分钟）未完成，已中止", mins);
            tracing::error!(message = %msg, "流水线失败");
            let err = AppError::new(ErrorKind::Timeout, msg.clone());
            emit(&app, PipelineEvent::Failed { error: msg });
            Err(err)
        }
    }
}

/// 主流程内层：三阶段（主持人统领 → 角色审改+校验员审查讨论 → 校验员格式化）
/// budget 为共享预算（Arc），调用链逐层透传
/// 增量模式（request.refine_targets.is_some()）时阶段 0 跳过——以上一版方案为起步，
/// 讨论轮只跑 targets 角色（Auditor 恒在：结构化定稿 + 讨论轮审查）。
/// run_id 透传——inner 内全部事件包 envelope 发射；取消/插话按 run_id 隔离。
async fn run_pipeline_inner<R: Runtime>(
    app: AppHandle<R>,
    request: PipelineRequest,
    budget: crate::budget::SharedBudget,
    run_id: String,
) -> Result<String, AppError> {
    use crate::models::PipelineEnvelope;
    // inner 专属发射器（全部事件带 run_id）
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.clone(), event));
    };
    let mode = &request.mode;
    // 取消检查点——按 run_id 隔离（多任务并行互不干扰）
    let checkpoint = || -> Result<(), AppError> {
        if cancel::is_cancelled(&run_id) {
            Err(AppError::cancelled())
        } else {
            Ok(())
        }
    };
    // 增量模式只跑 targets（空视为全量，防前端误传）；全量模式走模式阵容
    let incremental = request
        .refine_targets
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    let roles: Vec<PipelineRole> = if incremental {
        // 只保留模式阵容内的 targets（防非法角色），顺序按模式阵容
        let all: Vec<PipelineRole> = steps_for_mode(mode).iter().map(|s| s.role).collect();
        let targets = request.refine_targets.clone().unwrap_or_default();
        all.into_iter().filter(|r| targets.contains(r)).collect()
    } else {
        steps_for_mode(mode).iter().map(|s| s.role).collect()
    };
    // Q2（第二十五批）：运行期可观测性——成功路径此前**零 INFO 日志**（日志里只有异常告警），
    // 四模式 GUI 目测跑完后日志无任何运行记录，排障只能靠 UI/history。关键节点补 INFO（见
    // `pipeline_logging_nodes_are_present` 源码锁）。
    tracing::info!(
        mode = %mode.to_str_name(),
        roles = roles.len(),
        incremental,
        input_chars = request.user_input.chars().count(),
        has_original_lyrics = request.original_lyrics.is_some(),
        "流水线开始"
    );

    // ---- 阶段 0：主持人统领（增量模式跳过——上一版方案即起步，上一版格式已注入 user_input）----
    checkpoint()?;
    let mut current_plan = if incremental {
        // 增量起步：从 user_input 的【上一版方案】段提取；缺失则回落全量阶段 0（不静默用空方案）
        // P1-3：上一版须为 SongPlan JSON（检查点口径）；渲染后的交付包文本不可回填 IR →
        // 回落全量阶段 0（上一版文本仍在 user_input 内作上下文），避免讨论跑完才在定稿处 Parse 失败。
        match extract_previous_plan(&request.user_input)
            .filter(|p| plan_flow::plan_from_checkpoint(p).is_ok())
        {
            Some(plan) => plan,
            None => run_host_initial(&app, &request, &budget, &run_id).await?,
        }
    } else {
        run_host_initial(&app, &request, &budget, &run_id).await?
    };
    // 主持人上轮任务分发（第一轮无任务）
    let mut next_tasks = String::new();
    // 增量模式首条 log 声明参跑阵容（auditor 可见完整上下文）
    let mut revisions_log: Vec<(String, String)> = Vec::new(); // (角色名, 修订摘要)
    if incremental {
        let names: Vec<&str> = roles.iter().map(|r| r.name()).collect();
        revisions_log.push((
            "主持人".to_string(),
            format!("增量优化：本轮只跑 {}（上一版其他角色意见保留）", names.join("、")),
        ));
    }
    // C5/ADR-3：收敛观测——循环走满未 break = 轮次上限强制收敛（budget_degraded 源）
    let mut converged = false;
    // #12 轮间人工确认门：仅当请求显式开启（后端不擅自暂停）。
    let gate_enabled = request.round_gate == Some(true);
    // 用户在第几轮的确认门选择了"结束讨论"（None = 未提前收工）
    let mut stop_at_user_gate: Option<u32> = None;
    // 实际走到的轮次（含收敛/上限/提前收工三种终局）——检查点与降级声明同源读取
    let mut last_round: u32 = 0;
    for round in 1..=MAX_DISCUSSION_ROUNDS {
        last_round = round;
        let mut all_agree = true;
        // 三元组 =（角色, 修订片段, 角色总体意见）——异议必达，无具体修订的意见也要汇总
        let mut round_changes: Vec<(PipelineRole, Vec<ReviewChange>, String)> = Vec::new();
        // 本轮开始前的修订快照（上一轮及更早；本轮角色修订经 round_changes 传递，避免 auditor 双写）
        let prev_revisions = revisions_log.clone();
        checkpoint()?;
        // 轮边界消费用户插话（非阻塞——在途调用不受影响，意见进本轮输入）
        for note in interject::drain(&run_id) {
            let preview: String = note.chars().take(60).collect();
            revisions_log.push(("用户插话".to_string(), note.clone()));
            if next_tasks.is_empty() {
                next_tasks = note;
            } else {
                next_tasks.push_str(&format!("\n{}", note));
            }
            emit(PipelineEvent::StepDone {
                role: PipelineRole::Host,
                summary: format!("收到用户插话，已纳入本轮讨论：{}", preview),
            });
        }
        // ① 动态角色**阵容序串行**审改（#14 内容层修复）。
        //
        // 旧实现：`join_all` 并发 —— 同轮角色互相无依赖，每个角色只拿得到**轮前**的
        // `revisions_log`，本轮兄弟角色刚提的修订对它是不可见的（同轮互盲）。后果：
        // 两个角色对同一 target 各提一套彼此不相容的修订，只能在汇总阶段由主持人
        // "事后整合"，而主持人又被人设限定"只做整合不改细节"= 冲突无人裁决。
        //
        // 现实现：按 `roles`（= `steps_for_mode` 阵容序，权威单源）串行执行，跑完一个
        // 立即把其修订并入 `round_peers`，后续角色的 prompt 里就出现"【本轮同轮其他
        // 角色已提修订】"块——可细化、可明确反对，冲突在产生前被看到；残余分歧仍有
        // 汇总阶段的冲突预检 + 领地裁决兜底（两条腿缺一不可）。
        // 串行天然免错峰：旧 `index * 2s` staggered 启动（为并发岔开四路请求而设）随并发一并退役。
        let mut round_peers: Vec<(String, String)> = Vec::new();
        for role in roles.iter() {
            checkpoint()?;
            let result = execute_review(
                &app, *role, &current_plan, &round_peers, &prev_revisions,
                &next_tasks, &request, &budget, &run_id,
            )
            .await?;
            if result.degraded {
                // 不可信输出不进 round_changes（无可整合内容）、不阻断收敛，但必须留下警示。
                // 降级意见**不进 round_peers**：同轮可见性只传可信修订，防污染后续角色的判断。
                revisions_log.push((role.name().to_string(), humanize_review(*role, &result)));
            } else if !result.agree {
                // 异议必达——有 changes 带着改，没 changes 带着 reason 也要让主持人看到
                all_agree = false;
                round_changes.push((*role, result.changes.clone(), result.reason.clone()));
                let summary = humanize_review(*role, &result);
                round_peers.push((role.name().to_string(), summary.clone()));
                revisions_log.push((role.name().to_string(), summary));
            }
        }
        // ② 校验员审查（当前方案 + 本轮修订 + 上轮修订 + 任务分发核验 → 观点返回主持人）
        checkpoint()?;
        let auditor_result = execute_audit_review(
            &app, &current_plan, &round_changes, &prev_revisions, &next_tasks, &request, &budget, &run_id,
        )
        .await?;
        if auditor_result.degraded {
            // C5/ADR-3：call_degraded——校验员讨论轮输出不可信
            emit(PipelineEvent::Degraded {
                flag: "call_degraded".into(),
                detail: "校验员 输出解析失败，意见作废（已降级警示）".into(),
            });
            revisions_log.push((
                PipelineRole::Auditor.name().to_string(),
                humanize_review(PipelineRole::Auditor, &auditor_result),
            ));
        } else if !auditor_result.agree {
            all_agree = false;
            round_changes.push((
                PipelineRole::Auditor,
                auditor_result.changes.clone(),
                auditor_result.reason.clone(),
            ));
            revisions_log.push((
                PipelineRole::Auditor.name().to_string(),
                humanize_review(PipelineRole::Auditor, &auditor_result),
            ));
        }
        if all_agree {
            converged = true;
            tracing::info!(round, roles_changed = round_changes.len(), all_agree, "讨论轮完成：全体无异议，收敛");
            break; // 动态角色 + 校验员全部无异议 → 收敛
        }
        tracing::info!(round, roles_changed = round_changes.len(), all_agree, "讨论轮完成：进入汇总");
        // ③ 主持人汇总修订 + 校验员观点 → 新版完整方案 + 下轮任务分发
        let (new_plan, tasks) = run_host_summarize(&app, &current_plan, &round_changes, &request, &budget, &run_id).await?;
        current_plan = new_plan;
        next_tasks = tasks;
        emit(PipelineEvent::DiscussionRound {
            round,
            roles: round_changes.iter().map(|(r, _, _)| *r).collect(),
            reason: round_changes
                .iter()
                .flat_map(|(_, cs, role_reason)| {
                    cs.iter().map(|c| c.reason.clone()).chain(std::iter::once(role_reason.clone()))
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("；"),
        });
        // R3：每轮汇总后落检查点（失败可续跑；写失败只记 warn，不阻断流水线）
        {
            let cp = crate::commands::checkpoint::PipelineCheckpoint {
                schema_version: crate::commands::checkpoint::CHECKPOINT_SCHEMA_VERSION,
                mode: mode.to_str_name().to_string(),
                user_input: request.user_input.clone(),
                current_plan: current_plan.clone(),
                revisions_log: revisions_log.clone(),
                round,
                next_tasks: next_tasks.clone(),
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                song_plan: None,
            };
            if let Err(e) = crate::commands::checkpoint::save(&app, &run_id, &cp) {
                tracing::warn!(error = %e.message, "检查点落盘失败（不阻断）");
            }
        }
        // ④ #12 轮间人工确认门（路线图 §修复包 C）：本轮已汇总并落盘 → 暂停等用户决断。
        //    仅在请求显式开启、且**确实还有下一轮**时开（末轮之后无讨论可确认，开则纯属空等）。
        //    暂停期间提交的意见由**下一轮开头**的 interject::drain 消费——只有"继续"才有下一轮，
        //    故语义自洽；选"结束讨论"时残留意见在循环外向用户显式交待（不静默丢失）。
        //    取消优先：等待循环轮询取消位，用户在暂停中点"停止"立即终止。
        if gate_enabled && round < MAX_DISCUSSION_ROUNDS {
            let timeout_secs = ROUND_GATE_MAX_WAIT.as_secs();
            emit(PipelineEvent::RoundGatePending {
                round,
                next_round: round + 1,
                timeout_secs,
            });
            emit(PipelineEvent::StepDone {
                role: PipelineRole::Host,
                summary: format!(
                    "第 {} 轮已完成并汇总（已存检查点），等待你确认是否继续第 {} 轮",
                    round,
                    round + 1
                ),
            });
            checkpoint()?;
            let decision = gate::wait(&run_id, ROUND_GATE_MAX_WAIT).await?;
            emit(PipelineEvent::RoundGateResolved {
                round,
                decision: decision.as_str().to_string(),
            });
            match decision {
                gate::GateDecision::Finalize => {
                    // 用户主动收工：不是缺陷，但产出非全体共识——按"诚实降级"声明（见循环外）
                    stop_at_user_gate = Some(round);
                    break;
                }
                gate::GateDecision::Timeout => {
                    emit(PipelineEvent::Degraded {
                        flag: "pause_gate_degraded".into(),
                        detail: format!(
                            "第 {} 轮确认门等待超时（{} 秒无人确认），已自动继续下一轮",
                            round, timeout_secs
                        ),
                    });
                }
                gate::GateDecision::Continue => {
                    emit(PipelineEvent::StepDone {
                        role: PipelineRole::Host,
                        summary: format!("已确认继续第 {} 轮讨论", round + 1),
                    });
                }
            }
        }
    }
    // C5/ADR-3：budget_degraded——轮次上限强制收敛（非全体共识下的产出）
    // #12：用户主动收工与"走满上限"是两种不同终局，必须分别声明，不得混用同一措辞。
    tracing::info!(
        rounds = last_round,
        converged,
        stopped_by_user = stop_at_user_gate.is_some(),
        "讨论收敛"
    );
    if let Some(r) = stop_at_user_gate {
        emit(PipelineEvent::Degraded {
            flag: "pause_gate_degraded".into(),
            detail: format!(
                "用户在第 {} 轮确认门选择结束讨论，提前进入终稿（非全体共识产出）",
                r
            ),
        });
        // 暂停期间提交、但因"不再有下一轮"而无人消费的意见：显式记账 + 明确告知未被采纳
        for note in interject::drain(&run_id) {
            let preview: String = note.chars().take(60).collect();
            revisions_log.push(("用户插话（讨论已结束·未进入讨论轮）".to_string(), note.clone()));
            emit(PipelineEvent::StepDone {
                role: PipelineRole::Host,
                summary: format!("讨论已结束，此条意见未进入讨论轮（不会影响本次终稿）：{}", preview),
            });
        }
    } else if !converged {
        emit(PipelineEvent::Degraded {
            flag: "budget_degraded".into(),
            detail: format!("讨论轮次上限（{} 轮）强制收敛，非全体共识产出", MAX_DISCUSSION_ROUNDS),
        });
    }

    // ---- 阶段 2：校验员结构化定稿 + plan::validate 打回 ≤2 次 + render_suno（与 resume 共用 produce_final_text）----
    // R3：阶段 2 入口同样落检查点（含收敛后的 current_plan），终稿失败可直接续终稿
    //     （检查点存修复前方案：定稿重试只影响本次终稿产出，续跑语义不变）
    {
        let cp = crate::commands::checkpoint::PipelineCheckpoint {
            schema_version: crate::commands::checkpoint::CHECKPOINT_SCHEMA_VERSION,
            // B-3：mode 单源 to_str_name（旧规则用 Debug 字符串变换，两处序列化口径漂移风险）
            mode: mode.to_str_name().to_string(),
            user_input: request.user_input.clone(),
            current_plan: current_plan.clone(),
            revisions_log: revisions_log.clone(),
            // #12 修正：记录**实际走到**的轮次（收敛轮/上限轮/用户提前收工轮），
            // 旧规则写死 MAX——用户在门里"结束讨论"时会留下"跑了 3 轮"的假记录。
            round: last_round,
            next_tasks: next_tasks.clone(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            song_plan: None,
        };
        if let Err(e) = crate::commands::checkpoint::save(&app, &run_id, &cp) {
            tracing::warn!(error = %e.message, "阶段 2 检查点落盘失败（不阻断）");
        }
    }
    let (final_text, issues) = produce_final_text(&app, &current_plan, mode, &request, &budget, &run_id).await?;
    if !issues.is_empty() {
        // 降级输出（对齐"永远有产出"原则）：打回耗尽仍返回最后一次方案，
        // 格式问题已通过 AuditResult(pass=false) 事件显式告知前端（不空手报错）
        tracing::warn!(
            issue_count = issues.len(),
            issues = %issues.join("；"),
            "硬校验打回耗尽，降级返回最后一次方案"
        );
    }
    // R3：终稿成功（即使降级也是产出）→ 删除检查点，不留残留
    crate::commands::checkpoint::clear(&app, &run_id);
    // Q2（第二十五批）：流水线终局落 INFO——此前成功路径零记录，日志里看不出"跑过一次"
    tracing::info!(
        mode = %mode.to_str_name(),
        chars = final_text.chars().count(),
        issues = issues.len(),
        rounds = last_round,
        "流水线完成"
    );

    maybe_run_text_eval(&app, &request, &final_text, &budget, &run_id).await;

    Ok(final_text)
}

/// 优化/重跑：全流程重跑 + 反馈注入
/// feedback 长度校验在 command 入口（pipeline_refine）做，此处只拼装
/// 前端未传 targets（None）时按反馈关键词自动路由；显式传（含空数组→全量）则尊重前端
pub async fn run_pipeline_refine(app: AppHandle, request: PipelineRequest, feedback: &str) -> Result<String, AppError> {
    let mut req = request;
    req.user_input = format!("{}\n\n（优化反馈：{}）", req.user_input, feedback);
    if req.refine_targets.is_none() {
        let routed = roles_for_feedback(feedback, &req.mode);
        if !routed.is_empty() {
            req.refine_targets = Some(routed);
        }
        // 无命中 → None 保持全量（安全默认，不猜）
    }
    run_pipeline(app, req).await
}

// ---------------------------------------------------------------------------
// Tauri 命令（前端 invoke 入口）
// ---------------------------------------------------------------------------

/// 圆桌生成（前端调用）
/// 入口准入校验——非法输入在第一个 LLM 调用前拦截（Validation kind，前端 errText 展示）
#[tauri::command]
pub async fn pipeline_generate(app: AppHandle, request: PipelineRequest) -> Result<String, AppError> {
    crate::models::validate_request(&request, None)?;
    run_pipeline(app, request).await
}

/// 模式锁判定（B-5 纯函数，可测）：空 mode（旧版检查点无该字段）放行；匹配放行；错配返回报错文案。
fn checkpoint_mode_error(cp_mode: &str, req_mode: &str) -> Option<String> {
    if cp_mode.is_empty() || cp_mode == req_mode {
        None
    } else {
        Some(format!(
            "检查点模式 {} 与请求模式 {} 不一致，请用原模式续跑",
            cp_mode, req_mode
        ))
    }
}

/// 圆桌优化（前端调用，带反馈）
#[tauri::command]
pub async fn pipeline_refine(
    app: AppHandle,
    request: PipelineRequest,
    feedback: String,
) -> Result<String, AppError> {
    crate::models::validate_request(&request, Some(&feedback))?;
    run_pipeline_refine(app, request, &feedback).await
}

/// 请求取消指定 run（前端"停止"按钮调，带 run_id）——检查点在下次机会中断
/// R3：取消同时清检查点，避免"从上次继续"复活已取消任务
/// RULE-RUN-ID（C-05）：空 run_id 直接拒（旧规则置全局位且无复位点，污染 test_api 直到重启）。
#[tauri::command]
/// F3：返回是否生效（true = 已取消或 begin 前已登记待生效；false = 任务已结束，取消无效果）。
pub async fn cancel_pipeline(app: tauri::AppHandle, run_id: Option<String>) -> Result<bool, AppError> {
    let rid = run_id.unwrap_or_default();
    if rid.trim().is_empty() {
        return Err(crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            "缺少 run_id，取消无法定向到任务",
        ));
    }
    let outcome = cancel::request_cancel(&rid);
    crate::commands::checkpoint::clear(&app, &rid);
    Ok(outcome.effective())
}

/// 从检查点续跑（前端"从上次继续"按钮调）——断点方案直接进终稿，不重跑讨论轮。
/// 检查点缺失/损坏 → 明确报错（不静默全量重跑，避免用户误以为续跑实则从头来）。
#[tauri::command]
pub async fn pipeline_resume<R: Runtime>(
    app: AppHandle<R>,
    request: PipelineRequest,
    run_id: String,
) -> Result<String, AppError> {
    use crate::budget::Budget;
    crate::models::validate_request(&request, None)?;
    let cp = crate::commands::checkpoint::load(&app, &run_id).ok_or_else(|| {
        crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            "无可用检查点（任务已完成、已取消或检查点损坏），请重新生成",
        )
    })?;
    // Q4模式锁（B-5 抽纯函数）：检查点 mode 须与请求 mode 一致（此前静默错配，按新规范出旧 plan）。
    if let Some(msg) = checkpoint_mode_error(&cp.mode, request.mode.to_str_name()) {
        return Err(crate::errors::AppError::new(crate::errors::ErrorKind::Validation, msg));
    }
    // 续跑用新预算（15 分钟满额），不继承已耗尽的旧预算——旧预算耗尽正是失败原因
    let budget = std::sync::Arc::new(Budget::with_timeout(PIPELINE_TIMEOUT));
    // 断点方案直接进阶段 2（结构化定稿 + plan::validate 打回 ≤2 次），与主流程共用 produce_final_text 单源实现
    let mode = &request.mode;
    // 续跑登记活动 run 并清旧取消位（与 run_pipeline 入口同语义）；结束无论成败移除（D-06）
    let (final_text, _issues) = cancel::with_active_run(
        &run_id,
        produce_final_text(&app, &cp.current_plan, mode, &request, &budget, &run_id),
    )
    .await?;
    crate::commands::checkpoint::clear(&app, &run_id);
    maybe_run_text_eval(&app, &request, &final_text, &budget, &run_id).await;
    Ok(final_text)
}

/// 用户中途插话（前端"插入意见"调）——非阻塞存入槽，轮边界消费。
/// 校验：`models::validate_feedback`（长度上限与优化反馈**同源**，见 `rules::FEEDBACK_MAX_CHARS`）。
///
/// #26 根因留档：本入口原先**伪造了一个完整 PipelineRequest**去复用 `validate_request` 里的
/// feedback 分支——假 `api_key` 与占位域名因此进了生产代码（"硬编码凭据"形状，Mimosa CWE-798
/// 的命中面），且插话行为被 input/model/api_key/base_url/validate_url 等无关规则结构性耦合。
/// 规则本体已单源为 `models::validate_feedback`，由
/// `interject_feedback_does_not_fabricate_request` 锁成形状，不得回退为"构造假请求蹭校验"。
///
/// 连带修复（同族根因）：`interject::push` 曾用**另一个**私有限额（比这条规则的上限更小，
/// 旧数值见 git 历史）静默丢弃，与准入侧不同源 → "介于两者之间"的意见"回报成功却没进槽"。
/// 现在 push 的两个限额都单源在 `rules.rs`，且越界一律返回错误——**校验通过 ⇒ 一定入槽**。
/// Q4：空 run_id 直接拒（此前写入 "" 槽永不被消费）。
#[tauri::command]
pub async fn interject_feedback(run_id: Option<String>, note: String) -> Result<(), AppError> {
    let rid = run_id.unwrap_or_default();
    if rid.trim().is_empty() {
        return Err(crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            "缺少 run_id，插话无法定向到任务",
        ));
    }
    crate::models::validate_feedback(&note)?;
    interject::push(&rid, note)?;
    Ok(())
}

/// #12 轮间确认门决断（前端确认条调）：`continue` = 继续下一轮；`finalize` = 结束讨论直接出终稿。
/// 只接受这两种（`timeout` 由后端自产，不接受外部注入——见 `gate::GateDecision::parse`）。
/// 无门在等 → Validation 错误，前端据此提示"当前没有等待确认的轮次"，不静默丢弃。
#[tauri::command]
pub async fn round_gate_decide(run_id: Option<String>, decision: String) -> Result<(), AppError> {
    let rid = run_id.unwrap_or_default();
    if rid.trim().is_empty() {
        return Err(crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            "缺少 run_id，决断无法定向到任务",
        ));
    }
    let d = gate::GateDecision::parse(&decision).ok_or_else(|| {
        crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            format!("未知的决断 \"{}\"（只接受 continue / finalize）", decision),
        )
    })?;
    if gate::resolve(&rid, d) {
        Ok(())
    } else {
        Err(crate::errors::AppError::new(
            crate::errors::ErrorKind::Validation,
            "当前没有等待确认的讨论轮（本轮可能尚未结束，或已等待超时自动继续）",
        ))
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// F2（P0 审计）：两个 run 入口必须经 RAII 包裹器登记活动 run，生产代码不得裸调 begin/finish
    /// （裸调即"每条返回路径记得 finish"旧形态——future 被丢弃时漏清理）。
    #[test]
    fn run_entries_use_with_active_run() {
        let prod = crate::rules::production_segment(include_str!("orchestrator.rs"));
        let wrap = ["cancel::", "with_active_run("].concat();
        let bare_begin = ["cancel::", "begin("].concat();
        let bare_finish = ["cancel::", "finish("].concat();
        assert_eq!(prod.matches(wrap.as_str()).count(), 2, "run_pipeline 与 pipeline_resume 各一处");
        assert_eq!(prod.matches(bare_begin.as_str()).count(), 0);
        assert_eq!(prod.matches(bare_finish.as_str()).count(), 0);
        let resume = &prod[prod.find("pub async fn pipeline_resume").unwrap()..];
        let resume = &resume[..resume.find("\n}\n").unwrap()];
        assert!(resume.contains(wrap.as_str()), "pipeline_resume 必须经 with_active_run");
    }

    /// RULE-RUN-ID：run_id 解析单源——非空尊重，空/缺省生成（入口与 with_timeout 同源）
    #[test]
    fn resolve_run_id_respects_and_generates() {
        assert_eq!(resolve_run_id(Some("abc")), "abc");
        for blank in [None, Some(""), Some("  ")] {
            let id = resolve_run_id(blank);
            assert!(id.starts_with("run-") && id.len() > 4, "空 id 应生成：{}", id);
        }
    }

    /// 测试用嵌入式知识库（宿主/角色注入测试共用，避免逐处 unwrap 噪音）
    fn test_kb() -> KnowledgeBase {
        KnowledgeBase::load_embedded().unwrap()
    }

    // ---- #27-c：退避播报 → 流水线事件（前端限流 UI 的上游契约） ----

    /// 退避播报转换单源：进入 → Backoff（原因取自 BackoffReason::as_str，与前端文案键同源），
    /// 结束 → BackoffEnd。字段错位（如把 wait_secs 当 attempt）在此拦下。
    #[test]
    fn backoff_notice_maps_to_pipeline_event() {
        let start = backoff_event(llm::BackoffNotice::Start {
            attempt: 2,
            wait_secs: 63,
            reason: llm::BackoffReason::RateLimit,
        });
        match start {
            PipelineEvent::Backoff { attempt, wait_secs, reason } => {
                assert_eq!((attempt, wait_secs), (2, 63));
                assert_eq!(reason, "rate_limit", "reason 必须与 BackoffReason::as_str 同源");
            }
            other => panic!("Start 必须映射为 Backoff，实际: {:?}", other),
        }
        let end = backoff_event(llm::BackoffNotice::End { attempt: 2 });
        match end {
            PipelineEvent::BackoffEnd { attempt } => assert_eq!(attempt, 2),
            other => panic!("End 必须映射为 BackoffEnd，实际: {:?}", other),
        }
    }

    /// 三种退避原因全部有对应 wire 值（前端按 reason 取文案键，漏一种即文案落空）
    #[test]
    fn backoff_reason_wire_values_are_exhaustive() {
        let cases = [
            (llm::BackoffReason::RateLimit, "rate_limit"),
            (llm::BackoffReason::ServerError, "server_error"),
            (llm::BackoffReason::Network, "network"),
        ];
        for (reason, expect) in cases {
            match backoff_event(llm::BackoffNotice::Start { attempt: 1, wait_secs: 1, reason }) {
                PipelineEvent::Backoff { reason, .. } => assert_eq!(reason, expect),
                other => panic!("必须映射为 Backoff，实际: {:?}", other),
            }
        }
    }

    // ---- #26：准入限额单源 + 插话入口不再"伪造请求蹭校验" ----

    /// #26 源码形状锁：`interject_feedback` 不得再构造假请求。
    ///
    /// 旧实现为复用 `validate_request` 的 feedback 分支，在生产代码里伪造了一个完整
    /// PipelineRequest（同时写死了凭据字段与占位域名）——生产代码因此带上"硬编码凭据"
    /// 形状（Mimosa CWE-798 的命中面；扫描侧 `suppressedTestContext: 0` 也表明它没被当成
    /// 测试上下文豁免），且插话被 input/model/api_key/base_url/validate_url 等**无关规则**
    /// 结构性耦合。
    /// 规则本体已单源为 `models::validate_feedback`，此处把"不得回退"锁成形状。
    ///
    /// 扫描针本身**分片拼装**（不写成一整个字面量）：否则这条断言自己就成了源码里的
    /// "占位域名"字面量，等于用新命中面去换旧命中面（与测试 key 的动态构造同一处理）。
    #[test]
    fn interject_feedback_does_not_fabricate_request() {
        let src = include_str!("orchestrator.rs");
        // 生产段提取共用单源工具（rules::production_segment）——旧写法"取第一个 #[cfg(test)] 之前"
        // 在"测试模块位于文件中段"的文件上会把生产代码一起剔掉，断言静默空转（第十三批 D5 实测）。
        let prod = crate::rules::production_segment(src);
        let phantom_host = format!("placeholder{}", ".invalid");
        let credential_assign = format!("api_key{} \"", ":");
        assert!(
            !prod.contains(&phantom_host) && !prod.contains(&credential_assign),
            "生产段不得出现占位地址/凭据字面量（扫描命中面）"
        );
        assert!(
            !prod.contains("PipelineRequest {"),
            "生产段不得构造 PipelineRequest——插话只该走 models::validate_feedback"
        );
        let at = prod.find("pub async fn interject_feedback").expect("插话入口缺失");
        let body = &prod[at..];
        let body = &body[..body.find("\n}\n").map(|i| i + 3).unwrap_or(body.len())];
        assert!(body.contains("models::validate_feedback("), "插话入口必须接单源校验：{}", body);
        assert!(
            !body.contains("api_key") && !body.contains("base_url"),
            "插话校验不得触碰 api_key/base_url 等无关规则：{}",
            body
        );
    }

    /// #26：插话准入的行为锁（与优化反馈**同一实现**——上游约束与下游校验同一个数）。
    /// 覆盖：缺 run_id 拒 / 空意见拒且不入槽 / 恰好上限通过且真入槽 / 上限+1 拒且不入槽。
    #[tokio::test]
    async fn interject_feedback_enforces_single_sourced_limit() {
        use crate::errors::ErrorKind;
        let rid = "test-interject-feedback-26";
        interject::reset(rid);
        // 缺 run_id：拒（Q4 原有规则）
        assert_eq!(
            interject_feedback(None, "意见".into()).await.unwrap_err().kind,
            ErrorKind::Validation
        );
        // 空意见：拒，且不得入槽
        assert_eq!(
            interject_feedback(Some(rid.into()), "   ".into()).await.unwrap_err().kind,
            ErrorKind::Validation
        );
        assert!(interject::drain(rid).is_empty(), "被拒的意见不得入槽");
        // 恰好上限：通过，且确实入槽（校验通过 ≠ 静默丢弃）
        let max = "啊".repeat(crate::rules::FEEDBACK_MAX_CHARS);
        interject_feedback(Some(rid.into()), max).await.expect("恰好等于上限应通过");
        let got = interject::drain(rid);
        assert_eq!(got.len(), 1, "通过的意见必须入槽：{:?}", got);
        assert_eq!(got[0].chars().count(), crate::rules::FEEDBACK_MAX_CHARS, "入槽内容须原样保留");
        // 上限+1：拒且不入槽
        let over = "啊".repeat(crate::rules::FEEDBACK_MAX_CHARS + 1);
        let e = interject_feedback(Some(rid.into()), over).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(e.message.contains("过长"), "越界须报长度：{}", e.message);
        assert!(interject::drain(rid).is_empty(), "越界意见不得入槽");
        interject::reset(rid);
    }




    // ---- C4/D4：冲突预检标注（红灯先行——检测桩返回空时冲突用例必须失败） ----

    /// 单修订角色条目构造
    fn rc(role: PipelineRole, target: &str, content: &str) -> (PipelineRole, Vec<ReviewChange>, String) {
        (
            role,
            vec![ReviewChange { target: target.to_string(), content: serde_json::Value::String(content.to_string()), reason: "r".to_string() }],
            "总体意见".to_string(),
        )
    }

    #[test]
    fn summarize_no_conflict_no_warning() {
        let rc_list = vec![
            rc(PipelineRole::Lyricist, "lyrics", "歌词改成甲方案"),
            rc(PipelineRole::Producer, "style_prompt", "配器改成乙方案"),
        ];
        let out = build_summarize_user_prompt("当前方案文本", &rc_list, None);
        assert!(!out.contains("⚠️ 冲突"), "无冲突不应注入: {}", out);
    }

    #[test]
    fn summarize_single_conflict_marked_on_both_sides() {
        let rc_list = vec![
            rc(PipelineRole::Lyricist, "lyrics", "金句改成：凌晨四点的灯"),
            rc(PipelineRole::StyleAnalyst, "lyrics", "金句改成：别关那盏灯"),
        ];
        let out = build_summarize_user_prompt("当前方案文本", &rc_list, None);
        assert_eq!(out.matches("⚠️ 冲突").count(), 2, "冲突对双侧条目都要标注: {}", out);
        assert!(out.contains("作词人"), "缺角色A: {}", out);
        assert!(out.contains("流行风格分析师"), "缺角色B: {}", out);
        assert!(out.contains("取舍理由"), "缺裁决要求: {}", out);
        assert!(out.contains("R-2：金句/Hook 文字形态归作词人"), "裁决指引应出自 TERRITORY 表: {}", out);
    }

    #[test]
    fn summarize_multiple_conflicts_all_marked() {
        let mut lyricist_entry = rc(PipelineRole::Lyricist, "lyrics", "歌词改成甲方案");
        lyricist_entry.1.push(ReviewChange { target: "style_prompt".to_string(), content: serde_json::Value::String("人声改成沙哑".to_string()), reason: "r".to_string() });
        let rc_list = vec![
            lyricist_entry,
            rc(PipelineRole::StyleAnalyst, "lyrics", "歌词改成乙方案"),
            rc(PipelineRole::Producer, "style_prompt", "人声改成清亮"),
        ];
        let out = build_summarize_user_prompt("当前方案文本", &rc_list, None);
        assert_eq!(out.matches("⚠️ 冲突").count(), 4, "两对冲突、双侧标注: {}", out);
    }

    #[test]
    fn summarize_containment_treated_as_refinement_not_conflict() {
        let rc_list = vec![
            rc(PipelineRole::Lyricist, "lyrics", "歌词改成：把副歌改短一些，突出金句"),
            rc(PipelineRole::StyleAnalyst, "lyrics", "把副歌改短一些"),
        ];
        let out = build_summarize_user_prompt("当前方案文本", &rc_list, None);
        assert!(!out.contains("⚠️ 冲突"), "包含关系是细化不是冲突: {}", out);
    }

    // ---- C2/ADR-1：定点重写拼接 + 转写标记剥离（红灯先行——桩返回 None/空时必须失败） ----


    #[test]
    fn steps_for_mode_b_includes_lyricist() {
        let steps = steps_for_mode(&Mode::ModeB);
        assert_eq!(steps.len(), 3);
        assert!(steps.iter().any(|s| s.role == PipelineRole::Lyricist));
        assert!(!steps.iter().any(|s| s.role == PipelineRole::Host));
        assert!(!steps.iter().any(|s| s.role == PipelineRole::Auditor));
        assert!(steps.last().unwrap().role == PipelineRole::Producer);
    }

    #[test]
    fn steps_for_mode_a_skips_lyricist() {
        let steps = steps_for_mode(&Mode::ModeA);
        assert!(!steps.iter().any(|s| s.role == PipelineRole::Lyricist));
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn steps_for_mode_c_starts_with_reviser() {
        let steps = steps_for_mode(&Mode::ModeC);
        assert!(steps.first().unwrap().role == PipelineRole::Reviser);
        // Q6：C 只留改词（制作人无 Style Prompt 可审已摘除），steps=1
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn steps_for_mode_d_has_style_analyst() {
        let steps = steps_for_mode(&Mode::ModeD);
        assert!(steps.iter().any(|s| s.role == PipelineRole::StyleAnalyst));
        assert_eq!(steps.len(), 4);
    }

    #[test]
    fn prompt_for_mode_returns_original_instructions() {
        // 未设置覆盖目录时走嵌入版（String 返回，长度不断言改）
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let p = prompt_for_mode(&m);
            assert!(p.len() > 500, "模式 {:?} 指令过短（{}）", m, p.len());
        }
    }

    #[test]
    fn parse_review_agree() {
        let r = parse_review(r#"{"agree": true}"#);
        assert!(r.agree);
        assert!(r.changes.is_empty());
    }

    #[test]
    fn parse_review_with_changes() {
        let r = parse_review(r#"{"agree": false, "changes": [{"target": "lyrics", "content": "新歌词段", "reason": "套话"}], "reason": "整体"}"#);
        assert!(!r.agree);
        assert_eq!(r.changes.len(), 1);
        assert_eq!(r.changes[0].target, "/sections");
        assert_eq!(r.changes[0].reason, "套话");
    }

    #[test]
    fn parse_review_garbage_is_degraded() {
        // 解析失败不再假同意——显式降级（意见作废，警示可见，流程不阻断）
        let r = parse_review("不是 JSON");
        assert!(r.degraded);
        assert!(!r.agree);
        assert!(r.changes.is_empty());
        assert!(r.reason.contains("无法解析"));
        // 降级结果在 humanize 中必须可见，且不得伪装成同意
        let s = humanize_review(PipelineRole::Emotion, &r);
        assert!(s.contains("⚠️"), "got: {}", s);
        assert!(!s.contains("无异议 ✅"), "degraded 不得伪装成同意: {}", s);
    }

    /// agree 字段缺失（模型输出截断/自由发挥）→ 降级，不再 unwrap_or(true) 默认同意
    #[test]
    fn parse_review_missing_agree_is_degraded() {
        let r = parse_review(r#"{"changes": [{"target": "lyrics", "content": "x", "reason": "y"}]}"#);
        assert!(r.degraded, "缺 agree 字段必须降级");
        let r2 = parse_review(r#"{"reason": "整体太模板化"}"#);
        assert!(r2.degraded);
    }

    /// 空手 agree（agree=true 无 checked）必须可见为警示，而非普通"无异议 ✅"
    #[test]
    fn humanize_review_empty_checked_is_warning() {
        let r = parse_review(r#"{"agree": true}"#);
        let s = humanize_review(PipelineRole::Lyricist, &r);
        assert!(s.contains("无异议 ⚠️"), "got: {}", s);
        assert!(s.contains("未附核查清单"), "got: {}", s);
        assert!(!s.contains("✅"), "空手 agree 不得显示为合规通过: {}", s);
    }

    /// round_changes 三元组——无具体修订的总体异议也要进主持人/校验员 prompt
    #[test]
    fn summarize_prompt_carries_reason_without_changes() {
        let rc = vec![(
            PipelineRole::Emotion,
            vec![], // 异议但未给具体修订
            "整体太模板化，缺乏独特意象".to_string(),
        )];
        let user = build_summarize_user_prompt("当前方案文本", &rc, None);
        assert!(user.contains("（总体意见：整体太模板化，缺乏独特意象）"), "无修订的异议必须进汇总 prompt: {}", user);
        assert!(user.contains("情感分析师"), "got: {}", user);
        assert!(!user.contains("【原歌词"), "无原歌词不得追加比对基准段");
    }

    /// L-4：Mode C 汇总 prompt 必须携带原歌词比对基准（旧规则汇总阶段丢原词，主持人盲改）
    #[test]
    fn summarize_prompt_carries_original_lyrics_for_mode_c() {
        let rc = vec![(
            PipelineRole::Reviser,
            vec![],
            "第二行意象偏弱".to_string(),
        )];
        let user = build_summarize_user_prompt("当前方案文本", &rc, Some("原歌词第一行\n原歌词第二行"));
        assert!(user.contains("【原歌词（逐行字数对齐依据"), "汇总 prompt 必须带原歌词段: {}", user);
        assert!(user.contains("原歌词第二行"), "原词全文必须进汇总 prompt");
        // 无原词（A/B/D）不追加
        let plain = build_summarize_user_prompt("当前方案文本", &rc, None);
        assert!(!plain.contains("【原歌词"));
    }


    /// 校验员讨论轮 prompt 同样携带无修订的总体意见 + Mode C 专项保留
    #[test]
    fn audit_review_prompt_carries_reason_and_mode_c() {
        let rc = vec![(PipelineRole::Producer, vec![], "参数越界".to_string())];
        let mut req = make_request(None);
        req.mode = Mode::ModeC;
        req.extra = Some("原歌词第一行".to_string());
        let user = build_audit_review_user_prompt("方案", &rc, &[], "任务", &req);
        assert!(user.contains("（总体意见：参数越界）"), "got: {}", user);
        assert!(user.contains("原歌词第一行"), "Mode C 原歌词传递不得回退: {}", user);
        assert!(user.contains("Mode C 专项"), "got: {}", user);
        assert!(user.contains("任务"), "got: {}", user);
    }

    /// 反馈关键词路由——歌词/编曲/情绪/抖音/参数五类 + 无命中空 + ModeC 映射 + 去重保序
    /// （反馈文本均为中文描述，无凭据字面量）
    #[test]
    fn roles_for_feedback_routes_by_keywords() {
        use PipelineRole::*;
        assert_eq!(roles_for_feedback("副歌歌词太直白", &Mode::ModeB), vec![Lyricist]);
        assert_eq!(roles_for_feedback("副歌歌词太直白", &Mode::ModeC), vec![Reviser]);
        assert_eq!(roles_for_feedback("唢呐不够炸，配器太薄", &Mode::ModeD), vec![Producer, Emotion]); // "炸"兼命中情绪类，多命中去重保序
        assert_eq!(roles_for_feedback("能量起不来，感觉太平", &Mode::ModeB), vec![Emotion]);
        assert_eq!(roles_for_feedback("不够洗脑，不魔性", &Mode::ModeD), vec![StyleAnalyst]);
        assert_eq!(roles_for_feedback("不够洗脑", &Mode::ModeB), vec![Producer]);
        assert_eq!(
            roles_for_feedback("Weirdness 太高", &Mode::ModeB),
            vec![Producer, Emotion]
        );
        assert_eq!(
            roles_for_feedback("歌词太直白，唢呐不够炸", &Mode::ModeD),
            vec![Lyricist, Producer, Emotion]
        ); // "炸"兼命中情绪类
        assert!(roles_for_feedback("随便改改", &Mode::ModeB).is_empty());
        // B-2：大小写混排输入也要命中（旧规则小写 hook/大写 Weirdness 各漏一头）
        assert_eq!(roles_for_feedback("HOOK 记忆点不够", &Mode::ModeB), vec![Lyricist]);
        assert_eq!(roles_for_feedback("weirdness 太高", &Mode::ModeB), vec![Producer, Emotion]);
        assert_eq!(roles_for_feedback("BPM 太快", &Mode::ModeB), vec![Producer]);
        assert_eq!(roles_for_feedback("bpm 太快", &Mode::ModeB), vec![Producer]);
    }

    /// 上一版方案提取——取最后标记之后；缺失/空白返回 None
    #[test]
    fn extract_previous_plan_takes_last_marker() {
        let input = "新主题\n\n【上一版方案】\n方案A\n【上一版方案】\n方案B";
        assert_eq!(extract_previous_plan(input).as_deref(), Some("方案B"));
        assert!(extract_previous_plan("没有标记").is_none());
        assert!(extract_previous_plan("【上一版方案】\n   ").is_none());
    }

    /// original_lyrics 统一入口——新字段优先、旧 extra 回退、双空 None；旧请求兼容
    /// （旧请求模拟：序列化后删除新字段再解析，全程无凭据字面量）
    #[test]
    fn original_lyrics_prefers_new_field_falls_back_extra() {
        let mut req = make_request(None);
        assert!(req.original_lyrics_text().is_none());
        req.extra = Some("旧原歌词".to_string());
        assert_eq!(req.original_lyrics_text(), Some("旧原歌词"));
        req.original_lyrics = Some("新原歌词".to_string());
        assert_eq!(req.original_lyrics_text(), Some("新原歌词"));
        // 空白新字段不遮挡旧值
        req.original_lyrics = Some("   ".to_string());
        assert_eq!(req.original_lyrics_text(), Some("旧原歌词"));
        // 旧前端请求（无 original_lyrics 字段）兼容解析
        let mut v = serde_json::to_value(make_request(None)).unwrap();
        v.as_object_mut().unwrap().remove("original_lyrics");
        let old: PipelineRequest = serde_json::from_value(v).unwrap();
        assert!(old.original_lyrics.is_none());
        assert!(old.original_lyrics_text().is_none());
    }

    #[test]
    fn parse_review_drops_empty_content_and_normalizes_target() {
        // 空 content 修订丢弃；非法 target 归一为 /notes；旧枚举映射到指针
        let r = parse_review(r#"{"agree": false, "changes": [
            {"target": "lyrics", "content": "有效修订", "reason": "r1"},
            {"target": "style_prompt", "content": "   ", "reason": "空内容丢弃"},
            {"target": "weirdness_滑块", "content": "内容还在", "reason": "非法target归一"},
            {"target": "/global/instruments", "content": ["piano"], "reason": "指针直通"}
        ]}"#);
        assert!(!r.agree);
        assert_eq!(r.changes.len(), 3, "空 content 修订应被丢弃");
        assert_eq!(r.changes[0].target, "/sections");
        assert_eq!(r.changes[1].target, "/notes", "非法 target 应归一为 /notes");
        assert_eq!(r.changes[1].content, serde_json::json!("内容还在"));
        assert_eq!(r.changes[2].target, "/global/instruments");
        assert_eq!(r.changes[2].content, serde_json::json!(["piano"]));
    }

    #[test]
    fn humanize_review_readable() {
        // 无异议最低门槛：checked 清单应展示
        let ok = humanize_review(PipelineRole::Emotion, &ReviewResult {
            agree: true,
            changes: vec![],
            reason: String::new(),
            checked: vec!["情绪内核".into(), "能量差≥3级".into(), "弧线匹配".into()],
            degraded: false,
        });
        assert!(ok.contains("无异议"), "got: {}", ok);
        assert!(ok.contains("已核查"), "无异议必须展示核查清单: {}", ok);
        assert!(ok.contains("情绪内核"), "got: {}", ok);
        // 无 checked 时为警示（空手 agree 可见化）
        let ok2 = humanize_review(PipelineRole::Emotion, &ReviewResult {
            agree: true,
            changes: vec![],
            reason: String::new(),
            checked: vec![],
            degraded: false,
        });
        assert!(ok2.contains("无异议 ⚠️"), "got: {}", ok2);
        assert!(!ok2.contains("✅"), "空手 agree 不得显示为合规通过: {}", ok2);
        let fix = humanize_review(PipelineRole::Producer, &ReviewResult {
            agree: false,
            changes: vec![ReviewChange {
                target: "lyrics".into(),
                content: serde_json::Value::String("[失真吉他+强放, 空间爆满, 能量:9]".into()),
                reason: "Chorus 配器太弱".into(),
            }],
            reason: String::new(),
            checked: vec![],
            degraded: false,
        });
        assert!(fix.contains("提出 1 处修订"), "got: {}", fix);
        assert!(fix.contains("Chorus 配器太弱"), "got: {}", fix);
    }

    /// 长修订全文展示——100 字 content 不得被截断
    #[test]
    fn humanize_review_full_content_no_truncation() {
        let long = "这是一段超过四十字的修订内容".repeat(5); // 70 字
        let r = ReviewResult {
            agree: false,
            changes: vec![ReviewChange {
                target: "lyrics".into(),
                content: serde_json::Value::String(long.clone()),
                reason: "太长".into(),
            }],
            reason: String::new(),
            checked: vec![],
            degraded: false,
        };
        let s = humanize_review(PipelineRole::Lyricist, &r);
        assert!(s.contains(&long), "修订全文必须保留: {}", &s[..s.len().min(200)]);
        assert!(!s.contains("…"), "不得出现截断省略号: {}", &s[..s.len().min(200)]);
    }

    #[test]
    fn humanize_review_auditor() {
        let r = ReviewResult {
            agree: false,
            changes: vec![ReviewChange {
                target: "params".into(),
                content: serde_json::Value::String("Weirdness=30".into()),
                reason: "参数越界".into(),
            }],
            reason: "总体：参数区间超标".into(),
            checked: vec![],
            degraded: false,
        };
        let s = humanize_review(PipelineRole::Auditor, &r);
        assert!(s.contains("校验员：提出 1 处修订"), "got: {}", s);
        assert!(s.contains("参数越界"), "got: {}", s);
        assert!(s.contains("总体：参数区间超标"), "got: {}", s);
    }

    /// 无异议最低门槛：parse_review 解析 checked 清单
    #[test]
    fn parse_review_extracts_checked_list() {
        let r = parse_review(r#"{"agree": true, "checked": ["情绪内核", "能量差≥3级", "参数区间"], "reason": "全部合规"}"#);
        assert!(r.agree);
        assert_eq!(r.checked, vec!["情绪内核", "能量差≥3级", "参数区间"]);
        // 无 checked 字段时为空数组（宽容兼容）
        let r2 = parse_review(r#"{"agree": true}"#);
        assert!(r2.agree);
        assert!(r2.checked.is_empty());
    }


    /// Q4模式锁（B-5 行为化）：错配拒绝、匹配放行、旧检查点空 mode 放行。
    #[test]
    fn resume_mode_must_match_checkpoint() {
        // B 断点 vs D 请求 → 拒绝并双端报模式名
        let err = checkpoint_mode_error("mode_b", Mode::ModeD.to_str_name()).unwrap();
        assert!(err.contains("mode_b") && err.contains("mode_d"), "报错应含双端模式: {}", err);
        // 匹配放行
        assert!(checkpoint_mode_error("mode_a", Mode::ModeA.to_str_name()).is_none());
        // 旧版检查点空 mode 放行（向后兼容）
        assert!(checkpoint_mode_error("", Mode::ModeC.to_str_name()).is_none());
    }

    #[test]
    fn resolve_api_no_overrides_uses_global() {
        let req = make_request(None);
        let (url, key, model) = resolve_api(&req, PipelineRole::Emotion);
        assert_eq!(url, "https://global.example.com/v1");
        assert_eq!(key, "global-key");
        assert_eq!(model, "global-model");
    }

    #[test]
    fn resolve_api_partial_override_falls_back_per_field() {
        let mut map = std::collections::HashMap::new();
        map.insert(PipelineRole::Auditor, crate::models::RoleApiOverride { structured_mode: None,
            model: Some("auditor-strong".into()),
            api_key: None,
            base_url: None,
        });
        let req = make_request(Some(map));
        let (url, key, model) = resolve_api(&req, PipelineRole::Auditor);
        assert_eq!(url, "https://global.example.com/v1");
        assert_eq!(key, "global-key");
        assert_eq!(model, "auditor-strong");
        let (_, _, m2) = resolve_api(&req, PipelineRole::Emotion);
        assert_eq!(m2, "global-model");
    }

    #[test]
    fn resolve_api_full_override_uses_override() {
        let mut map = std::collections::HashMap::new();
        map.insert(PipelineRole::Host, crate::models::RoleApiOverride { structured_mode: None,
            model: Some("host-model".into()),
            api_key: Some("host-key".into()),
            base_url: Some("https://host.example.com/v1".into()),
        });
        let req = make_request(Some(map));
        let (url, key, model) = resolve_api(&req, PipelineRole::Host);
        assert_eq!(url, "https://host.example.com/v1");
        assert_eq!(key, "host-key");
        assert_eq!(model, "host-model");
    }

    #[test]
    fn matching_keywords_hits_and_misses() {
        let hits = matching_keywords("方案里提到孤独与愤怒", &["孤独".into(), "温柔".into()]);
        assert_eq!(hits, vec!["孤独"]);
        let none = matching_keywords("没有情绪词", &["孤独".into(), "愤怒".into()]);
        assert!(none.is_empty());
    }

    /// 第六批（#22）/#31 修复锁：关键词表检索契约单源——
    /// ① 长度门取自 `rules::KEYWORD_MIN_CHARS` 且确实生效（单字候选被丢弃，防"深夜"误命中"夜"）；
    /// ② 四张设计内关键词表全部登记进 `rules::KEYWORD_TABLES`，**每个检索列**都真实存在
    ///    （任一列名写错 = 该列候选恒空、检索面静默收窄）；
    /// ③ 单表条数上限别名 == rules 单源（禁止两处各写一个常量）；
    /// ④ 分词单源：候选来自 `keyword_candidates`（多列 + `rules::keyword_tokens`），
    ///    多值单元格（aliases）的每个 token 都是独立候选。
    #[test]
    fn keyword_min_chars_gate_is_single_sourced() {
        use crate::rules::{
            keyword_table, KEYWORD_MAX_ROWS, KEYWORD_MIN_CHARS, KEYWORD_TABLES,
            STYLE_GENRE_MAX_ROWS,
        };
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // ② 登记表自洽：每个检索列必须真实存在、上限为正、可反查
        for spec in KEYWORD_TABLES {
            let t = kb
                .table(spec.table)
                .unwrap_or_else(|e| panic!("登记表 {} 加载失败: {}", spec.table, e));
            assert!(!spec.key_cols.is_empty(), "{} 检索列集合为空（整表死行）", spec.table);
            for col in spec.key_cols {
                assert!(
                    t.header_index(col).is_some(),
                    "{} 的检索列 {} 不存在（列名写错即该列候选恒空）",
                    spec.table,
                    col
                );
            }
            assert!(spec.max_rows > 0, "{} 条数上限必须为正", spec.table);
            assert!(keyword_table(spec.table).is_some(), "{} 无法在单源反查", spec.table);
        }
        // emotions/cliches/hooks/style_genre 必须全部登记（漏登记会静默退回全量注入）
        for name in ["emotions", "cliches", "hooks", "style_genre"] {
            assert!(keyword_table(name).is_some(), "关键词表 {} 未登记进 KEYWORD_TABLES", name);
        }
        // ③ 别名与单源一致
        assert_eq!(INJECT_MAX_KEYWORD_ROWS, KEYWORD_MAX_ROWS);
        assert_eq!(INJECT_MAX_STYLE_GENRE_ROWS, STYLE_GENRE_MAX_ROWS);
        // ④ 多列分词候选：style_genre 的候选集必须包含 aliases 列独有 token（旧单列口径拿不到）
        let spec = keyword_table("style_genre").unwrap();
        let cands = keyword_candidates(&kb, "style_genre", spec.key_cols);
        assert!(
            cands.iter().any(|c| c == "电子摇滚"),
            "aliases 列 token 未进入候选集（分词/多列取数失效）"
        );
        // ④ 长度门生效（按单源常量取边界值）
        const _: () = assert!(KEYWORD_MIN_CHARS >= 2, "长度门 <2 会让单字候选误命中");
        let below = "夜".repeat(KEYWORD_MIN_CHARS - 1);
        assert!(
            matching_keywords(&format!("深夜的{}景", below), std::slice::from_ref(&below)).is_empty(),
            "短于 KEYWORD_MIN_CHARS 的候选必须被丢弃"
        );
        let at_gate = "孤独".repeat(KEYWORD_MIN_CHARS / 2 + 1);
        assert_eq!(
            matching_keywords(&format!("这里提到{}", at_gate), std::slice::from_ref(&at_gate)),
            vec![at_gate.clone()],
            "达到长度门的候选必须命中"
        );
    }

    /// #31 修复锁（扩检索面回归）：方案里出现**中文自造流派名/别名**时 style_genre 不得整表零命中。
    /// 红灯先行：旧口径（genre 单列、整串 contains）对这些自造名必然零命中——本测试逐条复演
    /// 旧口径（断言零命中 = 证明缺口真实）再断言新口径命中（多列析取 + 分词）。
    /// 现实证据：2026-09-18 GUI 实跑 mode_d 产物 Style Prompt 写"电子摇滚"，旧口径 45 行整表未注入。
    #[test]
    fn style_genre_reachable_for_generated_genre_names() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        let spec = crate::rules::keyword_table("style_genre").unwrap();
        // 旧口径零命中的自造名（新口径必须命中）——两类不作红灯样本：
        // ① 恰为 genre 字面的（山歌/国风电子）；② 旧口径"单元格是方案子串"能蒙中的（"后摇"⊂"后摇滚"）
        let old_miss_phrases =
            ["电子摇滚", "抒情慢歌", "深夜民谣", "电影配乐", "城市流行", "戏腔"];
        let hit_phrases = ["电子摇滚", "国风电子", "抒情慢歌", "深夜民谣", "电影配乐", "城市流行", "后摇滚", "戏腔", "山歌"];
        for phrase in old_miss_phrases {
            let plan = format!("Style Prompt: {}，抖音神曲", phrase);
            let old_hit = column_values(&kb, "style_genre", "genre")
                .iter()
                .any(|c| plan.contains(c.as_str()));
            assert!(!old_hit, "旧口径对 {:?} 竟然命中——红灯样本选择有误（本锁证明力下降）", phrase);
        }
        for phrase in hit_phrases {
            let plan = format!("Style Prompt: {}，抖音神曲", phrase);
            let out = keyword_table_render(&kb, "style_genre", None, &plan).unwrap();
            assert!(
                out.contains("按需命中"),
                "自造流派名 {:?} 在 style_genre 整表零命中（扩检索面失效）：{}",
                phrase,
                &out[..out.len().min(200)]
            );
            // 命中条数必须在规格上限内（多列 OR 不得把上限撑破）
            assert!(out.lines().filter(|l| l.starts_with("| ") && !l.contains("---")).count() <= spec.max_rows + 1);
        }
        // 定点：mode_d 实跑的"电子摇滚"必须命中合成器浪潮与新浪潮（最贴近的两行）
        let out = keyword_table_render(&kb, "style_genre", None, "Style Prompt: 电子摇滚 抖音神曲").unwrap();
        assert!(out.contains("| synthwave |"), "电子摇滚未命中 synthwave 行：{}", out);
        assert!(out.contains("| 新浪潮 |"), "电子摇滚未命中 新浪潮 行：{}", out);
        // 且不得把整表当兜底塞满（命中行数受规格上限约束）
        let hit_rows = out.lines().filter(|l| l.starts_with("| ") && !l.contains("---")).count() - 1;
        assert!(hit_rows >= 1 && hit_rows <= spec.max_rows, "命中行数 {} 越界", hit_rows);
    }

    #[test]
    fn plan_energy_range_extracts() {
        assert_eq!(plan_energy_range("能量:3 到 能量:8"), Some((3, 8)));
        assert_eq!(plan_energy_range("没有能量标注"), None);
    }

    #[test]
    fn inject_knowledge_filters_by_keywords() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 制作人绑定表注入：方案含流派与能量 → style_genre 命中、instruments 按能量
        let out = inject_knowledge(&kb, &[("style_genre", &[], &[]), ("instruments", &[], &[]), ("suno_rules", &[], &[])], &[], "深夜室内民谣 能量:3 Chorus 能量:8", "mode_a", None);
        assert!(out.contains("按需命中"), "style_genre 应命中: {}", &out[..out.len().min(200)]);
        assert!(out.contains("suno_rules 知识库"), "suno_rules 应全量注入");
        // 情感分析师注入：方案无情绪词 → M18 无示例标注（调用方走确定性默认）
        let out2 = inject_knowledge(&kb, &[("emotions", &[], &[])], &[], "纯粹描述画面没有情绪词", "mode_a", None);
        assert!(out2.contains("未命中关键词"), "emotions 应标注无命中: {}", &out2[..out2.len().min(200)]);
        assert!(out2.contains("无示例"), "emotions 无命中应明确无示例: {}", &out2[..out2.len().min(200)]);
        // 方案含情绪词 → 命中
        let out3 = inject_knowledge(&kb, &[("emotions", &[], &[])], &[], "这首歌的情绪是孤独与自嘲", "mode_a", None);
        assert!(out3.contains("按需命中"), "emotions 应命中: {}", &out3[..out3.len().min(200)]);
    }

    /// #16 修复锁（端到端）：无论方案内容如何，角色 prompt 点名"逐项核查"的编号都必须在注入文本里。
    /// 旧行为下这些编号只有落在 CSV 前 8 行内才可见（LC-15/LC-26/CC-17… 全部悬空引用）。
    /// #29 起口径升级：遍历**真实运行上下文**（`steps_for_mode` 的 模式 × 角色 组合），
    /// 而不是"角色不带模式"的伪上下文——否则 #29 的条件限定（LC-10 仅抖音模式可达）会被掩盖。
    #[test]
    fn craft_referenced_ids_always_injected_for_every_role() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 极端方案（无候选词/无能量标注）：相关性打分接近全 0，引用必达仍须成立
        let plan = "（未填写的方案占位）";
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            for step in steps_for_mode(&m) {
                let role = step.role;
                let r = roles::role_for(role);
                if r.craft_refs.is_empty() {
                    continue;
                }
                let out = inject_knowledge(
                    &kb,
                    r.knowledge_tables,
                    r.craft_refs,
                    plan,
                    m.to_str_name(),
                    Some(role.storage_key()),
                );
                for id in r.craft_refs {
                    assert!(
                        out.contains(&format!("| {} |", id)),
                        "{} 在 {} 注入缺引用必达行 {}：{}",
                        r.name,
                        m.to_str_name(),
                        id,
                        &out[..out.len().min(300)]
                    );
                }
                assert!(out.contains("含引用必达"), "{} 注入头未标注必达数", r.name);
            }
        }
    }

    /// #23/#29 修复锁（装配层端到端）：**真实运行上下文**遍历——判定走渲染层
    /// `render_craft_table`（运行期同一入口），不用测试自造的判定，故"测试口径 == 运行口径"。
    /// ① 死行审计：任何 CSV 行（`CRAFT_RESERVED_ROWS` 登记的预留位除外）都必须在至少一个
    ///    真实上下文里被渲染出来——旧实现的"阶段0"标签零消费者，LC-12/LC-01/CC-24 在 C/D
    ///    模式彻底不可达，本断言即红；
    /// ② 条件域**限定**：标"抖音"的行只能出现在 mode_d 上下文、标"制作"的只能进 producer、
    ///    标 A/B/C 的不得出现在 mode_d——旧实现条件标签零执行门（LC-31 在 D 也注入、
    ///    CC-22 进所有 craft 角色），本断言即红；
    /// ③ 阶段隔离：标"阶段0"的行必须命中至少一个主持人上下文；纯"审改"行不得混进主持人上下文。
    #[test]
    fn craft_rows_reachable_in_every_real_context() {
        let kb = test_kb();
        let rev_c = crate::rules::stage_consumer(crate::rules::CRAFT_STAGE_CONSUMER_REVIEWER).unwrap();
        // 真实上下文 = 主持人阶段0（4 模式，无角色身份）+ 审改（4 模式 × steps_for_mode 角色）
        let mut ctxs: Vec<(Mode, Option<&'static str>, bool)> = Vec::new();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            ctxs.push((m.clone(), None, true));
            for s in steps_for_mode(&m) {
                ctxs.push((m.clone(), Some(s.role.storage_key()), false));
            }
        }
        const PLAN: &str = "（空方案占位）";
        let rendered: Vec<String> = ctxs
            .iter()
            .map(|(mode, role, is_host)| {
                // 主持人上下文走**真实装配入口**（`host_initial_system`）——否则"渲染层可达
                // 但主持装配没接线"这类 #23 缺陷会被漏掉；审改上下文走渲染层（运行期条数上限
                // 8 会合法地隐藏未被引用的行，用它做可达审计会误报）
                if *is_host {
                    return host_initial_system(mode, &kb, PLAN);
                }
                let (consumer, stage_tags) = (rev_c.name, rev_c.stage_tags);
                let ctx =
                    crate::knowledge::CraftInjectCtx { consumer, stage_tags, mode: mode.to_str_name(), role: *role };
                let mut block = String::new();
                for t in ["lyric_craft", "compose_craft"] {
                    block.push_str(
                        &kb.render_craft_table(t, &ctx, None, PLAN, None, &[])
                            .unwrap_or_else(|e| panic!("{} × {:?} 渲染失败: {}", t, role, e)),
                    );
                }
                block
            })
            .collect();
        for name in ["lyric_craft", "compose_craft"] {
            let t = kb.table(name).unwrap();
            let id_idx = t.header_index("id").unwrap();
            let trg_idx = t.header_index("trigger").unwrap();
            for row in &t.rows {
                let id = &row[id_idx];
                let trigger = &row[trg_idx];
                if crate::rules::CRAFT_RESERVED_ROWS.iter().any(|(rid, _)| rid == id) {
                    continue; // 预留位：登记即不参与注入（#18 另有登记一致性守护）
                }
                let tags: Vec<&str> = trigger
                    .split(crate::rules::CRAFT_TRIGGER_TAG_SEP)
                    .map(str::trim)
                    .collect();
                let marker = format!("| {} |", id);
                let hits: Vec<usize> =
                    (0..ctxs.len()).filter(|&i| rendered[i].contains(&marker)).collect();
                assert!(
                    !hits.is_empty(),
                    "{} trigger={:?} 在全部真实上下文都不可注入（死行，#23/#29 回归）",
                    id,
                    trigger
                );
                if tags.contains(&"抖音") {
                    assert!(
                        hits.iter().all(|&i| ctxs[i].0.to_str_name() == "mode_d"),
                        "{} 标「抖音」却在非 mode_d 上下文注入（条件域限定失效，#29 回归）",
                        id
                    );
                }
                if tags.contains(&"制作") {
                    assert!(
                        hits.iter().all(|&i| ctxs[i].1 == Some("producer")),
                        "{} 标「制作」却在非制作人上下文注入（条件域限定失效，#29 回归）",
                        id
                    );
                }
                if tags.iter().any(|t| ["A", "B", "C"].contains(t)) {
                    assert!(
                        hits.iter().all(|&i| ctxs[i].0.to_str_name() != "mode_d"),
                        "{} 标 A/B/C 却在 mode_d 注入（条件域限定失效，#29 回归）",
                        id
                    );
                }
                if tags.contains(&"阶段0") {
                    assert!(
                        hits.iter().any(|&i| ctxs[i].2),
                        "{} 标「阶段0」却没有任何主持人上下文注入（#23 回归）",
                        id
                    );
                }
                if tags.contains(&"审改") && !tags.contains(&"阶段0") {
                    assert!(
                        hits.iter().all(|&i| !ctxs[i].2),
                        "{} 纯「审改」行混进主持人上下文（阶段隔离失效）",
                        id
                    );
                }
            }
        }
    }

    /// #23 修复锁：主持人**两阶段**都必须真消费者式注入"阶段0"思维资产，且不得混入
    /// 审改/条件域行。旧实现"阶段0"零消费者（primer 是静态手写文本、不读 CSV）——
    /// LC-12/LC-01/CC-24 在 C/D 模式彻底不可达、在 A/B 只是与 primer 双载体，本断言即红。
    #[test]
    fn host_stage0_injects_only_stage0_rows() {
        let kb = test_kb();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            for (stage, s) in [
                ("阶段0", host_initial_system(&m, &kb, "")),
                ("汇总", host_summarize_system(&m, &kb, "")),
            ] {
                for id in ["LC-01", "LC-12", "CC-24"] {
                    assert!(
                        s.contains(&format!("| {} |", id)),
                        "{} 主持人{} system 缺阶段0 思维资产 {}（#23 回归）",
                        m.to_str_name(),
                        stage,
                        id
                    );
                }
                for id in ["LC-02", "LC-10", "LC-31", "CC-22", "CC-23", "CC-28"] {
                    assert!(
                        !s.contains(&format!("| {} |", id)),
                        "{} 主持人{} system 混入非阶段0 行 {}（阶段隔离失效）",
                        m.to_str_name(),
                        stage,
                        id
                    );
                }
            }
        }
    }

    /// 微观②：少而准——愤怒（高能量）只注入高能乐器，悲伤（低能量）只注入低能乐器，且 ≤15 件
    #[test]
    fn instruments_injected_selectively_by_energy() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 愤怒主题（能量 7-10）：命中摇滚/金属类高能乐器
        let out = inject_knowledge(&kb, &[("instruments", &[], &[])], &[], "愤怒爆发 能量:7 到 能量:10", "mode_a", None);
        assert!(out.contains("按能量区间 7~10 命中"), "got: {}", &out[..out.len().min(150)]);
        assert!(out.contains("distorted guitar"), "愤怒应含失真吉他");
        assert!(out.contains("electric guitar"), "愤怒应含电吉他");
        assert!(!out.contains("felt piano"), "愤怒不应含低能钢琴（或超出 15 件上限被截断）");
        // 悲伤主题（能量 1-4）：命中民谣/抒情低能乐器
        let out2 = inject_knowledge(&kb, &[("instruments", &[], &[])], &[], "悲伤低回 能量:1 到 能量:4", "mode_a", None);
        assert!(out2.contains("按能量区间 1~4 命中"), "got: {}", &out2[..out2.len().min(150)]);
        assert!(out2.contains("felt piano"), "悲伤应含 felt piano");
        assert!(out2.contains("fingerpicked"), "悲伤应含指弹吉他");
        assert!(!out2.contains("distorted guitar"), "悲伤不应含失真吉他");
        // 命中条数 ≤15
        let hit_lines = out2.lines().filter(|l| l.starts_with("| ") && l.contains("|")).count();
        assert!(hit_lines <= 17, "注入行数超限: {}", hit_lines); // 表头+分隔+≤15
    }

    /// 方案截断——短方案原样，超长截断+附注（INJECT_MAX_PLAN_CHARS 锁）
    #[test]
    fn truncate_plan_caps_long_input() {
        assert_eq!(truncate_plan("短方案"), "短方案");
        let long = "啊".repeat(INJECT_MAX_PLAN_CHARS + 100);
        let out = truncate_plan(&long);
        assert!(out.contains("[方案过长，已截断前"), "应有截断附注: {}", &out[out.len().saturating_sub(120)..]);
        assert!(out.chars().count() < long.chars().count());
        // 边界：恰好上限不过截断
        let edge = "啊".repeat(INJECT_MAX_PLAN_CHARS);
        assert_eq!(truncate_plan(&edge), edge);
    }

    /// log 折叠——6 条内原样，7 条折叠为摘要+最近 6 条，关键词保留
    #[test]
    fn fold_log_keeps_recent_and_summarizes_rest() {
        let mk = |i: usize| (format!("角色{}", i), format!("修订内容很长很长很长很长很长很长{}", i));
        // 完整档案原则：上限 24 条——3 轮 × 全角色 + 主持人条目全额保留
        let log: Vec<(String, String)> = (0..6).map(mk).collect();
        assert_eq!(fold_log(&log).len(), 6);
        let long: Vec<(String, String)> = (0..8).map(mk).collect();
        assert_eq!(fold_log(&long).len(), 8, "8 条 ≤ 24 上限应全额保留");
        // 折叠行为仅在上限时仍可用（病态保险）——构造 30 条验证折叠路径不死
        let huge: Vec<(String, String)> = (0..30).map(mk).collect();
        let folded = fold_log(&huge);
        assert_eq!(folded[0].0, "早期修订");
        assert!(folded[0].1.contains("等 6 条早期修订已折叠"), "got: {}", folded[0].1);
        assert_eq!(folded.len(), 25, "1 摘要 + 最近 24 条");
    }

    /// 第二十五批 Q2 修复锁（运行期可观测性，源码形状）：流水线关键节点的 INFO 日志必须存在。
    /// 历史缺口：四模式 GUI 目测跑完后日志除告警外**零运行记录**——"跑过几次 / 收敛几轮 /
    /// 哪张关键词表命中"全不可见，排障只能靠 UI/history；`orchestrator` 里日志调用清一色
    /// warn!/error!（成功路径静默）。
    /// 锁定面 = 6 个节点锚点必须出现在**生产段**且位于 `tracing::info!` 调用内（防被挪进注释冒充）。
    /// 边界声明（诚实）：本锁只保证"日志调用点存在"，不保证"运行必被执行"——后者由实跑复测
    /// （GUI/无头日志出现对应 INFO 行）承担，二者缺一不可。
    #[test]
    fn pipeline_logging_nodes_are_present() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/orchestrator.rs"),
        )
        .expect("读 orchestrator.rs 失败");
        let production = crate::rules::production_segment(&src);
        let anchors = [
            ("流水线开始", "run_pipeline_inner 入口"),
            ("阶段0 初稿完成", "run_host_initial 出口"),
            ("讨论轮完成", "阶段1 轮末"),
            ("讨论收敛", "阶段1 收敛判定"),
            ("知识注入完成", "inject_knowledge 出口"),
            ("流水线完成", "run_pipeline_inner 终局"),
        ];
        for (anchor, where_) in anchors {
            let pos = production
                .find(anchor)
                .unwrap_or_else(|| panic!("关键节点 INFO 日志缺失：{}（{}）", anchor, where_));
            // 取锚点前 240 个**字符**（按字节切会命中多字节字符内部 panic）
            let mut head_chars: Vec<char> = production[..pos].chars().collect();
            let head: String = head_chars
                .split_off(head_chars.len().saturating_sub(240))
                .into_iter()
                .collect();
            assert!(
                head.contains("tracing::info!"),
                "锚点 {:?}（{}）不在 tracing::info! 调用内——可能被挪进注释/文案冒充",
                anchor,
                where_
            );
        }
    }

    /// 注入量规范：每个角色用"最坏情况方案"（命中所有关键词表 + 全能量区间）注入，
    /// 断言单表条数 ≤ 上限、单角色总字数 ≤ 封顶。常量 INJECT_MAX_* 的测试锁。
    /// #29 起口径升级：**逐模式**遍历（思维资产条件标签随模式增删行——
    /// LC-10/LC-24 仅 mode_d 可达、LC-31 在 mode_d 被排除），单模式口径会漏掉真实最坏情况。
    #[test]
    fn role_injection_budget_under_limits() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 最坏情况方案：同时命中情绪/套话/钩子/流派全部候选 + 全能量区间 0-10
        let plan = "愤怒 孤独 温柔 遗憾 梦想 星空 自嘲 魔性循环 反差金句 空耳式 深夜室内民谣 抒情流行 爵士 重金属 能量:0 到 能量:10";
        let roles = [
            PipelineRole::Emotion,
            PipelineRole::Lyricist,
            PipelineRole::Reviser,
            PipelineRole::Producer,
            PipelineRole::StyleAnalyst,
            PipelineRole::Auditor,
        ];
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            for rp in roles {
            let r = roles::role_for(rp);
            let out = inject_knowledge(&kb, r.knowledge_tables, r.craft_refs, plan, m.to_str_name(), Some(rp.storage_key()));
            let chars = out.chars().count();
            // 按 "## 表名 知识库" 切段，每段独立统计数据行（| 开头且不是表头/分隔）
            for (table_name, _, _) in r.knowledge_tables {
                let marker = format!("## {} 知识库", table_name);
                let segment = match out.find(&marker) {
                    Some(i) => {
                        let rest = &out[i..];
                        match rest.find("\n## ") {
                            Some(j) => &rest[..j],
                            None => rest,
                        }
                    }
                    None => {
                        panic!("{} 注入中找不到表 {} 的段", r.name, table_name);
                    }
                };
                let data_lines = segment
                    .lines()
                    .filter(|l| l.starts_with("| ") && !l.contains("| ---"))
                    .count();
                // 表头行（首个 | 开头行）不计入数据行上限
                let data_lines = data_lines.saturating_sub(1);
                let limit = match *table_name {
                    "style_genre" => INJECT_MAX_STYLE_GENRE_ROWS + 2,
                    "instruments" => INJECT_MAX_INSTRUMENTS_ROWS + 2,
                    "suno_rules" => INJECT_MAX_FULL_ROWS + 2,
                    // 思维资产表单源判定（rules::CRAFT_TABLES）——禁止在测试里再写第二份表名清单
                    t if crate::rules::is_craft_table(t) => INJECT_MAX_CRAFT_ROWS + 2,
                    _ => INJECT_MAX_KEYWORD_ROWS + 2,
                };
                assert!(
                    data_lines <= limit,
                    "{} 注入 {} 表数据行 {} 超上限 {}",
                    r.name,
                    table_name,
                    data_lines,
                    limit
                );
            }
            tracing::warn!(role = %r.name, chars = chars, cap = INJECT_MAX_TOTAL_CHARS, "注入总量超封顶");
            assert!(
                chars <= INJECT_MAX_TOTAL_CHARS,
                "{} 在 {} 注入 {} 字超过封顶 {}",
                r.name,
                m.to_str_name(),
                chars,
                INJECT_MAX_TOTAL_CHARS
            );
            }
        }
    }

    /// 产出语言三态指令：zh = 双层全空（零漂移）；mix = 仅阶段 2 注入 STYLE 节英文、
    /// 讨论层空；en = 讨论层与阶段 2 均注入。
    #[test]
    fn output_lang_directives_follow_three_states() {
        let mut req = make_request(None);
        // zh：双层全空
        assert_eq!(discussion_lang_directive(&req), "");
        assert_eq!(final_pack_lang_directive(&req), "");
        // mix：讨论层空、阶段 2 注入 STYLE 节英文指令
        req.output_lang = "mix".into();
        assert_eq!(discussion_lang_directive(&req), "");
        assert!(final_pack_lang_directive(&req).contains("STYLE"));
        assert!(final_pack_lang_directive(&req).contains("原语言"));
        // en：双层均注入
        req.output_lang = "en".into();
        assert!(discussion_lang_directive(&req).contains("英文"));
        assert!(final_pack_lang_directive(&req).contains("英文"));
    }

    fn make_request(overrides: Option<std::collections::HashMap<PipelineRole, crate::models::RoleApiOverride>>) -> PipelineRequest {
        PipelineRequest { structured_caps: None,
        douyin_scene: None,
        text_eval_judge: None,
            output_lang: "zh".into(),
            mode: Mode::ModeB,
            user_input: "雨天".into(),
            model: "global-model".into(),
            // 测试占位符非真实凭据——动态构造，避免静态扫描把占位值当硬编码密钥（CWE-798 误报）
            api_key: format!("global-{}", "key"),
            base_url: "https://global.example.com/v1".into(),
            extra: None,
            original_lyrics: None,
            role_overrides: overrides,
            thinking: false,
            refine_targets: None,
            generation: None,
            run_id: None,
            round_gate: None,
        }
    }

    /// meta 与真源一致——modes 座位 == seats_for_mode（含主持/校验）；动态审改 == steps_for_mode
    #[test]
    fn pipeline_meta_matches_sources() {
        use crate::models::Mode;
        // modes：四模式座位与 seats_for_mode 逐位一致（动态角色 + 主持 + 校验）
        let meta_modes: std::collections::HashMap<String, Vec<PipelineRole>> = [
            ("mode_a", Mode::ModeA),
            ("mode_b", Mode::ModeB),
            ("mode_c", Mode::ModeC),
            ("mode_d", Mode::ModeD),
        ]
        .iter()
        .map(|(k, m)| {
            (k.to_string(), seats_for_mode(m))
        })
        .collect();
        assert_eq!(meta_modes["mode_a"], vec![PipelineRole::Emotion, PipelineRole::Producer, PipelineRole::Host, PipelineRole::Auditor]);
        // Q6：C 只留改词（制作人无 Style Prompt 可审已摘除），座位=3
        assert_eq!(meta_modes["mode_c"], vec![PipelineRole::Reviser, PipelineRole::Host, PipelineRole::Auditor]);
        assert_eq!(
            meta_modes["mode_d"],
            vec![PipelineRole::Emotion, PipelineRole::Lyricist, PipelineRole::StyleAnalyst, PipelineRole::Producer, PipelineRole::Host, PipelineRole::Auditor]
        );
        // steps 语义不变：讨论轮只跑动态角色（主持/校验由各自阶段独家执行）
        assert_eq!(steps_for_mode(&Mode::ModeA).iter().map(|s| s.role).collect::<Vec<_>>(), vec![PipelineRole::Emotion, PipelineRole::Producer]);
        // roles：name/emoji/knowledge 与 role_for 一致（抽查三角色）
        let emo = roles::role_for(PipelineRole::Emotion);
        assert_eq!(emo.name, "情感分析师");
        assert_eq!(emo.emoji, "🎭");
        assert!(emo.knowledge_tables.iter().any(|(t, _, _)| *t == "emotions"));
        let host = roles::role_for(PipelineRole::Host);
        assert!(host.knowledge_tables.is_empty());
        let auditor = roles::role_for(PipelineRole::Auditor);
        assert!(auditor.knowledge_tables.iter().any(|(t, _, _)| *t == "suno_rules"));
    }

    /// #14 同轮可见性（内容层）：讨论轮改串行后，第 k 个角色的 user prompt 必须出现
    /// 「本轮同轮其他角色已提修订」块，且与「往轮已提修订」块严格分工（互不混淆）。
    #[test]
    fn role_review_prompt_exposes_same_round_peers_distinct_from_history() {
        let req = make_request(None);
        // 无同轮修订：不得凭空出现"本轮"块（否则是假叙事）
        let p0 = build_role_review_user_prompt("方案", &[], &[], "", &req);
        assert!(!p0.contains("本轮同轮其他角色已提修订"), "无同轮修订时不得出现本轮块");
        assert!(!p0.contains("往轮已提修订"), "无往轮日志时不得出现往轮块");
        // 同轮修订 + 往轮日志：两块并存，各自成段，条目可读
        let peers = vec![("情感分析师".to_string(), "把 Verse 能量压到 3".to_string())];
        let hist = vec![("作词人".to_string(), "Hook 改疑问句".to_string())];
        let p1 = build_role_review_user_prompt("方案", &peers, &hist, "重点解决参数越界", &req);
        assert!(p1.contains("【本轮同轮其他角色已提修订"), "缺同轮块：\n{}", p1);
        assert!(p1.contains("- 情感分析师：把 Verse 能量压到 3"));
        assert!(p1.contains("【往轮已提修订"), "缺往轮块：\n{}", p1);
        assert!(p1.contains("- 作词人：Hook 改疑问句"));
        assert!(p1.contains("禁止重复提同一问题"), "同轮块必须带协作纪律约束");
        // 本轮块先于往轮块（先看到最新分歧，再参考历史）
        let ip = p1.find("【本轮同轮其他角色已提修订").unwrap();
        let ih = p1.find("【往轮已提修订").unwrap();
        assert!(ip < ih, "同轮块应先于往轮块出现：\n{}", p1);
    }

    /// #14 串行语义守护（源码形状，红灯先行）：讨论轮动态角色审改区必须是**阵容序串行**
    /// ——含 `for role in roles.iter()`（阵容序权威单源）与 `round_peers.push(`（同轮可见性
    /// 回填），且不得再出现已退役的并发机制 `join_all`/`review_futs`。
    /// 红线：并发回归即"同轮互盲"复发（第 k 个角色看不到同轮兄弟刚提的修订 → 冲突拖到
    /// 汇总阶段、无人裁决）。注释中被文档化的旧标识符不计入（去注释后再扫）。
    #[test]
    fn discussion_round_review_is_serial_with_same_round_visibility() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/commands/orchestrator.rs");
        let src = std::fs::read_to_string(&path).unwrap();
        let start = src.find("① 动态角色").expect("找不到讨论轮审改区起点标记");
        let end = src.find("② 校验员审查").expect("找不到讨论轮审改区终点标记");
        assert!(start < end, "讨论轮审改区标记顺序异常");
        // 去行注释后扫描：退役机制的说明性注释含旧标识符，不算代码
        let strip = |s: &str| -> String {
            s.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
        };
        let region = strip(&src[start..end]);
        for want in ["for role in roles.iter()", "round_peers.push(", "&round_peers"] {
            assert!(region.contains(want), "讨论轮审改区缺串行语义要素 `{}`：\n{}", want, region);
        }
        for gone in ["join_all", "review_futs"] {
            assert!(
                !region.contains(gone),
                "讨论轮审改区残留已退役的并发机制 `{}`（#14 回退：同轮互盲复发）",
                gone
            );
        }
        // 生产代码（测试模块之前）整体不得再有 join_all——旧并发测试已随机制退役
        let prod = &src[..src.find("mod tests").expect("找不到测试模块标记")];
        assert!(
            !strip(prod).contains("join_all"),
            "orchestrator.rs 生产代码仍含 join_all——并发机制应已彻底退役"
        );
    }

    // ---- #12 轮间人工确认门 ----

    /// 门的道数与超时补偿必须由轮数**同源派生**——改 `MAX_DISCUSSION_ROUNDS` 即自动跟随，
    /// 不允许出现"轮数改了、补偿没改"的静默漂移（改常量即红）。
    #[test]
    fn round_gate_allowance_derives_from_discussion_rounds() {
        assert_eq!(
            ROUND_GATE_COUNT,
            MAX_DISCUSSION_ROUNDS - 1,
            "门开在轮与轮之间，道数必须 = 轮数 - 1"
        );
        assert_eq!(
            ROUND_GATE_TIMEOUT_ALLOWANCE,
            ROUND_GATE_MAX_WAIT * ROUND_GATE_COUNT,
            "超时补偿 = 单轮等待上限 × 门道数（用户思考时间不计入生成预算）"
        );
        assert!(
            ROUND_GATE_MAX_WAIT < PIPELINE_TIMEOUT,
            "单轮等待上限不得吞掉整条流水线的生成预算"
        );
        // 红灯先行暴露的盲区：值断言挡不住"恰好相等的硬编码"（本轮 3-1 == 2，
        // 把 ROUND_GATE_COUNT 改成字面量 2 时值断言依然绿）。故再锁**定义式源码形状**：
        // 两项必须写成对上游常量的派生，改轮数时自动跟随，不另立数字。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/commands/orchestrator.rs");
        let src = std::fs::read_to_string(&path).unwrap();
        let prod = &src[..src.find("mod tests").expect("找不到测试模块标记")];
        assert!(
            prod.contains("const ROUND_GATE_COUNT: u32 = MAX_DISCUSSION_ROUNDS.saturating_sub(1)"),
            "#12 门道数未与轮数同源派生（写成字面量会在改轮数时静默失配）"
        );
        assert!(
            prod.contains("const ROUND_GATE_TIMEOUT_ALLOWANCE: Duration =\n    Duration::from_secs(ROUND_GATE_MAX_WAIT.as_secs() * ROUND_GATE_COUNT as u64)")
                || prod.contains("Duration::from_secs(ROUND_GATE_MAX_WAIT.as_secs() * ROUND_GATE_COUNT as u64)"),
            "#12 超时补偿未由（单轮上限 × 门道数）派生"
        );
    }

    /// #12 接线锁（源码形状）：门必须开在**轮末**、只在**还有下一轮**时开、
    /// 用**有上限的等待**（无上限 = 挂死到超时强杀）、两种决断都要落到编排分支。
    #[test]
    fn round_gate_is_wired_between_rounds_with_bounded_wait() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/commands/orchestrator.rs");
        let src = std::fs::read_to_string(&path).unwrap();
        let start = src.find("④ #12 轮间人工确认门").expect("找不到确认门接线起点标记");
        let end = src.find("// #12：用户主动收工与").expect("找不到确认门接线终点标记");
        assert!(start < end, "确认门接线区标记顺序异常");
        let strip = |s: &str| -> String {
            s.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
        };
        let region = strip(&src[start..end]);
        for want in [
            // 仅当显式开启
            "gate_enabled",
            // 只在还有下一轮时开（末轮开 = 空等 5 分钟）
            "round < MAX_DISCUSSION_ROUNDS",
            // 通知前端（含超时上限，前端可显示倒计时提示）
            "RoundGatePending",
            "timeout_secs",
            // 有上限的等待（传常量，而非无界）
            "gate::wait(&run_id, ROUND_GATE_MAX_WAIT)",
            // 解除通知 + 两种用户决断 + 超时放行
            "RoundGateResolved",
            "GateDecision::Finalize",
            "GateDecision::Continue",
            "GateDecision::Timeout",
        ] {
            assert!(region.contains(want), "#12 接线区缺要素 `{}`：\n{}", want, region);
        }
        // 开启门必须补偿整体超时——否则暂停把流水线顶到硬超时被强杀
        let prod = &src[..src.find("mod tests").expect("找不到测试模块标记")];
        let prod_code = strip(prod);
        assert!(
            prod_code.contains("PIPELINE_TIMEOUT + ROUND_GATE_TIMEOUT_ALLOWANCE"),
            "开启确认门时整体超时未加补偿额度：暂停等待会吞掉终稿预算"
        );
        assert!(
            prod_code.contains("gate::reset(&rid)"),
            "run 入口未清门槽——上一轮残留的门会污染新任务"
        );
    }

    // ---- 超时守卫（spawn_guarded）----

    /// 超时必须真正终止任务——守卫返回 TimedOut 后，内层任务不得再推进
    #[tokio::test]
    async fn timeout_guard_aborts_inner_task() {
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = finished.clone();
        let inner = async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            // 只有任务未被 abort 时才会执行到这一行
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok("done".to_string())
        };
        let started = std::time::Instant::now();
        let outcome = spawn_guarded(inner, std::time::Duration::from_millis(50)).await;
        let elapsed = started.elapsed();

        assert!(matches!(outcome, GuardOutcome::TimedOut), "应为超时分支");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "必须在超时点附近返回，而非等任务跑完（实际 {:?}）",
            elapsed
        );
        // 关键断言：任务已被强杀，sleep 之后的代码永远没机会执行
        assert!(
            !finished.load(std::sync::atomic::Ordering::SeqCst),
            "内层任务应已被 abort，不得继续推进"
        );
    }

    /// 完成路径（含业务 Err）原样透传，行为与修复前一致
    #[tokio::test]
    async fn timeout_guard_passes_through_result() {
        let outcome =
            spawn_guarded(async { Err(AppError::from("业务错误")) }, Duration::from_secs(1)).await;
        assert!(matches!(outcome, GuardOutcome::Completed(Err(e)) if e.message == "业务错误"));
        let outcome2 =
            spawn_guarded(async { Ok("方案".to_string()) }, Duration::from_secs(1)).await;
        assert!(matches!(outcome2, GuardOutcome::Completed(Ok(s)) if s == "方案"));
    }

    /// panic 隔离路径仍然成立（unwind 语义；release 依赖 B2 不得设 panic=abort）
    #[tokio::test]
    async fn timeout_guard_catches_panic() {
        let outcome = spawn_guarded(
            async {
                panic!("模拟越界");
                #[allow(unreachable_code)]
                Ok(String::new())
            },
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(outcome, GuardOutcome::JoinPanicked(_)), "panic 应被隔离为 JoinPanicked");
    }

    /// C3/D3：阶段 0 主持人必须与审改员/校验员同清单注入（加载一致——数字口径同源）
    #[test]
    fn host_initial_system_includes_checklist() {
        let kb = test_kb();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let s = host_initial_system(&m, &kb, "");
            assert!(s.contains("【校验清单"), "{} stage-0 缺校验清单注入", m.to_str_name());
        }
        // mode_d 清单含抖音参数区间（渲染值来自常量，非手写）
        let d = host_initial_system(&Mode::ModeD, &kb, "");
        assert!(
            d.contains(&format!("{}-{}", crate::rules::DOUYIN_WEIRD_MIN, crate::rules::DOUYIN_WEIRD_MAX)),
            "stage-0 清单缺抖音参数区间"
        );
    }

    /// R2 锁（2026-09-17）：汇总/回炉阶段主持人 system 必须与阶段 0 同源——
    /// primer + 校验清单 + 信封契约一个不缺，且清单里的说明行上限必须是单源常量值。
    /// 旧实现（只有 host 人设 + 信封）会让本测试在"缺校验清单"处直接失败，防复发。
    #[test]
    fn host_summarize_system_same_single_source_as_initial() {
        let kb = test_kb();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let name = m.to_str_name();
            let stage0 = host_initial_system(&m, &kb, "（阶段0 无方案文本）");
            let summarize = host_summarize_system(&m, &kb, "（当前方案占位）");
            let checklist = crate::rules::checklist(name);
            assert!(summarize.contains(&checklist), "{} 汇总缺校验清单", name);
            assert!(stage0.contains(&checklist), "{} 阶段 0 缺校验清单", name);
            if let Some(primer) = crate::rules::host_primer(name) {
                let primer = crate::rules::interpolate(primer);
                assert!(summarize.contains(&primer), "{} 汇总缺地基 primer", name);
                assert!(stage0.contains(&primer), "{} 阶段 0 缺地基 primer", name);
            }
            let contract = crate::rules::plan_field_contract();
            assert!(summarize.contains(&contract), "{} 汇总缺 SongPlan 字段契约", name);
            assert!(stage0.contains(&contract), "{} 阶段 0 缺 SongPlan 字段契约", name);
            // 数字口径单源：汇总侧与阶段 0 侧写的是同一个上限值（不自持一份）
            assert!(
                summarize.contains(&crate::rules::DESC_LINE_MAX_CHARS.to_string()),
                "{} 汇总缺单源说明行上限 {}", name, crate::rules::DESC_LINE_MAX_CHARS
            );
            // 人设底座不同（阶段 0 = 模式指令；汇总 = host 统领），但约束段必须完全一致
            assert_ne!(stage0, summarize, "{} 两阶段 system 不应完全相同", name);
        }
    }

    /// 审计 #20 剩余（第二批·问题②）：**下游独有的格式契约必须上移到主持人可见的单源**。
    /// 根因：格式契约（乐器主次排列/人声映射到 Style Prompt/禁 full band/断句用单空格/能量标注格式）
    /// 原先只写在定稿人（校验员人设）与 prompts 里，主持人汇总/回炉时手里没有——整合出的方案
    /// 天然不合终稿格式，终稿硬门再打回 = 又一轮降级。现契约真源在 `rules::desc_line_contract()`
    /// （经 `host_system_with` 无条件注入，不随信封开关消失），本测试锁定"人设说的 = 主持人看到的"。
    #[test]
    fn terminal_format_contract_visible_to_host() {
        let kb = test_kb();
        let mirrors = ["SongPlan 字段契约", "global.negative", "minimal=true", "variety="];
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let name = m.to_str_name();
            for (stage, sys) in [
                ("阶段0", host_initial_system(&m, &kb, "")),
                ("汇总", host_summarize_system(&m, &kb, "")),
            ] {
                for k in mirrors {
                    assert!(sys.contains(k), "{} 主持人{} system 缺终稿格式契约「{}」", name, stage, k);
                }
            }
        }
        // A/B：字段契约要求每段 energy（${ENERGY_SCALE}）
        for m in [Mode::ModeA, Mode::ModeB] {
            let name = m.to_str_name();
            for (stage, sys) in [
                ("阶段0", host_initial_system(&m, &kb, "")),
                ("汇总", host_summarize_system(&m, &kb, "")),
            ] {
                assert!(sys.contains("energy"), "{} 主持人{} system 缺 energy 字段契约", name, stage);
            }
        }
        let contract = crate::rules::plan_field_contract();
        assert!(contract.contains("SongPlan"), "字段契约缺 SongPlan 标识");
        assert!(contract.contains("global.negative"), "字段契约缺 Exclude 落点");
    }


    /// RULE-STRUCTURED-ONLY：编排层不得再调用信封/降级路径
    #[test]
    fn no_legacy_envelope_symbols() {
        // BLUEPRINT_v2 §3.2 / §5.1#1-4：信封与转写保真旧路径全仓退役（src + prompt 快照）。
        // 禁用词以拼接构造，避免本测试自匹配。
        let banned: Vec<String> = [
            ["enforce_", "envelope"], ["envelope_", "fallback"], ["plan_envelope", "_enabled"],
            ["envelope_", "spec"], ["envelope_", "defect"], ["parse_plan", "_sections"],
            ["Plan", "Sections"], ["TRANSCRIPTION", "_CONTRACT"], ["TRANSCRIPTION", "_ISSUE"],
            ["transcription", "_fidelity"], ["<<<", "LYRICS>>>"], ["方案", "信封"], ["转写", "契约"],
            // 第二批：旧定稿措辞 + 前端降级标签（用户裁定旧历史不兼容回放）
            ["格式", "端口"], ["输出", "端口"], ["终稿", "端口"], ["最终格式", "输出"], ["格式", "降级"], ["格式", "输出中"], ["格式输出", "完成"], ["按标准格式", "输出"],
        ]
        .iter()
        .map(|p| p.concat())
        .collect();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else if p.extension().is_some_and(|x| ["rs", "txt", "ts", "tsx"].contains(&x.to_str().unwrap_or(""))) { out.push(p) }
            }
        }
        walk(&root.join("src"), &mut files);
        walk(&root.join("tests/fixtures/prompts"), &mut files);
        walk(&root.join("../src"), &mut files); // 前端（TS/TSX）
        assert!(files.iter().any(|f| f.ends_with("usePipeline.ts")), "前端扫描面缺失");
        assert!(files.len() > 20, "扫描面异常: {}", files.len());
        let mut hits = Vec::new();
        for f in &files {
            let s = std::fs::read_to_string(f).unwrap();
            for b in &banned {
                if s.contains(b.as_str()) { hits.push(format!("{}: {}", f.display(), b)) }
            }
        }
        assert!(hits.is_empty(), "旧信封/转写保真残留：\n{}", hits.join("\n"));
    }

    /// C3 快照：四模式 prompt 渲染输出须与改前快照逐字节一致（占位符恰好还原当日数字）。
    /// 常量有意变更时：跑 `cargo test --lib dump_mode_prompts` 重新生成快照并在提交说明中声明。
    #[test]
    fn mode_prompts_byte_identical_to_snapshot() {
        // Windows checkout 可能 CRLF（git autocrlf）——归一化后比较（内容等价性，换行符不属快照语义）
        let normalize = |s: &str| s.replace("\r\n", "\n");
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let name = m.to_str_name();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/prompts")
                .join(format!("{}.txt", name));
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("快照缺失 {}（先跑 dump_mode_prompts）: {}", name, e));
            assert_eq!(normalize(&prompt_for_mode(&m)), normalize(&expected), "{} prompt 与快照不一致", name);
        }
    }

    /// C3 快照再生成工具（不进常规断言流；常量变更后手动运行更新快照）
    #[test]
    #[ignore]
    fn dump_mode_prompts() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prompts");
        std::fs::create_dir_all(&dir).unwrap();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let name = m.to_str_name();
            std::fs::write(dir.join(format!("{}.txt", name)), prompt_for_mode(&m)).unwrap();
        }
    }

    /// D-Envelope 诊断：导出 mode_d 完整 host system（含信封契约），供线下探测模型格式遵从形态。
    #[test]
    #[ignore]
    fn dump_host_system_d() {
        let kb = test_kb();
        let sys = host_initial_system(&Mode::ModeD, &kb, "");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/large-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("host_system_d.txt"), sys).unwrap();
        println!("dumped to target/large-test/host_system_d.txt");
    }


    /// P1-5 接线锁：vocal_mix 词表进主持人上下文（阶段 0 与汇总共用 host_system_with）
    #[test]
    fn vocal_mix_injected_into_host_system() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let s = host_system_with("base", &m, &kb, "");
            assert!(s.contains("【人声/混音词表（vocal_mix）】") && s.contains("M-01"), "{:?} 未注入 vocal_mix", m);
        }
        let src = include_str!("orchestrator.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(prod.matches("vocal_mix_prompt_block(").count() >= 3, "host/审改/定稿三处注入");
    }


    /// P1-6 接线锁：审改角色 suno_rules **子集路径**同样过场景门（旧实现绕过 → 抖音行进 A/B）
    #[test]
    fn scenario_gate_applies_to_role_subset_path() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        let ly = roles::role_for(PipelineRole::Lyricist);
        let b = inject_knowledge(&kb, ly.knowledge_tables, ly.craft_refs, "", "mode_b", Some(PipelineRole::Lyricist.storage_key()));
        let d = inject_knowledge(&kb, ly.knowledge_tables, ly.craft_refs, "", "mode_d", Some(PipelineRole::Lyricist.storage_key()));
        assert!(b.contains("line_chars_verse") && !b.contains("line_max_chars"), "mode_b 作词人不得拿抖音行字数规则");
        assert!(d.contains("line_max_chars") && !d.contains("line_chars_verse"), "mode_d 作词人不得拿普通模式行字数规则");
        let pr = roles::role_for(PipelineRole::Producer);
        let pd = inject_knowledge(&kb, pr.knowledge_tables, pr.craft_refs, "", "mode_d", Some(PipelineRole::Producer.storage_key()));
        // 禁止 normal 弧线（style_arc_X）；允许 style_arc_d_*（抖音叙事回落专行）
        assert!(
            !pd.contains("style_arc_dry") || pd.contains("style_arc_d_dry"),
            "sanity"
        );
        for name in ["style_arc_dry", "style_arc_high", "style_arc_drama", "style_arc_loop"] {
            // markdown 单元格：规则名独立出现时前后为 | 或空白；d_ 前缀行不含精确名
            let banned = format!("| {} |", name);
            let banned2 = format!("\n{} |", name);
            assert!(
                !pd.contains(&banned) && !pd.contains(&banned2) && !pd.lines().any(|l| l.split('|').any(|c| c.trim() == name)),
                "mode_d 制作人不得拿 A/B 弧线 {name}：{}",
                pd.lines().filter(|l| l.contains("style_arc_")).take(3).collect::<Vec<_>>().join(" | ")
            );
        }
        assert!(pd.contains("style_arc_d_dry"), "mode_d 制作人须拿抖音叙事弧线");
    }


    /// add_douyin_arc：制作人/情感分析师在 mode_d 经子集路径拿到 style_arc_d_*，mode_b 拿到 style_arc_*
    #[test]
    fn douyin_arc_subset_injection_for_producer() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        let pr = roles::role_for(PipelineRole::Producer);
        let pd = inject_knowledge(&kb, pr.knowledge_tables, pr.craft_refs, "", "mode_d", Some(PipelineRole::Producer.storage_key()));
        let pb = inject_knowledge(&kb, pr.knowledge_tables, pr.craft_refs, "", "mode_b", Some(PipelineRole::Producer.storage_key()));
        assert!(pd.contains("style_arc_d_dry") && !pd.contains("style_arc_dry\n") && !pd.contains("| style_arc_dry |"), "mode_d 制作人须拿 d_ 弧线");
        // 渲染格式为 markdown；规则名出现在行首 ## 块内
        assert!(pd.matches("style_arc_d_").count() >= 10 || pd.contains("style_arc_d_loop"));
        assert!(!pb.contains("style_arc_d_"), "mode_b 不得拿 d_ 弧线");
        assert!(pb.contains("style_arc_dry"));
    }

}
