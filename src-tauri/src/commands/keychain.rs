//! API Key 系统钥匙串存储。
//! service 固定；account="global"（全局）或 "role:{role}"（角色级）。
//! 前端迁移策略见 useSettings.ts：localStorage 明文 → 钥匙串 → 删除明文（一次性）。
//!
//! 2026-09-14 根因修复（用户 GUI 实测"压根没法用"）：
//! 旧实现用 keyring Rust 库（走 Security Framework API），adhoc 签名应用
//! 每次构建签名变 → macOS 拒绝钥匙串访问 → 前端 9 处 catch 静默吞 → Key 永远空。
//! 新实现改用 `security` CLI 子进程——系统签名进程不受应用签名影响。
//! 这是 F6 决策（不买 Apple 账号）下的唯一可靠路径。
//! 跨平台：macOS 走 security CLI；Windows/Linux 回退 keyring 库（有正式签名或无需签名）。

use crate::errors::{AppError, ErrorKind};
use std::process::Command;

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

// ── security CLI 路径（macOS 专属）──────────────────────────────────

/// security CLI 退出码分类（仅 find-generic-password 读取路径）：
/// 0=成功；44=item not found（实证：缺失条目时 `echo $?` = 44）；
/// 其他=真实访问错误（ACL 拒绝、交互被禁等），上抛而非伪装成"无条目"
/// （旧规则任意非零都返回 None，真实错误不可见——上一会话误诊"回填失败"的根源之一）。
/// CLI 路径用字面量绝对路径 /usr/bin/security：不依赖进程 PATH（GUI launchd 环境防御）。
#[cfg(target_os = "macos")]
fn classify_get_exit(code: i32, stdout: &str, stderr: &str) -> Result<Option<String>, AppError> {
    match code {
        0 => {
            let pw = stdout.trim_end_matches('\n').to_string();
            if pw.is_empty() {
                Ok(None)
            } else {
                Ok(Some(pw))
            }
        }
        44 => Ok(None),
        _ => Err(AppError::new(
            ErrorKind::Internal,
            format!("security CLI 读取失败(exit {}): {}", code, stderr.trim()),
        )),
    }
}

#[cfg(target_os = "macos")]
fn cli_get(account: &str) -> Result<Option<String>, AppError> {
    let out = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", SERVICE, "-a", account, "-w"])
        .output()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("security CLI 调用失败: {}", e)))?;
    classify_get_exit(
        out.status.code().unwrap_or(-1),
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

#[cfg(target_os = "macos")]
fn cli_set(account: &str, secret: &str) -> Result<(), AppError> {
    // -U 更新已存在条目；-A 允许任意应用读取（adhoc 签名友好）
    let out = Command::new("/usr/bin/security")
        .args(["add-generic-password", "-U", "-A", "-s", SERVICE, "-a", account, "-w", secret])
        .output()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("security CLI 写入失败: {}", e)))?;
    if out.status.success() {
        tracing::info!(account = %account, "钥匙串写入成功");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        tracing::warn!(account = %account, code = out.status.code(), "钥匙串写入失败");
        Err(AppError::new(ErrorKind::Internal, format!("钥匙串写入失败: {}", stderr)))
    }
}

#[cfg(target_os = "macos")]
fn cli_delete(account: &str) -> Result<(), AppError> {
    let out = Command::new("/usr/bin/security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", account])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            tracing::info!(account = %account, "钥匙串删除成功");
            Ok(())
        }
        // 幂等：条目不存在（exit 44）也算成功
        Ok(o) if o.status.code() == Some(44) => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!(account = %account, code = o.status.code(), "钥匙串删除失败");
            Err(AppError::new(ErrorKind::Internal, format!("钥匙串删除失败: {}", stderr)))
        }
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("security CLI 调用失败: {}", e))),
    }
}

// ── keyring 库路径（Windows/Linux 回退）────────────────────────────

#[cfg(not(target_os = "macos"))]
fn lib_entry(account: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, account).map_err(|e| {
        AppError::new(ErrorKind::Internal, format!("钥匙串初始化失败: {}", e))
    })
}

