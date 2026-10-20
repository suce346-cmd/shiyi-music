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
#[cfg(target_os = "macos")]
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

// ── RULE-KEYCHAIN-RO（BLUEPRINT_v2 §3.1 / A-01）──────────────────────
//
// 定义点：本节（唯一）。作用域：`#[cfg(test)]` 构建下的全部写/删入口（macOS CLI + keyring 库）。
// 规则：测试可**只读**用户真实条目，**永不写/覆盖/删除**受保护账户（`global` 与 `role:*`）；
// 测试需要写钥匙串时只用 `test:shiyi:{pid}:{nanos}` 隔离账户并以 Drop guard 清理。
// 根因：旧 `cli_round_trip` 直接拿 "global" 做写→删往返，`cargo test` 即删用户真实 Key（审计实测
// exit 0 → 44，测试仍报 ok）。双层守护：①运行期——测试构建下对受保护账户写/删即 panic；
// ②静态——`keychain_tests_never_write_protected` 扫描本文件测试模块的写/删调用实参。
// 生产构建（非 test）行为零变化。

/// 受保护账户（用户真实凭据命名空间）：`global` 与 `role:` 前缀。
pub(crate) const PROTECTED_ACCOUNT_EXACT: &str = "global";
pub(crate) const PROTECTED_ACCOUNT_PREFIX: &str = "role:";
/// 测试隔离账户前缀（不在白名单内，只能由本模块私有 cli_*/lib_* 直接使用）。
#[cfg(all(test, target_os = "macos"))]
pub(crate) const TEST_ACCOUNT_PREFIX: &str = "test:shiyi:";

pub(crate) fn is_protected_account(account: &str) -> bool {
    account == PROTECTED_ACCOUNT_EXACT || account.starts_with(PROTECTED_ACCOUNT_PREFIX)
}

/// 写/删入口的测试期守卫：测试构建中触碰受保护账户立即 panic（生产构建为空操作）。
#[inline]
fn guard_test_write(account: &str) {
    if cfg!(test) && is_protected_account(account) {
        panic!(
            "RULE-KEYCHAIN-RO：测试禁止写/删受保护钥匙串账户 `{}`（请使用 `test:shiyi:` 隔离账户）",
            account
        );
    }
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

/// 密钥格式门——**仅 macOS `security -i` 写入路径**的前提（F5：交互模式按空白切词）。
/// 非 macOS 走 keyring crate（无切词问题），不施加此门，避免误拒合法 Key。
/// 无空白/引号/反斜杠/控制字符的可打印 ASCII。拒绝信息经 AppError 回到前端设置界面显示。
#[cfg(any(target_os = "macos", test))]
pub(crate) fn validate_secret_format(secret: &str) -> Result<(), AppError> {
    if is_safe_token(secret) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorKind::Validation,
            "API Key 含空白、引号、反斜杠或非 ASCII 字符，请检查是否粘贴错误",
        ))
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_safe_token(v: &str) -> bool {
    !v.is_empty() && v.chars().all(|c| c.is_ascii_graphic() && c != '"' && c != '\'' && c != '\\')
}

/// C-08/D-02（P0-10）：密钥不进 argv。旧实现 `add-generic-password ... -w <secret>` 把明文放在命令行参数，
/// 写入期间任何本机进程 `ps -ef` 可见。新实现走 `security -i`（交互模式，命令从 stdin 读），
/// 明文只经管道传给系统签名进程。
///
/// 交互模式按空白切词：为不依赖其引号语义，密钥限定为无空白/引号/反斜杠/控制字符的可打印 ASCII
/// （主流厂商 Key 均满足；含空白多为粘贴错误），否则明确报错而非静默截断。
/// 纯函数，跨平台单测。
#[cfg(any(target_os = "macos", test))]
pub(crate) fn interactive_set_command(account: &str, secret: &str) -> Result<String, AppError> {
    validate_secret_format(secret)?;
    if !is_safe_token(account) {
        return Err(AppError::new(ErrorKind::Validation, format!("非法的钥匙串账户: {}", account)));
    }
    // -U 更新已存在条目；-A 允许任意应用读取（adhoc 签名友好）
    Ok(format!("add-generic-password -U -A -s {} -a {} -w {}\n", SERVICE, account, secret))
}

