//! 日志落盘（tracing + 文件）。
//! 路径：{app_data}/logs/shiyi.log（按日滚动，保留 3 天）；级别 INFO。
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
    tracing::info!(log_dir = %log_dir.display(), "日志系统就绪");
    Ok(())
}

/// 日志目录路径（open_log_dir 命令内部复用；保留供未来诊断命令用）
#[allow(dead_code)]
pub fn log_dir(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join("logs")
}
