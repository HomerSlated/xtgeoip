pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }
pub const CLI_MATRIX: &[CliExample] = &[
    CliExample { cmd: "xtgeoip build", valid: true, outcome: "build using local copy of database" },
    CliExample { cmd: "xtgeoip build -b -p", valid: true, outcome: "backup, prune backups, then build" },
    CliExample { cmd: "xtgeoip build -p", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip build -c -f", valid: true, outcome: "clean using manifest, then build using local copy of database" },
    CliExample { cmd: "xtgeoip build -b -c -p", valid: true, outcome: "backup, prune backups, clean, then build" },
    CliExample { cmd: "xtgeoip build -b -c -p -f", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip fetch", valid: true, outcome: "fetch CSVs" },
    CliExample { cmd: "xtgeoip fetch -p", valid: true, outcome: "fetch CSVs, then prune CSVs" },
    CliExample { cmd: "xtgeoip fetch -b", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip run", valid: true, outcome: "fetch, then build" },
    CliExample { cmd: "xtgeoip run -p", valid: true, outcome: "fetch, then prune CSVs, then build" },
    CliExample { cmd: "xtgeoip run -c -p", valid: true, outcome: "clean using manifest, then fetch, then prune CSVs, then build" },
    CliExample { cmd: "xtgeoip run -c -f", valid: true, outcome: "force clean without manifest, then fetch, then build" },
    CliExample { cmd: "xtgeoip run -c -p -f", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip run -b -c -p", valid: false, outcome: "" },
    CliExample { cmd: "xtgeoip show", valid: true, outcome: "show configured paths and status" },
];

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cli_matrix_valid() {
        for ex in CLI_MATRIX {
            println!("{} → {} → {}", ex.cmd, ex.valid, ex.outcome);
        }
    }
}
