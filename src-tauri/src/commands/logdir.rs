//! "打开日志目录"命令（设置面板诊断用；opener 插件打开系统文件管理器）。

use crate::errors::{AppError, ErrorKind};

/// 打开目录通用逻辑（logs / 配置根）
async fn open_dir(app: &tauri::AppHandle, sub: &str) -> Result<String, AppError> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("应用数据目录不可用: {}", e)))?
        .join(sub);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("创建目录失败: {}", e)))?;
    // 配置根目录预建 prompts/knowledge 子目录（用户直接放文件）
    if sub.is_empty() {
        let _ = std::fs::create_dir_all(dir.join("prompts"));
        let _ = std::fs::create_dir_all(dir.join("knowledge"));
    }
    tauri_plugin_opener::open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("打开目录失败: {}", e)))?;
    Ok(dir.to_string_lossy().to_string())
}

/// 打开 {app_data}/logs 目录（不存在则先创建，空目录也打开）
#[tauri::command]
pub async fn open_log_dir(app: tauri::AppHandle) -> Result<String, AppError> {
    open_dir(&app, "logs").await
}

/// 打开配置根目录（{app_data}，含 prompts/knowledge 子目录说明）
#[tauri::command]
pub async fn open_config_dir(app: tauri::AppHandle) -> Result<String, AppError> {
    open_dir(&app, "").await
}
