//! 用户取消支持——按 run_id 隔离的取消注册表。
//! 检查点：send_with_retry / stream_response / run_pipeline_inner 各阶段入口（均按 run_id 查询）。
//!
//! RULE-RUN-ID（BLUEPRINT_v2 §3.6 / P0-3，C-05/D-06）——定义点：本文件。
//! - 取消**只认非空 run_id**。旧规则空 id 置全局兼容位（原 AtomicBool 静态量）且生产无复位点：
//!   无 run_id 时按"停止"→ `test_api`（传空 id）永远返回"已取消"直到重启（C-05 根因）。
//!   全局位已删除；空 id 的 request_cancel 为 no-op 并返回 false，is_cancelled("") 恒 false。
//! - run 开始 `begin(run_id)` 登记活动；取消只对活动 run 生效（结束后迟到的取消为 no-op）。
//! - run 结束必须 `finish(run_id)` 移除条目（D-06：旧集合只增不减）。
//!   F2（P0 审计）：`begin` 返回 RAII `ActiveRun` 守卫，Drop 时 finish——future 被丢弃
//!   （超时 abort / 调用方取消 / panic unwind）也必然清理，不再依赖"每条返回路径都记得 finish"。
//! - F3（P0 审计）：**begin 之前的取消**（前端已生成 run_id、后端尚未登记的竞态窗口）
//!   记为 pending（TTL `PENDING_TTL`），`begin` 消费后该 run 立即处于取消态；
//!   已结束 run 的迟到取消（TTL 内记得 finished）仍为 no-op。`request_cancel` 返回
//!   `CancelOutcome`，`cancel_pipeline` 据此告诉前端是否生效。
//! - **F2 孤儿 future**：`with_active_run` 的 future 被丢弃 / 超时 abort 时，`ActiveRun` Drop
//!   仍 `finish`（活动表不泄漏）；协作式取消点是否已被轮询到不在此保证。
//! - **F3 30s 边界**：pending/finished 超过 `PENDING_TTL` 即剪除；超时后同 id 迟到停止为 Noop。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// pending（begin 前取消）与 finished（迟到取消判定）记忆时长；到期即剪除（不留无界条目）。
pub(crate) const PENDING_TTL: Duration = Duration::from_secs(30);

/// 注册表：active = 正在运行的 run；cancelled ⊆ active（取消只对活动 run 生效）。
#[derive(Default)]
struct Registry {
    active: HashSet<String>,
    cancelled: HashSet<String>,
    /// begin 之前到达的取消（id → 到达时刻）
    pending: HashMap<String, Instant>,
    /// 最近结束的 run（id → 结束时刻）：迟到取消据此判 no-op，而不是误记成 pending
    finished: HashMap<String, Instant>,
}

impl Registry {
    fn prune(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, t| now.duration_since(*t) < PENDING_TTL);
        self.finished.retain(|_, t| now.duration_since(*t) < PENDING_TTL);
    }
}

/// `request_cancel` 结果（前端据此反映"停止"是否生效）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelOutcome {
    /// 活动 run 已置取消位
    Cancelled,
    /// run 尚未登记：记为 pending，begin 时立即生效
    Pending,
    /// 空 id / 已结束：无效果
    Noop,
}

impl CancelOutcome {
    pub(crate) fn effective(self) -> bool {
        self != CancelOutcome::Noop
    }
}

/// RAII 活动 run 守卫：Drop 即 finish（F2）。
#[must_use = "守卫一旦丢弃 run 即结束；须绑定到变量并活过整个 run"]
pub(crate) struct ActiveRun {
    run_id: String,
}

impl Drop for ActiveRun {
    fn drop(&mut self) {
        finish(&self.run_id);
    }
}

static REGISTRY: std::sync::LazyLock<Mutex<Registry>> =
    std::sync::LazyLock::new(|| Mutex::new(Registry::default()));

fn lock() -> std::sync::MutexGuard<'static, Registry> {
    // O-3：锁中毒恢复——内层集合本身仍一致，取回继续（取消位宁可可用）
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_blank(run_id: &str) -> bool {
    run_id.trim().is_empty()
}

/// run 开始：登记为活动，清掉同 id 的旧取消位；若有 pending 取消则消费并立即置取消态（F3）。
/// 返回 RAII 守卫（Drop = finish）。空 id 不登记（守卫 Drop 为 no-op）。
pub(crate) fn begin(run_id: &str) -> ActiveRun {
    let guard = ActiveRun { run_id: run_id.to_string() };
    if is_blank(run_id) {
        return guard;
    }
    let mut r = lock();
    r.prune();
    r.cancelled.remove(run_id);
    r.finished.remove(run_id);
    if r.pending.remove(run_id).is_some() {
        r.cancelled.insert(run_id.to_string());
    }
    r.active.insert(run_id.to_string());
    guard
}

/// F2：run 包裹器（单源）——登记活动 run，future 结束或被丢弃时守卫 finish。
/// `run_pipeline` 与 `pipeline_resume` 必须经此入口（源码锁 `run_entries_use_with_active_run`）。
pub(crate) async fn with_active_run<F: std::future::Future>(run_id: &str, fut: F) -> F::Output {
    let _active = begin(run_id);
    fut.await
}

/// 请求取消指定 run（前端"停止"按钮调）。活动 run → Cancelled；尚未登记 → Pending（F3）；
/// 空 id / TTL 内已结束 → Noop（迟到取消不留 cancelled 条目）。
pub(crate) fn request_cancel(run_id: &str) -> CancelOutcome {
    if is_blank(run_id) {
        return CancelOutcome::Noop;
    }
    let mut r = lock();
    r.prune();
    if r.active.contains(run_id) {
        r.cancelled.insert(run_id.to_string());
        return CancelOutcome::Cancelled;
    }
    if r.finished.contains_key(run_id) {
        return CancelOutcome::Noop;
    }
    r.pending.insert(run_id.to_string(), Instant::now());
    CancelOutcome::Pending
}

