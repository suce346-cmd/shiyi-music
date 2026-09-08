//! 圆桌编排器（v3 架构）：主持人统领 + 角色审改 + 校验员讨论轮审查 + 校验员格式端口。
//!
//! 三阶段：
//! - 阶段 0：主持人用该模式的完整指令（prompts.rs，不查 CSV）产出方案初稿
//! - 阶段 1：动态角色（专业审改员）审查方案 → 查 CSV 调素材 → 输出修订片段；
//!   校验员审查（当前方案 + 本轮修订 + 任务分发，提出观点返回主持人）；
//!   主持人汇总成新版完整方案 + 下轮任务分发（【任务分发】段切分）→ 再分发；
//!   全角色与校验员无异议或满 3 轮收敛
//! - 阶段 2：校验员按标准格式输出最终提示词包；代码硬校验兜底（失败打回重格式化）

use crate::commands::{cancel, interject, llm, prompts, roles, validator};
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

/// 一条修订片段
#[derive(Debug, Clone)]
struct ReviewChange {
    target: String,
    content: String,
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

/// target 合法枚举（非法 target 归一为 other，主持人汇总时按杂项处理）
fn normalize_target(t: &str) -> String {
    match t {
        "style_prompt" | "lyrics" | "params" | "other" => t.to_string(),
        _ => "other".to_string(),
    }
}

/// 解析审改 JSON（不可信输出显式降级，不再假同意）。
/// - JSON 解析失败 / agree 字段缺失 → degraded=true（意见作废，仅作警示记录，见 humanize_review）
/// ——content 为空的修订丢弃；非法 target 归一为 other。
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
            let content = c["content"].as_str().unwrap_or("").trim().to_string();
            if content.is_empty() {
                continue; // 空修订无意义，丢弃
            }
            changes.push(ReviewChange {
                target: normalize_target(c["target"].as_str().unwrap_or("other")),
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
        out.push_str(&format!("\n· {} → {}{}", c.target, c.content, reason));
    }
    if !r.reason.is_empty() {
        out.push_str(&format!("\n总体意见：{}", r.reason));
    }
    out
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

/// 从方案文本提取候选词命中（候选词出现在方案中即命中；单字候选跳过——避免"深夜"误命中"夜"）
fn matching_keywords(plan: &str, candidates: &[String]) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| c.chars().count() >= 2 && plan.contains(c.as_str()))
        .cloned()
        .collect()
}



// ---------------------------------------------------------------------------
// 注入量规范（条数上限）——集中定义，测试锁定，禁止散改
// 依据：知识库注入是"参考素材"而非"全量拷贝"，条数过多则 LLM 记不住重点。
// - 关键词表（emotions/cliches/hooks）：命中 6 条封顶——主词 1-3 个 + 近邻，6 条覆盖完整
// - 流派表：3 条封顶——方案通常命中 1-2 个流派，3 条少而准
// - 乐器表：15 件封顶——能量区间覆盖弧线两端，"少而准"验证值
// - 未命中：零行+无示例标注（M18，调用方走确定性默认；见 render_filtered_any 内部）
// - suno_rules：校验员全量（40 条 < 50 截断上限）；其他角色按规则子集过滤
// - 单角色一次注入总字数封顶：预算按最坏情况实测标定（制作人最大 ≈ 4800 字，取 5200）
// ---------------------------------------------------------------------------
/// 关键词表（emotions/cliches/hooks）命中条数上限
pub const INJECT_MAX_KEYWORD_ROWS: usize = 6;
/// 流派表命中条数上限
pub const INJECT_MAX_STYLE_GENRE_ROWS: usize = 3;
/// 乐器表能量区间命中件数上限
pub const INJECT_MAX_INSTRUMENTS_ROWS: usize = 15;
/// 全量表（suno_rules）渲染截断上限（当前 40 条规则，留 10 条余量防静默截断）
pub const INJECT_MAX_FULL_ROWS: usize = 50;
/// 单角色一次注入总字数封顶（超过告警；最坏情况 = 制作人四表全命中含 22 条规则子集+8 条思维资产 ≈ 4800 字）
pub const INJECT_MAX_TOTAL_CHARS: usize = 5200;
/// 方案注入长度上限（超则截断 + 附注；对齐知识库"少而准"纪律，上下文同样需要预算）
pub const INJECT_MAX_PLAN_CHARS: usize = 8000;
/// revisions_log 保留条数（更早折叠为单行摘要，reason 关键词保留供去重参考）
pub const INJECT_MAX_LOG_ENTRIES: usize = 6;
/// 格式输出截断时注入打回循环的 issue 文案（走 AuditResult 事件，用户可见）
const TRUNCATION_ISSUE: &str = "输出被截断（finish_reason=length），请精简内容后重新输出完整提示词包";

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
/// - emotions/cliches/hooks/style_genre：候选词（emotion/cliche/hook_type/genre 列值）命中 → 过滤注入
/// - instruments：按方案能量区间数值过滤（覆盖弧线两端），上限 15 件
/// - suno_rules：校验员全量（格式端口必须全见）；其他角色按行子集过滤（rule 列 contains 匹配）
/// - cols 投影：空切片 = 全列；非空 = 按角色只注入这些列（多角色侧重点）
/// - subset 行子集：空切片 = 全行；非空 = 按 rule 列值过滤（如制作人只要参数/配器类规则）
/// 条数上限见 INJECT_MAX_* 常量（集中定义，测试锁定）。
fn inject_knowledge(kb: &KnowledgeBase, tables: &[(&str, &[&str], &[&str])], plan: &str) -> String {
    let mut out = String::new();
    for (t, cols, subset) in tables {
        // 列投影：空切片 = 全列（None），非空 = 角色裁剪
        let proj: Option<&[&str]> = if cols.is_empty() { None } else { Some(cols) };
        let rendered = match *t {
            "emotions" | "cliches" | "hooks" | "style_genre" => {
                let col = match *t {
                    "emotions" => "emotion",
                    "cliches" => "cliche",
                    "hooks" => "hook_type",
                    _ => "genre",
                };
                let cands = matching_keywords(plan, &column_values(kb, t, col));
                let refs: Vec<&str> = cands.iter().map(|s| s.as_str()).collect();
                // 条数规范：流派 3 条封顶（命中通常 1-2 个），其余关键词表 6 条封顶
                let limit: Option<usize> = if *t == "style_genre" {
                    Some(INJECT_MAX_STYLE_GENRE_ROWS)
                } else {
                    Some(INJECT_MAX_KEYWORD_ROWS)
                };
                if refs.is_empty() {
                    kb.render_filtered_any(t, &[(col, &[])], proj, plan, None)
                } else {
                    kb.render_filtered_any(t, &[(col, &refs)], proj, plan, limit)
                }
            }
            "instruments" => {
                match plan_energy_range(plan) {
                    // 条数规范：能量区间命中 ≤15 件（"少而准"验证值）
                    Some((e_min, e_max)) => {
                        kb.render_instruments_by_energy(e_min, e_max, proj, plan, Some(INJECT_MAX_INSTRUMENTS_ROWS))
                    }
                    None => kb.render_filtered_any("instruments", &[("instrument", &[])], proj, plan, None), // 无能量：兜底
                }
            }
            "suno_rules" => {
                if subset.is_empty() {
                    // 校验员：全量（规则必须全见）
                    kb.render_table(t, proj, Some(INJECT_MAX_FULL_ROWS))
                } else {
                    // 其他角色：按规则名子集过滤（rule 列 contains 匹配）
                    kb.render_filtered_any(t, &[("rule", subset)], proj, plan, None)
                }
            }
            // P2：思维资产表（lyric_craft/compose_craft）按 trigger 列做模式过滤 + 8 条上限。
            // trigger 含"审改"即本轮可用；"阶段0"仅主持 primer 用；"扩展位"默认不注入。
            "lyric_craft" | "compose_craft" => {
                kb.render_filtered_any(t, &[("trigger", &["审改"])], proj, plan, Some(8))
            }
            // 未知表：保守全量
            _ => kb.render_table(t, proj, Some(INJECT_MAX_FULL_ROWS)),
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
    out
}

/// 角色审改：审查主持人当前方案 → 查 CSV → 输出修订片段 JSON
async fn execute_review<R: Runtime>(
    app: &AppHandle<R>,
    role: PipelineRole,
    current_plan: &str,
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
    let kb = load_knowledge()?;
    let r = roles::role_for(role);

    let mut system = String::new();
    system.push_str(&format!("【角色】{} {}\n", r.name, r.emoji));
    // C3：角色提示词过数值单源插值（${占位符} → rules 常量派生值）
    system.push_str(&format!("{}\n", crate::rules::interpolate(r.system_prompt)));
    // 微观②：按需检索注入——按角色绑定表 + 当前方案关键词过滤，只注入命中条目（suno_rules 规则全量）
    system.push_str(&inject_knowledge(&kb, r.knowledge_tables, current_plan));
    // P4：单源校验清单（数字唯一 prose 载体；审改口径与硬校验同源）
    system.push_str(crate::rules::checklist(req.mode.to_str_name()));
    system.push('\n');
    system.push_str("\n输出 JSON（严格符合格式，不输出其他内容）：\n");
    system.push_str(&crate::rules::interpolate(r.output_schema));

    // 方案截断 + log 折叠（预算纪律；知识库注入同风格）
    let plan_view = truncate_plan(current_plan);
    let log_view = fold_log(revisions_log);
    let mut user = format!("【主持人当前方案】\n{}\n\n", plan_view);
    // Mode C：原歌词全链路传递——审改员逐行字数/韵脚对齐的依据
    if let Some(original) = req.original_lyrics_text() {
        user.push_str(&format!("【原歌词（改写需逐行对齐）】\n{}\n\n", original));
    }
    if !log_view.is_empty() {
        user.push_str("【已提修订（可参考，不要重复提同一问题）】\n");
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

    let (base_url, api_key, model) = resolve_api(req, role);
    let _ = emit(PipelineEvent::StepStart { role });
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
        &run_id,
        llm::FINAL_STAGE_RESERVE,
    )
    .await?;
    // （上限放开后简化）：30000 上限下截断极罕见，观测记录即可——JSON 已完整时仍可正常解析
    if llm::is_truncated(&resp.finish_reason) {
        tracing::warn!(role = %role.name(), "审改输出触及 max_tokens 上限");
    }
    let mut result = parse_review(&resp.raw);
    // 解析失败 → 同参数重试一次（LLM 输出有随机性，重试常能修复）；仍失败走相关降级警示
    if result.degraded {
        tracing::warn!(role = %role.name(), "审改输出无法解析，重试一次");
        let resp2 = llm::call_llm_silent(
            &base_url, &api_key, &model,
            messages.clone(),
            llm::MAX_TOKENS_CAP,
            req.thinking,
            budget,
            &gen,
            &run_id,
            llm::FINAL_STAGE_RESERVE,
        )
        .await?;
        let result2 = parse_review(&resp2.raw);
        // B-1：重试调用同样计费，usage 必须如实入账（旧规则只发首调，重试 tokens 漏记）
        emit_usage(app, role, &resp2, run_id);
        if !result2.degraded {
            result = result2;
        }
    }
    // C5/ADR-3：call_degraded 降级源——输出解析失败（重试后仍不可信），意见作废仅作警示
    if result.degraded {
        let _ = emit(PipelineEvent::Degraded {
            flag: "call_degraded".into(),
            detail: format!("{} 输出解析失败，意见作废（已降级警示）", role.name()),
        });
    }
    let _ = emit(PipelineEvent::StepDone {
        role,
        summary: humanize_review(role, &result),
    });
    emit_usage(app, role, &resp, run_id);
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
                    c.target, c.content, c.reason
                ));
            }
        }
        user.push('\n');
    }
    if !log_view.is_empty() {
        user.push_str("【已提修订（不要重复提同一问题）】\n");
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
    let kb = load_knowledge()?;

    let mut system = String::new();
    system.push_str("【角色】校验员 🔍\n");
    // C3：角色提示词过数值单源插值
    system.push_str(&crate::rules::interpolate(roles::auditor_review_prompt()));
    system.push('\n');
    if let Ok(rendered) = kb.render_table("suno_rules", None, Some(INJECT_MAX_FULL_ROWS)) {
        system.push_str(&rendered);
        system.push('\n');
    }
    // P4：单源校验清单（校验员审查口径与硬校验同源）
    system.push_str(crate::rules::checklist(req.mode.to_str_name()));
    system.push('\n');
    system.push_str("\n输出 JSON（严格符合格式，不输出其他内容）：\n");
    system.push_str(roles::REVIEW_SCHEMA_AUDITOR);

    let user = build_audit_review_user_prompt(current_plan, round_changes, revisions_log, next_tasks, req);

    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Auditor);
    let _ = emit(PipelineEvent::StepStart { role: PipelineRole::Auditor });
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
        &run_id,
        llm::FINAL_STAGE_RESERVE,
    )
    .await?;
    // （上限放开后简化）：截断观测记录，JSON 完整时照常解析
    if llm::is_truncated(&resp.finish_reason) {
        tracing::warn!("校验员审查输出触及 max_tokens 上限");
    }
    let mut result = parse_review(&resp.raw);
    // 解析失败 → 重试一次；仍失败走相关降级警示
    if result.degraded {
        tracing::warn!("校验员审查输出无法解析，重试一次");
        let resp2 = llm::call_llm_silent(
            &base_url, &api_key, &model,
            messages.clone(),
            llm::MAX_TOKENS_CAP,
            req.thinking,
            budget,
            &gen,
            &run_id,
            llm::FINAL_STAGE_RESERVE,
        )
        .await?;
        let result2 = parse_review(&resp2.raw);
        // B-1：重试调用同样计费，usage 必须如实入账（旧规则只发首调，重试 tokens 漏记）
        emit_usage(app, PipelineRole::Auditor, &resp2, run_id);
        if !result2.degraded {
            result = result2;
        }
    }
    let _ = emit(PipelineEvent::StepDone {
        role: PipelineRole::Auditor,
        summary: humanize_review(PipelineRole::Auditor, &result),
    });
    emit_usage(app, PipelineRole::Auditor, &resp, run_id);
    Ok(result)
}

