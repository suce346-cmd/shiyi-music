use crate::budget::SharedBudget;
use crate::errors::{AppError, ErrorKind};
use crate::models::{LLMResponse, TokenUsage};
#[cfg(test)]
use crate::models::StreamChunk;
#[cfg(test)]
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
#[cfg(test)]
use tauri::{AppHandle, Emitter, Runtime};

/// 截断错误响应体（防敏感信息/超长回显，历史修复）
pub(crate) fn truncate_err(s: &str) -> String {
    const MAX: usize = 300;
    if s.chars().count() > MAX {
        s.chars().take(MAX).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// 退避原因（wire format 单源）：与前端 `BackoffReason` 逐字对应（事件 `reason` 字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackoffReason {
    /// 429 限流
    RateLimit,
    /// 5xx 服务端错误
    ServerError,
    /// 网络错误（连接失败/超时/断流）
    Network,
}

impl BackoffReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            BackoffReason::RateLimit => "rate_limit",
            BackoffReason::ServerError => "server_error",
            BackoffReason::Network => "network",
        }
    }
}

/// 退避计划：等待秒数 + 原因。**单源**——两者由同一次判定产出；
/// 若拆成两个 match，改了一处忘了另一处就会冒出"等了 30s 却报网络错误"的错配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RetryPlan {
    wait_secs: u64,
    reason: BackoffReason,
}

/// 重试决策（纯函数，可测）：返回本次退避计划（None = 不重试）。
/// - 429 限流：30s/60s 退避（API 配额恢复后自动续跑）
/// - 5xx 服务端错误：15s/30s 退避（瞬时故障，重试通常可恢复）
/// - 网络错误（连接失败/超时）：10s/20s 退避（断网恢复后重试）
/// - 其他 4xx（400/401/403 等凭据或请求问题）：不可重试，重试无意义
fn retry_plan(attempt: usize, status: Option<u16>, network_err: bool) -> Option<RetryPlan> {
    let plan = |wait_secs: u64, reason: BackoffReason| Some(RetryPlan { wait_secs, reason });
    match (status, network_err) {
        (Some(429), _) => plan(if attempt == 0 { 30 } else { 60 }, BackoffReason::RateLimit),
        (Some(s), _) if s >= 500 => plan(if attempt == 0 { 15 } else { 30 }, BackoffReason::ServerError),
        (None, true) => plan(if attempt == 0 { 10 } else { 20 }, BackoffReason::Network),
        _ => None, // 4xx 或其他：不重试
    }
}

/// 重试耗尽的终态类别（**单源**：复用 `ErrorKind::for_status`）：
/// 429 → RateLimit，5xx/其他 → Network；全程无响应（网络错误）→ Network。
/// 旧规则一律报 Network——429 走到这里被报成"网络错误"，排障方向被误导（#27-c 一并修复）。
fn terminal_kind(last_status: Option<u16>) -> ErrorKind {
    match last_status {
        Some(s) => ErrorKind::for_status(s),
        None => ErrorKind::Network,
    }
}

/// 退避播报（进/出）：本层只陈述"发生了什么"，转成什么 UI 事件由上层决定
/// （本层不依赖 Tauri，故不直接 emit）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackoffNotice {
    /// 进入退避等待：wait_secs **已含抖动**（前端倒计时应显示实际等待时长）
    Start { attempt: u32, wait_secs: u64, reason: BackoffReason },
    /// 退避结束、即将发起下一次尝试
    End { attempt: u32 },
}

/// 退避播报钩子（可空）：无 UI 场景（测试连接 / 单测）传 None。
/// 用 trait object 而非泛型——`None` 处无需标注具体类型，调用点更轻。
pub(crate) type BackoffHook<'a> = &'a (dyn Fn(BackoffNotice) + Send + Sync);

/// R4：退避抖动 0–5 秒（无 rand 依赖，用纳秒取模；纯函数阈值不动，抖动包在外面）。
/// 429 错峰重发，避免同秒齐射撞出第二波 60s。
///
/// #14（第八批）更正注记：原设计前提是"四角色**并发**同时 429"（讨论轮 join_all）。
/// 讨论轮已改阵容序串行，同轮至多一路请求，该前提不再成立；但抖动机制本身仍覆盖
/// 其他并发源（终稿流式与重试重叠、跨 run 边界），故保留不动——**只更正前提，不改行为**。
fn backoff_jitter_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 6)
        .unwrap_or(0)
}

/// 终稿保底额度（R1）：讨论轮内每次尝试/等待最多花掉 remaining - 该值，
/// 保证阶段 2（run_audit_format）至少有一次完整尝试 + 30s 退避的额度。
/// 阶段 2 入口传 reserve=ZERO 即解除约束，全额使用剩余预算。
pub(crate) const FINAL_STAGE_RESERVE: std::time::Duration = std::time::Duration::from_secs(120);

/// 退避等待（HTTP 错误与网络错误**唯一共用路径**）：播报进入 → 等待（可按保底截断）→ 播报结束。
/// 旧规则两条分支各写一份等待/截断逻辑（同一规则两处实现，改一处必漂移）；
/// 收口后新增播报只需改这一处。返回 Err = 等待被预算打断或保底截断，调用方直接中止本轮。
async fn apply_backoff(
    plan: RetryPlan,
    attempt: usize,
    budget: &SharedBudget,
    reserve: std::time::Duration,
    notify: Option<BackoffHook<'_>>,
) -> Result<(), AppError> {
    // R4：计划等待 + 0–5s 抖动错峰（阈值不动，离散包在外面；#14 后讨论轮串行，
    // 抖动仍覆盖终稿/跨 run 等其他并发源）
    let planned = plan.wait_secs + backoff_jitter_secs();
    tracing::warn!(wait_secs = planned, attempt = attempt + 1, reason = plan.reason.as_str(), "LLM 请求失败，退避重试");
    if let Some(n) = notify {
        n(BackoffNotice::Start { attempt: (attempt + 1) as u32, wait_secs: planned, reason: plan.reason });
    }
    // 退避等待可被预算到期中断——不等满，只等到 deadline
    // R1：讨论轮等待上限为 remaining - 保底，不等满时按保底直接进终稿
    let wait_dur = std::time::Duration::from_secs(planned).min(budget.remaining_for_discussion(reserve));
    if tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_dur)).await.is_err() {
        return Err(AppError::new(ErrorKind::Timeout, budget_stop_msg(reserve, "等待重试中止")));
    }
    // 保底截断了等待：不等满直接中止本轮，不再消耗（Q3：此前文案写“转入终稿”，
    // 实际讨论轮 Timeout 经 .await? 直接中止整线，无跳终稿分支；文案如实改中止）。
    if wait_dur < std::time::Duration::from_secs(planned) {
        return Err(AppError::new(
            ErrorKind::Timeout,
            budget_stop_msg(reserve, &format!("退避被保底截断（已等 {:?}，计划 {}s），中止本轮", wait_dur, planned)),
        ));
    }
    if let Some(n) = notify {
        n(BackoffNotice::End { attempt: (attempt + 1) as u32 });
    }
    Ok(())
}

/// 带退避重试的请求发送：429/5xx/网络错误按 retry_plan 退避，最多 3 次（4xx 凭据类错误不重试）。
/// 错误分类——reqwest 层失败=Network；重试耗尽按**最后一次响应状态**分类（429→RateLimit）。
/// 每次尝试前查共享预算——剩余不足则跳过重试直接 Timeout；退避等待可被预算到期中断。
/// R1：讨论轮传 reserve=FINAL_STAGE_RESERVE 预留终稿额度；阶段 2 传 reserve=ZERO 全额使用。
/// #27-c：每次进入退避经 notify 播报（前端据此显示原因 + 倒计时），不再静默干等。
async fn send_with_retry(
    req: reqwest::RequestBuilder,
    budget: &SharedBudget,
    run_id: &str,
    reserve: std::time::Duration,
    notify: Option<BackoffHook<'_>>,
) -> Result<reqwest::Response, AppError> {
    /// 预算不足以再尝试一次的最低门槛（一次 HTTP 往返的悲观下限）
    const MIN_ATTEMPT_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);
    let mut last_err = "unknown".to_string();
    // 最后一次**有响应**的状态码（纯网络层失败保持 None）——决定重试耗尽的终态类别
    let mut last_status: Option<u16> = None;
    for attempt in 0..3 {
        // 取消检查优先于预算（按 run_id 隔离）
        if crate::commands::cancel::is_cancelled(run_id) {
            return Err(AppError::cancelled());
        }
        // 预算闸门——不够一次尝试就直接失败，不再烧钱等超时强杀
        // R1：讨论轮按 remaining - 保底判定，终稿（reserve=ZERO）按全额判定
        if budget.remaining_for_discussion(reserve) < MIN_ATTEMPT_BUDGET {
            // 终稿保底已解除仍不足：如实报全额剩余；讨论轮被保底拦：明示保底存在
            let hint = if reserve.is_zero() {
                format!("流水线预算不足（剩余 {:?}），停止重试", budget.remaining())
            } else {
                format!(
                    "讨论轮预算不足（可用 {:?}，已预留终稿 {:?}），停止重试",
                    budget.remaining_for_discussion(reserve),
                    reserve
                )
            };
            return Err(AppError::new(ErrorKind::Timeout, hint));
        }
        let builder = req.try_clone().ok_or_else(|| AppError::new(ErrorKind::Internal, "请求无法克隆（重试不可用）"))?;
        // 本轮结论：可重试 → 退避计划；不可重试 → 就地返回（响应交调用方分类 / 网络错误直接报）
        let plan = match builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                match retry_plan(attempt, Some(status.as_u16()), false) {
                    Some(p) => {
                        last_status = Some(status.as_u16());
                        last_err = format!("{} 错误", status);
                        p
                    }
                    None => return Ok(resp),
                }
            }
            Err(e) => match retry_plan(attempt, None, true) {
                Some(p) => {
                    last_err = format!("网络错误: {}", e);
                    p
                }
                None => return Err(AppError::new(ErrorKind::Network, format!("API request failed: {}", e))),
            },
        };
        apply_backoff(plan, attempt, budget, reserve, notify).await?;
    }
    let kind = terminal_kind(last_status);
    // 限流终态给可操作指引（旧规则文案与网络错误同形，用户分不清"该等"还是"该换 Key"）
    let message = if kind == ErrorKind::RateLimit {
        format!("API 限流：重试 3 次后仍被拒绝（{}）。请稍后重试，或检查该 Key 的配额与限流档位", last_err)
    } else {
        format!("API 请求失败（重试 3 次后仍失败）：{}", last_err)
    };
    Err(AppError::new(kind, message))
}

