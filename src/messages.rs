use anyhow::Result;
use chrono::{Local, SecondsFormat};
use log::Level;
use syslog::{Facility, Formatter3164};

/// Initialize logging.
///
/// stdout/stderr output is always installed so the tool is never silent on
/// the terminal; file logging is added only when `log_file` is `Some`.
pub fn init_logger(log_file: Option<&str>) -> Result<()> {
    let base_dispatch = fern::Dispatch::new().level(log::LevelFilter::Info);

    // stdout/stderr logging with custom formatting
    let stderr_dispatch = fern::Dispatch::new()
        .level(log::LevelFilter::Error)
        .format(|out, message, _record| {
            out.finish(format_args!("Error: {}", message));
        })
        .chain(std::io::stderr());

    let stdout_dispatch = fern::Dispatch::new()
        .level(log::LevelFilter::Info) // keep Info/Warn
        .filter(|metadata| metadata.level() != log::LevelFilter::Error)
        .format(|out, message, record| {
            let msg = match record.level() {
                Level::Info => format!("{}", message),
                Level::Warn => format!("Warning: {}", message),
                _ => format!("{}", message),
            };
            out.finish(format_args!("{}", msg));
        })
        .chain(std::io::stdout());

    // terminal output is unconditional
    let mut dispatch =
        base_dispatch.chain(stdout_dispatch).chain(stderr_dispatch);

    // file logging with timestamp + level — only when configured
    if let Some(log_file) = log_file {
        let file_dispatch = fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "{} [{}] {}",
                    Local::now().to_rfc3339_opts(SecondsFormat::Micros, false),
                    record.level(),
                    message
                ))
            })
            .chain(fern::log_file(log_file)?);
        dispatch = dispatch.chain(file_dispatch);
    }

    dispatch.apply()?;

    Ok(())
}

/// Log configuration load failures to syslog
pub fn log_early_error(msg: &str) {
    if let Ok(mut logger) = syslog::unix(Formatter3164 {
        facility: Facility::LOG_DAEMON,
        hostname: None,
        process: "xtgeoip".into(),
        pid: 0,
    }) {
        let _ = logger.err(msg);
    }
}

/// Generic log function
pub fn log_print(msg: &str, level: Level) {
    log::log!(level, "{msg}");
}

/// Convenience helpers
pub fn info(msg: &str) {
    log_print(msg, Level::Info);
}

pub fn warn(msg: &str) {
    log_print(msg, Level::Warn);
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    log_print(msg, Level::Error);
}
