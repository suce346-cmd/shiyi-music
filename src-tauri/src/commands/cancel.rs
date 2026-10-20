//! 用户取消支持——按 run_id 隔离的取消注册表。
//! 检查点：send_with_retry / stream_response / run_pipeline_inner 各阶段入口（均按 run_id 查询）。
//!
//! RULE-RUN-ID（BLUEPRINT_v2 §3.6 / P0-3，C-05/D-06）——定义点：本文件。
//! - 取消**只认非空 run_id**。旧规则空 id 置全局兼容位（原 AtomicBool 静态量）且生产无复位点：
//!   无 run_id 时按"停止"→ `test_api`（传空 id）永远返回"已取消"直到重启（C-05 根因）。
//!   全局位已删除；空 id 的 request_cancel 为 no-op 并返回 false，is_cancelled("") 恒 false。
//! - run 开始 `begin(run_id)` 登记活动；取消只对活动 run 生效（结束后迟到的取消为 no-op）。
//! - run 结束必须 `finish(run_id)` 移除条目（D-06：旧集合只增不减）。

use std::collections::HashSet;
use std::sync::Mutex;

/// 注册表：active = 正在运行的 run；cancelled ⊆ active（取消只对活动 run 生效）。
#[derive(Default)]
struct Registry {
    active: HashSet<String>,
    cancelled: HashSet<String>,
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

/// run 开始：登记为活动并清掉同 id 的旧取消位（run_pipeline / pipeline_resume 入口调）。空 id no-op。
pub(crate) fn begin(run_id: &str) {
    if is_blank(run_id) {
        return;
    }
    let mut r = lock();
    r.cancelled.remove(run_id);
    r.active.insert(run_id.to_string());
}

/// 请求取消指定 run（前端"停止"按钮调）。仅对**活动** run 生效并返回 true；
/// 空 id、未开始或已结束的 id 一律 no-op 返回 false（结束后迟到的取消不留残留条目）。
pub(crate) fn request_cancel(run_id: &str) -> bool {
    if is_blank(run_id) {
        return false;
    }
    let mut r = lock();
    if !r.active.contains(run_id) {
        return false;
    }
    r.cancelled.insert(run_id.to_string());
    true
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
    r.active.remove(run_id);
    r.cancelled.remove(run_id);
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
        begin("r1");
        begin("r2");
        assert!(!is_cancelled("r1"));
        assert!(request_cancel("r1"));
        assert!(is_cancelled("r1"));
        assert!(!is_cancelled("r2"), "取消 r1 不得影响 r2");
        finish("r1");
        finish("r2");
        assert!(!is_cancelled("r1"));
    }

    /// C-05 回归：空 id 取消是 no-op
    #[test]
    fn empty_run_id_cancel_is_noop() {
        begin("");
        assert!(!request_cancel(""));
        assert!(!request_cancel("   "));
        assert!(!is_cancelled(""), "空 id 恒不处于取消态");
        assert!(!is_active(""));
    }

    /// D-06：finish 移除活动与取消条目，幂等
    #[test]
    fn finish_removes_entry() {
        let id = "finish-d06";
        begin(id);
        assert!(request_cancel(id));
        finish(id);
        assert!(!is_cancelled(id));
        assert!(!is_active(id));
        finish(id);
        finish("");
    }

    /// 残留 #1：未开始 / 已结束的 run 收到取消 → no-op，不留条目
    #[test]
    fn cancel_on_inactive_run_is_noop() {
        assert!(!request_cancel("never-started"));
        assert!(!is_cancelled("never-started"));
        let id = "late-cancel";
        begin(id);
        finish(id);
        assert!(!request_cancel(id), "结束后迟到的取消必须 no-op");
        assert!(!is_cancelled(id));
        assert!(!lock().cancelled.contains(id), "不得留残留条目");
    }

    /// 取消 A 后立即开 B：B 不受影响；A 结束后对 A 的再取消 no-op；同 id 重开清旧取消位
    #[test]
    fn cancel_then_new_run_isolated() {
        begin("seq-a");
        assert!(request_cancel("seq-a"));
        begin("seq-b");
        assert!(!is_cancelled("seq-b"));
        finish("seq-a");
        assert!(!request_cancel("seq-a"));
        assert!(!is_cancelled("seq-b"));
        finish("seq-b");
        begin("seq-a");
        assert!(!is_cancelled("seq-a"), "同 id 重开须清旧取消位");
        finish("seq-a");
    }

    /// 守护：全局兼容位不得回流（源码扫描）
    #[test]
    fn no_global_cancel_flag() {
        let src = include_str!("cancel.rs");
        let needle = ["CANCELLED", "_GLOBAL"].concat();
        assert_eq!(src.matches(needle.as_str()).count(), 0, "RULE-RUN-ID：禁止恢复全局取消位");
    }
}