/// G-1 单源：预算打断文案按 reserve 分流——终稿（reserve=ZERO）说"流水线预算"，
/// 讨论轮明示"已预留终稿"；两处退避打断共用，不再各写各的（预算闸门分支文案在 send_with_retry 内）。
fn budget_stop_msg(reserve: std::time::Duration, what: &str) -> String {
    if reserve.is_zero() {
        format!("流水线预算耗尽，{}", what)
    } else {
        format!("讨论轮预算耗尽（已预留终稿 {:?}），{}", reserve, what)
    }
}

/// 构建 HTTP 客户端：connect 10s；总超时按调用场景（普通 120s，思考模式 300s——思维链+长输出耗时更长）
/// S-1：关闭自动重定向——重定向目标不经过 validate_url，跟随即绕过 SSRF 防线；
/// 公网 API 网关不依赖 3xx 跳转，遇重定向按非 2xx 原样报错（fail loud）。
pub(crate) fn build_client(timeout_secs: u64) -> Result<Client, AppError> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Failed to build HTTP client: {}", e)))
}

/// 全部调用统一的 max_tokens 上限（用户决策 2026-09-06：放开截断限制到 30000——
/// max_tokens 是输出上限不是预扣费用，实际按生成量计费；B4 的 3000→6000 提额阶梯随之取消）
pub(crate) const MAX_TOKENS_CAP: u32 = 30000;

/// 思考模式开启时的 max_tokens 下限：思维链会吃掉一部分输出预算，防长输出被截断
/// （与 MAX_TOKENS_CAP 同值——上限放开后思维链同样有充足预算）
const THINKING_MAX_TOKENS: u32 = MAX_TOKENS_CAP;

/// 思考参数路由表（纯函数，可测）：按模型名匹配各厂商的思考参数模板。
/// - DeepSeek 系（含讯飞 MaaS xopdeepseek*）：thinking 二元开关（实测网关接受，返回 reasoning_content）
/// - OpenAI o 系 / gpt-5：reasoning_effort 等级（o 系专用，gpt-4o 传了会 400，故只匹配 o 系/gpt-5）
/// - Anthropic 系（OpenAI 兼容网关）：thinking + budget_tokens
/// - 未知模型：None = 不带任何参数（安全默认，避免不支持的网关 400 报错）
///
/// 未来换模型只需在此加一行。
pub(crate) fn thinking_params(model: &str) -> Option<Value> {
    let m = model.to_lowercase();
    if m.contains("deepseek") {
        Some(serde_json::json!({"thinking": {"type": "enabled"}}))
    } else if m.contains("claude") {
        Some(serde_json::json!({"thinking": {"type": "enabled", "budget_tokens": 2048}}))
    } else if ["o1", "o3", "o4", "gpt-5"].iter().any(|k| m.contains(k)) {
        Some(serde_json::json!({"reasoning_effort": "medium"}))
    } else {
        None
    }
}

/// 开思考时：注入厂商思考参数；仅思考模式抬升 max_tokens 防思维链挤占输出（Q5：此前无条件抬升，
/// 非思考用户设 1000 也被抬到 30000；现非思考尊重用户值，思考才抬）
fn apply_thinking(body: &mut Value, model: &str, max_tokens: u32, thinking: bool) {
    if let Some(params) = thinking_params(model) {
        if thinking {
            if let Value::Object(map) = params {
                for (k, v) in map {
                    body[k] = v;
                }
            }
        }
    }
    if thinking {
        body["max_tokens"] = max_tokens.max(THINKING_MAX_TOKENS).into();
    } else {
        body["max_tokens"] = max_tokens.into();
    }
}

/// 截断判定——finish_reason == "length"（网关 max_tokens 截断）
/// 或 "truncated_by_budget"（R8 保底断流：可用预算耗尽主动断开，内容不全）都算截断，
/// 调用方按既有分级处置（阶段 0 报错、汇总继续、格式进 issue）。
pub(crate) fn is_truncated(finish_reason: &Option<String>) -> bool {
    matches!(finish_reason.as_deref(), Some("length") | Some("truncated_by_budget"))
}

/// 从非流式响应中提取（正文 content, finish_reason, usage）（纯函数，可测）。
/// content 缺失或空字符串都算"无正文"（思考模式下思维链吃满 max_tokens 时 content 可能为空）。
/// finish_reason 一并返回——"length"（截断）由调用方分级处置，不再静默丢弃。
/// usage 一并返回（网关不返回时为 None，不阻塞流程）。
fn extract_message(json: &Value) -> Option<(String, Option<String>, Option<TokenUsage>)> {
    let c = json["choices"][0]["message"]["content"].as_str()?;
    if c.trim().is_empty() { return None; }
    Some((
        c.to_string(),
        json["choices"][0]["finish_reason"].as_str().map(String::from),
        TokenUsage::from_json(json),
    ))
}

/// 发送请求并解析正文；无正文返回 Err（供思考模式降级重试判断）。
/// HTTP 状态分类（401/403=Auth，429=RateLimit，5xx/其他=Network）；解析失败=Parse。
/// budget 透传给 send_with_retry（预算闸门）；notify 透传退避播报钩子（#27-c）。
async fn send_and_extract(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
    budget: &SharedBudget,
    run_id: &str,
    reserve: std::time::Duration,
    notify: Option<BackoffHook<'_>>,
) -> Result<LLMResponse, AppError> {
    // R7：非流式 body 解码失败重试（网关抖动下 body 半截是常态）。
    // 仅解码路径重试：HTTP 状态错误仍直接分类返回，不碰 retry_plan 通道，避免双重退避。
    // body 字节一次读完后解析，失败则同参重发（预算闸门同样生效）。
    // 注意：首次发送在 extract_once 内，不在此处预发（避免空耗一次预算）。
    async fn extract_once(
        client: &Client,
        url: &str,
        api_key: &str,
        body: &Value,
        budget: &SharedBudget,
        run_id: &str,
        reserve: std::time::Duration,
        notify: Option<BackoffHook<'_>>,
    ) -> Result<LLMResponse, AppError> {
        let r = send_with_retry(
            client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(body),
            budget,
            run_id,
            reserve,
            notify,
        )
        .await?;
        let status = r.status();
        if !status.is_success() {
            let err = r
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(AppError::api_status(status.as_u16(), &truncate_err(&err)));
        }
        let bytes = r
            .bytes()
            .await
            .map_err(|e| AppError::new(ErrorKind::Network, format!("Stream error: {}", e)))?;
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::new(ErrorKind::Parse, format!("API response parse failed: {}", e)))?;
        let (raw, finish_reason, usage) = extract_message(&json)
            .ok_or_else(|| AppError::new(ErrorKind::Parse, "API response missing content".to_string()))?;
        Ok(LLMResponse { raw, finish_reason, usage })
    }
    match extract_once(client, url, api_key, body, budget, run_id, reserve, notify).await {
        Ok(resp) => Ok(resp),
        Err(e) if e.kind == ErrorKind::Parse || e.message.starts_with("Stream error") => {
            // R9：解码失败提到最多 3 次（首次 + 2 次重发，等待 5s/10s，可被预算打断）。
            // 实网重跑证明连续半截 body 是常态，一次不够。
            tracing::warn!(error = %e.message, "非流式响应解码失败，进入重发循环");
            let mut last = e;
            for attempt in 1..3 {
                // Q3：等待按 remaining - 保底截断，不吃终稿额度（与 send_with_retry 同口径）。
                let wait = std::time::Duration::from_secs(if attempt == 1 { 5 } else { 10 });
                let wait_capped = wait.min(budget.remaining_for_discussion(reserve));
                if wait_capped.is_zero() || tokio::time::timeout(budget.remaining(), tokio::time::sleep(wait_capped)).await.is_err() {
                    // G-2：丢因修复——旧规则 break 后透出的是底层解码错误，"预算打断"这一真实原因丢失，
                    // 且 Parse 形态会被外层误判为可重试再烧一轮。改置 Timeout 结论并保留最后一次错误。
                    tracing::warn!("非流式重发等待被预算打断，不再重试");
                    return Err(AppError::new(
                        ErrorKind::Timeout,
                        format!("预算打断，停止解码重发；最后一次错误: {}", truncate_err(&last.message)),
                    ));
                }
                match extract_once(client, url, api_key, body, budget, run_id, reserve, notify).await {
                    Ok(resp) => return Ok(resp),
                    Err(e2) if e2.kind == ErrorKind::Parse || e2.message.starts_with("Stream error") => {
                        tracing::warn!(attempt = attempt + 1, error = %e2.message, "非流式响应解码失败，重发");
                        last = e2;
                    }
                    Err(e2) => return Err(e2),
                }
            }
            Err(last)
        }
        Err(e) => Err(e),
    }
}

