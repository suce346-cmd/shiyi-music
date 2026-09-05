//! F10：用户中途插话——非阻塞意见槽（轮边界消费，不中断在途 LLM 调用）。
//! 设计：
//! - 单任务全局槽（与 cancel.rs 同约束：一次仅一条流水线在途；A9 run-id 化后升级）。
//! - 非阻塞：take() 拿走即清空；轮边界（每轮讨论开始前）drain 全取，注入 revisions_log + next_tasks。
//! - 与取消互斥无关：插话不终止任务，只影响下一轮输入。
//! - 与 F1 复用 targets 机制：插话文本同样经 roles_for_feedback 路由（调用方做）。

use std::sync::Mutex;

static SLOT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 存入一条用户意见（前端"插入意见"按钮调；多条累积，轮边界一次性消费）
pub(crate) fn push(text: String) {
    if let Ok(mut slot) = SLOT.lock() {
        slot.push(text);
    }
}

/// 取走全部累积意见（轮边界调用；拿走即清空，无残留）
pub(crate) fn drain() -> Vec<String> {
    if let Ok(mut slot) = SLOT.lock() {
        std::mem::take(&mut *slot)
    } else {
        Vec::new()
    }
}

/// 新 run 清槽（防上一轮残留污染；与 cancel::reset 同位置调用）
pub(crate) fn reset() {
    if let Ok(mut slot) = SLOT.lock() {
        slot.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interject_push_drain_reset() {
        reset();
        assert!(drain().is_empty());
        push("意见一".to_string());
        push("意见二".to_string());
        assert_eq!(drain(), vec!["意见一".to_string(), "意见二".to_string()]);
        // 拿走即清空
        assert!(drain().is_empty());
        push("残留".to_string());
        reset();
        assert!(drain().is_empty());
    }
}
