//! 轮间人工确认门（#12）——按 run_id 隔离的"暂停并等待用户决断"槽。
//!
//! 与相邻机制的分工（三者职责正交，不重叠）：
//! - `interject`：用户**随时**投意见（非阻塞，流水线照跑，轮边界消费）；
//! - `gate`：流水线在**轮末真的停下**，等用户选"继续下一轮"或"结束讨论直接出终稿"；
//! - `cancel`：用户在暂停中点"停止" → 等待循环轮询取消位，立即终止（复用既有取消链路）。
//!
//! 暂停期间提交的意见由**下一轮开头**的 `interject::drain` 消费——只有"继续"才有下一轮，
//! 故语义自洽；选"结束讨论"时残留意见由编排层显式记账（不静默丢失）。
//!
//! 设计要点：等待有上限（`max_wait`）——无人确认时按 `Timeout` 放行并如实标记降级，
//! 绝不因用户离开而把整条流水线挂死到超时强杀。

use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

/// 门内轮询间隔（取消/超时的发现延迟上限；不消耗 LLM 预算）
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// 用户的决断
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateDecision {
    /// 继续下一轮讨论
    Continue,
    /// 结束讨论，直接进终稿
    Finalize,
    /// 等待超时（无人确认）——按"继续"放行，并由编排层如实标记降级
    Timeout,
}

impl GateDecision {
    /// 决断的线上字符串（事件/前端/日志同源，wire format 由 models 层锁定）
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            GateDecision::Continue => "continue",
            GateDecision::Finalize => "finalize",
            GateDecision::Timeout => "timeout",
        }
    }

    /// 从前端命令入参解析——只接受用户可选的两种决断。
    /// `timeout` 由后端在等待超时后自行产生，**不接受外部注入**（防伪造"自动放行"）。
    pub(crate) fn parse(s: &str) -> Option<GateDecision> {
        match s.trim().to_ascii_lowercase().as_str() {
            "continue" => Some(GateDecision::Continue),
            "finalize" => Some(GateDecision::Finalize),
            _ => None,
        }
    }
}

/// 等待者的发送端（决断回送用）
static GATES: std::sync::LazyLock<Mutex<HashMap<String, oneshot::Sender<GateDecision>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 开一道门并返回接收端。
/// 同 run 重复开：旧发送端被丢弃 → 旧等待者收到 `RecvError` → 按 `Timeout`（自动放行）退出，
/// 不会悬挂（一个 run 同时只应有一道门；重复开属防御路径）。
fn arm(run_id: &str) -> oneshot::Receiver<GateDecision> {
    let (tx, rx) = oneshot::channel();
    let mut gates = GATES.lock().unwrap_or_else(|e| e.into_inner());
    gates.insert(run_id.to_string(), tx);
    rx
}

/// 前端决断回送。返回 false = 当前没有等待中的门（前端据此提示，而非静默丢弃）。
pub(crate) fn resolve(run_id: &str, decision: GateDecision) -> bool {
    let mut gates = GATES.lock().unwrap_or_else(|e| e.into_inner());
    match gates.remove(run_id) {
        Some(tx) => tx.send(decision).is_ok(),
        None => false,
    }
}

/// 清槽（run 入口与门结束同位置调用，防上一轮残留污染）
pub(crate) fn reset(run_id: &str) {
    let mut gates = GATES.lock().unwrap_or_else(|e| e.into_inner());
    gates.remove(run_id);
}

/// 是否有等待中的门（供测试与防御性自检）
#[cfg(test)]
pub(crate) fn is_pending(run_id: &str) -> bool {
    GATES.lock().unwrap_or_else(|e| e.into_inner()).contains_key(run_id)
}