/// 无流式的单次 LLM 调用（流水线步骤用）。
/// 返回 LLMResponse（含 finish_reason，供调用方做截断分级处置）。
/// 思考模式下若响应无正文（思维链偶发吃满预算），自动降级为无思考重试一次，保证流水线不中断。
/// budget 透传（单调用超时按剩余预算收紧，见 build_client_for_budget）。
/// gen 缺省走内置默认（temperature 0.6 / max_tokens=调用方传入值）。
/// notify 透传退避播报钩子（#27-c：流水线调用点传 Some，无 UI 场景传 None）。
pub(crate) async fn call_llm_silent(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: Vec<Value>,
    max_tokens: u32,
    thinking: bool,
    budget: &SharedBudget,
    gen: &crate::models::GenerationConfig,
    run_id: &str,
    reserve: std::time::Duration,
    notify: Option<BackoffHook<'_>>,
) -> Result<LLMResponse, AppError> {
    // 单调用超时取 min(场景默认, 剩余预算)——预算不足时 reqwest 层即快速失败
    let client_timeout = budget.remaining().as_secs().min(if thinking { 300 } else { 120 }).max(10);
    let client = build_client(client_timeout)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    // temperature 缺省 0.6；max_tokens 取 min(调用方, 配置)——配置只收紧不放宽旧行为
    let temperature = gen.silent_temperature();
    let max_tokens = max_tokens.min(gen.max_tokens());
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "temperature": temperature,
        "max_tokens": max_tokens,
    });
    if thinking {
        apply_thinking(&mut body, model, max_tokens, true);
    }
    match send_and_extract(&client, &url, api_key, &body, budget, run_id, reserve, notify).await {
        Ok(resp) => Ok(resp),
        Err(e) if thinking && e.message.contains("missing content") => {
            // 降级：去掉思考参数重试一次（同一 prompt 无思考直接输出，必有正文）
            tracing::warn!("思考模式响应无正文，降级为无思考重试一次");
            let fallback = serde_json::json!({
                "model": model,
                "messages": body["messages"],
                "stream": false,
                "temperature": temperature,
                "max_tokens": max_tokens,
            });
            send_and_extract(&client, &url, api_key, &fallback, budget, run_id, reserve, notify).await
        }
        Err(e) => Err(e),
    }
}

/// P1-2 结构化调用的请求体（纯函数，离线可测）。
/// `none` 档不应走到这里（调用方先拒）；json_object 档在最前插入 schema system 消息。
pub(crate) fn structured_body(
    model: &str,
    messages: &[Value],
    max_tokens: u32,
    temperature: f32,
    mode: crate::llm_caps::StructuredMode,
    schema_name: &str,
    schema: &Value,
) -> Value {
    use crate::llm_caps::{response_format, schema_system_message, StructuredMode};
    let mut msgs: Vec<Value> = Vec::with_capacity(messages.len() + 1);
    if mode == StructuredMode::JsonObject {
        msgs.push(schema_system_message(schema));
    }
    msgs.extend(messages.iter().cloned());
    let mut body = serde_json::json!({
        "model": model,
        "messages": msgs,
        "stream": false,
        "temperature": temperature,
        "max_tokens": max_tokens,
    });
    if let Some(rf) = response_format(mode, schema_name, schema) {
        body["response_format"] = rf;
    }
    body
}

/// `none` 档的固定报错（用户可见；不发请求、不自由文本兜底）。
pub(crate) fn structured_unsupported_error(model: &str) -> AppError {
    AppError::new(
        ErrorKind::Validation,
        format!(
            "模型 {} 不支持结构化输出（json_schema / json_object 均不可用），请在设置中换用支持的模型或网关",
            model
        ),
    )
}

/// 模型正文 → JSON → 本地 schema 校验 → 类型化。失败 = Parse，detail 含 JSON 路径。
/// 全量复审 2026-10-20：推理模型/部分网关在 json_object 或 none 档会把 JSON 包在
/// `<think>…</think>` 前缀或 ```json 围栏里。只剥这两种外壳，不做任何"修复"（内容仍须原样合法 + 过 schema）。
pub(crate) fn strip_structured_wrappers(raw: &str) -> &str {
    let mut s = raw.trim();
    if s.starts_with("<think>") {
        if let Some(end) = s.find("</think>") {
            s = s[end + "</think>".len()..].trim();
        }
    }
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.strip_prefix("json").or_else(|| rest.strip_prefix("JSON")).unwrap_or(rest);
        if let Some(inner) = rest.trim_end().strip_suffix("```") {
            s = inner.trim();
        }
    }
    s
}

pub(crate) fn parse_structured<T: serde::de::DeserializeOwned>(raw: &str, schema: &Value) -> Result<(T, Value), AppError> {
    let v: Value = serde_json::from_str(strip_structured_wrappers(raw)).map_err(|e| {
        AppError::new(ErrorKind::Parse, format!("结构化输出不是合法 JSON: {}", e))
    })?;
    crate::plan::schema::check(schema, &v).map_err(|errs| {
        let detail: Vec<String> = errs.iter().take(8).map(|e| e.to_string()).collect();
        AppError::new(ErrorKind::Parse, format!("结构化输出未通过 schema 校验: {}", detail.join("; ")))
    })?;
    let t = serde_json::from_value(v.clone())
        .map_err(|e| AppError::new(ErrorKind::Parse, format!("结构化输出反序列化失败: {}", e)))?;
    Ok((t, v))
}

/// 结构化调用参数（端点 + 生成参数 + 档位/schema）；预算、消息与退避钩子单独传。
pub(crate) struct StructuredCall<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub max_tokens: u32,
    pub gen: &'a crate::models::GenerationConfig,
    pub run_id: &'a str,
    pub reserve: std::time::Duration,
    pub mode: crate::llm_caps::StructuredMode,
    pub schema_name: &'a str,
    pub schema: &'a Value,
}

/// P1-2 结构化调用（BLUEPRINT_v2 §2.2）：按档位带 response_format，返回本地校验通过的类型化值。
/// - `none` 档直接报错，不发请求。
/// - 允许 1 次**同请求**重发，仅限网络类错误或截断（finish_reason=length）；
///   解析/校验失败不重发、不发"修复提示"，原样以 Parse 报出。
/// - 非流式：结构化阶段不出 token 流，进度由调用方发 `PhaseProgress` 事件。
/// - 思考参数不注入（部分网关 thinking 与 response_format 互斥）。
pub(crate) async fn call_llm_structured<T: serde::de::DeserializeOwned>(
    c: &StructuredCall<'_>,
    messages: Vec<Value>,
    budget: &SharedBudget,
    notify: Option<BackoffHook<'_>>,
) -> Result<(T, LLMResponse), AppError> {
    let StructuredCall { base_url, api_key, model, max_tokens, gen, run_id, reserve, mode, schema_name, schema } = *c;
    if mode == crate::llm_caps::StructuredMode::None {
        return Err(structured_unsupported_error(model));
    }
    let client_timeout = budget.remaining().as_secs().clamp(10, 180);
    let client = build_client(client_timeout)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let max_tokens = max_tokens.min(gen.max_tokens());
    let body = structured_body(model, &messages, max_tokens, gen.silent_temperature(), mode, schema_name, schema);
    let mut resent = false;
    loop {
        let attempt = send_and_extract(&client, &url, api_key, &body, budget, run_id, reserve, notify).await;
        let retryable = match &attempt {
            Ok(resp) => is_truncated(&resp.finish_reason),
            Err(e) => e.kind == ErrorKind::Network,
        };
        if retryable && !resent {
            resent = true;
            tracing::warn!(mode = mode.as_str(), "结构化调用网络失败或截断，同请求重发一次");
            continue;
        }
        let resp = attempt?;
        if is_truncated(&resp.finish_reason) {
            return Err(AppError::new(ErrorKind::Parse, "结构化输出被截断（finish_reason=length），重发后仍截断".to_string()));
        }
        let (t, _) = parse_structured::<T>(&resp.raw, schema)?;
        return Ok((t, resp));
    }
}

