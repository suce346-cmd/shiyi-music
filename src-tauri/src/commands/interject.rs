//! 用户中途插话——按 run_id 隔离的非阻塞意见槽（轮边界消费，不中断在途 LLM 调用）。
//! 设计：
//! - 按任务隔离：多任务并行时插话进各自任务。
//! - 非阻塞：drain 拿走即清空；轮边界（每轮讨论开始前）全取，注入 revisions_log + next_tasks。
//! - 与取消互斥无关：插话不终止任务，只影响下一轮输入。
//! - 与 F1 复用 targets 机制：插话文本同样经 roles_for_feedback 路由（调用方做）。

use std::collections::HashMap;
use std::sync::Mutex;

static SLOTS: std::sync::LazyLock<Mutex<HashMap<String, Vec<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 存入一条用户意见（前端"插入意见"按钮调，带 run_id；多条累积，轮边界一次性消费）
pub(crate) fn push(run_id: &str, text: String) {
    if let Ok(mut slots) = SLOTS.lock() {
        slots.entry(run_id.to_string()).or_default().push(text);
    }
}

/// 取走指定 run 的全部累积意见（轮边界调用；拿走即清空，无残留）
pub(crate) fn drain(run_id: &str) -> Vec<String> {
    if let Ok(mut slots) = SLOTS.lock() {
        slots.remove(run_id).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// 新 run 清槽（防上一轮残留污染；与 cancel::reset 同位置调用）
pub(crate) fn reset(run_id: &str) {
    if let Ok(mut slots) = SLOTS.lock() {
        slots.remove(run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interject_isolated_per_run() {
        reset("r1");
        reset("r2");
        assert!(drain("r1").is_empty());
        push("r1", "意见一".to_string());
        push("r2", "意见二".to_string());
        assert_eq!(drain("r1"), vec!["意见一".to_string()]);
        // 拿走即清空；r2 不受影响
        assert!(drain("r1").is_empty());
        assert_eq!(drain("r2"), vec!["意见二".to_string()]);
        push("r1", "残留".to_string());
        reset("r1");
        assert!(drain("r1").is_empty());
    }
}
