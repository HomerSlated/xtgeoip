use log::Level;

pub fn log_print(msg: &str, level: Level) {
    match level {
        Level::Error | Level::Warn => eprintln!("{msg}"),
        _ => println!("{msg}"),
    }

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
