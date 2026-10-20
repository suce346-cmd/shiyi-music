//! P1-3/4：结构化 PlanIR/SongPlan 编排辅助（Host 初稿 / 角色 patch / 汇总 / Auditor 定稿）。
//! 产出口只走 `call_llm_structured`；终稿硬门只走 `plan::validate`；交付包只走 `plan::render_suno`。

use crate::commands::llm::{self, StructuredCall};
use crate::errors::{AppError, ErrorKind};
use crate::llm_caps::{self, StructuredMode};
use crate::models::{
    GenerationConfig, PhaseKind, PipelineEnvelope, PipelineEvent, PipelineRequest, PipelineRole,
};
use crate::plan::render_suno::{self, Rendered};
use crate::plan::schema::{self, PLAN_IR_SCHEMA_NAME, SONG_PLAN_SCHEMA_NAME};
use crate::plan::validate::{self, Issue, ValidateCtx};
use crate::plan::{PlanIr, SongPlan};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Runtime};

/// 主持人汇总结构化输出：完整方案 + 下轮任务（空 = 已收敛）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostSummary {
    pub plan: SongPlan,
    /// 下轮任务分发；空字符串表示无需继续讨论
    pub next_tasks: String,
}

/// 审改 JSON（与 roles REVIEW_SCHEMA 同形；content 为任意 JSON 替换值）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewOut {
    pub agree: bool,
    #[serde(default)]
    pub checked: Vec<String>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub changes: Vec<ReviewPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewPatch {
    /// SongPlan JSON Pointer（如 `/sections/0/instruments`）
    pub target: String,
    /// 该字段替换后的完整 JSON 值
    pub content: Value,
    #[serde(default)]
    pub reason: String,
}

pub fn review_schema() -> Value {
    let mut v = serde_json::to_value(schemars::schema_for!(ReviewOut)).expect("review schema");
    // 与 plan schema 同口径：strict 后处理（剥 description/format、补 required/additionalProperties）
    crate::plan::schema::strictify(&mut v);
    v
}

pub fn host_summary_schema() -> Value {
    let mut v = serde_json::to_value(schemars::schema_for!(HostSummary)).expect("summary schema");
    crate::plan::schema::strictify(&mut v);
    v
}

/// 解析角色结构化档位：覆盖 > structured_caps 缓存 > 默认 json_object。
pub fn resolve_structured_mode(req: &PipelineRequest, role: PipelineRole, base_url: &str, model: &str) -> StructuredMode {
    let explicit = req
        .role_overrides
        .as_ref()
        .and_then(|m| m.get(&role))
        .and_then(|ov| ov.structured_mode);
    llm_caps::resolve_mode(explicit, req.structured_caps.as_ref(), base_url, model)
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
    role: PipelineRole,
    phase: PhaseKind,
    preview: Option<String>,
) {
    let _ = app.emit(
        "pipeline",
        PipelineEnvelope::new(run_id.to_string(), PipelineEvent::PhaseProgress { role, phase, preview }),
    );
}

fn emit_audit_result<R: Runtime>(app: &AppHandle<R>, run_id: &str, issues: &[Issue]) {
    let pass = validate::passed(issues);
    let findings: Vec<String> = issues.iter().map(|i| i.line()).collect();
    let _ = app.emit(
        "pipeline",
        PipelineEnvelope::new(run_id.to_string(), PipelineEvent::AuditResult { pass, findings }),
    );
}

fn structured_call<'a>(
    base_url: &'a str,
    api_key: &'a str,
    model: &'a str,
    gen: &'a GenerationConfig,
    run_id: &'a str,
    reserve: std::time::Duration,
    mode: StructuredMode,
    schema_name: &'a str,
    schema: &'a Value,
) -> StructuredCall<'a> {
    StructuredCall {
        base_url,
        api_key,
        model,
        max_tokens: llm::MAX_TOKENS_CAP,
        gen,
        run_id,
        reserve,
        mode,
        schema_name,
        schema,
    }
}

/// SongPlan / PlanIR → 检查点字符串（JSON，紧凑）。
pub fn plan_to_checkpoint(plan: &SongPlan) -> String {
    serde_json::to_string(plan).unwrap_or_else(|_| "{}".into())
}

/// 检查点 / 讨论输入 → SongPlan（不合规则 Parse）。
pub fn plan_from_checkpoint(raw: &str) -> Result<SongPlan, AppError> {
    let v: Value = serde_json::from_str(raw.trim()).map_err(|e| {
        AppError::new(ErrorKind::Parse, format!("方案不是合法 JSON（结构化管线要求 SongPlan）: {e}"))
    })?;
    // PlanIr 透明包装与 SongPlan 同形
    schema::parse_song_plan(&v).map_err(|errs| {
        AppError::new(
            ErrorKind::Parse,
            format!(
                "方案未通过 SongPlan schema: {}",
                errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
            ),
        )
    })
}