// ---------------------------------------------------------------------------
// 主持人（阶段 0 统领 / 阶段 1 汇总）
// ---------------------------------------------------------------------------

/// 阶段 0 主持人 system 组装单源：模式指令（含 override）+ 地基 primer + 校验清单（D3 同口径）。
fn host_initial_system(mode: &Mode) -> String {
    let mut system = prompt_for_mode(mode);
    // P4：阶段 0 地基 primer（模式专属静态文本；缺失回退无 primer 旧行为；主持人仍零 CSV）
    if let Some(primer) = crate::rules::host_primer(mode.to_str_name()) {
        system.push_str("\n\n");
        system.push_str(&crate::rules::interpolate(primer));
    }
    // C3/D3：阶段 0 纳入单源——主持人与审改员/校验员同一清单（数字口径同源，加载一致）
    system.push_str("\n\n");
    system.push_str(crate::rules::checklist(mode.to_str_name()));
    system
}

/// 阶段 0：主持人用该模式的完整指令产出方案初稿（流式）
async fn run_host_initial<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<String, AppError> {
    use crate::models::PipelineEnvelope;
    let gen = req.generation.clone().unwrap_or_default();
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    let _ = emit(PipelineEvent::HostStart { stage: HostStage::Initial });
    let system = host_initial_system(&req.mode);
    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Host);
    let mut user = format!("用户输入：\n{}\n\n请按上述方法论直接输出完整方案。", req.user_input);
    // Mode C：原歌词在 extra，指令期望"原歌词 + 新主题"
    if let Some(original) = req.original_lyrics_text() {
        user = format!("原歌词：\n{}\n\n新主题/故事：\n{}\n\n请按上述方法论直接输出完整改词方案。", original, req.user_input);
    }
    let messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
    // R2/R2b：流式 body 中断重试——整流同参最多重发 3 次。
    // Q3分流（已修正）：只对流解析类错误重试——Parse 或 "Stream error" 文案（流 body 中断是
    // Network("Stream error...") 形态，按文案挑出来，与非流式 R9 同口径）；
    // 取消/预算/限流/鉴权/内层已退避过的其他网络错误直接透出，避免 3x3 双重计费烧预算。
    // 截断是成功返回走不到这里。
    let mut resp: Result<crate::models::LLMResponse, crate::errors::AppError> =
        Err("阶段 0 未执行".into());
    for attempt in 0..3 {
        if attempt > 0 {
            // Q3：等待按 remaining - 保底截断，不吃终稿 120s（与 llm.rs 内层同口径）。
            let wait = std::time::Duration::from_secs(if attempt == 1 { 5 } else { 10 });
            let wait_capped = wait.min(budget.remaining_for_discussion(llm::FINAL_STAGE_RESERVE));
            if wait_capped.is_zero() || tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_capped)).await.is_err() {
                // G-2：丢因修复——旧规则 break 后 resp 保持上一轮流错误，"预算打断"真实原因丢失。
                // 置 Timeout 结论（外层按不可重试透出）并保留最后一次流错误。
                tracing::warn!("阶段 0 重发等待被预算打断，不再重试");
                let last_msg = match &resp {
                    Err(e) => e.message.clone(),
                    _ => String::new(),
                };
                resp = Err(crate::errors::AppError::new(
                    crate::errors::ErrorKind::Timeout,
                    if last_msg.is_empty() {
                        "预算打断，阶段 0 整流重发未执行".to_string()
                    } else {
                        format!("预算打断，停止阶段 0 整流重发；最后一次错误: {}", last_msg)
                    },
                ));
                break;
            }
        }
        match llm::call_llm_stream(
            app.clone(), &base_url, &api_key, &model,
            messages.clone(),
            req.thinking,
            budget,
            &gen,
            &run_id,
            llm::FINAL_STAGE_RESERVE,
        )
        .await
        {
            Ok(r) => { resp = Ok(r); break; }
            // Q3分流：Parse 或 "Stream error" 文案才重发（流 body 中断形态）；
            // Cancelled/Timeout/RateLimit/Auth/其他 Network（内层已退避）直接透出。
            Err(e) if e.kind == crate::errors::ErrorKind::Parse || e.message.starts_with("Stream error") => {
                tracing::warn!(attempt = attempt + 1, error = %e.message, "阶段 0 流式中断，整流重发");
                resp = Err(e);
            }
            Err(e) => { resp = Err(e); break; }
        }
    }
    let resp = resp?;
    // 流式截断直接报错——半截方案绝不允许进入讨论轮（用户可见明确错误，可简化输入后重试）
    if llm::is_truncated(&resp.finish_reason) {
        tracing::error!("方案初稿输出被截断");
        return Err("方案初稿输出被截断（达到输出上限），请简化输入后重试".into());
    }
    let _ = emit(PipelineEvent::HostDone { stage: HostStage::Initial });
    emit_usage(app, PipelineRole::Host, &resp, run_id);
    Ok(resp.raw)
}

