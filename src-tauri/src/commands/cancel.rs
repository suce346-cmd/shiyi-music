//! 用户取消支持——按 run_id 隔离的取消注册表。
//! 检查点：send_with_retry / stream_response / run_pipeline_inner 各阶段入口（均按 run_id 查询）。
//!
//! RULE-RUN-ID（BLUEPRINT_v2 §3.6 / P0-3，C-05/D-06）——定义点：本文件。
//! - 取消**只认非空 run_id**。旧规则空 id 置全局兼容位（原 AtomicBool 静态量）且生产无复位点：
//!   无 run_id 时按"停止"→ `test_api`（传空 id）永远返回"已取消"直到重启（C-05 根因）。
//!   全局位已删除；空 id 的 request_cancel 为 no-op 并返回 false，is_cancelled("") 恒 false。
//! - run 结束必须 `finish(run_id)` 移除条目（D-06：旧集合只增不减）。

use std::collections::HashSet;
use std::sync::Mutex;

static CANCELLED_RUNS: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

fn is_blank(run_id: &str) -> bool {
    run_id.trim().is_empty()
}

/// 请求取消指定 run（前端"停止"按钮调，带 run_id）。空 id 不做任何事，返回 false。
pub(crate) fn request_cancel(run_id: &str) -> bool {
    if is_blank(run_id) {
        return false;
    }
    // O-3：锁中毒恢复——poisoned 的 Mutex 里的 HashSet 本身仍一致，取回内层数据继续（取消位宁可可用）
    let mut set = CANCELLED_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    set.insert(run_id.to_string());
    true
}

/// 指定 run 是否被取消（空 id 恒 false：无归属的调用不受任何取消影响）
pub(crate) fn is_cancelled(run_id: &str) -> bool {
    if is_blank(run_id) {
        return false;
    }
    CANCELLED_RUNS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(run_id)
}

/// 新 run 清理取消位（与 interject::reset 同位置调用）；空 id no-op。
pub(crate) fn reset(run_id: &str) {
    if is_blank(run_id) {
        return;
    }
    let mut set = CANCELLED_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    set.remove(run_id);
}

/// D-06：run 结束（成功/失败/取消/超时）移除条目，集合不再只增不减。语义同 reset，命名区分调用点意图。
pub(crate) fn finish(run_id: &str) {
    reset(run_id);
}

#[cfg(test)]
pub(crate) fn tracked_len() -> usize {
    CANCELLED_RUNS.lock().unwrap_or_else(|e| e.into_inner()).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_isolated_per_run() {
        reset("r1");
        reset("r2");
        assert!(!is_cancelled("r1"));
        assert!(request_cancel("r1"));
        assert!(is_cancelled("r1"));
        assert!(!is_cancelled("r2"), "取消 r1 不得影响 r2");
        reset("r1");
        assert!(!is_cancelled("r1"));
    }

    /// C-05 回归：空 id 取消是 no-op——不得出现任何"全局取消位"污染后续空 id 调用（如 test_api）
    #[test]
    fn empty_run_id_cancel_is_noop() {
        assert!(!request_cancel(""));
        assert!(!request_cancel("   "));
        assert!(!is_cancelled(""), "空 id 恒不处于取消态（旧规则：全局位置 1 后永不复位）");
        assert!(!is_cancelled("other-empty-check"));
    }

    /// D-06：finish 移除条目，集合不随 run 累积
    #[test]
    fn finish_removes_entry() {
        let id = "finish-d06";
        assert!(request_cancel(id));
        assert!(is_cancelled(id));
        finish(id);
        assert!(!is_cancelled(id));
        // 同 id 多次 finish 幂等
        finish(id);
        finish("");
        let _ = tracked_len();
    }

    /// 守护：全局兼容位不得回流（源码扫描）
    #[test]
    fn no_global_cancel_flag() {
        let src = include_str!("cancel.rs");
        let needle = ["CANCELLED", "_GLOBAL"].concat();
        let count = src.matches(needle.as_str()).count();
        assert_eq!(count, 0, "RULE-RUN-ID：禁止恢复全局取消位");
    }
}