/// 单档探测：发最小 response_format 请求，按状态码分类。
async fn probe_one(client: &Client, url: &str, api_key: &str, model: &str, mode: crate::llm_caps::StructuredMode, budget: &SharedBudget, run_id: &str) -> Result<crate::llm_caps::ProbeOutcome, AppError> {
    let body = crate::llm_caps::probe_body(model, mode);
    let r = send_with_retry(
        client.post(url).header("Authorization", format!("Bearer {}", api_key)).header("Content-Type", "application/json").json(&body),
        budget,
        run_id,
        std::time::Duration::ZERO,
        None,
    )
    .await?;
    let status = r.status().as_u16();
    let outcome = crate::llm_caps::classify_probe_status(status);
    match outcome {
        crate::llm_caps::ProbeOutcome::Supported => {
            // 全量复审 2026-10-20：200 不等于真支持——有网关静默忽略 response_format 返回散文。
            // 读正文：非空且剥壳后不是 JSON → 本档 Rejected；空正文（推理模型 token 被 reasoning 吃光）
            // 或 body 读不出 → 仍按 Supported（只凭状态码，与旧行为一致，不误降档）。
            let body: Option<Value> = r.json().await.ok();
            Ok(crate::llm_caps::classify_probe_body(body.as_ref()))
        }
        crate::llm_caps::ProbeOutcome::Fatal => {
            let err = r.text().await.unwrap_or_else(|_| "unknown error".to_string());
            Err(AppError::api_status(status, &truncate_err(&err)))
        }
        crate::llm_caps::ProbeOutcome::Rejected => {
            // 只有 body 明示与 response_format 相关才算"本档不支持"；否则当 Fatal，避免把缺模型/欠费误记为 none
            let err = r.text().await.unwrap_or_else(|_| "unknown error".to_string());
            if crate::llm_caps::response_format_rejection(&err) {
                Ok(crate::llm_caps::ProbeOutcome::Rejected)
            } else {
                Err(AppError::api_status(status, &truncate_err(&err)))
            }
        }
    }
}

/// P1-2 能力探测（设置页"测试连接"成功后调用）：json_schema → json_object → none。
/// 结果由前端按 `cap_key(base_url, model)` 缓存进 settings。鉴权/限流/5xx 原样报错，不记能力。
#[tauri::command]
pub async fn probe_structured_caps(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<crate::llm_caps::StructuredMode, AppError> {
    // S-1：与 test_api / 流水线同一道 URL 闸门
    crate::models::validate_url(&base_url).map_err(|m| {
        AppError::new(ErrorKind::Validation, format!("API 地址不合规：{}，请在设置中检查", m))
    })?;
    probe_structured_caps_unchecked(&base_url, &api_key, &model).await
}

/// 探测本体（URL 已过闸门；离线 mock 测试直接调用本函数）。
async fn probe_structured_caps_unchecked(
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<crate::llm_caps::StructuredMode, AppError> {
    use crate::budget::Budget;
    use crate::llm_caps::{ProbeOutcome, StructuredMode};
    use std::sync::Arc;
    let client = build_client(60)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(120)));
    let run_id = format!(
        "probe-caps-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    for mode in [StructuredMode::JsonSchema, StructuredMode::JsonObject] {
        if probe_one(&client, &url, api_key, model, mode, &budget, &run_id).await? == ProbeOutcome::Supported {
            return Ok(mode);
        }
    }
    Ok(StructuredMode::None)
}


/// 解析单行 SSE 数据（`data: ...` 前缀行，兼容 `data:{...}` 无空格变体，尾部行修复）。
/// 返回本次解析出的增量内容（未 emit，由调用方决定是否转发）。
/// 流式 usage 块（`{"choices":[],"usage":{...}}` 尾部形态）累加进 stream_usage。
/// 纯函数，可独立单测（不含网络）。
#[cfg(test)]
fn parse_sse_line(
    line: &str,
    full_content: &mut String,
    finish_reason: &mut Option<String>,
    stream_usage: &mut TokenUsage,
) -> Option<String> {
    let trimmed = line.trim();
    if let Some(data) = trimmed.strip_prefix("data:") {
        let data = data.trim_start();
        if data == "[DONE]" {
            return None;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
            // usage 块可能独立出现（choices 为空），先累加再处理 delta
            if let Some(u) = TokenUsage::from_json(&parsed) {
                stream_usage.add(&u);
            }
            let mut delta = None;
            if let Some(d) = parsed["choices"][0]["delta"]["content"].as_str() {
                full_content.push_str(d);
                delta = Some(d.to_string());
            }
            if let Some(fr) = parsed["choices"][0]["finish_reason"].as_str() {
                *finish_reason = Some(fr.to_string());
            }
            return delta;
        }
    }
    None
}