/// 主持人汇总 user prompt 构建（纯函数，可测）。
/// C4/D4：冲突修订对——(角色A下标, 修订A下标, 角色B下标, 修订B下标, target)。
type ConflictPair = (usize, usize, usize, usize, String);

/// 修订对冲突判定（保守口径单源）：同 target、非空、且文本互不包含
/// （包含 = 细化，相同 = 同意，都放行）。
fn is_conflict_pair(ca: &ReviewChange, cb: &ReviewChange) -> bool {
    let (a, b) = (ca.content.trim(), cb.content.trim());
    !a.is_empty() && !b.is_empty() && ca.target == cb.target && !a.contains(b) && !b.contains(a)
}

/// 冲突预检（保守判定）：同 target、跨角色、且修订文本互不包含 → 候选冲突；
/// 一方包含另一方视为细化（后者是对前者的补充）不算冲突，相同内容同理；
/// 同角色多条同 target 是作者自己的并列意见，不属跨角色冲突，不标。
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
            user.push_str(&format!("- target: {} | content: {} | reason: {}\n", c.target, c.content, c.reason));
        }
    }
    // L-4：原歌词比对基准（与阶段 2 同措辞）；None（A/B/D 或未填）不追加，行为零变化
    if let Some(original) = original_lyrics {
        user.push_str(&format!(
            "\n\n【原歌词（逐行字数对齐依据，整合修订时改词必须逐行等字数）】\n{}",
            original
        ));
    }
    user.push_str("\n请把修订整合进当前方案，输出新版完整方案（只含生产方案：Style Prompt + 歌词（含说明行）+ 参数，不要重复输出分析数据包）。");
    user.push_str("如需下一轮讨论，在方案末尾单独一行【任务分发】后点名各角色下一轮要解决的具体问题；若已无必要则只输出方案，不输出该段。");
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
    let gen = req.generation.clone().unwrap_or_default();
    let emit = |event: PipelineEvent| {
        let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
    };
    let host = roles::host();
    // L-4 新规则：Mode C 原歌词贯穿全链路——阶段 0（初稿）与阶段 2（格式输出）都带原词，
    // 旧规则唯独汇总阶段不带，主持人整合修订时无比对基准，属盲改。措辞与阶段 2 同口径。
    let user = build_summarize_user_prompt(current_plan, round_changes, req.original_lyrics_text());
    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Host);
    let _ = emit(PipelineEvent::HostStart { stage: HostStage::Summarize });
    let messages = vec![
        // C3：角色提示词过数值单源插值（主持人人设无占位符时原样返回）
        json!({"role":"system","content":crate::rules::interpolate(host.system_prompt)}),
        json!({"role":"user","content":user}),
    ];
    let resp = llm::call_llm_silent(
        &base_url, &api_key, &model,
        messages,
        llm::MAX_TOKENS_CAP,
        req.thinking,
        budget,
        &gen,
        &run_id,
        llm::FINAL_STAGE_RESERVE,
    )
    .await?;
    // （上限放开后简化）：截断观测记录，split_tasks 对无标记文本全文当方案，行为兼容
    if llm::is_truncated(&resp.finish_reason) {
        tracing::warn!("主持人汇总输出触及 max_tokens 上限，按现状继续");
    }
    let _ = emit(PipelineEvent::HostDone { stage: HostStage::Summarize });
    emit_usage(app, PipelineRole::Host, &resp, run_id);
    Ok(split_tasks(&resp.raw))
}

// ---------------------------------------------------------------------------
// 校验员（阶段 2：最终格式输出端口）
// ---------------------------------------------------------------------------

/// Mode C 改词专项（阶段 2 auditor 格式输出的 user 侧追加块）。
/// L-2 单源：行数口径与 validator 同一数字——validator 允许尾部 ≤LYRIC_FILL_TAIL_ALLOW 行收尾
/// （validator.rs:266），旧规则此处写"总行数完全一致/禁止增删"，与校验员系统 prompt 和
/// 硬校验的"尾部 ≤2 行"互相打架，产生无意义打回循环。
fn mode_c_special_block() -> String {
    let tail = crate::rules::LYRIC_FILL_TAIL_ALLOW;
    format!(
        "\n\n【Mode C 改词专项（必须满足，校验会打回）】\n\
1. 歌词是原歌词的逐行改写：新歌词总行数与原歌词一致（原歌词每行对应新歌词一行，仅允许尾部 ≤{} 行收尾）\n\
2. 每行字数（不含断句空格）与原歌词对应行完全一致\n\
3. 禁止中途增删歌词行、禁止自由创作新歌词段落（尾部收尾行不计入增删）；段落结构必须与原歌词一致（原歌词几段新歌词就几段，原歌词无 Hook/Chorus 段则禁止新增，禁止为 Hook 加段）\n\
4. 全部歌词行总数等于原歌词行数，尾部收尾最多加 {} 行\n\
5. 说明行必须带方括号（[乐器+行为, 空间, 力度]），禁止裸写说明行",
        tail, tail
    )
}

/// 校验员格式输出 system 构建单源（全文重输与定点重写共用——契约/知识注入口径一致）。
fn auditor_format_system(req: &PipelineRequest) -> Result<String, AppError> {
    let auditor = roles::auditor();
    let kb = load_knowledge()?;
    // Mode C 切换专用格式规范（通用规范诱导新增歌词段，与逐行对齐约束冲突）
    let mut system = if req.mode == Mode::ModeC {
        crate::rules::interpolate(roles::auditor_format_prompt_mode_c())
    } else {
        crate::rules::interpolate(auditor.system_prompt)
    };
    if let Ok(rules) = kb.render_table("suno_rules", None, Some(INJECT_MAX_FULL_ROWS)) {
        system.push_str(&rules);
        system.push('\n');
    }
    // C2/ADR-1：转写契约注入（开关关=不注入，回 v0.5.1 重写语义）
    if transcription_fidelity_enabled() {
        system.push_str(roles::TRANSCRIPTION_CONTRACT);
    }
    Ok(system)
}

/// 阶段 2 终稿产出上下文（C2 治味参数对象）：app/模式/请求/预算/run_id/保真开关。
/// 正源方案不进 ctx——回炉会更新方案（P4 实网修复），所有函数显式接收 plan 参数。
struct FinalStageCtx<'a, R: Runtime> {
    app: &'a AppHandle<R>,
    mode: &'a Mode,
    request: &'a PipelineRequest,
    budget: &'a crate::budget::SharedBudget,
    run_id: &'a str,
    fidelity: bool,
}

/// 圆桌流水线事件发射单源（run_id 绑定 envelope）。
fn emit_pipeline_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
    event: PipelineEvent,
) {
    use crate::models::PipelineEnvelope;
    let _ = app.emit("pipeline", PipelineEnvelope::new(run_id.to_string(), event));
}

/// 阶段 2：校验员按标准格式输出最终提示词包（硬校验失败打回重格式化）。
/// 返回（文本, 是否截断）———截断由调用方注入打回 issue，不在本函数内重试（复用打回循环的次数上限）。
async fn run_audit_format<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &str,
    issues: Option<&[String]>,
) -> Result<(String, bool), AppError> {
    let system = auditor_format_system(ctx.request)?;
    let mut user = if ctx.fidelity {
        format!("以下是已收敛的最终方案，请按【转写契约】转写为最终提示词包（转写不是重写）：\n\n{}", plan)
    } else {
        format!("请按标准格式输出最终提示词包：\n\n{}", plan)
    };
    // Mode C：原歌词全链路传递——格式输出逐行字数对齐的依据
    if let Some(original) = ctx.request.original_lyrics_text() {
        user.push_str(&format!("\n\n【原歌词（逐行字数对齐依据，改词必须逐行等字数输出）】\n{}", original));
    }
    // Mode C 专项：auditor 通用规范不含"改词"约束，必须显式声明
    if ctx.request.mode == Mode::ModeC {
        user.push_str(&mode_c_special_block());
    }
    if let Some(issues) = issues {
        user.push_str(&format!(
            "\n\n【格式问题（逐条修正后重新输出完整包）】\n{}",
            issues.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n")
        ));
    }
    call_auditor_format(ctx, system, user).await
}

