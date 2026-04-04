use anyhow::Result;
use chrono::Local;
use log::info;

fn setup_logger() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                Local::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(fern::log_file("/var/log/testlog.log")?)
        .apply()?;

    Ok(())
}

fn main() -> Result<()> {
    setup_logger()?;
    info!("Hello, World!");
    Ok(())
}
