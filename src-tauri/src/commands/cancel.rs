//! B3：用户取消支持——全局取消标志（当前应用为单任务设计，一次仅一条流水线在途）。
//! 检查点：send_with_retry / stream_response / run_pipeline_inner 各阶段入口。
//! A9（run-id）落地后升级为按任务的取消注册表。

use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub(crate) fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

pub(crate) fn reset() {
    CANCELLED.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_flag_roundtrip() {
        reset();
        assert!(!is_cancelled());
        request_cancel();
        assert!(is_cancelled());
        reset();
        assert!(!is_cancelled());
    }
}
