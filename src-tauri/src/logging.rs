//! 日志落盘（tracing + 文件）。
//! 路径：{app_data}/logs/shiyi.log（按日滚动）；级别 INFO。
//! O-1：启动时清理 3 天前的滚动产物（旧规则注释宣称保留 3 天但从不删除，行为与注释不符）。
//! 初始化失败不阻断启动（回退 stderr fmt 层）；dev 终端双写保留。

use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// 初始化日志（lib.rs run() 首行调用；失败返回 Err 由调用方回退）
pub fn init(app_data_dir: &std::path::Path) -> Result<(), String> {
    let log_dir = app_data_dir.join("logs");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("创建日志目录失败: {}", e))?;
    let file_appender = rolling::RollingFileAppender::new(
        rolling::Rotation::DAILY,
        &log_dir,
        "shiyi.log",
    );
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    // guard 必须泄漏持有（否则后台线程退出丢日志）；进程级单例可接受
    std::mem::forget(_guard);
    let filter = EnvFilter::try_new("info").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(file_writer).with_ansi(false))
        .try_init()
        .map_err(|e| format!("日志初始化失败: {}", e))?;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let removed = cleanup_old_logs(&log_dir, now_secs);
    if removed > 0 {
        tracing::info!(removed, "已清理 3 天前的旧日志");
    }
    tracing::info!(log_dir = %log_dir.display(), "日志系统就绪");
    Ok(())
}

/// 清理 3 天前的滚动日志产物（shiyi.log.YYYY-MM-DD；当日 shiyi.log 在写不动）。
/// 按文件名日期判定（mtime 需额外依赖且网络盘不可靠）；返回删除数（可测）。
pub fn cleanup_old_logs(log_dir: &std::path::Path, now_secs: u64) -> usize {
    const KEEP_DAYS: i64 = 3;
    let today_days = (now_secs / 86_400) as i64;
    let mut removed = 0;
    let Ok(entries) = std::fs::read_dir(log_dir) else { return 0 };
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(date) = name.strip_prefix("shiyi.log.") else { continue };
        let parts: Vec<&str> = date.split('-').collect();
        if parts.len() != 3 { continue; }
        let (Ok(y), Ok(m), Ok(d)) = (
            parts[0].parse::<i64>(),
            parts[1].parse::<i64>(),
            parts[2].parse::<i64>(),
        ) else { continue };
        if today_days - days_from_civil(y, m, d) > KEEP_DAYS {
            if std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// "YYYY-MM-DD" → days since epoch（Howard Hinnant days_from_civil 算法，纯函数可测）
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O-1：日志清理——3 天前的滚动产物删除，3 天内与当日文件保留，非日志文件不动
    #[test]
    fn cleanup_old_logs_removes_only_stale_daily_files() {
        let dir = std::env::temp_dir().join(format!("shiyi-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["shiyi.log.2020-01-01", "shiyi.log.2000-12-31", "shiyi.log", "other.txt"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        // now 取 2026-09-06 前后均可：1999/2019 早于 3 天前，必然删除
        let now = 1_789_000_000u64; // ~2026-09
        let removed = cleanup_old_logs(&dir, now);
        assert_eq!(removed, 2, "应删除两个远期滚动日志");
        assert!(dir.join("shiyi.log").exists(), "当日文件不动");
        assert!(dir.join("other.txt").exists(), "非日志文件不动");
        assert!(!dir.join("shiyi.log.2020-01-01").exists());
        assert!(!dir.join("shiyi.log.2000-12-31").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// days_from_civil 已知值校验（1970-01-01 = day 0）
    #[test]
    fn days_from_civil_known_values() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 6), 20702);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
    }
}

/// 日志目录路径（open_log_dir 命令内部复用；保留供未来诊断命令用）
#[allow(dead_code)]
pub fn log_dir(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join("logs")
}
