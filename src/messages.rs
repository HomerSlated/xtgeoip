use anyhow::Result;
use chrono::{Local, SecondsFormat};
use log::Level;
use syslog::{Facility, Formatter3164};

/// Initialize logging.
///
/// stdout/stderr output is always installed so the tool is never silent on
/// the terminal; file logging is added only when `log_file` is `Some`.
/// Which file the log should go to, if any — the CLI overrides the config.
///
/// `--no-log` wins over `--log-file` (clap rejects the pair, so this only
/// matters if they are ever both set programmatically), and either wins over
/// `[logging]`. The #1 core fix made terminal output independent of file
/// logging, so "no file" here means exactly that and not "no output".
pub fn resolve_log_file(
    no_log: bool,
    cli_log_file: Option<&str>,
    config_log_file: Option<&str>,
) -> Option<String> {
    if no_log {
        return None;
    }
    cli_log_file.or(config_log_file).map(str::to_owned)
}

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

    // file logging with timestamp + level — only when configured.
    //
    // A file sink that cannot be opened must not take the terminal sinks
    // down with it. Chaining `fern::log_file(..)?` directly did exactly
    // that: one `?` inside the chain aborted the whole dispatch, nothing
    // was installed, and the resulting error was then reported *through*
    // the logger that had just failed to install — so the process exited 1
    // in complete silence. That inverted this module's own contract above
    // ("no file" must never mean "no output") by making a *broken* file
    // worse than no file at all. The failure is now degraded to a warning,
    // emitted after `apply()` so there is a logger to emit it with.
    let mut file_error = None;
    if let Some(log_file) = log_file {
        match open_file_sink(log_file) {
            Ok(handle) => {
                let file_dispatch = fern::Dispatch::new()
                    .format(|out, message, record| {
                        out.finish(format_args!(
                            "{} [{}] {}",
                            Local::now()
                                .to_rfc3339_opts(SecondsFormat::Micros, false),
                            record.level(),
                            message
                        ))
                    })
                    .chain(handle);
                dispatch = dispatch.chain(file_dispatch);
            }
            Err(reason) => file_error = Some(reason),
        }
    }

    dispatch.apply()?;

    if let Some(reason) = file_error {
        // Both destinations on purpose: the terminal for an interactive
        // run, syslog for the cron case where stdout goes nowhere and this
        // would otherwise be the one message no one ever sees.
        warn(&format!("file logging disabled — {reason}"));
        log_early_error(&format!("could not open log file: {reason}"));
    }

    Ok(())
}

/// Open the log file, or say why not.
///
/// Split out from `init_logger` so the degrade decision is testable: the
/// global logger can only be installed once per process, so `init_logger`
/// itself cannot be exercised twice in one test binary.
fn open_file_sink(
    log_file: &str,
) -> std::result::Result<std::fs::File, String> {
    fern::log_file(log_file).map_err(|e| format!("{log_file}: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The #1 residual in one line: the flag wins.
    #[test]
    fn cli_log_file_overrides_the_config() {
        assert_eq!(
            resolve_log_file(false, Some("/tmp/cli.log"), Some("/var/log/x")),
            Some("/tmp/cli.log".to_string())
        );
    }

    #[test]
    fn config_is_used_when_no_flag_is_given() {
        assert_eq!(
            resolve_log_file(false, None, Some("/var/log/x")),
            Some("/var/log/x".to_string())
        );
    }

    /// `--no-log` beats both, including an explicit path. clap rejects that
    /// pair at parse time, so this pins the behaviour for any caller that
    /// constructs the arguments directly.
    #[test]
    fn no_log_wins_over_everything() {
        assert_eq!(resolve_log_file(true, None, Some("/var/log/x")), None);
        assert_eq!(
            resolve_log_file(true, Some("/tmp/cli.log"), Some("/var/log/x")),
            None
        );
    }

    /// No flag and no `[logging]` is the pre-existing default, and must stay
    /// "no file" rather than becoming "no output" — that conflation was the
    /// original #1 bug.
    #[test]
    fn absent_everywhere_is_none() {
        assert_eq!(resolve_log_file(false, None, None), None);
    }

    /// A usable path opens, and the sink is appended to rather than
    /// truncated — `fern::log_file` is the only writer and a run must not
    /// erase the previous run's log.
    #[test]
    fn a_writable_path_opens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("xtgeoip.log");
        std::fs::write(&path, b"previous run\n").expect("seed");

        let sink = open_file_sink(path.to_str().expect("utf-8"));
        assert!(sink.is_ok(), "expected Ok, got {:?}", sink.err());
        drop(sink);

        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "previous run\n",
            "opening the sink must not truncate an existing log"
        );
    }

    /// The F-001 case. A log path whose parent does not exist must report a
    /// reason naming the path, *not* propagate an error that can only be
    /// printed through the logger this call is trying to install.
    #[test]
    fn a_missing_parent_directory_yields_a_reason_naming_the_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-such-dir").join("xtgeoip.log");
        let path = path.to_str().expect("utf-8");

        let reason = open_file_sink(path).expect_err("must not open");
        assert!(
            reason.contains(path),
            "reason must name the path, got: {reason}"
        );
        assert!(
            reason.len() > path.len(),
            "reason must carry the OS error too, got: {reason}"
        );
    }
}