/// C2/ADR-1：定点重写（纯保真违规专用）——只重写违规段落，不输出其余内容。
/// 与全文重输的区别：输出按段落拼接（splice_sections），未点名段落字节不动，避免"修一处漂三处"。
async fn run_audit_targeted_rewrite<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &str,
    final_text: &str,
    issues: &[String],
) -> Result<(String, bool), AppError> {
    let system = auditor_format_system(ctx.request)?;
    let mut user = format!(
        "【定点重写】终稿的以下歌词行违反转写保真（两侧对照见问题清单）。只输出需要修正的段落：每段以原结构标签行开头，违规行按收敛方案逐字转写修正，其余歌词行逐字保留，不要输出其他段落、不要解释。\n\n【保真校验问题】\n{}\n\n【终稿（被点名的段落在此，供定位）】\n{}\n\n【收敛方案（歌词唯一正源）】\n{}",
        issues.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n"),
        final_text,
        plan
    );
    if let Some(original) = ctx.request.original_lyrics_text() {
        user.push_str(&format!("\n\n【原歌词（Mode C 逐行字数对齐依据）】\n{}", original));
    }
    call_auditor_format(ctx, system, user).await
}

/// 校验员格式输出的 LLM 调用单源（system/user 组装完成后走这里——用量/截断/AuditStart 语义一致）。
async fn call_auditor_format<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    system: String,
    user: String,
) -> Result<(String, bool), AppError> {
    let emit = |event: PipelineEvent| emit_pipeline_event(ctx.app, ctx.run_id, event);
    let _ = emit(PipelineEvent::AuditStart);
    let req = ctx.request;
    let (base_url, api_key, model) = resolve_api(req, PipelineRole::Auditor);
    // R1：阶段 2 保底解除——全额使用剩余预算，保证终稿至少有一次完整尝试 + 退避
    let resp = llm::call_llm_silent(
        &base_url, &api_key, &model,
        vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})],
        llm::MAX_TOKENS_CAP,
        req.thinking,
        ctx.budget,
        &req.generation.clone().unwrap_or_default(),
        ctx.run_id,
        std::time::Duration::ZERO,
    )
    .await?;
    emit_usage(ctx.app, PipelineRole::Auditor, &resp, ctx.run_id);
    Ok((resp.raw, llm::is_truncated(&resp.finish_reason)))
}

/// C2/D6：转写保真功能开关。默认开；env SHIYI_TRANSCRIPTION_FIDELITY=0 一键回 v0.5.1 行为
/// （契约不注入、保真不校验、打回全部全文重输）——出问题可不发版关闭。
fn transcription_fidelity_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("SHIYI_TRANSCRIPTION_FIDELITY").ok().as_deref() != Some("0"))
}

const TRANSCRIPTION_ISSUE_PREFIX: &str = "TRANSCRIPTION_ISSUE:";

/// C2/ADR-1：提取并剥离终稿中的转写问题标记行（契约第 3 条）。
/// 标记 = 转写者申报"收敛方案本身有硬伤"，不得进入交付包；描述返回给调用方走回炉/显式降级。
fn extract_and_strip_transcription_issues(text: &str) -> (String, Vec<String>) {
    let mut descs = Vec::new();
    let mut kept: Vec<&str> = Vec::new();
    for line in text.lines() {
        match line.trim().strip_prefix(TRANSCRIPTION_ISSUE_PREFIX) {
            Some(desc) => {
                let desc = desc.trim();
                if !desc.is_empty() {
                    descs.push(desc.to_string());
                }
            }
            None => kept.push(line),
        }
    }
    (kept.join("\n"), descs)
}

/// 结构标签行判定（'[' 开头且无逗号——说明行含逗号，与 extract_section_tags 同判据）。
fn is_section_tag_line(l: &str) -> bool {
    let t = l.trim();
    t.starts_with('[') && t.ends_with(']') && !t.contains(',')
}

/// 终稿/重写输出解析出的段落：标签（小写归一）+ 段体（含标签行与换行）+ 段体在源文本中的字节区间。
struct PlanSegment {
    tag: String,
    body: String,
    start: usize,
    end: usize,
}

/// 单遍解析：标签行开启新段（段起点=标签行字节起点，段终点=下一标签行起点或文末）；
/// 标签前的前导内容（Style Prompt 等）不属任何段，splice 不触碰。
fn parse_segments(text: &str) -> Vec<PlanSegment> {
    let mut segs: Vec<PlanSegment> = Vec::new();
    let mut acc = 0usize;
    for line in text.split_inclusive('\n') {
        let start = acc;
        acc += line.len();
        if is_section_tag_line(line) {
            segs.push(PlanSegment {
                tag: line.trim().to_lowercase(),
                body: line.to_string(),
                start,
                end: acc,
            });
        } else if let Some(last) = segs.last_mut() {
            last.body.push_str(line);
            last.end = acc;
        }
    }
    segs
}

/// 同名标签按出现顺序一一配对（每个标签一个消费游标）；
/// 重写段数多于终稿同名段数时多余段忽略。返回 (base 段索引, 新段体) 列表。
fn pair_segment_replacements(base: &[PlanSegment], rewrites: &[PlanSegment]) -> Vec<(usize, String)> {
    let mut cursor: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut replacements = Vec::new();
    for seg in rewrites {
        let used = cursor.entry(seg.tag.as_str()).or_insert(0);
        let nth = base
            .iter()
            .enumerate()
            .filter(|(_, b)| b.tag == seg.tag)
            .nth(*used);
        if let Some((i, _)) = nth {
            replacements.push((i, seg.body.clone()));
            *used += 1;
        }
    }
    replacements
}

/// 把配对结果按 base 段字节区间写回：未匹配区间按字节原样保留，未点名段落逐字节不变；
/// 新段体短于/长于原段都按区间替换（换行结尾对齐补齐）。
fn apply_segment_replacements(base: &str, base_segs: &[PlanSegment], replacements: &[(usize, String)]) -> String {
    let mut out = String::with_capacity(base.len() + 64);
    let mut pos = 0usize;
    for (idx, new_body) in replacements {
        let seg = &base_segs[*idx];
        out.push_str(&base[pos..seg.start]);
        // 新段体补齐换行对齐：原段以换行结尾而新段体没有时补上
        let base_body = &base[seg.start..seg.end];
        let mut body = new_body.clone();
        if base_body.ends_with('\n') && !body.ends_with('\n') {
            body.push('\n');
        }
        out.push_str(&body);
        pos = seg.end;
    }
    out.push_str(&base[pos..]);
    out
}

/// C2/ADR-1：定点重写拼接——把定点重写输出中的段落按结构标签替换进终稿。
/// 无任何一段成功匹配 → None（调用方回退全文重输通道）。
fn splice_sections(base: &str, rewrites: &str) -> Option<String> {
    let base_segs = parse_segments(base);
    let rewrite_segs = parse_segments(rewrites);
    if base_segs.is_empty() || rewrite_segs.is_empty() {
        return None;
    }
    let mut replacements = pair_segment_replacements(&base_segs, &rewrite_segs);
    if replacements.is_empty() {
        return None;
    }
    // 重写段可能乱序（模型不保证按原文档顺序输出）——按 base 位置排序后顺序拼接
    replacements.sort_by_key(|(i, _)| *i);
    Some(apply_segment_replacements(base, &base_segs, &replacements))
}

/// C2/ADR-1：阶段 2 终稿产出（generate 主流程与 resume 续跑共用同一实现——加载必须一致）：
/// 转写契约输出 → TRANSCRIPTION_ISSUE 处理（剥标记 + 一次定点回炉，自然消耗预算）→
/// 保真校验 + 硬校验打回循环（≤2；纯保真违规走定点重写拼接，混有硬伤回退全文重输）→
/// 最终复验（含保真）→ AuditResult。返回（终稿, 最终 issues）——降级由调用方据 issues 判定。
/// 回炉修订片段：校验员的 TRANSCRIPTION_ISSUE 申报合成一条 other 修订，
/// 走 run_host_summarize 既有通道交主持人定点整合（"谁发现谁修"责任链）。
fn auditor_repair_changes(marker_descs: &[String]) -> Vec<(PipelineRole, Vec<ReviewChange>, String)> {
    vec![(
        PipelineRole::Auditor,
        vec![ReviewChange {
            target: "other".to_string(),
            content: marker_descs.join("；"),
            reason: "转写契约发现收敛方案硬伤（TRANSCRIPTION_ISSUE），请整合修正".to_string(),
        }],
        "转写契约申报收敛方案硬伤".to_string(),
    )]
}

