use anyhow::Result;
use chrono::{Local, SecondsFormat};
use log::Level;
use syslog::{Facility, Formatter3164};

pub fn init_logger(log_file: &str) -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                Local::now().to_rfc3339_opts(SecondsFormat::Micros, false),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(fern::log_file(log_file)?)
        .apply()?;

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
