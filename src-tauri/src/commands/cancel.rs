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
    if let Ok(mut set) = CANCELLED_RUNS.lock() {
        set.insert(run_id.to_string());
    }
}

/// 指定 run 是否被取消（空 run_id 查全局兼容位）
pub(crate) fn is_cancelled(run_id: &str) -> bool {
    if run_id.trim().is_empty() {
        return CANCELLED_GLOBAL.load(std::sync::atomic::Ordering::SeqCst);
    }
    CANCELLED_RUNS
        .lock()
        .map(|set| set.contains(run_id))
        .unwrap_or(false)
}

/// 新 run 清理指定 run_id 条目 + 全局位（防残留污染；与 interject::reset 同位置调用）
pub(crate) fn reset(run_id: &str) {
    if let Ok(mut set) = CANCELLED_RUNS.lock() {
        set.remove(run_id);
    }
    CANCELLED_GLOBAL.store(false, std::sync::atomic::Ordering::SeqCst);
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
}