/// Host 阶段 0：结构化 PlanIR。
pub async fn host_initial_plan<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    system: String,
    user: String,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<SongPlan, AppError> {
    let gen = req.generation.clone().unwrap_or_default();
    let mode = resolve_structured_mode(req, PipelineRole::Host, base_url, model);
    let schema = schema::plan_ir_schema();
    emit_progress(app, run_id, PipelineRole::Host, PhaseKind::Structuring, None);
    let call = structured_call(
        base_url,
        api_key,
        model,
        &gen,
        run_id,
        llm::FINAL_STAGE_RESERVE,
        mode,
        PLAN_IR_SCHEMA_NAME,
        &schema,
    );
    let messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
    let hook = super::orchestrator::backoff_hook(app.clone(), run_id.to_string());
    let (ir, resp): (PlanIr, _) = llm::call_llm_structured(&call, messages, budget, Some(&hook)).await?;
    super::orchestrator::emit_usage(app, PipelineRole::Host, &resp, run_id);
    let preview = Some(format!("title={:?} sections={}", ir.0.title, ir.0.sections.len()));
    emit_progress(app, run_id, PipelineRole::Host, PhaseKind::Parsed, preview);
    Ok(ir.0)
}

/// Host 汇总：结构化 HostSummary。
pub async fn host_summarize_plan<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    system: String,
    user: String,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<(SongPlan, String), AppError> {
    let gen = req.generation.clone().unwrap_or_default();
    let mode = resolve_structured_mode(req, PipelineRole::Host, base_url, model);
    let schema = host_summary_schema();
    emit_progress(app, run_id, PipelineRole::Host, PhaseKind::Structuring, None);
    let call = structured_call(
        base_url,
        api_key,
        model,
        &gen,
        run_id,
        llm::FINAL_STAGE_RESERVE,
        mode,
        "host_summary",
        &schema,
    );
    let messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
    let hook = super::orchestrator::backoff_hook(app.clone(), run_id.to_string());
    let (summary, resp): (HostSummary, _) = llm::call_llm_structured(&call, messages, budget, Some(&hook)).await?;
    super::orchestrator::emit_usage(app, PipelineRole::Host, &resp, run_id);
    emit_progress(
        app,
        run_id,
        PipelineRole::Host,
        PhaseKind::Parsed,
        Some(format!("sections={} tasks_empty={}", summary.plan.sections.len(), summary.next_tasks.is_empty())),
    );
    Ok((summary.plan, summary.next_tasks.trim().to_string()))
}

/// 角色审改：结构化 ReviewOut。
pub async fn role_review_structured<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    role: PipelineRole,
    system: String,
    user: String,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<(ReviewOut, crate::models::LLMResponse), AppError> {
    let gen = req.generation.clone().unwrap_or_default();
    let mode = resolve_structured_mode(req, role, base_url, model);
    let schema = review_schema();
    emit_progress(app, run_id, role, PhaseKind::Structuring, None);
    let call = structured_call(
        base_url,
        api_key,
        model,
        &gen,
        run_id,
        llm::FINAL_STAGE_RESERVE,
        mode,
        "review_out",
        &schema,
    );
    let messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
    let hook = super::orchestrator::backoff_hook(app.clone(), run_id.to_string());
    let (out, resp) = llm::call_llm_structured::<ReviewOut>(&call, messages, budget, Some(&hook)).await?;
    emit_progress(
        app,
        run_id,
        role,
        PhaseKind::Parsed,
        Some(format!("agree={} changes={}", out.agree, out.changes.len())),
    );
    Ok((out, resp))
}

