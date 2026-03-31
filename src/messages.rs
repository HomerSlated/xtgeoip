use log::Level;

pub fn log_print(msg: &str, level: Level) {
    match level {
        Level::Error | Level::Warn => eprintln!("{msg}"),
        _ => println!("{msg}"),
    }

    log::log!(level, "{msg}");
}

#[allow(dead_code)]
pub fn info(msg: &str) {
    log_print(msg, Level::Info);
}

#[allow(dead_code)]
pub fn warn(msg: &str) {
    log_print(msg, Level::Warn);
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    log_print(msg, Level::Error);
}