#[cfg(target_os = "macos")]
fn cli_set(account: &str, secret: &str) -> Result<(), AppError> {
    use std::io::Write;
    use std::process::Stdio;
    guard_test_write(account);
    let line = interactive_set_command(account, secret)?;
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("security CLI 写入失败: {}", e)))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::new(ErrorKind::Internal, "security CLI stdin 不可用"))?;
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| AppError::new(ErrorKind::Internal, format!("security CLI 写入失败: {}", e)))?;
        // drop(stdin) → EOF，交互模式退出
    }
    let out = child
        .wait_with_output()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("security CLI 写入失败: {}", e)))?;
    // 交互模式的退出码不保证反映内部命令成败 → 写后读回校验（不一致即报错，不伪装成功）
    match cli_get(account) {
        Ok(Some(pw)) if pw == secret => {
            tracing::info!(account = %account, "钥匙串写入成功");
            Ok(())
        }
        _ => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(account = %account, code = out.status.code(), "钥匙串写入失败（读回不一致）");
            Err(AppError::new(
                ErrorKind::Internal,
                format!("钥匙串写入失败（读回校验不一致）: {}", stderr.trim()),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
fn cli_delete(account: &str) -> Result<(), AppError> {
    guard_test_write(account);
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
    guard_test_write(account);
    let e = lib_entry(account)?;
    e.set_password(secret)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("钥匙串写入失败: {}", e)))
}

#[cfg(not(target_os = "macos"))]
fn lib_delete(account: &str) -> Result<(), AppError> {
    guard_test_write(account);
    let e = lib_entry(account)?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("钥匙串删除失败: {}", e))),
    }
}

// ── 统一入口（前端调用的 Tauri 命令）────────────────────────────────

/// 写入前置校验（纯函数，不触碰钥匙串）：账户白名单 + 非空 + 格式门（仅 CLI 路径，F5）。
/// `cli_path` = 是否走 macOS `security -i`（生产传 `cfg!(target_os = "macos")`；单测两支都测）。
fn precheck_set(account: &str, secret: &str, cli_path: bool) -> Result<(), AppError> {
    validate_account(account)?;
    // O-5：空 secret 拒绝——清空应走 keychain_delete，写空值会在钥匙串留下空条目
    if secret.is_empty() {
        return Err(AppError::new(ErrorKind::Validation, "密钥为空，清空配置请使用删除"));
    }
    #[cfg(any(target_os = "macos", test))]
    if cli_path {
        return validate_secret_format(secret);
    }
    let _ = cli_path;
    Ok(())
}