/// 解析 SSE 流并逐 chunk 发送给前端。
/// 维护跨 chunk 行缓冲区，确保被拆分的 JSON 行不会丢失。
/// R8：按保底断流——可用预算（remaining - reserve）耗尽时不断死，
/// 带着已收内容正常返回（finish_reason 记 truncated_by_budget，调用方按截断分级处置）。
#[cfg(test)]
async fn stream_response<R: Runtime>(
    app: AppHandle<R>,
    response: reqwest::Response,
    run_id: &str,
    budget: &SharedBudget,
    reserve: std::time::Duration,
) -> Result<LLMResponse, AppError> {
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(AppError::api_status(status.as_u16(), &truncate_err(&error_text)));
    }

    let mut full_content = String::new();
    let mut finish_reason: Option<String> = None;
    // 流式 usage 累加（尾部独立块形态）
    let mut stream_usage = TokenUsage::default();
    // C-07（P0-4）：字节级行缓冲。旧实现每个网络 chunk 先 from_utf8_lossy 再拼接，
    // 多字节字符（中文 3 字节）被 chunk 边界切开时两半各自变 U+FFFD。
    // 现按 b'\n' 切完整行后再解码（'\n' 不会出现在多字节序列内部），残余字节留待下一 chunk。
    let mut line_buf: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    let started = std::time::Instant::now();
    let mut last_log = std::time::Instant::now();
    // R8：是否触发过保底断流（触发后 finish_reason 记 truncated_by_budget）
    let mut budget_cut = false;

    while let Some(chunk_result) = stream.next().await {
        // 取消检查点——按 run_id 隔离
        if crate::commands::cancel::is_cancelled(run_id) {
            return Err(AppError::cancelled());
        }
        // R8：保底断流——可用耗尽时不断死，带已收内容返回（空正文才报错）
        if budget.remaining_for_discussion(reserve).is_zero() {
            tracing::warn!(
                elapsed_secs = started.elapsed().as_secs(),
                chars = full_content.chars().count(),
                "流式可用预算耗尽，保底断流（已收内容返回，调用方按截断处置）"
            );
            budget_cut = true;
            break;
        }
        if last_log.elapsed().as_secs() >= 30 {
            tracing::info!(elapsed_secs = started.elapsed().as_secs(), chars = full_content.chars().count(), "流式进行中");
            last_log = std::time::Instant::now();
        }
        let chunk = chunk_result.map_err(|e| {
            tracing::warn!(elapsed_secs = started.elapsed().as_secs(), error = %e, "流式错误");
            AppError::new(ErrorKind::Network, format!("Stream error: {}", e))
        })?;
        line_buf.extend_from_slice(&chunk);

        // 只处理以 \n 结尾的完整行，不完整的尾部（含被切开的多字节字符）留到下一个 chunk
        while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = line_buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes[..pos]);

            if let Some(delta) = parse_sse_line(&line, &mut full_content, &mut finish_reason, &mut stream_usage) {
                let _ = app.emit(
                    "llm-chunk",
                    StreamChunk {
                        run_id: run_id.to_string(),
                        content: delta,
                    },
                );
            }
        }
    }

    // 流结束后 flush 残留的不完整行（尾部行修复：尾部 data 行不再静默丢弃）
    let tail = String::from_utf8_lossy(&line_buf);
    if !tail.trim().is_empty() {
        if let Some(delta) = parse_sse_line(&tail, &mut full_content, &mut finish_reason, &mut stream_usage) {
            let _ = app.emit("llm-chunk", StreamChunk { run_id: run_id.to_string(), content: delta });
        }
    }

    // 思考模式兜底：思维链吃满预算时可能全程无正文，禁止静默产出空方案
    if full_content.trim().is_empty() {
        return Err(AppError::new(
            ErrorKind::Parse,
            "思考模式流式响应无正文（思维链可能耗尽预算）",
        ));
    }
    // R8：保底断流且网关没给 finish_reason（没发完）→ 记 truncated_by_budget；
    // 网关给了（正常 stop/length）则保留网关的。调用方 is_truncated 认该标记，
    // 按既有截断分级处置（阶段 0 报错、汇总继续、格式进 issue）。
    if budget_cut && finish_reason.is_none() {
        finish_reason = Some("truncated_by_budget".to_string());
    }

    Ok(LLMResponse {
        raw: full_content,
        finish_reason,
        // 流式 usage 全零时记 None（与非流式宽容语义一致）
        usage: if stream_usage.prompt_tokens == 0 && stream_usage.completion_tokens == 0 {
            None
        } else {
            Some(stream_usage)
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_extracts_delta_and_finish() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#;
        let delta = parse_sse_line(line, &mut full, &mut finish, &mut usage);
        assert_eq!(delta.as_deref(), Some("你好"));
        assert_eq!(full, "你好");
        assert_eq!(finish, None);
    }

    #[test]
    fn sse_handles_done() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line("data: [DONE]", &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
    }

    #[test]
    fn sse_extracts_finish_reason_stop() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        parse_sse_line(line, &mut full, &mut finish, &mut usage);
        assert_eq!(finish.as_deref(), Some("stop"));
    }

    #[test]
    fn sse_ignores_non_data_lines() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line(": keep-alive", &mut full, &mut finish, &mut usage), None);
        assert_eq!(parse_sse_line("", &mut full, &mut finish, &mut usage), None);
    }

    /// 重试决策：429 退避 30s/60s；5xx 退避 15s/30s；网络错误 10s/20s；4xx 不重试。
    /// #27-c：等待秒数与原因由同一次判定产出（单源），故两者一并断言——只测秒数会让
    /// "原因写错"漏网（前端据此显示文案，错配即误导）。
    #[test]
    fn retry_plan_classifies_errors() {
        let rl = |w| RetryPlan { wait_secs: w, reason: BackoffReason::RateLimit };
        let se = |w| RetryPlan { wait_secs: w, reason: BackoffReason::ServerError };
        let nw = |w| RetryPlan { wait_secs: w, reason: BackoffReason::Network };
        // 429 限流：可重试，30s/60s
        assert_eq!(retry_plan(0, Some(429), false), Some(rl(30)));
        assert_eq!(retry_plan(1, Some(429), false), Some(rl(60)));
        // 5xx 服务端错误：可重试，15s/30s
        assert_eq!(retry_plan(0, Some(500), false), Some(se(15)));
        assert_eq!(retry_plan(1, Some(502), false), Some(se(30)));
        assert_eq!(retry_plan(0, Some(503), false), Some(se(15)));
        // 网络错误：可重试，10s/20s
        assert_eq!(retry_plan(0, None, true), Some(nw(10)));
        assert_eq!(retry_plan(1, None, true), Some(nw(20)));
        // 4xx 凭据/请求错误：不可重试
        assert_eq!(retry_plan(0, Some(400), false), None);
        assert_eq!(retry_plan(0, Some(401), false), None);
        assert_eq!(retry_plan(0, Some(403), false), None);
        assert_eq!(retry_plan(0, Some(404), false), None);
        // 成功响应无需重试决策（调用方直接返回）
        assert_eq!(retry_plan(0, Some(200), false), None);
    }

    /// #27-c 退避原因 wire format 锁——前端按 `reason` 取文案，改名即红
    #[test]
    fn backoff_reason_wire_format() {
        assert_eq!(BackoffReason::RateLimit.as_str(), "rate_limit");
        assert_eq!(BackoffReason::ServerError.as_str(), "server_error");
        assert_eq!(BackoffReason::Network.as_str(), "network");
    }

    /// #27-c 重试耗尽的终态类别：429 必须报限流（旧规则一律报 Network，误导排障方向）；
    /// 5xx/无响应保持 Network；类别与 `ErrorKind::for_status` 同源
    #[test]
    fn terminal_kind_keeps_rate_limit() {
        assert_eq!(terminal_kind(Some(429)), ErrorKind::RateLimit);
        assert_eq!(terminal_kind(Some(500)), ErrorKind::Network);
        assert_eq!(terminal_kind(Some(503)), ErrorKind::Network);
        assert_eq!(terminal_kind(None), ErrorKind::Network);
    }

    /// #27-c 退避播报契约：进入播报在等待之前、结束播报在等待之后（顺序即契约，
    /// 前端靠 End 撤下倒计时；若顺序反了 UI 会在等待中提前消失）。
    /// 注意：`wait_secs` 播报的是**含抖动**的实际等待（计划 0s + 0–5s 抖动），
    /// 故此处只断言抖动上界，不写死 0——写死等于把"抖动必须存在"这条规则测没了。
    #[tokio::test]
    async fn apply_backoff_reports_start_then_end() {
        use crate::budget::Budget;
        use std::sync::{Arc, Mutex};
        let seen: Arc<Mutex<Vec<BackoffNotice>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let hook = move |n: BackoffNotice| sink.lock().unwrap().push(n);
        let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        // 计划等待 0s（抖动最多再补 5s）——仍走完整"播报→等待→播报"路径
        let plan = RetryPlan { wait_secs: 0, reason: BackoffReason::Network };
        apply_backoff(plan, 0, &budget, std::time::Duration::ZERO, Some(&hook))
            .await
            .unwrap();
        let got = seen.lock().unwrap().clone();
        assert_eq!(got.len(), 2, "必须恰好播报两次（Start + End），实际 {:?}", got);
        match (&got[0], &got[1]) {
            (
                BackoffNotice::Start { attempt, wait_secs, reason },
                BackoffNotice::End { attempt: end_attempt },
            ) => {
                assert_eq!(*attempt, 1);
                // End 必须与 Start 同一次尝试——前端按 attempt 配对，错配会撤错提示
                assert_eq!(*end_attempt, 1);
                assert_eq!(*reason, BackoffReason::Network);
                assert!(*wait_secs <= 5, "抖动上界 5s，实际播报 {}s", wait_secs);
            }
            other => panic!("播报顺序必须是 Start → End，实际 {:?}", other),
        }
    }

    /// 无播报钩子（测试连接等无 UI 场景）不得 panic
    #[tokio::test]
    async fn apply_backoff_without_hook_is_noop_safe() {
        use crate::budget::Budget;
        use std::sync::Arc;
        let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        let plan = RetryPlan { wait_secs: 0, reason: BackoffReason::RateLimit };
        assert!(apply_backoff(plan, 1, &budget, std::time::Duration::ZERO, None).await.is_ok());
    }

    /// R4：退避抖动恒在 0–5s（阈值不动，离散包在外面；多次采样不越界）
    #[test]
    fn backoff_jitter_within_five_secs() {
        for _ in 0..50 {
            assert!(backoff_jitter_secs() <= 5, "抖动必须在 0–5s 内");
        }
    }

    #[test]
    fn sse_ignores_malformed_json() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line("data: not-json", &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
    }

    #[test]
    fn build_client_configures_timeouts() {
        let client = build_client(120).unwrap();
        // 仅验证构建成功即可（超时值在 reqwest 内部）
        assert!(client.get("https://example.com").build().is_ok());
        assert!(build_client(300).is_ok());
    }

    #[test]
    fn sse_content_with_finish_reason_accumulates() {
        // 模拟完整 SSE 流：内容 + finish_reason
        let lines = [
            r#"data: {"choices":[{"delta":{"content":"你"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"好"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ];
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        for line in lines {
            parse_sse_line(line, &mut full, &mut finish, &mut usage);
        }
        assert_eq!(full, "你好");
        assert_eq!(finish.as_deref(), Some("stop"));
    }

    /// 思考模式流式：思维链 chunk（delta 只带 reasoning_content）必须被跳过，不污染正文
    #[test]
    fn sse_skips_reasoning_content_chunks() {
        let mut full = String::new();
        let mut finish = None;
        // 思考阶段：delta 只有 reasoning_content，无 content
        let thinking = r#"data: {"choices":[{"delta":{"reasoning_content":"让我想想这个方案…"},"finish_reason":null}]}"#;
        let mut usage = TokenUsage::default();
        assert_eq!(parse_sse_line(thinking, &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
        // 思考结束进入正文：delta 带 content
        let content = r#"data: {"choices":[{"delta":{"content":"最终方案"},"finish_reason":null}]}"#;
        assert_eq!(parse_sse_line(content, &mut full, &mut finish, &mut usage), Some("最终方案".to_string()));
        assert_eq!(full, "最终方案");
    }

    /// 思考参数路由：DeepSeek 系（含讯飞 MaaS）→ thinking 开关
    #[test]
    fn thinking_params_deepseek_family() {
        for m in ["xopdeepseekv4flash0731", "deepseek-chat", "DeepSeek-V3"] {
            let p = thinking_params(m).unwrap();
            assert_eq!(p["thinking"]["type"], "enabled", "模型 {} 应注入 thinking.enabled", m);
        }
    }

    /// 思考参数路由：OpenAI o 系/gpt-5 → reasoning_effort；gpt-4o 等旧模型不匹配（传了会 400）
    #[test]
    fn thinking_params_openai_family() {
        for m in ["o3-mini", "o4-mini", "gpt-5"] {
            let p = thinking_params(m).unwrap();
            assert_eq!(p["reasoning_effort"], "medium", "模型 {} 应注入 reasoning_effort", m);
        }
        for m in ["gpt-4o", "gpt-4o-mini"] {
            assert_eq!(thinking_params(m), None, "{} 不支持 reasoning_effort，必须不带参数", m);
        }
    }

    /// 思考参数路由：Anthropic 系 → thinking + budget；未知模型 → 安全默认不带参数
    #[test]
    fn thinking_params_claude_and_unknown() {
        let p = thinking_params("claude-sonnet-4-5").unwrap();
        assert_eq!(p["thinking"]["type"], "enabled");
        assert_eq!(p["thinking"]["budget_tokens"], 2048);
        for m in ["sensenova-6.7-flash-lite", "qwen-max", "glm-4", "kimi"] {
            assert_eq!(thinking_params(m), None, "未登记模型 {} 必须不带参数（安全默认）", m);
        }
    }

    /// Q5：思考才抬升，非思考尊重用户值；思考参数仅思考模式注入
    #[test]
    fn apply_thinking_raises_max_tokens() {
        let mut body = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 3000});
        apply_thinking(&mut body, "xopdeepseekv4flash0731", 3000, true);
        assert_eq!(body["max_tokens"], THINKING_MAX_TOKENS);
        assert_eq!(body["thinking"]["type"], "enabled");
        // 调用方 max_tokens 已超上限：保持原值不降低
        let mut body2 = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 32000});
        apply_thinking(&mut body2, "xopdeepseekv4flash0731", 32000, true);
        assert_eq!(body2["max_tokens"], 32000);
        // 非思考：用户 1000 不被抬，且不注入思考参数
        let mut body3 = serde_json::json!({"model": "xopdeepseekv4flash0731", "max_tokens": 1000});
        apply_thinking(&mut body3, "xopdeepseekv4flash0731", 1000, false);
        assert_eq!(body3["max_tokens"], 1000);
        assert!(body3.get("thinking").is_none(), "非思考不得注入思考参数");
    }

    /// 正文与 finish_reason 联合提取——正常 / 缺失 / 空字符串（思考模式思维链吃满预算）都要正确判定
    #[test]
    fn extract_message_handles_missing_and_empty() {
        // 正常：content + finish_reason 一起返回
        let ok = serde_json::json!({"choices": [{"message": {"content": "方案内容"}, "finish_reason": "stop"}]});
        let (c, fr, u) = extract_message(&ok).unwrap();
        assert_eq!(c, "方案内容");
        assert_eq!(fr.as_deref(), Some("stop"));
        assert!(u.is_none(), "无 usage 字段应为 None");
        // finish_reason 缺省也要容忍（部分网关不返回）
        let no_fr = serde_json::json!({"choices": [{"message": {"content": "方案内容"}}]});
        let (_, fr2, u2) = extract_message(&no_fr).unwrap();
        assert_eq!(fr2, None);
        assert!(u2.is_none(), "无 usage 字段应为 None");
        // 缺失 content（思维链吃满预算时网关可能不返回 content）
        let missing = serde_json::json!({"choices": [{"message": {"reasoning_content": "思考..."}}]});
        assert!(extract_message(&missing).is_none());
        // content 为空字符串也算无正文
        let empty = serde_json::json!({"choices": [{"message": {"content": ""}}]});
        assert!(extract_message(&empty).is_none());
        let blank = serde_json::json!({"choices": [{"message": {"content": "   "}}]});
        assert!(extract_message(&blank).is_none());
    }

    /// 预算耗尽时 send_with_retry 不发起请求，直接 Timeout（用不可达地址验证：若发起请求会是 Network 错误）
    #[tokio::test]
    async fn send_with_retry_budget_exhausted_skips() {
        use crate::budget::Budget;
        use std::sync::Arc;
        let client = build_client(10).unwrap();
        let req = client.get("http://127.0.0.1:1/unreachable");
        // 预算已耗尽（0ms）：必须直接 Timeout，不得尝试发送
        let spent = Arc::new(Budget::with_timeout(std::time::Duration::from_millis(0)));
        std::thread::sleep(std::time::Duration::from_millis(5));
        let err = send_with_retry(req, &spent, "", std::time::Duration::ZERO, None).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Timeout, "预算耗尽应直接 Timeout: {:?}", err);
        // 预算充足时同样不可达地址应走 Network 路径（证明闸门是预算触发的，不是地址问题）
        let rich = Arc::new(Budget::unlimited());
        let req2 = build_client(2).unwrap().get("http://127.0.0.1:1/unreachable");
        let err2 = send_with_retry(req2, &rich, "", std::time::Duration::ZERO, None).await.unwrap_err();
        assert_eq!(err2.kind, ErrorKind::Network, "预算充足时应尝试发送并报 Network: {:?}", err2);
    }

    /// R1：讨论轮保底——剩余刚好等于保底时，可用归零，门直接拦（终稿靠全额续命）
    #[tokio::test]
    async fn send_with_retry_reserve_blocks_discussion_but_not_final() {
        use crate::budget::Budget;
        use std::sync::Arc;
        let client = build_client(10).unwrap();
        // 剩余 130s，保底 120s：可用 10s < 15s 门 → 讨论轮被拦，文案明示保底
        let tight = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(130)));
        let req = client.get("http://127.0.0.1:1/unreachable");
        let err = send_with_retry(req, &tight, "", FINAL_STAGE_RESERVE, None).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::Timeout);
        assert!(err.message.contains("终稿"), "讨论轮被拦文案应明示保底，实际: {}", err.message);
        // 同一预算终稿（reserve=ZERO）按全额判定：130s 充足 → 放行尝试（不可达地址走 Network）
        let req2 = build_client(2).unwrap().get("http://127.0.0.1:1/unreachable");
        let err2 = send_with_retry(req2, &tight, "", std::time::Duration::ZERO, None).await.unwrap_err();
        assert_eq!(err2.kind, ErrorKind::Network, "终稿全额下应放行尝试: {:?}", err2);
    }

    /// 非流式 usage 提取——正常返回 Some，缺字段/全零为 None
    /// 全量复审 2026-10-20：推理模型字段（reasoning / reasoning_details）不得混入正文
    #[test]
    fn reasoning_fields_never_leak_into_content() {
        let j = serde_json::json!({"choices":[{"message":{"role":"assistant","content":"","reasoning":"x","reasoning_details":[{"type":"reasoning.text","text":"x"}]},"finish_reason":"length"}]});
        assert!(extract_message(&j).is_none(), "空正文+reasoning → None（走降级/截断判定）");
        let j2 = serde_json::json!({"choices":[{"message":{"content":"{\"a\":1}","reasoning":"long"}}]});
        assert_eq!(extract_message(&j2).unwrap().0, "{\"a\":1}");
        let (mut full, mut fr, mut u) = (String::new(), None, TokenUsage::default());
        let r = parse_sse_line(r#"data: {"choices":[{"delta":{"reasoning":"think","reasoning_details":[{"text":"t"}]}}]}"#, &mut full, &mut fr, &mut u);
        assert!(r.map(|s| s.is_empty()).unwrap_or(true) && full.is_empty(), "reasoning delta 不进正文");
        parse_sse_line(r#"data: {"choices":[{"delta":{"content":"hi","reasoning":"z"}}]}"#, &mut full, &mut fr, &mut u);
        assert_eq!(full, "hi");
    }

    #[test]
    fn structured_wrappers_stripped_only_think_and_fence() {
        assert_eq!(strip_structured_wrappers("<think>abc</think>\n{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_structured_wrappers("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_structured_wrappers(" {\"a\":1} "), "{\"a\":1}");
        assert_eq!(strip_structured_wrappers("Here: {\"a\":1}"), "Here: {\"a\":1}", "散文前缀不修复");
    }

    #[test]
    fn extract_message_returns_usage() {
        let ok = serde_json::json!({
            "choices": [{"message": {"content": "x"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 120, "completion_tokens": 34, "total_tokens": 154}
        });
        let (_, _, u) = extract_message(&ok).unwrap();
        let u = u.unwrap();
        assert_eq!((u.prompt_tokens, u.completion_tokens), (120, 34));
        let no_u = serde_json::json!({"choices": [{"message": {"content": "x"}}]});
        let (_, _, u2) = extract_message(&no_u).unwrap();
        assert!(u2.is_none());
    }

    /// 流式 usage 尾部块（choices 为空）累加，不污染正文
    #[test]
    fn sse_usage_block_accumulates() {
        let mut full = String::new();
        let mut finish = None;
        let mut usage = TokenUsage::default();
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":200,"completion_tokens":50}}"#;
        assert_eq!(parse_sse_line(line, &mut full, &mut finish, &mut usage), None);
        assert_eq!(full, "");
        assert_eq!((usage.prompt_tokens, usage.completion_tokens), (200, 50));
    }

    /// test_api_body 组装——max_tokens=16（旧 1 会让思考模型误报），模型名透传
    #[test]
    fn test_api_body_uses_16_tokens() {
        let b = test_api_body("my-model");
        assert_eq!(b["max_tokens"], 16);
        assert_eq!(b["model"], "my-model");
        assert_eq!(b["stream"], false);
    }

    /// 截断判定——只有 finish_reason == "length" 算截断（stop/缺失/其他值都不算）
    #[test]
    fn is_truncated_only_for_length() {
        assert!(is_truncated(&Some("length".to_string())));
        // R8：保底断流标记同样算截断（调用方按既有分级处置）
        assert!(is_truncated(&Some("truncated_by_budget".to_string())));
        assert!(!is_truncated(&Some("stop".to_string())));
        assert!(!is_truncated(&None));
        assert!(!is_truncated(&Some("tool_calls".to_string())));
    }
}

/// 测试连接请求体组装（纯函数，可测）——max_tokens=16（足够返回 hi 级响应；
/// 旧 max_tokens=1 会让思考型模型无正文/400 误报"连接失败"）
fn test_api_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16,
        "stream": false,
    })
}

/// 测试 API 连接（前端"测试连接"按钮用）。
/// 走 Rust 后端发请求，绕过 WebView CORS 限制（前端 fetch 跨域会被拦）。
/// 成功判定：HTTP 200 即成功（不要求 content 非空——思考模型可能只回 reasoning）。
/// 失败分类复用 api_status（401/403=Auth，429=RateLimit，其余=Network）。
#[tauri::command]
pub async fn test_api(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, AppError> {
    // R5：改走 send_with_retry（间歇 429 自动退避重试，不再一次定胜负误报）。
    // 预算独立 120s（不占流水线 15 分钟池，天然隔离）；成功判定与失败分类原样保留。
    use crate::budget::Budget;
    use std::sync::Arc;
    // S-1 新规则：测试连接与流水线同一道 URL 闸门（旧规则此处无任何校验，可直连任意 host）
    crate::models::validate_url(&base_url).map_err(|m| {
        AppError::new(ErrorKind::Validation, format!("API 地址不合规：{}，请在设置中检查", m))
    })?;
    let client = build_client(120)?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = test_api_body(&model);
    let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(120)));
    let test_api_run_id = format!(
        "test-api-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let resp = send_with_retry(
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body),
        &budget,
        // RULE-RUN-ID（C-05）：测试连接用独立 id（不与任何流水线 run 共享取消位；无人可取消它，
        // 旧规则传空 id 读全局取消位，用户空 id"停止"后测试连接永远"已取消"）
        &test_api_run_id,
        std::time::Duration::ZERO,
        // #27-c：测试连接无流水线 run（不播报退避事件）；退避期间前端"测试中…"即为等待观感
        None,
    )
    .await
    // G-3：kind 原样透传（旧规则硬转 Network——key 填错被报成"网络请求失败"，排障方向被误导）
    .map_err(|e| AppError::new(e.kind, format!("请求失败: {}", truncate_err(&e.message))))?;
    let status = resp.status();
    if status.is_success() {
        Ok("连接成功".into())
    } else {
        let err = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(AppError::api_status(status.as_u16(), &truncate_err(&err)))
    }
}

