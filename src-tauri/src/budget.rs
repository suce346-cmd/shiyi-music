//! 流水线级共享预算——全局耗时在数学上不可能超过 deadline。
//! 与相关项的关系：预算是"事前约束"（勤俭），B1 的 abort 是"事后强杀"（最后防线）。
//! 与相关项的关系：用户取消优先于预算（取消检查排在预算检查之前）。
//! 并发安全：Budget 只读 Instant，对 Arc 共享无锁。

use std::sync::Arc;
use std::time::{Duration, Instant};

/// 流水线共享预算：创建时定死 deadline，调用链逐层透传
#[derive(Debug, Clone)]
pub struct Budget {
    deadline: Instant,
}

impl Budget {
    /// 以"从现在起 dur"为限创建
    pub fn with_timeout(dur: Duration) -> Self {
        Self { deadline: Instant::now() + dur }
    }

    /// 测试/兜底：几乎无限（1 小时，远超任何单轮耗时）
    #[allow(dead_code)] // 测试专用（生产走 with_timeout）
    pub fn unlimited() -> Self {
        Self::with_timeout(Duration::from_secs(3600))
    }

    /// 剩余时间（已耗尽返回 Duration::ZERO）
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// 是否还有至少 `need` 的预算
    pub fn has(&self, need: Duration) -> bool {
        self.remaining() >= need
    }

    /// 讨论轮可用预算：从剩余中预留终稿保底额度（R1）。
    /// 退避等待按该值截断，保证阶段 2 至少有一次完整尝试 + 等待的额度。
    /// 阶段 2 入口改回读全额 remaining（保底解除），此处只约束讨论轮。
    pub fn remaining_for_discussion(&self, reserve: Duration) -> Duration {
        self.remaining().saturating_sub(reserve)
    }
}

/// Arc 快捷类型：调用链透传用
pub type SharedBudget = Arc<Budget>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_counts_down() {
        let b = Budget::with_timeout(Duration::from_millis(200));
        assert!(b.has(Duration::from_millis(50)));
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(b.remaining(), Duration::ZERO);
        assert!(!b.has(Duration::from_secs(1)));
    }

    #[test]
    fn unlimited_has_plenty() {
        let b = Budget::unlimited();
        assert!(b.has(Duration::from_secs(600)));
    }

    /// R1：讨论轮可用预算 = 剩余 - 保底；剩余不足保底时归零（阶段 2 靠全额 remaining 续命）
    #[test]
    fn remaining_for_discussion_reserves_final_stage() {
        let b = Budget::with_timeout(Duration::from_secs(200));
        let avail = b.remaining_for_discussion(Duration::from_secs(120));
        assert!(avail <= Duration::from_secs(200));
        assert!(avail >= Duration::from_secs(60), "200s 剩余预留 120s 后应剩约 80s，实际 {:?}", avail);
        // 剩余不足保底：归零而非下溢
        let tiny = Budget::with_timeout(Duration::from_secs(10));
        assert_eq!(tiny.remaining_for_discussion(Duration::from_secs(120)), Duration::ZERO);
    }
}
