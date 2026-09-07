//! 流水线检查点：讨论轮中间态落盘，失败可续跑（R3）。
//! 与 history.json 的区别：history 只存成功终稿；检查点存进行中的 current_plan +
//! revisions_log + round + next_tasks，供 pipeline_resume 从断点继续。
//! 路径约定：{app_data}/pipeline_checkpoint_{run_id}.json（原子写 .tmp+rename）。
//! 成功后删除；取消不写，避免复活已取消任务。

use crate::errors::{AppError, ErrorKind};
use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};

/// 检查点内容（全部 serde default，前后兼容）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineCheckpoint {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub user_input: String,
    #[serde(default)]
    pub current_plan: String,
    #[serde(default)]
    pub revisions_log: Vec<(String, String)>,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub next_tasks: String,
    #[serde(default)]
    pub updated_at: i64,
}

/// run_id 合法性（路径穿越防线：仅字母数字与 -_.）
pub fn is_valid_run_id(run_id: &str) -> bool {
    !run_id.trim().is_empty()
        && run_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn checkpoint_path<R: Runtime>(app: &tauri::AppHandle<R>, run_id: &str) -> Result<std::path::PathBuf, AppError> {
    if !is_valid_run_id(run_id) {
        return Err(AppError::new(ErrorKind::Validation, "run_id 非法，检查点不可用"));
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("应用数据目录不可用: {}", e)))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("创建数据目录失败: {}", e)))?;
    Ok(dir.join(format!("pipeline_checkpoint_{}.json", run_id)))
}

/// 写检查点（原子写 .tmp+rename；失败只记 warn，不阻断流水线——检查点是锦上添花）
pub fn save<R: Runtime>(app: &tauri::AppHandle<R>, run_id: &str, cp: &PipelineCheckpoint) -> Result<(), AppError> {
    let path = checkpoint_path(app, run_id)?;
    let tmp = path.with_extension("json.tmp");
    let content = serde_json::to_string(cp)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("检查点序列化失败: {}", e)))?;
    std::fs::write(&tmp, content)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("写入检查点失败: {}", e)))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("检查点落盘失败: {}", e)))?;
    Ok(())
}

/// 读检查点（不存在 → None；损坏 → 备份 .bak 后 None，不丢数据且不阻断）
/// B-5：读取 IO 失败不再静默 None（旧规则与"不存在"混在一起，续跑文案误导用户以为任务没跑过）
pub fn load<R: Runtime>(app: &tauri::AppHandle<R>, run_id: &str) -> Option<PipelineCheckpoint> {
    let path = checkpoint_path(app, run_id).ok()?;
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<PipelineCheckpoint>(&content) {
            Ok(cp) => Some(cp),
            Err(e) => {
                let bak = path.with_extension("json.bak");
                let _ = std::fs::rename(&path, &bak);
                tracing::warn!(backup = ?bak, error = %e, "检查点文件损坏已备份");
                None
            }
        },
        Err(e) => {
            tracing::warn!(path = ?path, error = %e, "检查点读取失败（按不存在处理，但原因已记录）");
            None
        }
    }
}

/// 删检查点（成功终稿后调用；文件不存在算成功，幂等）
pub fn clear<R: Runtime>(app: &tauri::AppHandle<R>, run_id: &str) {
    if let Ok(path) = checkpoint_path(app, run_id) {
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 检查点 JSON 前后兼容：缺字段可解析，多字段忽略
    #[test]
    fn checkpoint_serde_compatible() {
        let cp: PipelineCheckpoint = serde_json::from_str("{}").unwrap();
        assert!(cp.current_plan.is_empty());
        assert_eq!(cp.round, 0);
        let full = serde_json::json!({
            "mode": "mode_d", "user_input": "u", "current_plan": "p",
            "revisions_log": [["a", "b"]], "round": 2, "next_tasks": "t",
            "updated_at": 1, "future_field": "ignore"
        });
        let cp2: PipelineCheckpoint = serde_json::from_value(full).unwrap();
        assert_eq!(cp2.mode, "mode_d");
        assert_eq!(cp2.round, 2);
        assert_eq!(cp2.revisions_log, vec![("a".to_string(), "b".to_string())]);
    }

    /// run_id 非法直接拒绝（路径穿越防线；纯函数可测，不碰 AppHandle）
    #[test]
    fn checkpoint_rejects_bad_run_id() {
        assert!(!is_valid_run_id(""));
        assert!(!is_valid_run_id("   "));
        assert!(!is_valid_run_id("../evil"));
        assert!(!is_valid_run_id("a/b"));
        assert!(is_valid_run_id("run-123"));
        assert!(is_valid_run_id("run_1.2"));
    }
}
