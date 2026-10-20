//! 流水线检查点：讨论轮中间态落盘，失败可续跑（R3）。
//! P1-10 / RULE-HISTORY-V2：`schema_version: 2`；TTL 7 天，过期读时丢弃并删文件。
//! 路径：{app_data}/pipeline_checkpoint_{run_id}.json（原子写 .tmp+rename）。
//! 成功后删除；取消不写，避免复活已取消任务。

use crate::errors::{AppError, ErrorKind};
use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};

/// RULE-HISTORY-V2：检查点 schema
pub const CHECKPOINT_SCHEMA_VERSION: u32 = 2;
/// 检查点 TTL（秒）— D-03：7 天
pub const CHECKPOINT_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// 检查点内容（全部 serde default，前后兼容）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineCheckpoint {
    /// P1-10：缺省 0（旧文件）→ 读时按过期/丢弃策略处理
    #[serde(default)]
    pub schema_version: u32,
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
    /// P1-10：可选 SongPlan JSON（缺省 null）
    #[serde(default)]
    pub song_plan: Option<serde_json::Value>,
}

/// 纯函数：是否已过 TTL（updated_at=0 视为未知/过期）
pub fn is_expired(updated_at: i64, now_epoch_secs: i64) -> bool {
    if updated_at <= 0 {
        return true;
    }
    now_epoch_secs.saturating_sub(updated_at) > CHECKPOINT_TTL_SECS
}

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// run_id 合法性（路径穿越防线：仅字母数字与 -_.）
pub fn is_valid_run_id(run_id: &str) -> bool {
    !run_id.trim().is_empty()
        && run_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn checkpoint_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    run_id: &str,
) -> Result<std::path::PathBuf, AppError> {
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

/// 写检查点（原子写；自动盖 schema_version=2）
pub fn save<R: Runtime>(
    app: &tauri::AppHandle<R>,
    run_id: &str,
    cp: &PipelineCheckpoint,
) -> Result<(), AppError> {
    let path = checkpoint_path(app, run_id)?;
    let tmp = path.with_extension("json.tmp");
    let mut to_write = cp.clone();
    to_write.schema_version = CHECKPOINT_SCHEMA_VERSION;
    if to_write.updated_at <= 0 {
        to_write.updated_at = now_epoch_secs();
    }
    let content = serde_json::to_string(&to_write)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("检查点序列化失败: {}", e)))?;
    std::fs::write(&tmp, content)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("写入检查点失败: {}", e)))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("检查点落盘失败: {}", e)))?;
    Ok(())
}

/// 读检查点：不存在/损坏/过期/非 v2 → None（过期则删文件）
pub fn load<R: Runtime>(app: &tauri::AppHandle<R>, run_id: &str) -> Option<PipelineCheckpoint> {
    let path = checkpoint_path(app, run_id).ok()?;
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<PipelineCheckpoint>(&content) {
            Ok(cp) => {
                if cp.schema_version != CHECKPOINT_SCHEMA_VERSION {
                    tracing::info!(run_id, ver = cp.schema_version, "检查点 schema 非 v2，丢弃");
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                if is_expired(cp.updated_at, now_epoch_secs()) {
                    tracing::info!(run_id, updated_at = cp.updated_at, "检查点超过 TTL，丢弃");
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                Some(cp)
            }
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

/// 启动时清扫过期检查点（仅删 pipeline_checkpoint_*.json）
pub fn purge_expired<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = now_epoch_secs();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("pipeline_checkpoint_") || !name.ends_with(".json") {
            continue;
        }
        let path = ent.path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(cp) = serde_json::from_str::<PipelineCheckpoint>(&content) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if cp.schema_version != CHECKPOINT_SCHEMA_VERSION || is_expired(cp.updated_at, now) {
            let _ = std::fs::remove_file(&path);
            tracing::info!(file = %name, "启动清扫过期/非 v2 检查点");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_serde_compatible() {
        let cp: PipelineCheckpoint = serde_json::from_str("{}").unwrap();
        assert!(cp.current_plan.is_empty());
        assert_eq!(cp.round, 0);
        assert_eq!(cp.schema_version, 0);
        let full = serde_json::json!({
            "schema_version": 2,
            "mode": "mode_d", "user_input": "u", "current_plan": "p",
            "revisions_log": [["a", "b"]], "round": 2, "next_tasks": "t",
            "updated_at": 1, "future_field": "ignore",
            "song_plan": {"schema_version": 1}
        });
        let cp2: PipelineCheckpoint = serde_json::from_value(full).unwrap();
        assert_eq!(cp2.mode, "mode_d");
        assert_eq!(cp2.round, 2);
        assert_eq!(cp2.schema_version, 2);
        assert!(cp2.song_plan.is_some());
        assert_eq!(cp2.revisions_log, vec![("a".to_string(), "b".to_string())]);
    }

    #[test]
    fn checkpoint_ttl_boundary() {
        let now = 1_000_000_i64;
        assert!(is_expired(0, now));
        assert!(!is_expired(now, now));
        assert!(!is_expired(now - CHECKPOINT_TTL_SECS, now));
        assert!(is_expired(now - CHECKPOINT_TTL_SECS - 1, now));
    }

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
