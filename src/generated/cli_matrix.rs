pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }
pub const CLI_MATRIX: &[CliExample] = &[
    CliExample { cmd: "xtgeoip build", valid: true, outcome: "build using local copy of database" },
    CliExample { cmd: "xtgeoip build -l", valid: true, outcome: "build using local copy of database in legacy mode" },
    CliExample { cmd: "xtgeoip build -b -p", valid: true, outcome: "backup, prune backups, then build" },
    CliExample { cmd: "xtgeoip build -p", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip build -c -f", valid: true, outcome: "clean using manifest, then build using local copy of database" },
    CliExample { cmd: "xtgeoip build -b -c -p", valid: true, outcome: "backup, prune backups, clean, then build" },
    CliExample { cmd: "xtgeoip build -b -c -p -f", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip conf", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip conf -s", valid: true, outcome: "show current config" },
    CliExample { cmd: "xtgeoip conf -d", valid: true, outcome: "show default config" },
    CliExample { cmd: "xtgeoip conf -e", valid: true, outcome: "edit config" },
    CliExample { cmd: "xtgeoip fetch", valid: true, outcome: "fetch CSVs" },
    CliExample { cmd: "xtgeoip fetch -p", valid: true, outcome: "fetch CSVs, then prune CSVs" },
    CliExample { cmd: "xtgeoip fetch -l", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip fetch -b", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip run", valid: true, outcome: "fetch, then build" },
    CliExample { cmd: "xtgeoip run -l", valid: true, outcome: "fetch, then build in legacy mode" },
    CliExample { cmd: "xtgeoip run -p", valid: true, outcome: "fetch, then prune CSVs, then build" },
    CliExample { cmd: "xtgeoip run -c -p", valid: true, outcome: "clean using manifest, then fetch, then prune CSVs, then build" },
    CliExample { cmd: "xtgeoip run -c -f", valid: true, outcome: "force clean without manifest, then fetch, then build" },
    CliExample { cmd: "xtgeoip run -c -p -f", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip run -b -c -p", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip -V", valid: true, outcome: "display version" },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_matrix() {
        assert!(!CLI_MATRIX.is_empty());
    }
}
