use anyhow::Result;
use chrono::Utc;
use log::{Level, LevelFilter};

/// Initialize logging
/// `log_file` is optional; if None, no file logging
pub fn init_logging(log_file: Option<&str>) -> Result<()> {
    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            // Timestamp in RFC3339 UTC
            let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

            // File logging format: timestamp + level + message
            if record.level() == Level::Info {
                out.finish(format_args!("{ts} [{level}] {msg}",
                    ts = ts,
                    level = record.level(),
                    msg = message
                ));
            } else {
                out.finish(format_args!("{ts} [{level}] {msg}",
                    ts = ts,
                    level = record.level(),
                    msg = message
                ));
            }
        })
        .level(LevelFilter::Info);

    // stdout/stderr for console
    dispatch = dispatch.chain(
        fern::Dispatch::new()
            .format(|out, message, record| {
                let msg = match record.level() {
                    Level::Info => format!("{}", message),
                    Level::Warn => format!("Warning: {}", message),
                    Level::Error => format!("Error: {}", message),
                    _ => format!("{}", message),
                };
                out.finish(format_args!("{}", msg));
            })
            .chain(std::io::stdout())
    );

    // optional file logging
    if let Some(path) = log_file {
        dispatch = dispatch.chain(fern::log_file(path)?);
    }

    dispatch.apply()?;
    Ok(())
}

/// Helper functions for convenience
pub fn info(msg: &str) {
    log::info!("{msg}");
}

pub fn warn(msg: &str) {
    log::warn!("{msg}");
}

pub fn error(msg: &str) {
    log::error!("{msg}");
}