/// 写入密钥（前端 updateSettings 同步调用）
#[tauri::command]
pub async fn keychain_set(account: String, secret: String) -> Result<(), AppError> {
    precheck_set(&account, &secret, cfg!(target_os = "macos"))?;
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

    /// C-08/D-02：密钥只走 stdin 命令行（不进 argv）；非法字符明确拒绝
    #[test]
    fn interactive_set_command_shape_and_rejects() {
        let k = ["sk", "abc123", "XYZ_9.-"].join("-");
        let cmd = interactive_set_command("test:shiyi:1", &k).unwrap();
        assert_eq!(cmd, format!("add-generic-password -U -A -s shiyi-music -a test:shiyi:1 -w {}\n", k));
        for bad in ["", "sk 1", "sk\t1", "sk\"1", "sk'1", "sk\\1", "sk\n1", "密钥"] {
            assert!(interactive_set_command("test:shiyi:1", bad).is_err(), "应拒绝 {:?}", bad);
        }
    }

    /// 守护：写入路径不得把密钥放进 argv（源码扫描 add-generic-password 只出现在 stdin 命令构造里）
    #[test]
    fn cli_set_never_passes_secret_in_argv() {
        let src = include_str!("keychain.rs");
        let body_start = src.find(concat!("fn cli", "_set(")).unwrap();
        let body = &src[body_start..src[body_start..].find("\n}\n").unwrap() + body_start];
        assert!(!body.contains(concat!("\"add-", "generic-password\"")), "cli_set 不得以 argv 传参调用 add-generic-password");
        assert!(body.contains(".arg(\"-i\")"));
    }

    /// 全平台入口同口径：keychain_set 前置校验对非法 Key 返回 Validation（纯函数，不触碰钥匙串）
    #[test]
    fn precheck_rejects_bad_secret_before_write() {
        let e = precheck_set("global", "sk bad key", true).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert!(e.message.contains("API Key"));
        // F5：格式门只作用于 macOS CLI 路径；keyring 路径不误拒
        assert!(precheck_set("global", "sk bad key", false).is_ok());
        assert!(precheck_set("global", "", false).is_err(), "空值两条路径都拒");
        assert!(precheck_set("role:hacker", "sk-ok", false).is_err(), "账户白名单两条路径都执行");
    }

    /// F5：常见特殊字符（# ; $ & | * ? ! % ~ ` < > = @ , : / + 等）作为**单个 token** 原样进入
    /// stdin 命令行，不被截断/改写；真实 `security -i` 解析由 Mac 验收脚本在隔离账户上实写读回确认
    /// （scripts/mac-verify.sh 第 3 步）。
    #[test]
    fn interactive_set_command_passes_special_chars_verbatim() {
        for k in ["sk-a#b", "sk;rm", "sk$HOME", "sk&x|y", "sk*?!", "sk%~`", "sk<>=@", "sk,:/+", "#leading", "trail;"] {
            let cmd = interactive_set_command("test:shiyi:1", k).unwrap();
            let toks: Vec<&str> = cmd.trim_end().split_whitespace().collect();
            assert_eq!(toks.last().copied(), Some(k), "密钥必须是最后一个完整 token: {cmd:?}");
            assert_eq!(toks.len(), 9, "不得多出/丢失 token: {cmd:?}");
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

    /// 测试隔离账户：`test:shiyi:{pid}:{nanos}`；Drop 时删除（不论断言成败）。
    #[cfg(target_os = "macos")]
    struct IsolatedAccount(String);
    #[cfg(target_os = "macos")]
    impl IsolatedAccount {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let acct = format!("{}{}:{}", TEST_ACCOUNT_PREFIX, std::process::id(), nanos);
            assert!(!is_protected_account(&acct), "隔离账户不得落入受保护命名空间");
            IsolatedAccount(acct)
        }
    }
    #[cfg(target_os = "macos")]
    impl Drop for IsolatedAccount {
        fn drop(&mut self) {
            let _ = cli_delete(&self.0);
        }
    }

    /// macOS CLI 路径往返测试（写入 → 读取 → 删除 → 确认空）——只在隔离账户上进行，
    /// 永不触碰 `global`（A-01 根治；旧实现以 "global" 往返，跑测试即删用户真实 Key）。
    #[cfg(target_os = "macos")]
    #[test]
    fn cli_round_trip() {
        let acct = IsolatedAccount::new();
        let test_secret = "test-key-12345";
        cli_set(&acct.0, test_secret).expect("写入失败");
        let got = cli_get(&acct.0).expect("读取失败");
        assert_eq!(got.as_deref(), Some(test_secret), "读回值应与写入一致");
        cli_delete(&acct.0).expect("删除失败");
        let after = cli_get(&acct.0).expect("删除后读取应返回 None");
        assert!(after.is_none(), "删除后应为 None");
    }

    /// 只读真实条目是允许的（D6）：读取 `global` 不得改变其存在性/内容。
    /// 只做 get 两次比对，不写不删；条目缺失（None）同样通过。
    #[cfg(target_os = "macos")]
    #[test]
    fn cli_get_global_is_read_only() {
        let before = cli_get(PROTECTED_ACCOUNT_EXACT);
        let after = cli_get(PROTECTED_ACCOUNT_EXACT);
        match (before, after) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "两次只读结果应一致"),
            // 钥匙串交互被禁（CI/无 GUI 会话）等真实错误：只读路径不做断言，不得因此改写条目
            _ => {}
        }
    }

    /// 运行期守卫：受保护账户写/删在测试构建中 panic（跨平台，不触达任何钥匙串）。
    #[test]
    fn guard_panics_on_protected_accounts() {
        for acct in ["global", "role:host", "role:anything"] {
            let r = std::panic::catch_unwind(|| guard_test_write(acct));
            assert!(r.is_err(), "`{}` 写/删应被测试期守卫拦截", acct);
        }
        for acct in ["test:shiyi:1:2", "globalx", "xrole:host"] {
            let r = std::panic::catch_unwind(|| guard_test_write(acct));
            assert!(r.is_ok(), "`{}` 非受保护账户不应被拦截", acct);
        }
    }

    /// 静态守护：本文件测试模块中，写/删调用的实参不得是受保护账户字面量。
    /// 防回归：有人把隔离账户改回 "global"/"role:..." 字面量即红（运行期守卫之外的第二层）。
    #[test]
    fn keychain_tests_never_write_protected() {
        let src = include_str!("keychain.rs");
        let marker = concat!("#[cfg(test)]", "\n", "mod tests");
        let idx = src.find(marker).expect("应能定位测试模块");
        let tests_src = &src[idx..];
        let writers: Vec<String> = ["cli_set", "cli_delete", "lib_set", "lib_delete", "keychain_set", "keychain_delete"]
            .iter()
            .map(|f| format!("{}(", f))
            .collect();
        let q = '"';
        let banned = [format!("{q}global{q}"), format!("{q}role:")];
        let mut hits = Vec::new();
        for (n, line) in tests_src.lines().enumerate() {
            if writers.iter().any(|w| line.contains(w.as_str()))
                && (banned.iter().any(|b| line.contains(b.as_str())) || line.contains("PROTECTED_ACCOUNT"))
            {
                hits.push(format!("tests+{}: {}", n + 1, line.trim()));
            }
        }
        assert!(hits.is_empty(), "测试模块写/删了受保护账户：{:?}", hits);
    }
}
