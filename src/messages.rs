use log::Level;

/// Log a message at the given level
pub fn log_print(msg: &str, level: Level) {
    log::log!(level, "{msg}");
}

/// Log an info message
pub fn info(msg: &str) {
    log_print(msg, Level::Info);
}

/// Log a warning message
pub fn warn(msg: &str) {
    log_print(msg, Level::Warn);
}

/// Log an error message
pub fn error(msg: &str) {
    log_print(msg, Level::Error);
}