/// RULE-RUN-ID 集成级（无 GUI）：mock_app + 本地假 SSE 网关，走真实 stream_response 发射路径。
#[cfg(test)]
pub(crate) mod stream_it {
    use super::*;
    use crate::budget::Budget;
    use std::io::{Read, Write};
    use std::sync::{mpsc, Arc, Mutex};
    use tauri::Listener;

    /// 本地假网关：接受一个连接，按顺序写出 parts；每段前等待 gate（None = 不等）。
    pub(crate) fn serve_sse(parts: Vec<Vec<u8>>, gate: Option<mpsc::Receiver<()>>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut req = Vec::new();
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
            for (i, p) in parts.into_iter().enumerate() {
                if i > 0 {
                    if let Some(g) = &gate {
                        if g.recv().is_err() {
                            return;
                        }
                    }
                }
                if s.write_all(&p).is_err() {
                    return;
                }
                let _ = s.flush();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        format!("http://{addr}/")
    }

    pub(crate) fn sse(content: &str) -> Vec<u8> {
        format!("data: {}\n\n", serde_json::json!({"choices":[{"delta":{"content":content}}]})).into_bytes()
    }

    pub(crate) async fn open(url: &str) -> reqwest::Response {
        reqwest::Client::new().get(url).send().await.unwrap()
    }

    /// 取消 A 后立即开 B：A 停止发射并返回 Cancelled；所有事件都带归属 run_id；
    /// 按当前 run（B）过滤后正文只含 B 的内容（与前端 acceptStreamChunk 同规则）。
    #[tokio::test]
    async fn cancel_then_new_run_chunks_tagged_and_filtered() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let events: Arc<Mutex<Vec<StreamChunk>>> = Arc::default();
        let ev = events.clone();
        handle.listen_any("llm-chunk", move |e| {
            let v: serde_json::Value = serde_json::from_str(e.payload()).unwrap();
            ev.lock().unwrap().push(StreamChunk {
                run_id: v["run_id"].as_str().unwrap().to_string(),
                content: v["content"].as_str().unwrap().to_string(),
            });
        });
        let budget: SharedBudget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        let reserve = std::time::Duration::ZERO;

        let (tx, rx) = mpsc::channel();
        let url_a = serve_sse(vec![sse("旧A1"), sse("旧A2"), sse("旧A3")], Some(rx));
        let ga = crate::commands::cancel::begin("it-a");
        let resp_a = open(&url_a).await;
        let (h2, b2) = (handle.clone(), budget.clone());
        let task_a = tokio::spawn(async move { stream_response(h2, resp_a, "it-a", &b2, reserve).await });
        for _ in 0..200 {
            if !events.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // 用户点停止 → 立刻开新 run
        assert!(crate::commands::cancel::request_cancel("it-a").effective());
        let gb = crate::commands::cancel::begin("it-b");
        let _ = tx.send(()); // A 的在途数据继续到达
        let _ = tx.send(());
        let out_a = task_a.await.unwrap();
        assert_eq!(out_a.unwrap_err().kind, ErrorKind::Cancelled);
        drop(ga);

        let url_b = serve_sse(vec![sse("新B1"), sse("新B2")], None);
        let resp_b = open(&url_b).await;
        let out_b = stream_response(handle.clone(), resp_b, "it-b", &budget, reserve).await.unwrap();
        drop(gb);
        assert_eq!(out_b.raw, "新B1新B2");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let evs = events.lock().unwrap().clone();
        assert!(evs.iter().all(|c| c.run_id == "it-a" || c.run_id == "it-b"), "每个 chunk 必须带归属 run_id: {evs:?}");
        assert!(!evs.iter().any(|c| c.run_id == "it-a" && c.content != "旧A1"), "取消后 A 不得再发射: {evs:?}");
        let shown: String = evs.iter().filter(|c| c.run_id == "it-b").map(|c| c.content.as_str()).collect();
        assert_eq!(shown, "新B1新B2");
        // 结束后迟到的取消不留条目
        assert!(!crate::commands::cancel::request_cancel("it-a").effective());
        assert!(!crate::commands::cancel::is_cancelled("it-a"));
    }

    /// C-07（P0-4）：中文 3 字节字符在任意字节切点拆进两个网络 chunk → 正文与事件均无 U+FFFD
    #[tokio::test]
    async fn utf8_split_at_any_cut_point_no_replacement_char() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let budget: SharedBudget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        let text = "雨落长街";
        let mut body = sse(text);
        body.extend_from_slice(b"data: [DONE]\n\n");
        let rid = "p04-utf8";
        let events: Arc<Mutex<Vec<String>>> = Arc::default();
        let ev = events.clone();
        handle.listen_any("llm-chunk", move |e| {
            let v: serde_json::Value = serde_json::from_str(e.payload()).unwrap();
            if v["run_id"] == "p04-utf8" {
                ev.lock().unwrap().push(v["content"].as_str().unwrap().to_string());
            }
        });
        // 覆盖每个切点（含每个汉字的 3 个字节内部的 2 个切点）
        for cut in 1..body.len() {
            events.lock().unwrap().clear();
            let parts = vec![body[..cut].to_vec(), body[cut..].to_vec()];
            let url = serve_sse(parts, None);
            let out = stream_response(handle.clone(), open(&url).await, rid, &budget, std::time::Duration::ZERO)
                .await
                .unwrap();
            assert_eq!(out.raw, text, "切点 {cut}");
            assert!(!out.raw.contains('\u{FFFD}'), "切点 {cut} 出现 U+FFFD");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let joined: String = events.lock().unwrap().concat();
            assert_eq!(joined, text, "切点 {cut} 事件正文");
        }
        // 每字节一个 chunk 的极端情形
        let parts: Vec<Vec<u8>> = body.iter().map(|b| vec![*b]).collect();
        let url = serve_sse(parts, None);
        let out = stream_response(handle.clone(), open(&url).await, rid, &budget, std::time::Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(out.raw, text);
    }

    /// 无结尾换行的尾部行仍被 flush（尾部行修复不回退），且尾部多字节完整
    #[tokio::test]
    async fn tail_line_without_newline_flushed_utf8_intact() {
        let app = tauri::test::mock_app();
        let budget: SharedBudget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        let mut body = sse("前");
        let tail = sse("尾行");
        body.extend_from_slice(&tail[..tail.len() - 2]); // 去掉 \n\n
        let cut = body.len() - 5;
        let url = serve_sse(vec![body[..cut].to_vec(), body[cut..].to_vec()], None);
        let out = stream_response(app.handle().clone(), open(&url).await, "p04-tail", &budget, std::time::Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(out.raw, "前尾行");
    }
}

/// P1-2 离线 mock 网关：三档（json_schema / json_object / none）+ 探测 + 失败语义。
#[cfg(test)]
mod structured_it {
    use super::*;
    use crate::budget::Budget;
    use crate::llm_caps::StructuredMode;
    use crate::plan::{schema::song_plan_schema, SongPlan};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    /// 依次应答 responses（每个连接一条，Connection: close），记录每个请求 JSON body。
    fn serve_json(responses: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Value>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut s, _)) = listener.accept() else { return };
                let mut buf = [0u8; 8192];
                let mut req = Vec::new();
                let head_end = loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 { return; }
                    req.extend_from_slice(&buf[..n]);
                    if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") { break i + 4; }
                };
                let head = String::from_utf8_lossy(&req[..head_end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                while req.len() < head_end + len {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 { break; }
                    req.extend_from_slice(&buf[..n]);
                }
                if let Ok(v) = serde_json::from_slice::<Value>(&req[head_end..]) {
                    seen2.lock().unwrap().push(v);
                }
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn completion(content: &str, finish: &str) -> String {
        serde_json::json!({"choices":[{"message":{"content":content},"finish_reason":finish}]}).to_string()
    }

    fn plan_json() -> String {
        serde_json::to_string(&crate::plan::fixtures::minimal_plan()).unwrap()
    }

    async fn call(url: &str, mode: StructuredMode) -> Result<(SongPlan, LLMResponse), AppError> {
        let budget = Arc::new(Budget::with_timeout(std::time::Duration::from_secs(60)));
        let schema = song_plan_schema();
        let gen = crate::models::GenerationConfig::default();
        let c = StructuredCall {
            base_url: url,
            api_key: "k",
            model: "m",
            max_tokens: 4000,
            gen: &gen,
            run_id: "run-structured-test",
            reserve: std::time::Duration::ZERO,
            mode,
            schema_name: crate::plan::schema::SONG_PLAN_SCHEMA_NAME,
            schema: &schema,
        };
        call_llm_structured::<SongPlan>(&c, vec![serde_json::json!({"role":"user","content":"x"})], &budget, None)
        .await
    }

    #[tokio::test]
    async fn json_schema_tier_sends_strict_schema_and_validates() {
        let (url, seen) = serve_json(vec![(200, completion(&plan_json(), "stop"))]);
        let (plan, _) = call(&url, StructuredMode::JsonSchema).await.expect("应通过");
        assert_eq!(plan, crate::plan::fixtures::minimal_plan());
        let req = &seen.lock().unwrap()[0];
        assert_eq!(req["response_format"]["type"], "json_schema");
        assert_eq!(req["response_format"]["json_schema"]["strict"], true);
        assert_eq!(req["response_format"]["json_schema"]["schema"], song_plan_schema());
        assert!(req.get("thinking").is_none());
        assert_eq!(req["stream"], false);
    }

    #[tokio::test]
    async fn json_object_tier_puts_schema_in_system_and_validates() {
        let (url, seen) = serve_json(vec![(200, completion(&plan_json(), "stop"))]);
        call(&url, StructuredMode::JsonObject).await.expect("应通过");
        let req = &seen.lock().unwrap()[0];
        assert_eq!(req["response_format"], serde_json::json!({"type":"json_object"}));
        assert_eq!(req["messages"][0]["role"], "system");
        assert!(req["messages"][0]["content"].as_str().unwrap().contains("\"additionalProperties\":false"));
        assert_eq!(req["messages"][1]["content"], "x");
    }

    #[tokio::test]
    async fn none_tier_errors_without_request_and_tells_user_to_switch() {
        let (url, seen) = serve_json(vec![]);
        let e = call(&url, StructuredMode::None).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(e.message.contains("换用支持的模型"), "{}", e.message);
        assert!(seen.lock().unwrap().is_empty(), "none 档不得发请求");
    }

    #[tokio::test]
    async fn schema_violation_is_parse_with_json_path_and_no_retry() {
        let mut v: Value = serde_json::from_str(&plan_json()).unwrap();
        v.as_object_mut().unwrap().insert("bogus".into(), Value::Bool(true));
        let (url, seen) = serve_json(vec![(200, completion(&v.to_string(), "stop")), (200, completion(&plan_json(), "stop"))]);
        let e = call(&url, StructuredMode::JsonSchema).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.message.contains("schema 校验") && e.message.contains("bogus"), "{}", e.message);
        assert_eq!(seen.lock().unwrap().len(), 1, "校验失败不重发、不发修复提示");
    }

    #[tokio::test]
    async fn non_json_text_is_parse_not_free_text_fallback() {
        let (url, _) = serve_json(vec![(200, completion("好的，方案如下：……", "stop"))]);
        let e = call(&url, StructuredMode::JsonObject).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.message.contains("不是合法 JSON"), "{}", e.message);
    }

