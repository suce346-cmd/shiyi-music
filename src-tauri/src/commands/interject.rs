//! 用户中途插话——按 run_id 隔离的非阻塞意见槽（轮边界消费，不中断在途 LLM 调用）。
//! 设计：
//! - 按任务隔离：多任务并行时插话进各自任务。
//! - 非阻塞：drain 拿走即清空；轮边界（每轮讨论开始前）全取，注入 revisions_log + next_tasks。
//! - 与取消互斥无关：插话不终止任务，只影响下一轮输入。
//! - 与 F1 复用 targets 机制：插话文本同样经 roles_for_feedback 路由（调用方做）。

use std::collections::HashMap;
use std::sync::Mutex;
use crate::errors::{AppError, ErrorKind};

static SLOTS: std::sync::LazyLock<Mutex<HashMap<String, Vec<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 存入一条用户意见（前端"插入意见"按钮调，带 run_id；多条累积，轮边界一次性消费）。
///
/// #26 根因留档：本函数原先自带两个**私有且与准入侧不同源**的限额——
/// 单条长度（局部常量 `MAX_TEXT_CHARS`，值比准入侧上限更小）与条数（`MAX_PER_RUN`），
/// 超限走 `tracing::warn` + `return` **静默丢弃**。而准入侧
/// （`orchestrator::interject_feedback`）用的是 feedback 规则的单源上限：于是"长度介于
/// 两者之间"的意见会**通过校验并回报成功**（前端提示"已送达"），内容却根本没进槽——
/// 上游约束与下游执行不同源造成的假成功，用户无从察觉（旧数值见 git 历史，
/// 本注释不复述，避免第二份拷贝随常量漂移）。
/// 现改为：单条长度规则单一真源 `rules::FEEDBACK_MAX_CHARS`（准入处先拦，此处同值兜底），
/// 条数规则 `rules::INTERJECT_MAX_PER_RUN`；两者越界一律**返回 Validation 错误**
/// 由命令原样透传给前端，绝不静默丢弃。锁中毒恢复（O-3）保留。
pub(crate) fn push(run_id: &str, text: String) -> Result<(), AppError> {
    let n = text.chars().count();
    if n > crate::rules::FEEDBACK_MAX_CHARS {
        return Err(AppError::new(
            ErrorKind::Validation,
            format!(
                "插话过长（{} 字符，上限 {}），请精简后重试",
                n,
                crate::rules::FEEDBACK_MAX_CHARS
            ),
        ));
    }
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    let slot = slots.entry(run_id.to_string()).or_default();
    if slot.len() >= crate::rules::INTERJECT_MAX_PER_RUN {
        return Err(AppError::new(
            ErrorKind::Validation,
            format!(
                "待处理的插话已达上限（{} 条）——本轮意见会在下一轮讨论开始时被消费，请稍后再插",
                crate::rules::INTERJECT_MAX_PER_RUN
            ),
        ));
    }
    slot.push(text);
    Ok(())
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
        push("r1", "意见一".to_string()).unwrap();
        push("r2", "意见二".to_string()).unwrap();
        assert_eq!(drain("r1"), vec!["意见一".to_string()]);
        // 拿走即清空；r2 不受影响
        assert!(drain("r1").is_empty());
        assert_eq!(drain("r2"), vec!["意见二".to_string()]);
        push("r1", "残留".to_string()).unwrap();
        reset("r1");
        assert!(drain("r1").is_empty());
    }

    /// O-4 / #26：两个限额都必须**报错而不是静默丢弃**，且数字单一真源来自 rules.rs。
    /// 旧实现在此静默丢弃（只留 tracing warn）：条数超限丢最新、单条超 1000 字丢弃——
    /// 后者与准入侧的 2000 打架，造成"回报成功、内容没进槽"的假成功。
    #[test]
    fn interject_limits_error_instead_of_silent_drop() {
        reset("cap1");
        for i in 0..crate::rules::INTERJECT_MAX_PER_RUN {
            push("cap1", format!("意见{}", i)).expect("未达上限应存入");
        }
        let e = push("cap1", "第 11 条".to_string()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(e.message.contains(&crate::rules::INTERJECT_MAX_PER_RUN.to_string()), "{}", e.message);
        let drained = drain("cap1");
        assert_eq!(drained.len(), crate::rules::INTERJECT_MAX_PER_RUN, "超限不得改变已存内容");
        assert_eq!(drained[0], "意见0", "保留先到的条目");

        // 单条长度：与准入侧**同一个常量**（rules::FEEDBACK_MAX_CHARS）——等值通过、+1 报错
        reset("cap2");
        let over = "长".repeat(crate::rules::FEEDBACK_MAX_CHARS + 1);
        let e = push("cap2", over).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(drain("cap2").is_empty(), "越界文本不得入槽");
        push("cap2", "长".repeat(crate::rules::FEEDBACK_MAX_CHARS)).expect("等值应通过");
        assert_eq!(drain("cap2").len(), 1);
    }
}