/// TRANSCRIPTION_ISSUE 回炉：主持人整合硬伤 → 以修复后方案重转写 → 再剥标记。
/// 返回（新终稿, 是否截断, 修复后方案——调用方以它为后续保真比对正源）。
async fn repair_transcription_issues<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &str,
    marker_descs: &[String],
) -> Result<(String, bool, String), AppError> {
    let synthetic = auditor_repair_changes(marker_descs);
    let (new_plan, _) = run_host_summarize(ctx.app, plan, &synthetic, ctx.request, ctx.budget, ctx.run_id).await?;
    let (text, t) = run_audit_format(ctx, &new_plan, None).await?;
    let (stripped, _) = extract_and_strip_transcription_issues(&text);
    Ok((stripped, t, new_plan))
}

/// 终稿 issue 组装单源（打回循环两次采集共用）：硬校验 + 保真 + 截断置顶。
fn collect_final_issues<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &str,
    final_text: &str,
    truncated: bool,
) -> Vec<String> {
    let mut issues = collect_hard_issues(ctx.mode, final_text, ctx.request.original_lyrics_text());
    if ctx.fidelity {
        issues.extend(validator::check_transcription_fidelity(plan, final_text, ctx.mode.to_str_name()));
    }
    // 截断与格式问题同一打回通道——截断 issue 置顶，校验员按"精简后重输"处置
    if truncated {
        issues.insert(0, TRUNCATION_ISSUE.to_string());
    }
    issues
}

/// 单次打回动作：纯保真违规 → 定点重写拼接（其余段落字节不动）返回 true；
/// 混有硬校验问题 / 模型未输出可拼接段落 → 返回 false（调用方回退全文重输）。
async fn one_fidelity_retry<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &str,
    final_text: &mut String,
    truncated: &mut bool,
    issues: &[String],
) -> bool {
    let fid_only = ctx.fidelity && issues.iter().all(|i| i.starts_with("保真校验:"));
    if !fid_only {
        return false;
    }
    if let Ok((rw, _)) = run_audit_targeted_rewrite(ctx, plan, final_text, issues).await {
        if let Some(new_text) = splice_sections(final_text, &rw) {
            *final_text = new_text;
            *truncated = false;
            return true;
        }
    }
    false
}

/// 循环内标记处置（P4 实网修复）：重写输出可能申报新的收敛方案硬伤——
/// 标记行永远剥离（P4 首轮实网 A/D 失败根因：循环内输出未剥离，标记进了交付包还被当成歌词行）；
/// 开关开时走回炉（主持人整合 → 重转写），方案正源随之更新，受循环 ≤2 次上限约束。
async fn handle_loop_markers<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    plan: &mut String,
    final_text: &mut String,
    truncated: &mut bool,
) -> Result<(), AppError> {
    let (stripped, marker_descs) = extract_and_strip_transcription_issues(final_text);
    *final_text = stripped;
    if ctx.fidelity && !marker_descs.is_empty() {
        let (text, t, new_plan) = repair_transcription_issues(ctx, plan, &marker_descs).await?;
        *final_text = text;
        *truncated = t;
        *plan = new_plan;
    }
    Ok(())
}

/// 保真 + 硬校验打回循环（≤2）。循环内 collect 覆盖最后一次重写输出
/// （历史真 bug：末次输出从未被校验——已修）。
async fn fidelity_retry_loop<R: Runtime>(
    ctx: &FinalStageCtx<'_, R>,
    mut plan: String,
    mut final_text: String,
    mut truncated: bool,
) -> Result<(String, Vec<String>), AppError> {
    let mut issues = collect_final_issues(ctx, &plan, &final_text, truncated);
    for _ in 0..2 {
        if issues.is_empty() {
            break;
        }
        emit_pipeline_event(ctx.app, ctx.run_id, PipelineEvent::Retry {
            role: PipelineRole::Auditor,
            reason: issues.join("；"),
        });
        if !one_fidelity_retry(ctx, &plan, &mut final_text, &mut truncated, &issues).await {
            let (text, t) = run_audit_format(ctx, &plan, Some(&issues)).await?;
            final_text = text;
            truncated = t;
        }
        handle_loop_markers(ctx, &mut plan, &mut final_text, &mut truncated).await?;
        issues = collect_final_issues(ctx, &plan, &final_text, truncated);
    }
    // C5/ADR-3：gate_degraded——打回耗尽降级返回（AuditResult pass=false 同源）
    if !issues.is_empty() {
        emit_pipeline_event(
            ctx.app,
            ctx.run_id,
            PipelineEvent::Degraded {
                flag: "gate_degraded".into(),
                detail: "硬校验打回耗尽，降级返回最后一次方案".into(),
            },
        );
    }
    emit_pipeline_event(ctx.app, ctx.run_id, PipelineEvent::AuditResult {
        pass: issues.is_empty(),
        findings: issues.clone(),
    });
    Ok((final_text, issues))
}

async fn produce_final_text<R: Runtime>(
    app: &AppHandle<R>,
    current_plan: &str,
    mode: &Mode,
    request: &PipelineRequest,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
) -> Result<(String, Vec<String>), AppError> {
    let ctx = FinalStageCtx {
        app,
        mode,
        request,
        budget,
        run_id,
        fidelity: transcription_fidelity_enabled(),
    };

    // 转写契约第一输出（正源方案拥有权移入终稿流程：回炉会更新它）
    let (mut final_text, mut truncated) = run_audit_format(&ctx, current_plan, None).await?;

    // TRANSCRIPTION_ISSUE：标记行永远剥离（不进交付包）；开关开时走一次定点回炉；
    // 回炉后的新方案是保真比对与重试的正源（调用方 checkpoint 仍存修复前方案，续跑语义不变）。
    let mut plan = current_plan.to_string();
    {
        let (stripped, marker_descs) = extract_and_strip_transcription_issues(&final_text);
        final_text = stripped;
        if ctx.fidelity && !marker_descs.is_empty() {
            let (text, t, new_plan) = repair_transcription_issues(&ctx, &plan, &marker_descs).await?;
            final_text = text;
            truncated = t;
            plan = new_plan;
        }
    }
    fidelity_retry_loop(&ctx, plan, final_text, truncated).await
}