    #[tokio::test]
    async fn truncation_resends_same_request_exactly_once() {
        let (url, seen) = serve_json(vec![(200, completion("{\"sch", "length")), (200, completion(&plan_json(), "stop"))]);
        call(&url, StructuredMode::JsonSchema).await.expect("重发一次后应通过");
        {
            let s = seen.lock().unwrap();
            assert_eq!(s.len(), 2);
            assert_eq!(s[0], s[1], "必须是同请求重发");
        }

        let (url, seen) = serve_json(vec![(200, completion("{", "length")), (200, completion("{", "length")), (200, completion(&plan_json(), "stop"))]);
        let e = call(&url, StructuredMode::JsonSchema).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.message.contains("截断"), "{}", e.message);
        assert_eq!(seen.lock().unwrap().len(), 2, "最多重发 1 次");
    }

    async fn probe(responses: Vec<(u16, String)>) -> (Result<StructuredMode, AppError>, Vec<Value>) {
        let (url, seen) = serve_json(responses);
        let r = probe_structured_caps_unchecked(&url, "k", "m").await;
        let s = seen.lock().unwrap().clone();
        (r, s)
    }

    #[tokio::test]
    async fn probe_tiers_schema_object_none() {
        let ok = || (200, completion("{\"ok\":true}", "stop"));
        let bad = || (400, "{\"error\":\"response_format unsupported\"}".to_string());
        let (r, s) = probe(vec![ok()]).await;
        assert_eq!(r.unwrap(), StructuredMode::JsonSchema);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["response_format"]["type"], "json_schema");

        let (r, s) = probe(vec![bad(), ok()]).await;
        assert_eq!(r.unwrap(), StructuredMode::JsonObject);
        assert_eq!(s[1]["response_format"]["type"], "json_object");

        let (r, s) = probe(vec![bad(), bad()]).await;
        assert_eq!(r.unwrap(), StructuredMode::None);
        assert_eq!(s.len(), 2);

        let (r, s) = probe(vec![(401, "{}".into())]).await;
        assert_eq!(r.unwrap_err().kind, ErrorKind::Auth, "鉴权失败不得被记成能力档");
        assert_eq!(s.len(), 1);
    }

    #[tokio::test]
    async fn probe_command_keeps_url_gate() {
        let (url, seen) = serve_json(vec![(200, completion("{}", "stop"))]);
        let e = probe_structured_caps(url, "k".into(), "m".into()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(seen.lock().unwrap().is_empty(), "闸门拒绝时不得发出请求");
    }
}
