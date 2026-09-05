//! A6："打开日志目录"命令（设置面板诊断用；opener 插件打开系统文件管理器）。

use crate::errors::{AppError, ErrorKind};

/// 打开 {app_data}/logs 目录（不存在则先创建，空目录也打开）
#[tauri::command]
pub async fn open_log_dir(app: tauri::AppHandle) -> Result<String, AppError> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("应用数据目录不可用: {}", e)))?
        .join("logs");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("创建日志目录失败: {}", e)))?;
    tauri_plugin_opener::open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("打开目录失败: {}", e)))?;
    Ok(dir.to_string_lossy().to_string())
}