/// 指定 run 是否被取消（空 id 恒 false：无归属的调用不受任何取消影响）
pub(crate) fn is_cancelled(run_id: &str) -> bool {
    if is_blank(run_id) {
        return false;
    }
    lock().cancelled.contains(run_id)
}

/// D-06：run 结束（成功/失败/取消/超时）移除活动与取消条目。幂等；空 id no-op。
pub(crate) fn finish(run_id: &str) {
    if is_blank(run_id) {
        return;
    }
    let mut r = lock();
    r.prune();
    let was_active = r.active.remove(run_id);
    r.cancelled.remove(run_id);
    r.pending.remove(run_id);
    if was_active {
        r.finished.insert(run_id.to_string(), Instant::now());
    }
}

#[cfg(test)]
pub(crate) fn is_active(run_id: &str) -> bool {
    lock().active.contains(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_isolated_per_run() {
        let g1 = begin("r1");
        let _g2 = begin("r2");
        assert!(!is_cancelled("r1"));
        assert_eq!(request_cancel("r1"), CancelOutcome::Cancelled);
        assert!(is_cancelled("r1"));
        assert!(!is_cancelled("r2"), "取消 r1 不得影响 r2");
        drop(g1);
        assert!(!is_cancelled("r1"));
    }

    /// C-05 回归：空 id 取消是 no-op
    #[test]
    fn empty_run_id_cancel_is_noop() {
        let _g = begin("");
        assert_eq!(request_cancel(""), CancelOutcome::Noop);
        assert_eq!(request_cancel("   "), CancelOutcome::Noop);
        assert!(!is_cancelled(""), "空 id 恒不处于取消态");
        assert!(!is_active(""));
    }

    /// D-06：finish 移除活动与取消条目，幂等
    #[test]
    fn finish_removes_entry() {
        let id = "finish-d06";
        let g = begin(id);
        assert!(request_cancel(id).effective());
        drop(g);
        assert!(!is_cancelled(id));
        assert!(!is_active(id));
        finish(id);
        finish("");
    }

    /// 残留 #1：已结束的 run 收到取消 → no-op，不留条目
    #[test]
    fn late_cancel_after_finish_is_noop() {
        let id = "late-cancel";
        drop(begin(id));
        assert_eq!(request_cancel(id), CancelOutcome::Noop, "结束后迟到的取消必须 no-op");
        assert!(!is_cancelled(id));
        let r = lock();
        assert!(!r.cancelled.contains(id) && !r.pending.contains_key(id), "不得留残留条目");
    }

    /// F3：begin 之前到达的取消 → Pending；begin 消费后立即处于取消态；finish 后不留条目
    #[test]
    fn cancel_before_begin_is_consumed_by_begin() {
        let id = "cancel-before-begin";
        assert_eq!(request_cancel(id), CancelOutcome::Pending);
        assert!(!is_cancelled(id), "未登记前不处于取消态");
        let g = begin(id);
        assert!(is_cancelled(id), "pending 取消必须被 begin 消费并生效");
        drop(g);
        let r = lock();
        assert!(!r.pending.contains_key(id) && !r.cancelled.contains(id) && !r.active.contains(id));
    }

    /// F2：守卫 Drop 即 finish——包括 future 在 await 中途被丢弃
    #[tokio::test]
    async fn guard_finishes_when_future_dropped() {
        let id = "dropped-future";
        let fut = async {
            let _g = begin(id);
            std::future::pending::<()>().await;
        };
        let _ = tokio::time::timeout(Duration::from_millis(20), fut).await;
        assert!(!is_active(id), "被丢弃的 future 必须经守卫 finish");
    }

    /// F2：with_active_run 运行期间登记活动、结束后 finish；被丢弃同样 finish
    #[tokio::test]
    async fn with_active_run_registers_during_and_finishes_after() {
        let id = "war-normal";
        let seen = with_active_run(id, async { is_active(id) }).await;
        assert!(seen, "运行期间必须处于活动态（守卫被立即丢弃时此处为 false）");
        assert!(!is_active(id), "结束后必须 finish");
        let id2 = "war-dropped";
        let _ = tokio::time::timeout(Duration::from_millis(20), with_active_run(id2, std::future::pending::<()>())).await;
        assert!(!is_active(id2));
        // F3：begin 前的取消在 run 开始即生效
        let id3 = "war-pending";
        assert_eq!(request_cancel(id3), CancelOutcome::Pending);
        assert!(with_active_run(id3, async { is_cancelled(id3) }).await);
    }

    /// 取消 A 后立即开 B：B 不受影响；A 结束后对 A 的再取消 no-op；同 id 重开清旧取消位
    #[test]
    fn cancel_then_new_run_isolated() {
        let ga = begin("seq-a");
        assert!(request_cancel("seq-a").effective());
        let gb = begin("seq-b");
        assert!(!is_cancelled("seq-b"));
        drop(ga);
        assert_eq!(request_cancel("seq-a"), CancelOutcome::Noop);
        assert!(!is_cancelled("seq-b"));
        drop(gb);
        let ga2 = begin("seq-a");
        assert!(!is_cancelled("seq-a"), "同 id 重开须清旧取消位");
        drop(ga2);
    }

    /// 守护：全局兼容位不得回流（源码扫描）
    #[test]
    fn no_global_cancel_flag() {
        let src = include_str!("cancel.rs");
        let needle = ["CANCELLED", "_GLOBAL"].concat();
        assert_eq!(src.matches(needle.as_str()).count(), 0, "RULE-RUN-ID：禁止恢复全局取消位");
    }
}
