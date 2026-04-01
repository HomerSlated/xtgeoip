use anyhow::Result;
use chrono::{Local, SecondsFormat};
use log::Level;
use syslog::{Facility, Formatter3164};

pub fn init_logger(log_file: &str) -> Result<()> {
    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            // File log: timestamp + level + message
            out.finish(format_args!(
                "{} [{}] {}",
                Local::now().to_rfc3339_opts(SecondsFormat::Micros, false),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info);

    // Console formatting: custom prefixes
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
            .chain(std::io::stdout()),
    );

    // File logging
    dispatch = dispatch.chain(fern::log_file(log_file)?);

    dispatch.apply()?;
    Ok(())
}

pub fn log_config_failure(msg: &str) {
    if let Ok(mut logger) = syslog::unix(Formatter3164 {
        facility: Facility::LOG_DAEMON,
        hostname: None,
        process: "xtgeoip".into(),
        pid: 0,
    }) {
        let _ = logger.err(msg);
    }
}

pub fn log_print(msg: &str, level: Level) {
    log::log!(level, "{msg}");
}

pub fn info(msg: &str) {
    log_print(msg, Level::Info);
}

pub fn warn(msg: &str) {
    log_print(msg, Level::Warn);
}

pub fn error(msg: &str) {
    log_print(msg, Level::Error);
}
