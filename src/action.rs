/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip Actions
use crate::config::ConfAction;

pub enum Action {
    TopLevelBackup { clean: bool, force: bool, prune: bool },
    TopLevelClean { force: bool },
    Run { prune: bool, legacy: bool, backup: bool, clean: bool, force: bool },
    Build { legacy: bool, backup: bool, clean: bool, force: bool, prune: bool },
    Fetch { prune: bool },
    Conf(ConfAction),
}

