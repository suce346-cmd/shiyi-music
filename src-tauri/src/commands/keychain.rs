//! A12：API Key 系统钥匙串存储（macOS Keychain / Windows Credential Manager / Linux Secret Service）。
//! service 固定；account="global"（全局）或 "role:{role}"（角色级）。
//! 前端迁移策略见 useSettings.ts：localStorage 明文 → 钥匙串 → 删除明文（一次性）。

use crate::errors::{AppError, ErrorKind};

const SERVICE: &str = "shiyi-music";

fn entry(account: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, account).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("钥匙串初始化失败: {}", e),
        )
    })
}

/// 写入密钥（前端 updateSettings 同步调用）
#[tauri::command]
pub async fn keychain_set(account: String, secret: String) -> Result<(), AppError> {
    let e = entry(&account)?;
    e.set_password(&secret)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("钥匙串写入失败: {}", e)))
}

/// 读取密钥（前端启动回填；缺条目返回 None 而非报错——新用户/未迁移场景正常）
#[tauri::command]
pub async fn keychain_get(account: String) -> Result<Option<String>, AppError> {
    let e = entry(&account)?;
    match e.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::new(
            ErrorKind::Internal,
            format!("钥匙串读取失败: {}", e),
        )),
    }
}

/// 删除密钥（前端清除配置用）
#[tauri::command]
pub async fn keychain_delete(account: String) -> Result<(), AppError> {
    let e = entry(&account)?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        // 无条目也算成功（幂等，前端无需区分）
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::new(
            ErrorKind::Internal,
            format!("钥匙串删除失败: {}", e),
        )),
    }
}
