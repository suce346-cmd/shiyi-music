//! 用户取消支持——按 run_id 隔离的取消注册表。
//! 检查点：send_with_retry / stream_response / run_pipeline_inner 各阶段入口（均按 run_id 查询）。
//! 单任务时代是全局标志；多任务并行互不干扰。空 run_id（旧内部调用）走全局兼容位。

use std::collections::HashSet;
use std::sync::Mutex;

static CANCELLED_RUNS: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));
/// 兼容位：run_id 为空时的全局取消（旧测试/内部路径）
static CANCELLED_GLOBAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 请求取消指定 run（前端"停止"按钮调，带 run_id）
pub(crate) fn request_cancel(run_id: &str) {
    if run_id.trim().is_empty() {
        CANCELLED_GLOBAL.store(true, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    // O-3：锁中毒恢复——poisoned 的 Mutex 里的 HashSet 本身仍一致，取回内层数据继续（取消位宁可可用）
    let mut set = CANCELLED_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    set.insert(run_id.to_string());
}

/// 指定 run 是否被取消（空 run_id 查全局兼容位）
pub(crate) fn is_cancelled(run_id: &str) -> bool {
    if run_id.trim().is_empty() {
        return CANCELLED_GLOBAL.load(std::sync::atomic::Ordering::SeqCst);
    }
    CANCELLED_RUNS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(run_id)
}

/// 新 run 清理取消位（与 interject::reset 同位置调用）
/// B-6 语义收窄：具名 reset 只删自身条目（并行互不干扰）；空参只清全局位。
/// 旧规则具名 reset 连带清全局——会误清其他遗留空 id 任务的取消状态。
pub(crate) fn reset(run_id: &str) {
    if run_id.trim().is_empty() {
        CANCELLED_GLOBAL.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    let mut set = CANCELLED_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    set.remove(run_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_isolated_per_run() {
        reset("r1");
        reset("r2");
        assert!(!is_cancelled("r1"));
        request_cancel("r1");
        assert!(is_cancelled("r1"));
        assert!(!is_cancelled("r2"), "取消 r1 不得影响 r2");
        reset("r1");
        assert!(!is_cancelled("r1"));
    }

    #[test]
    fn cancel_empty_run_id_uses_global() {
        CANCELLED_GLOBAL.store(false, std::sync::atomic::Ordering::SeqCst);
        request_cancel("");
        assert!(is_cancelled(""));
        assert!(!is_cancelled("other"), "全局位不污染具名 run");
        reset("");
    }

    /// Q4：空参只置全局位（停空 id 遗留任务）；具名任务必须带 id 停
    #[test]
    fn empty_cancel_only_sets_global() {
        reset("qx1");
        CANCELLED_GLOBAL.store(false, std::sync::atomic::Ordering::SeqCst);
        request_cancel("");
        assert!(is_cancelled(""));
        assert!(!is_cancelled("qx1"), "空参不污染具名 run；具名任务须带 id 取消");
        reset("");
    }
}