/// 暂停并等待用户决断。
/// - 用户决断 → `Ok(决断)`；
/// - `max_wait` 到期仍未决断 → `Ok(GateDecision::Timeout)`（永不挂死）；
/// - 等待期间用户取消 → `Err(cancelled)`（与流水线其余取消点同语义）。
pub(crate) async fn wait(
    run_id: &str,
    max_wait: std::time::Duration,
) -> Result<GateDecision, crate::errors::AppError> {
    let mut rx = arm(run_id);
    let deadline = tokio::time::Instant::now() + max_wait;
    loop {
        // 取消优先于等待（与 send_with_retry 同口径）
        if crate::commands::cancel::is_cancelled(run_id) {
            reset(run_id);
            return Err(crate::errors::AppError::cancelled());
        }
        tokio::select! {
            got = &mut rx => {
                reset(run_id);
                // 发送端被丢弃（重置/重复开）→ 门已不可达，按超时放行，不悬挂
                return Ok(got.unwrap_or(GateDecision::Timeout));
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {
                if tokio::time::Instant::now() >= deadline {
                    reset(run_id);
                    return Ok(GateDecision::Timeout);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 决断协议锁：线上字符串与前端口径同源（改字面量即红）
    #[test]
    fn gate_decision_protocol_is_single_sourced() {
        assert_eq!(GateDecision::Continue.as_str(), "continue");
        assert_eq!(GateDecision::Finalize.as_str(), "finalize");
        assert_eq!(GateDecision::Timeout.as_str(), "timeout");
        assert_eq!(GateDecision::parse("  CONTINUE "), Some(GateDecision::Continue));
        assert_eq!(GateDecision::parse("finalize"), Some(GateDecision::Finalize));
        // timeout 不可由外部注入（只能后端自产）
        assert_eq!(GateDecision::parse("timeout"), None);
        assert_eq!(GateDecision::parse(""), None);
        assert_eq!(GateDecision::parse("garbage"), None);
    }

    /// 用户决断回送即放行，且与决断一致
    #[tokio::test]
    async fn resolve_delivers_decision() {
        reset("g1");
        let waiter = tokio::spawn(async { wait("g1", Duration::from_secs(5)).await });
        // 等门开
        for _ in 0..50 {
            if is_pending("g1") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(is_pending("g1"), "门未开启");
        assert!(resolve("g1", GateDecision::Finalize), "决断应送达");
        assert_eq!(waiter.await.unwrap().unwrap(), GateDecision::Finalize);
        assert!(!is_pending("g1"), "决断后门槽应清空");
    }

    /// 无人确认 = 超时自动放行（绝不挂死），且清槽
    #[tokio::test]
    async fn wait_times_out_instead_of_hanging() {
        reset("g2");
        let d = wait("g2", Duration::from_millis(150)).await.unwrap();
        assert_eq!(d, GateDecision::Timeout);
        assert!(!is_pending("g2"));
    }

    /// 无门在等时回送 → false（前端据此提示，不静默丢弃）
    #[test]
    fn resolve_without_pending_gate_reports_false() {
        reset("g3");
        assert!(!resolve("g3", GateDecision::Continue));
    }

    /// 暂停期间点"停止" → 取消优先，立即返回 cancelled
    #[tokio::test]
    async fn wait_aborts_on_cancel() {
        crate::commands::cancel::reset("g4");
        let waiter = tokio::spawn(async { wait("g4", Duration::from_secs(30)).await });
        for _ in 0..50 {
            if is_pending("g4") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        crate::commands::cancel::request_cancel("g4");
        let err = waiter.await.unwrap().unwrap_err();
        assert_eq!(err.kind, crate::errors::ErrorKind::Cancelled);
        assert!(!is_pending("g4"), "取消后门槽应清空");
        crate::commands::cancel::reset("g4");
    }

    /// 重复开同一 run 的门：旧等待者按超时放行（不悬挂），新门可正常决断
    #[tokio::test]
    async fn rearm_does_not_hang_stale_waiter() {
        reset("g5");
        let stale = tokio::spawn(async { wait("g5", Duration::from_secs(30)).await });
        for _ in 0..50 {
            if is_pending("g5") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // 第二次 arm（丢弃旧发送端）
        let _rx = arm("g5");
        assert_eq!(stale.await.unwrap().unwrap(), GateDecision::Timeout);
        reset("g5");
    }
}