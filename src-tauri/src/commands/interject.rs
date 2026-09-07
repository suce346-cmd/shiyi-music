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

/// 单 run 插话槽上限（O-4：防 UI 异常循环 push 撑爆内存）；超出丢弃最新并记 warn
const MAX_PER_RUN: usize = 10;
/// 单条插话上限字符数（与前端输入框约束同量级的后端兜底）
const MAX_TEXT_CHARS: usize = 1000;

/// 存入一条用户意见（前端"插入意见"按钮调，带 run_id；多条累积，轮边界一次性消费）
/// O-3/O-4：锁中毒恢复；超限（条数/单条字符）丢弃并记 warn，不撑爆内存
pub(crate) fn push(run_id: &str, text: String) {
    if text.chars().count() > MAX_TEXT_CHARS {
        tracing::warn!(run_id = %run_id, len = text.chars().count(), "插话超长已丢弃");
        return;
    }
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    let slot = slots.entry(run_id.to_string()).or_default();
    if slot.len() >= MAX_PER_RUN {
        tracing::warn!(run_id = %run_id, cap = MAX_PER_RUN, "插话条数达上限，丢弃最新");
        return;
    }
    slot.push(text);
}

/// 取走指定 run 的全部累积意见（轮边界调用；拿走即清空，无残留）
pub(crate) fn drain(run_id: &str) -> Vec<String> {
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    slots.remove(run_id).unwrap_or_default()
}

/// 新 run 清槽（防上一轮残留污染；与 cancel::reset 同位置调用）
pub(crate) fn reset(run_id: &str) {
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    slots.remove(run_id);
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

    /// O-4：插话槽上限——超过 10 条丢弃最新、超长文本丢弃，不撑爆内存
    #[test]
    fn interject_slot_capped() {
        reset("cap1");
        for i in 0..15 {
            push("cap1", format!("意见{}", i));
        }
        let drained = drain("cap1");
        assert_eq!(drained.len(), 10, "单 run 上限 10 条: {:?}", drained.len());
        assert_eq!(drained[9], "意见9", "超限后最新丢弃，保留先到的 10 条");
        // 超长文本丢弃
        reset("cap2");
        push("cap2", "长".repeat(1001));
        assert!(drain("cap2").is_empty(), "超长插话应被丢弃");
    }
}