/// Auditor 定稿：结构化 SongPlan → validate（≤2 次带问题重试）→ render → AuditResult。
pub async fn auditor_finalize<R: Runtime>(
    app: &AppHandle<R>,
    req: &PipelineRequest,
    converged: &SongPlan,
    budget: &crate::budget::SharedBudget,
    run_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    system: String,
) -> Result<(String, Vec<String>, SongPlan), AppError> {
    let gen = req.generation.clone().unwrap_or_default();
    let mode = resolve_structured_mode(req, PipelineRole::Auditor, base_url, model);
    let schema = schema::song_plan_schema();
    let mode_name = req.mode.to_str_name();
    let original = req.original_lyrics_text();

    let mut plan = converged.clone();
    let ctx0 = ValidateCtx {
        mode: mode_name,
        original_lyrics: original,
        converged: Some(converged),
    };
    let mut last_issues = validate::validate(&plan, &ctx0);
    if validate::passed(&last_issues) {
        emit_audit_result(app, run_id, &last_issues);
        let rendered = render_suno::render(&plan);
        return Ok((package_text(&rendered), Vec::new(), plan));
    }

    for attempt in 0..2u32 {
        emit_progress(app, run_id, PipelineRole::Auditor, PhaseKind::Structuring, None);
        let _ = app.emit(
            "pipeline",
            PipelineEnvelope::new(run_id.to_string(), PipelineEvent::AuditStart),
        );
        let issue_lines: Vec<String> = last_issues.iter().map(|i| i.line()).collect();
        let user = format!(
            "【收敛方案（SongPlan JSON）】\n{}\n\n【硬校验问题（逐条修正后输出完整 SongPlan）】\n{}\n\n请输出修正后的完整 SongPlan JSON（不得省略字段）。",
            plan_to_checkpoint(&plan),
            issue_lines.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
        );
        let call = structured_call(
            base_url,
            api_key,
            model,
            &gen,
            run_id,
            std::time::Duration::ZERO,
            mode,
            SONG_PLAN_SCHEMA_NAME,
            &schema,
        );
        let messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
        let hook = super::orchestrator::backoff_hook(app.clone(), run_id.to_string());
        let (next, resp) = llm::call_llm_structured::<SongPlan>(&call, messages, budget, Some(&hook)).await?;
        super::orchestrator::emit_usage(app, PipelineRole::Auditor, &resp, run_id);
        plan = next;
        emit_progress(
            app,
            run_id,
            PipelineRole::Auditor,
            PhaseKind::Parsed,
            Some(format!("attempt={} sections={}", attempt + 1, plan.sections.len())),
        );
        let ctx = ValidateCtx {
            mode: mode_name,
            original_lyrics: original,
            converged: Some(converged),
        };
        last_issues = validate::validate(&plan, &ctx);
        if validate::passed(&last_issues) {
            break;
        }
        let _ = app.emit(
            "pipeline",
            PipelineEnvelope::new(
                run_id.to_string(),
                PipelineEvent::Retry {
                    role: PipelineRole::Auditor,
                    reason: format!("硬校验未过（第 {} 次）", attempt + 1),
                },
            ),
        );
    }

    emit_audit_result(app, run_id, &last_issues);
    let findings: Vec<String> = last_issues.iter().map(|i| i.line()).collect();
    let rendered = render_suno::render(&plan);
    Ok((package_text(&rendered), findings, plan))
}


fn package_text(r: &Rendered) -> String {
    render_suno::to_package_text(r)
}

/// 把 content 写入 SongPlan JSON 的 pointer 处（host 汇总前可选用；失败则原样保留）。
pub fn apply_pointer(plan: &mut Value, pointer: &str, content: Value) -> Result<(), String> {
    if !pointer.starts_with('/') {
        return Err(format!("target 必须是 JSON Pointer（以 / 开头）: {pointer}"));
    }
    if pointer == "/" {
        *plan = content;
        return Ok(());
    }
    let mut cur = plan;
    let parts: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
    let (last, parents) = parts.split_last().ok_or_else(|| "空 pointer".to_string())?;
    for p in parents {
        let key = unescape_pointer_token(p);
        cur = match cur {
            Value::Object(map) => map
                .entry(key)
                .or_insert(Value::Null),
            Value::Array(arr) => {
                let idx: usize = key.parse().map_err(|_| format!("数组下标非法: {key}"))?;
                while arr.len() <= idx {
                    arr.push(Value::Null);
                }
                &mut arr[idx]
            }
            _ => return Err(format!("无法沿 pointer 下行: {pointer}")),
        };
    }
    let key = unescape_pointer_token(last);
    match cur {
        Value::Object(map) => {
            map.insert(key, content);
        }
        Value::Array(arr) => {
            let idx: usize = key.parse().map_err(|_| format!("数组下标非法: {key}"))?;
            while arr.len() <= idx {
                arr.push(Value::Null);
            }
            arr[idx] = content;
        }
        _ => return Err(format!("pointer 终点不可写: {pointer}")),
    }
    Ok(())
}

fn unescape_pointer_token(t: &str) -> String {
    t.replace("~1", "/").replace("~0", "~")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::fixtures::minimal_plan;

    #[test]
    fn apply_pointer_replaces_array_field() {
        let plan = minimal_plan();
        let mut v = serde_json::to_value(&plan).unwrap();
        apply_pointer(&mut v, "/global/instruments", json!(["piano", "bass"])).unwrap();
        let back: SongPlan = serde_json::from_value(v).unwrap();
        assert_eq!(back.global.instruments, vec!["piano", "bass"]);
    }

    #[test]
    fn checkpoint_roundtrip() {
        let p = minimal_plan();
        let s = plan_to_checkpoint(&p);
        let back = plan_from_checkpoint(&s).unwrap();
        assert_eq!(back.sections.len(), p.sections.len());
    }

    #[test]
    fn review_schema_is_object() {
        let s = review_schema();
        assert_eq!(s["type"], "object");
        assert!(s["properties"]["changes"].is_object() || s["properties"]["changes"]["type"] == "array");
    }
}
