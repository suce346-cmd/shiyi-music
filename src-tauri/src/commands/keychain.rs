//! API Key 系统钥匙串存储（macOS Keychain / Windows Credential Manager / Linux Secret Service）。
//! service 固定；account="global"（全局）或 "role:{role}"（角色级）。
//! 前端迁移策略见 useSettings.ts：localStorage 明文 → 钥匙串 → 删除明文（一次性）。

use crate::errors::{AppError, ErrorKind};

const SERVICE: &str = "shiyi-music";

/// O-5：account 白名单——仅 "global" 与 "role:{已知角色}"（真源 PipelineRole::storage_key，
/// 与前端 role:{id} 同名）。旧规则任意 account 直写钥匙串，可污染用户钥匙串命名空间。
fn validate_account(account: &str) -> Result<(), AppError> {
    if account == "global" {
        return Ok(());
    }
    if let Some(role) = account.strip_prefix("role:") {
        let known = [
            crate::models::PipelineRole::Host,
            crate::models::PipelineRole::Auditor,
            crate::models::PipelineRole::Emotion,
            crate::models::PipelineRole::Lyricist,
            crate::models::PipelineRole::Reviser,
            crate::models::PipelineRole::Producer,
            crate::models::PipelineRole::StyleAnalyst,
        ];
        if known.iter().any(|r| r.storage_key() == role) {
            return Ok(());
        }
    }
    Err(AppError::new(
        ErrorKind::Validation,
        format!("非法的钥匙串账户: {}（仅允许 global 与已知角色）", account),
    ))
}

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
    validate_account(&account)?;
    // O-5：空 secret 拒绝——清空应走 keychain_delete，写空值会在钥匙串留下空条目
    if secret.is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "密钥为空，清空配置请使用删除"));
    }
    let e = entry(&account)?;
    e.set_password(&secret)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("钥匙串写入失败: {}", e)))
}

/// 读取密钥（前端启动回填；缺条目返回 None 而非报错——新用户/未迁移场景正常）
#[tauri::command]
pub async fn keychain_get(account: String) -> Result<Option<String>, AppError> {
    validate_account(&account)?;
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
    validate_account(&account)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// O-5：account 白名单——global/已知角色放行，未知角色与任意串拒绝
    #[test]
    fn account_allowlist() {
        assert!(validate_account("global").is_ok());
        for role in crate::models::PipelineRole::all() {
            assert!(validate_account(&format!("role:{}", role.storage_key())).is_ok(), "role:{} 应放行", role.storage_key());
        }
        assert!(validate_account("role:hacker").is_err());
        assert!(validate_account("random").is_err());
        assert!(validate_account("").is_err());
        assert!(validate_account("role:").is_err());
    }
}