#[cfg(not(target_os = "macos"))]
fn lib_get(account: &str) -> Result<Option<String>, AppError> {
    let e = lib_entry(account)?;
    match e.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("钥匙串读取失败: {}", e))),
    }
}

#[cfg(not(target_os = "macos"))]
fn lib_set(account: &str, secret: &str) -> Result<(), AppError> {
    let e = lib_entry(account)?;
    e.set_password(secret)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("钥匙串写入失败: {}", e)))
}

#[cfg(not(target_os = "macos"))]
fn lib_delete(account: &str) -> Result<(), AppError> {
    let e = lib_entry(account)?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("钥匙串删除失败: {}", e))),
    }
}

// ── 统一入口（前端调用的 Tauri 命令）────────────────────────────────

/// 写入密钥（前端 updateSettings 同步调用）
#[tauri::command]
pub async fn keychain_set(account: String, secret: String) -> Result<(), AppError> {
    validate_account(&account)?;
    // O-5：空 secret 拒绝——清空应走 keychain_delete，写空值会在钥匙串留下空条目
    if secret.is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "密钥为空，清空配置请使用删除"));
    }
    #[cfg(target_os = "macos")]
    { cli_set(&account, &secret) }
    #[cfg(not(target_os = "macos"))]
    { lib_set(&account, &secret) }
}

/// 读取密钥（前端启动回填；缺条目返回 None 而非报错——新用户/未迁移场景正常）
#[tauri::command]
pub async fn keychain_get(account: String) -> Result<Option<String>, AppError> {
    validate_account(&account)?;
    #[cfg(target_os = "macos")]
    { cli_get(&account) }
    #[cfg(not(target_os = "macos"))]
    { lib_get(&account) }
}

/// 删除密钥（前端清除配置用）
#[tauri::command]
pub async fn keychain_delete(account: String) -> Result<(), AppError> {
    validate_account(&account)?;
    #[cfg(target_os = "macos")]
    { cli_delete(&account) }
    #[cfg(not(target_os = "macos"))]
    { lib_delete(&account) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R3 修复：security CLI 退出码分类——0=成功；44=item not found（正常缺条目，返回 None）；
    /// 其他退出码=真实访问错误（ACL 拒绝、交互被禁等），必须上抛而非伪装成"无条目"。
    /// 退出码 44 已实证（`security find-generic-password` 未命中时 echo $? = 44）。
    #[cfg(target_os = "macos")]
    #[test]
    fn classify_get_exit_44_means_absent() {
        let r = classify_get_exit(44, "", "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.");
        assert!(r.is_ok());
        assert!(r.unwrap().is_none(), "exit 44 应返回 None（条目不存在），不得报错");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn classify_get_exit_zero_parses_password() {
        let r = classify_get_exit(0, "ak-test\n", "");
        assert_eq!(r.unwrap(), Some("ak-test".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn classify_get_exit_other_codes_are_errors() {
        // ACL 拒绝/交互被禁等真实错误不得伪装成 None（上一会话误诊根因之一）
        for code in [1, 45, 51] {
            let r = classify_get_exit(code, "", "errSecInteractionNotAllowed");
            assert!(r.is_err(), "exit {} 应上抛错误而非返回 None", code);
        }
    }

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

    /// macOS CLI 路径往返测试（写入 → 读取 → 删除 → 确认空）
    #[cfg(target_os = "macos")]
    #[test]
    fn cli_round_trip() {
        let test_acct = "global";
        let test_secret = "test-key-12345";
        // 清理旧残留
        let _ = cli_delete(test_acct);
        // 写入
        cli_set(test_acct, test_secret).expect("写入失败");
        // 读取验证
        let got = cli_get(test_acct).expect("读取失败");
        assert_eq!(got.as_deref(), Some(test_secret), "读回值应与写入一致");
        // 删除
        cli_delete(test_acct).expect("删除失败");
        // 确认已删
        let after = cli_get(test_acct).expect("删除后读取应返回 None");
        assert!(after.is_none(), "删除后应为 None");
    }
}