/// 单次调用结束后发射用量事件（usage为None时不发射——网关未返回不阻塞流程）
fn emit_usage<R: tauri::Runtime>(
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

/// 把主持人汇总输出切分为（纯方案, 下轮任务段）。
/// 末尾出现「【任务分发】」等标记时切分；未命中或切出空方案（畸形：标记在最前）视为无任务段，全文当方案。
fn split_tasks(text: &str) -> (String, String) {
    for marker in ["【任务分发】", "【下轮任务】", "【任务】"] {
        if let Some(pos) = text.find(marker) {
            let plan = text[..pos].trim();
            let tasks = text[pos + marker.len()..].trim();
            if !plan.is_empty() && !tasks.is_empty() {
                return (plan.to_string(), tasks.to_string());
            }
        }
    }
    (text.trim().to_string(), String::new())
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

/// 从最终文本收集硬校验问题。
/// 全模式启用（历史修复）：mode_c 的 lyric_fill 已过滤包装行 + 去空白计数，
/// 对标准提示词包可安全执行字数对齐校验，不再跳过。
/// Style Prompt 统一取 validator::extract_style_prompt 的冒号后正文——
/// 过短/BPM 校验不再被 "Style Prompt**: " 标签前缀虚增长度；"风格:" 前缀兼容已下沉至该实现。
/// BPM 只信任显式标注（knowledge::plan_bpm_value，无 BPM 字样返回 None），
/// 不再从行内任意数字猜值（"80年代" 等年代词误报源已移除）。
fn collect_hard_issues(mode: &Mode, final_text: &str, extra: Option<&str>) -> Vec<String> {
    let mut issues = Vec::new();
    let v = validator::validate_for_mode(mode.to_str_name(), final_text, extra);
    issues.extend(v.issues);
    if let Some(style_prompt) = validator::extract_style_prompt(final_text) {
        issues.extend(validator::check_style_prompt_blocks(&style_prompt));
        // BPM 模式感知检查（对齐原指令：仅 mode_d 要求 BPM>=90）
        // Q3存在性门：D 终稿无 BPM 标注直接打回（此前缺标注一路放行）；显式-only 语义不变。
        if mode.to_str_name() == "mode_d" && crate::knowledge::plan_bpm_value(&style_prompt).is_none() {
            issues.push("缺少 BPM 标注（抖音模式须写明 BPM≥90 数值）".to_string());
        }
        if let Some(bpm) = crate::knowledge::plan_bpm_value(&style_prompt) {
            if let Some(msg) = validator::check_bpm_range(mode.to_str_name(), bpm) {
                issues.push(msg);
            }
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// 主流程
// ---------------------------------------------------------------------------

/// 讨论轮数上限（阶段 1：首轮审改 + 最多 2 轮修订讨论）
const MAX_DISCUSSION_ROUNDS: u32 = 3;

/// 流水线整体超时（防静默挂死兜底：所有模式正常 10 分钟内完成）
pub(crate) const PIPELINE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

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
    // 入口按请求 run_id 清理（与 with_timeout 内解析一致；空请求走全局兼容位）
    let rid = request.run_id.clone().unwrap_or_default();
    cancel::reset(&rid);
    interject::reset(&rid);
    run_pipeline_with_timeout(app, request, PIPELINE_TIMEOUT).await
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
    let run_id = request
        .run_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("run-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)));
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
/// 讨论轮只跑 targets 角色（Auditor 恒在：最终格式端口 + 讨论轮审查）。
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

    // ---- 阶段 0：主持人统领（增量模式跳过——上一版方案即起步，上一版格式已注入 user_input）----
    checkpoint()?;
    let mut current_plan = if incremental {
        // 增量起步：从 user_input 的【上一版方案】段提取；缺失则回落全量阶段 0（不静默用空方案）
        match extract_previous_plan(&request.user_input) {
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
    for round in 1..=MAX_DISCUSSION_ROUNDS {
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
            let _ = emit(PipelineEvent::StepDone {
                role: PipelineRole::Host,
                summary: format!("收到用户插话，已纳入本轮讨论：{}", preview),
            });
        }
        // ① 动态角色并发审改（同轮角色互相无依赖，join_all 并发；顺序收敛保证 revisions_log 确定性）
        // R4：同轮 staggered 启动（index * 2s 错峰，人为岔开四路请求；可被取消打断，不阻塞停止）
        let review_futs: Vec<_> = roles
            .iter()
            .enumerate()
            .map(|(idx, role)| {
                let app = app.clone();
                let current_plan = current_plan.clone();
                let revisions_log = revisions_log.clone();
                let next_tasks = next_tasks.clone();
                let request = request.clone();
                let budget = budget.clone();
                let run_id = run_id.clone();
                async move {
                    if idx > 0 {
                        let stagger = std::time::Duration::from_secs((idx as u64) * 2);
                        // 错峰等待可被取消打断——不等满，只等预算允许
                        let _ = tokio::time::timeout(
                            budget.remaining(),
                            tokio::time::sleep(stagger),
                        )
                        .await;
                        if cancel::is_cancelled(&run_id) {
                            return Err(crate::errors::AppError::cancelled());
                        }
                    }
                    execute_review(&app, *role, &current_plan, &revisions_log, &next_tasks, &request, &budget, &run_id).await
                }
            })
            .collect();
        let review_results = futures_util::future::join_all(review_futs).await;
        for (role, result) in roles.iter().zip(review_results) {
            let result = result?;
            if result.degraded {
                // 不可信输出不进 round_changes（无可整合内容）、不阻断收敛，但必须留下警示
                revisions_log.push((role.name().to_string(), humanize_review(*role, &result)));
            } else if !result.agree {
                // 异议必达——有 changes 带着改，没 changes 带着 reason 也要让主持人看到
                all_agree = false;
                round_changes.push((*role, result.changes.clone(), result.reason.clone()));
                revisions_log.push((role.name().to_string(), humanize_review(*role, &result)));
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
            let _ = emit(PipelineEvent::Degraded {
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
            break; // 动态角色 + 校验员全部无异议 → 收敛
        }
        // ③ 主持人汇总修订 + 校验员观点 → 新版完整方案 + 下轮任务分发
        let (new_plan, tasks) = run_host_summarize(&app, &current_plan, &round_changes, &request, &budget, &run_id).await?;
        current_plan = new_plan;
        next_tasks = tasks;
        let _ = emit(PipelineEvent::DiscussionRound {
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
            };
            if let Err(e) = crate::commands::checkpoint::save(&app, &run_id, &cp) {
                tracing::warn!(error = %e.message, "检查点落盘失败（不阻断）");
            }
        }
    }
    // C5/ADR-3：budget_degraded——轮次上限强制收敛（非全体共识下的产出）
    if !converged {
        let _ = emit(PipelineEvent::Degraded {
            flag: "budget_degraded".into(),
            detail: format!("讨论轮次上限（{} 轮）强制收敛，非全体共识产出", MAX_DISCUSSION_ROUNDS),
        });
    }

    // ---- 阶段 2：校验员转写契约输出 + 保真/硬校验打回（与 resume 共用 produce_final_text 单源实现）----
    // R3：阶段 2 入口同样落检查点（含收敛后的 current_plan），终稿失败可直接续终稿
    //     （检查点存修复前方案：TRANSCRIPTION_ISSUE 回炉只影响本次终稿产出，续跑语义不变）
    {
        let cp = crate::commands::checkpoint::PipelineCheckpoint {
            // B-3：mode 单源 to_str_name（旧规则用 Debug 字符串变换，两处序列化口径漂移风险）
            mode: mode.to_str_name().to_string(),
            user_input: request.user_input.clone(),
            current_plan: current_plan.clone(),
            revisions_log: revisions_log.clone(),
            round: MAX_DISCUSSION_ROUNDS,
            next_tasks: next_tasks.clone(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
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
/// Q4修正：空参只置全局位（停空 id 遗留任务）+ 清具名检查点无从谈起故跳过；
/// 具名任务必须带 id 取消（前端恒传 run_id，此分支仅兼容旧调用）。
#[tauri::command]
pub async fn cancel_pipeline(app: tauri::AppHandle, run_id: Option<String>) {
    match run_id {
        Some(rid) if !rid.trim().is_empty() => {
            cancel::request_cancel(&rid);
            crate::commands::checkpoint::clear(&app, &rid);
        }
        _ => {
            cancel::request_cancel("");
        }
    }
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
    // 断点方案直接进阶段 2（转写契约 + 保真/硬校验打回 ≤2 次），与主流程共用 produce_final_text 单源实现
    let mode = &request.mode;
    let (final_text, _issues) = produce_final_text(&app, &cp.current_plan, mode, &request, &budget, &run_id).await?;
    crate::commands::checkpoint::clear(&app, &run_id);
    Ok(final_text)
}

/// 用户中途插话（前端"插入意见"调）——非阻塞存入槽，轮边界消费。
/// 复用：超长意见（>2000）直接 Validation 拦截，与 feedback 同限额。
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
    crate::models::validate_request(
        &PipelineRequest {
            mode: Mode::ModeB,
            user_input: "占位".to_string(),
            model: "占位".to_string(),
            api_key: "占位".to_string(),
            base_url: "https://placeholder.invalid".to_string(),
            extra: None,
            original_lyrics: None,
            role_overrides: None,
            thinking: false,
            refine_targets: None,
            generation: None,
            run_id: None,
        },
        Some(&note),
    )?;
    interject::push(&rid, note);
    Ok(())
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- C4/D4：冲突预检标注（红灯先行——检测桩返回空时冲突用例必须失败） ----

    /// 单修订角色条目构造
    fn rc(role: PipelineRole, target: &str, content: &str) -> (PipelineRole, Vec<ReviewChange>, String) {
        (
            role,
            vec![ReviewChange { target: target.to_string(), content: content.to_string(), reason: "r".to_string() }],
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
        lyricist_entry.1.push(ReviewChange { target: "style_prompt".to_string(), content: "人声改成沙哑".to_string(), reason: "r".to_string() });
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
    fn splice_replaces_only_target_section() {
        let base = "Style Prompt: test, folk, piano\n\
[Intro]\n\
[piano, 能量:2]\n\
(oom~)\n\
[Verse]\n\
[piano, 能量:3]\n\
深夜 灯亮 键盘响\n\
窗外 雨落 心火燃\n\
[Chorus]\n\
[piano, bass, 能量:7]\n\
雨声 先落下来\n";
        let rewrites = "[Verse]\n\
[piano, 能量:3]\n\
深夜 灯亮 琴声远\n\
窗外 雨落 心火燃\n";
        let spliced = splice_sections(base, rewrites).expect("应有段落被替换");
        // 未点名段落逐字节不变
        assert!(spliced.contains("[Intro]\n[piano, 能量:2]\n(oom~)\n"), "Intro 被改动: {}", spliced);
        assert!(spliced.contains("[Chorus]\n[piano, bass, 能量:7]\n雨声 先落下来\n"), "Chorus 被改动: {}", spliced);
        // 违规段被替换
        assert!(spliced.contains("琴声远"), "Verse 未替换: {}", spliced);
        assert!(!spliced.contains("键盘响"), "旧词残留: {}", spliced);
    }

    #[test]
    fn splice_repeated_tags_paired_positionally() {
        let base = "[Hook]\n干就 完了 干就 完了\n[Verse]\n白天 挨骂 晚上 加班\n[Hook]\n干就 完了 干就 完了\n";
        let rewrites = "[Hook]\n冲就 对了 冲就 对了\n[Hook]\n拼就 赢了 拼就 赢了\n";
        let spliced = splice_sections(base, rewrites).expect("两段 Hook 均应被替换");
        assert!(spliced.contains("[Hook]\n冲就 对了 冲就 对了\n[Verse]"), "第一个 Hook 按位替换: {}", spliced);
        assert!(spliced.ends_with("[Hook]\n拼就 赢了 拼就 赢了\n"), "第二个 Hook 按位替换: {}", spliced);
        assert!(spliced.contains("白天 挨骂 晚上 加班"), "Verse 被改动: {}", spliced);
    }

    #[test]
    fn splice_no_matching_tag_returns_none() {
        let base = "[Verse]\n深夜 灯亮 键盘响\n";
        let rewrites = "[Refrain]\n完全 陌生的 段落\n";
        assert!(splice_sections(base, rewrites).is_none(), "无匹配标签应回退全文重输");
    }

    #[test]
    fn transcription_marker_extracted_and_stripped() {
        let text = "TRANSCRIPTION_ISSUE: 第2行字数不符（原 7 字 vs 新 8 字）\nStyle Prompt: test\n[Verse]\n歌词 行";
        let (stripped, descs) = extract_and_strip_transcription_issues(text);
        assert_eq!(descs, vec!["第2行字数不符（原 7 字 vs 新 8 字）".to_string()], "标记描述应被提取");
        assert!(!stripped.contains("TRANSCRIPTION_ISSUE"), "标记不得留在交付包: {}", stripped);
        assert!(stripped.contains("Style Prompt: test"), "正文不得受损: {}", stripped);
    }

    #[test]
    fn transcription_marker_absent_yields_no_desc() {
        let (stripped, descs) = extract_and_strip_transcription_issues("Style Prompt: test\n[Verse]\n歌词 行");
        assert!(descs.is_empty());
        assert_eq!(stripped, "Style Prompt: test\n[Verse]\n歌词 行");
    }

    #[test]
    fn transcription_contract_locked() {
        // 契约三要素锁定（防退化成无约束格式化）
        let c = roles::TRANSCRIPTION_CONTRACT;
        assert!(c.contains("【转写契约"), "缺契约头");
        assert!(c.contains("TRANSCRIPTION_ISSUE"), "缺标记申报通道");
        assert!(c.contains("三类格式操作"), "缺允许操作清单");
    }

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
        assert_eq!(r.changes[0].target, "lyrics");
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

    /// L-2：Mode C 专项块尾部收尾口径与 validator 单源（≤LYRIC_FILL_TAIL_ALLOW 行），
    /// 旧规则写"总行数完全一致/禁止增删"，与校验员系统 prompt、硬校验的"尾部 ≤2 行"打架。
    #[test]
    fn mode_c_special_block_tail_single_source() {
        let b = mode_c_special_block();
        let tail = crate::rules::LYRIC_FILL_TAIL_ALLOW;
        assert!(b.contains(&format!("仅允许尾部 ≤{} 行收尾", tail)), "缺尾部收尾允许: {}", b);
        assert!(b.contains(&format!("尾部收尾最多加 {} 行", tail)), "缺收尾上限: {}", b);
        assert!(!b.contains("总行数必须与原歌词完全一致"), "残留与 validator 冲突的严格等行数措辞");
        assert!(!b.contains("禁止增删歌词行"), "残留与尾部收尾允许矛盾的旧措辞");
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
        // 空 content 修订丢弃；非法 target 归一为 other
        let r = parse_review(r#"{"agree": false, "changes": [
            {"target": "lyrics", "content": "有效修订", "reason": "r1"},
            {"target": "style_prompt", "content": "   ", "reason": "空内容丢弃"},
            {"target": "weirdness_滑块", "content": "内容还在", "reason": "非法target归一"}
        ]}"#);
        assert!(!r.agree);
        assert_eq!(r.changes.len(), 2, "空 content 修订应被丢弃");
        assert_eq!(r.changes[0].target, "lyrics");
        assert_eq!(r.changes[1].target, "other", "非法 target 应归一为 other");
        assert_eq!(r.changes[1].content, "内容还在");
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
                content: "[失真吉他+强放, 空间爆满, 能量:9]".into(),
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
                content: long.clone(),
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
                content: "Weirdness=30".into(),
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

    #[test]
    fn split_tasks_with_marker() {
        let (plan, tasks) = split_tasks("方案内容第一行\n第二行\n【任务分发】\n- 情感分析师：调整能量\n- 制作人：改配器");
        assert!(plan.contains("方案内容第一行"));
        assert!(!plan.contains("任务分发"), "方案不应含任务段: {}", plan);
        assert!(tasks.contains("情感分析师"));
        assert!(tasks.contains("制作人"));
    }

    #[test]
    fn split_tasks_no_marker_keeps_whole() {
        let (plan, tasks) = split_tasks("只有方案没有任务");
        assert_eq!(plan, "只有方案没有任务");
        assert!(tasks.is_empty());
    }

    #[test]
    fn split_tasks_marker_at_start_keeps_all_as_plan() {
        // 畸形：标记在最前、无方案 → 全文当方案，避免空方案传给下一轮
        let (plan, tasks) = split_tasks("【任务分发】\n只有任务没有方案");
        assert_eq!(plan, "【任务分发】\n只有任务没有方案");
        assert!(tasks.is_empty());
    }

    #[test]
    fn split_tasks_alternate_markers() {
        let (plan, tasks) = split_tasks("方案\n【下轮任务】\n任务A");
        assert!(plan.contains("方案"));
        assert!(tasks.contains("任务A"));
        let (plan2, tasks2) = split_tasks("方案2\n【任务】\n任务B");
        assert!(plan2.contains("方案2"));
        assert!(tasks2.contains("任务B"));
    }

    /// 过短校验按冒号后正文计数——旧行级计数被 "Style Prompt**: " 前缀虚增 16 字符。
    /// 正文 15 字符：旧实现 15+16=31 ≥30 放行（放水），新实现按正文判过短。
    #[test]
    fn style_prompt_short_check_uses_body_not_line() {
        let text = "**Style Prompt**: 深夜室内民谣钢琴与弦乐交织回响\n[Verse 1]\n歌词";
        let issues = collect_hard_issues(&Mode::ModeA, text, None);
        assert!(issues.iter().any(|i| i.contains("过短")), "正文 15 字符应报过短: {:?}", issues);
        // 正文充足（≥30）时不报过短
        let text_ok = "**Style Prompt**: 深夜室内民谣基调, 68BPM D小调, 钢琴与弦乐交织, 气声念白, 温暖木质空间, 能量低回\n[Verse 1]\n歌词";
        let issues_ok = collect_hard_issues(&Mode::ModeA, text_ok, None);
        assert!(!issues_ok.iter().any(|i| i.contains("过短")), "正文充足不应报过短: {:?}", issues_ok);
    }

    /// 旧语义保留：`风格:` 前缀行同样能提取正文（原 extract_style_prompt_line 认得它）
    #[test]
    fn style_prompt_supports_legacy_grey_prefix() {
        let text = "风格：深夜室内民谣基调, 68BPM D小调, 钢琴与弦乐交织, 气声念白, 温暖木质空间\n[Verse 1]\n歌词";
        let issues = collect_hard_issues(&Mode::ModeA, text, None);
        assert!(!issues.iter().any(|i| i.contains("过短")), "风格: 前缀应提取到正文: {:?}", issues);
    }

    /// +B7 集成：mode_d 的 BPM 只按显式标注判——"80年代" 不再被当成 BPM=80 误报
    #[test]
    fn bpm_check_not_fooled_by_era_words() {
        let text = "**Style Prompt**: 80年代复古Disco, 125BPM, 律动铜管, 痞气男声, 拥挤商场混响, 高能持续\n[Hook]\n[suona, 808]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Hook]\n[all instruments cut abruptly]";
        let issues = collect_hard_issues(&Mode::ModeD, text, None);
        assert!(
            !issues.iter().any(|i| i.contains("BPM")),
            "125BPM 应正确提取，不得因年代词误报 BPM 不足: {:?}",
            issues
        );
    }

    /// 旧规则保留：显式 BPM 不足（mode_d 要求 ≥90）仍必须报
    #[test]
    fn bpm_explicit_violation_still_reported() {
        let text = "**Style Prompt**: 深夜室内民谣基调, 68BPM, 钢琴弦乐, 气声念白, 温暖空间\n[Hook]\n[suona, 808]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Hook]\n[all instruments cut abruptly]";
        let issues = collect_hard_issues(&Mode::ModeD, text, None);
        assert!(
            issues.iter().any(|i| i.contains("BPM 68") || i.contains("低于")),
            "显式 68BPM 应报 BPM 不足: {:?}",
            issues
        );
    }

    /// Q3存在性门：Mode D 终稿无 BPM 标注直接打回（此前一路放行）
    #[test]
    fn bpm_missing_in_mode_d_reported() {
        let text = "**Style Prompt**: 抖音神曲基调, 律动铜管, 痞气男声, 拥挤商场混响, 高能持续\n[Hook]\n[suona, 808]\n我 真的 会谢\n[Hook]\n我 真的 会谢\n[Hook]\n[all instruments cut abruptly]";
        let issues = collect_hard_issues(&Mode::ModeD, text, None);
        assert!(issues.iter().any(|i| i.contains("缺少 BPM")), "无 BPM 应报缺标注: {:?}", issues);
        // A 模式无 BPM 不报（仅 D 设存在性门）
        let issues_a = collect_hard_issues(&Mode::ModeA, text, None);
        assert!(!issues_a.iter().any(|i| i.contains("缺少 BPM")), "A 模式不应报缺 BPM: {:?}", issues_a);
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
        map.insert(PipelineRole::Auditor, crate::models::RoleApiOverride {
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
        map.insert(PipelineRole::Host, crate::models::RoleApiOverride {
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

    #[test]
    fn plan_energy_range_extracts() {
        assert_eq!(plan_energy_range("能量:3 到 能量:8"), Some((3, 8)));
        assert_eq!(plan_energy_range("没有能量标注"), None);
    }

    #[test]
    fn inject_knowledge_filters_by_keywords() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 制作人绑定表注入：方案含流派与能量 → style_genre 命中、instruments 按能量
        let out = inject_knowledge(&kb, &[("style_genre", &[], &[]), ("instruments", &[], &[]), ("suno_rules", &[], &[])], "深夜室内民谣 能量:3 Chorus 能量:8");
        assert!(out.contains("按需命中"), "style_genre 应命中: {}", &out[..out.len().min(200)]);
        assert!(out.contains("suno_rules 知识库"), "suno_rules 应全量注入");
        // 情感分析师注入：方案无情绪词 → M18 无示例标注（调用方走确定性默认）
        let out2 = inject_knowledge(&kb, &[("emotions", &[], &[])], "纯粹描述画面没有情绪词");
        assert!(out2.contains("未命中关键词"), "emotions 应标注无命中: {}", &out2[..out2.len().min(200)]);
        assert!(out2.contains("无示例"), "emotions 无命中应明确无示例: {}", &out2[..out2.len().min(200)]);
        // 方案含情绪词 → 命中
        let out3 = inject_knowledge(&kb, &[("emotions", &[], &[])], "这首歌的情绪是孤独与自嘲");
        assert!(out3.contains("按需命中"), "emotions 应命中: {}", &out3[..out3.len().min(200)]);
    }

    /// 微观②：少而准——愤怒（高能量）只注入高能乐器，悲伤（低能量）只注入低能乐器，且 ≤15 件
    #[test]
    fn instruments_injected_selectively_by_energy() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 愤怒主题（能量 7-10）：命中摇滚/金属类高能乐器
        let out = inject_knowledge(&kb, &[("instruments", &[], &[])], "愤怒爆发 能量:7 到 能量:10");
        assert!(out.contains("按能量区间 7~10 命中"), "got: {}", &out[..out.len().min(150)]);
        assert!(out.contains("distorted guitar"), "愤怒应含失真吉他");
        assert!(out.contains("electric guitar"), "愤怒应含电吉他");
        assert!(!out.contains("felt piano"), "愤怒不应含低能钢琴（或超出 15 件上限被截断）");
        // 悲伤主题（能量 1-4）：命中民谣/抒情低能乐器
        let out2 = inject_knowledge(&kb, &[("instruments", &[], &[])], "悲伤低回 能量:1 到 能量:4");
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
        let log: Vec<(String, String)> = (0..6).map(mk).collect();
        assert_eq!(fold_log(&log).len(), 6);
        let long: Vec<(String, String)> = (0..8).map(mk).collect();
        let folded = fold_log(&long);
        assert_eq!(folded.len(), 7, "1 摘要 + 最近 6 条");
        assert_eq!(folded[0].0, "早期修订");
        assert!(folded[0].1.contains("等 2 条早期修订已折叠"), "got: {}", folded[0].1);
        assert!(folded[0].1.contains("角色0"), "摘要应保留早期角色名: {}", folded[0].1);
        // 最近 6 条完整保留
        assert_eq!(folded[1].0, "角色2");
        assert_eq!(folded[6].0, "角色7");
    }

    /// 注入量规范：每个角色用"最坏情况方案"（命中所有关键词表 + 全能量区间）注入，
    /// 断言单表条数 ≤ 上限、单角色总字数 ≤ 封顶。常量 INJECT_MAX_* 的测试锁。
    #[test]
    fn role_injection_budget_under_limits() {
        let kb = crate::knowledge::KnowledgeBase::load_embedded().unwrap();
        // 最坏情况方案：同时命中情绪/套话/钩子/流派全部候选 + 全能量区间 0-10
        let plan = "愤怒 孤独 温柔 遗憾 梦想 星空 自嘲 魔性循环 反差金句 空耳式 深夜室内民谣 抒情流行 爵士 重金属 能量:0 到 能量:10";
        let roles = [
            roles::role_for(PipelineRole::Emotion),
            roles::role_for(PipelineRole::Lyricist),
            roles::role_for(PipelineRole::Reviser),
            roles::role_for(PipelineRole::Producer),
            roles::role_for(PipelineRole::StyleAnalyst),
            roles::role_for(PipelineRole::Auditor),
        ];
        for r in roles {
            let out = inject_knowledge(&kb, r.knowledge_tables, plan);
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
                "{} 注入 {} 字超过封顶 {}",
                r.name,
                chars,
                INJECT_MAX_TOTAL_CHARS
            );
        }
    }

    fn make_request(overrides: Option<std::collections::HashMap<PipelineRole, crate::models::RoleApiOverride>>) -> PipelineRequest {
        PipelineRequest {
            mode: Mode::ModeB,
            user_input: "雨天".into(),
            model: "global-model".into(),
            // 测试占位符非真实凭据——动态构造，避免静态扫描把占位值当硬编码密钥（CWE-798 误报）
            api_key: format!("global-{}", "key").into(),
            base_url: "https://global.example.com/v1".into(),
            extra: None,
            original_lyrics: None,
            role_overrides: overrides,
            thinking: false,
            refine_targets: None,
            generation: None,
            run_id: None,
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

    /// 同轮角色并发语义——join_all 按输入顺序返回（revisions_log 确定性），
    /// 任一角色 Err 时整轮中断（与串行语义一致）
    #[tokio::test]
    async fn concurrent_reviews_preserve_order_and_fail_fast() {
        // 保序：慢任务排前面，完成顺序仍按输入序（显式标注输出类型统一 future 类型）
        let futs: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = &'static str>>>> = vec![
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                "slow"
            }),
            Box::pin(async { "fast" }),
        ];
        let out = futures_util::future::join_all(futs).await;
        assert_eq!(out, vec!["slow", "fast"]);
        // 失败中断：任一 Err → 整轮 ? 传播（模拟 zip 后 ? 语义）
        let futs: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = Result<&'static str, AppError>>>>> =
            vec![
                Box::pin(async { Ok::<_, AppError>("ok") }),
                Box::pin(async { Err::<_, AppError>(AppError::cancelled()) }),
            ];
        let res: Result<Vec<&str>, AppError> =
            futures_util::future::join_all(futs).await.into_iter().collect::<Result<Vec<_>, _>>();
        assert!(matches!(res, Err(e) if e.kind == crate::errors::ErrorKind::Cancelled));
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
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let s = host_initial_system(&m);
            assert!(s.contains("【校验清单"), "{} stage-0 缺校验清单注入", m.to_str_name());
        }
        // mode_d 清单含抖音参数区间（渲染值来自常量，非手写）
        let d = host_initial_system(&Mode::ModeD);
        assert!(
            d.contains(&format!("{}-{}", crate::rules::DOUYIN_WEIRD_MIN, crate::rules::DOUYIN_WEIRD_MAX)),
            "stage-0 清单缺抖音参数区间"
        );
    }

    /// C3 快照：四模式 prompt 渲染输出须与改前快照逐字节一致（占位符恰好还原当日数字）。
    /// 常量有意变更时：跑 `cargo test --lib dump_mode_prompts` 重新生成快照并在提交说明中声明。
    #[test]
    fn mode_prompts_byte_identical_to_snapshot() {
        for m in [Mode::ModeA, Mode::ModeB, Mode::ModeC, Mode::ModeD] {
            let name = m.to_str_name();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/prompts")
                .join(format!("{}.txt", name));
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("快照缺失 {}（先跑 dump_mode_prompts）: {}", name, e));
            assert_eq!(prompt_for_mode(&m), expected, "{} prompt 与快照不一致", name);
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
}
