/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use std::{ffi::OsString, path::PathBuf};

use anyhow::{Result, anyhow};
use clap::{
    Args, CommandFactory, FromArgMatches, Parser, Subcommand, error::ErrorKind,
    parser::ValueSource,
};

use crate::{
    action::Action,
    conf::ConfAction,
    generated::cli_rules::{self, Guard},
};

pub enum CliOutcome {
    Action(Action),
    ShowHelp,
}

#[derive(Args)]
pub struct CommonFlags {
    /// Back up current database before replacing it
    #[arg(short, long)]
    pub backup: bool,

    /// Delete current binary database files
    #[arg(short, long)]
    pub clean: bool,

    /// Force the operation (overrides safety checks)
    ///
    /// With `build -c`, extends the clean to stale-owned files — those left
    /// by a previous manifest, such as EU.iv4/EU.iv6 after leaving legacy
    /// mode. It does not widen the clean to files xtgeoip did not create:
    /// eligibility is structural (extension iv4/iv6 and a two-character
    /// [A-Z0-9] stem), so --force cannot reach an unowned file.
    #[arg(short, long)]
    pub force: bool,

    /// Enable legacy mode (historical compatibility only)
    ///
    /// Switching back to default mode leaves EU.iv4/EU.iv6 behind; they are
    /// listed in the `orphaned` file. Which clean form removes them depends
    /// on when you act, because --clean runs before build regenerates the
    /// manifest: `build -c` in the same invocation that leaves legacy mode
    /// (still owned), or `build -c -f` afterwards (stale-owned, needs the
    /// glob). Files xtgeoip did not create are never touched by either. See
    /// xtgeoip(1), FILE OWNERSHIP and LEGACY MODE.
    #[arg(short = 'l', long)]
    pub legacy: bool,
}

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Build and manage xt_geoip data from MaxMind GeoLite2 CSVs",
    propagate_version = false,
    disable_help_subcommand = true,
    // What `args_conflicts_with_subcommands` used to render. Without the
    // setting clap derives `xtgeoip [OPTIONS] [COMMAND]`, which reads as though
    // `xtgeoip -b build` were valid; `try_parse_argv` still rejects it.
    override_usage = "xtgeoip [OPTIONS]\n       xtgeoip <COMMAND>"
)]
pub struct Cli {
    #[command(flatten)]
    pub common: CommonFlags,

    /// Prune old bin archives (requires --backup)
    #[arg(short, long)]
    pub prune: bool,

    /// Write the log to PATH, overriding [logging] in the config
    ///
    /// Takes precedence over `log_file` in `/etc/xtgeoip.conf`. Because the
    /// override is known before the config is read, it also captures a
    /// config-load failure — which the configured path cannot, since that
    /// path is only known once the load has succeeded.
    #[arg(long, value_name = "PATH", global = true, conflicts_with = "no_log")]
    pub log_file: Option<String>,

    /// Disable file logging, overriding [logging] in the config
    ///
    /// Terminal output is unaffected: `init_logger` always installs the
    /// stdout/stderr dispatches, and only the file sink is conditional (#1).
    #[arg(long, global = true)]
    pub no_log: bool,

    /// Read configuration from PATH instead of /etc/xtgeoip.conf
    ///
    /// Applies to every command, `conf` included: `conf --show --config P`
    /// shows `P`, and `conf --set-credentials --config P` encrypts into `P`.
    /// A flag that redirected reads but not writes would be a trap.
    ///
    /// This is what makes the integration suite able to run against a
    /// temporary tree instead of the live `/usr/share/xt_geoip` and
    /// `/var/lib/xt_geoip` — `output_dir` and `archive_dir` are ordinary
    /// `[paths]` keys, so redirecting the config redirects everything the
    /// program touches. It is a general capability rather than a test hook,
    /// which is why it is documented rather than hidden: an operator running
    /// a non-default install has the same need.
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,

    /// Verify the MaxMind server against the CA bundle in PATH
    ///
    /// Replaces the system trust roots for this run rather than adding to
    /// them: naming a CA says which CA to trust, and merging would let a
    /// wrong or stale bundle succeed against a different anchor than the one
    /// named. Verification is otherwise unchanged — chain, hostname and
    /// validity are still checked, against these roots instead of the
    /// system's. It narrows what is trusted; it never disables a check.
    ///
    /// For an installation behind a TLS-intercepting proxy, or against a
    /// private mirror with a self-signed certificate. It is also what lets
    /// the integration suite verify a local https stub without touching the
    /// host's trust configuration, which `SSL_CERT_FILE` cannot do: the
    /// suite spawns cases via `sudo`, and `sudo` resets the environment
    /// while passing arguments through unchanged.
    #[arg(long, value_name = "PATH", global = true)]
    pub ca_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    /// Parse `argv` the way the program must: clap's parse, then the one rule
    /// clap cannot express here — a top-level *action* flag and a subcommand
    /// are mutually exclusive.
    ///
    /// Always call this rather than `Parser::try_parse_from`, which skips the
    /// rule and would accept `xtgeoip -b build`.
    ///
    /// Why the rule is not clap's `args_conflicts_with_subcommands`, which
    /// enforced it until 2026-09-13: that setting counts *every* matched
    /// argument, `global = true` ones included (`clap_builder` 4.6.6 sets
    /// `valid_arg_found` with no test for globals), so it also rejected
    /// `xtgeoip --config X build` — a global option in the position a user
    /// types first. The globals select no mode, so they cannot create the
    /// ambiguity the rule exists to prevent (`-b build`: a top-level backup, or
    /// a build with one?). Checking here keeps the rule and exempts exactly
    /// them. It is derived from clap's own argument list rather than a list of
    /// flags, so a new top-level flag is covered the day it is added, and it
    /// still fails as a clap `ArgumentConflict`, so `main` still exits 2.
    pub fn try_parse_argv<I, T>(argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let mut cmd = Self::command();
        let matches = cmd.try_get_matches_from_mut(argv)?;

        if let Some(subcommand) = matches.subcommand_name() {
            let conflicting: Vec<String> = cmd
                .get_arguments()
                .filter(|a| !a.is_global_set())
                .filter(|a| {
                    matches.value_source(a.get_id().as_str())
                        == Some(ValueSource::CommandLine)
                })
                .map(|a| a.to_string())
                .collect();
            // Worded as clap worded it, so the message is unchanged too.
            let with = match conflicting.as_slice() {
                [] => None,
                [one] => Some(format!(" '{one}'")),
                many => Some(format!(":\n  {}", many.join("\n  "))),
            };
            if let Some(with) = with {
                return Err(cmd.error(
                    ErrorKind::ArgumentConflict,
                    format!(
                        "the subcommand '{subcommand}' cannot be used \
                         with{with}"
                    ),
                ));
            }
        }

        Self::from_arg_matches(&matches).map_err(|e| e.format(&mut cmd))
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Fetch then build the full pipeline
    #[command(disable_version_flag = true)]
    Run {
        #[command(flatten)]
        common: CommonFlags,

        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,
    },

    /// Build binary database from local CSV archive
    #[command(disable_version_flag = true)]
    Build {
        #[command(flatten)]
        common: CommonFlags,

        /// Prune old bin archives after backup (requires --backup)
        #[arg(short, long)]
        prune: bool,
    },

    /// Download GeoLite2 CSV archive from MaxMind
    #[command(disable_version_flag = true)]
    Fetch {
        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,

        // These flags are not supported for fetch; accepted here to emit
        // proper keyed errors rather than generic clap parse errors.
        #[arg(short = 'l', long, hide = true)]
        legacy: bool,
        #[arg(short = 'b', long, hide = true)]
        backup: bool,
        #[arg(short = 'c', long, hide = true)]
        clean: bool,
        #[arg(short = 'f', long, hide = true)]
        force: bool,
    },

    /// Manage system configuration
    // required(true) is intentionally omitted — the missing-flag case is
    // handled in normalize_cli_to_action to emit a keyed error.
    #[command(group(
        clap::ArgGroup::new("conf_action").multiple(false)
    ))]
    #[command(disable_version_flag = true)]
    Conf {
        /// Show default (example) configuration
        #[arg(short = 'd', long = "default", group = "conf_action")]
        default: bool,

        /// Show system configuration
        #[arg(short = 's', long = "show", group = "conf_action")]
        show: bool,

        /// Open system configuration in $EDITOR
        #[arg(short = 'e', long = "edit", group = "conf_action")]
        edit: bool,

        /// Encrypt and store MaxMind account_id/license_key (#103)
        #[arg(short = 'c', long = "set-credentials", group = "conf_action")]
        set_credentials: bool,
    },
}

fn conf_action(default: bool, show: bool, set_credentials: bool) -> ConfAction {
    if default {
        ConfAction::Default
    } else if show {
        ConfAction::Show
    } else if set_credentials {
        ConfAction::SetCredentials
    } else {
        ConfAction::Edit
    }
}

fn keyed_err(key: &str, msg: &str) -> anyhow::Error {
    anyhow!("[{key}]: {msg}")
}

/// Pack the flag universe into the `u8` bitmask the generated guard tables use.
/// Bit positions are defined by `cli_rules` (sorted flag-universe order).
fn flag_mask(b: bool, c: bool, f: bool, l: bool, p: bool) -> u8 {
    use cli_rules::{B, C, F, L, P};
    let mut m = 0;
    if b {
        m |= B;
    }
    if c {
        m |= C;
    }
    if f {
        m |= F;
    }
    if l {
        m |= L;
    }
    if p {
        m |= P;
    }
    m
}

/// First guard whose `require` bits are all set and `forbid` bits all clear.
/// First match wins — table order encodes precedence (see `cli_rules`).
fn first_guard(flags: u8, guards: &'static [Guard]) -> Option<&'static Guard> {
    guards
        .iter()
        .find(|g| flags & g.require == g.require && flags & g.forbid == 0)
}

/// Normalize CLI input into a CliOutcome.
///
/// The combination rules live in `docs/spec/cli.yaml` and are compiled by
/// `xtgeoip-docgen` into the per-context guard tables in
/// `crate::generated::cli_rules`. This function only packs the parsed flags
/// into a bitmask, evaluates the first matching guard, and otherwise constructs
/// the `Action`. `conf` is outside the guard model (its rules are owned by
/// clap's `ArgGroup` + the required positional; only the missing-flag case is
/// keyed here).
pub fn normalize_cli_to_action(cli: &Cli) -> Result<CliOutcome> {
    use Commands::*;

    if let Some(cmd) = &cli.command {
        match cmd {
            Conf {
                default,
                show,
                edit,
                set_credentials,
            } => {
                if !default && !show && !edit && !set_credentials {
                    return Err(keyed_err(
                        "conf_missing_flag",
                        "conf requires one of: --default (-d), --show (-s), \
                         --edit (-e), --set-credentials (-c)",
                    ));
                }
                Ok(CliOutcome::Action(Action::Conf(conf_action(
                    *default,
                    *show,
                    *set_credentials,
                ))))
            }

            Run { common, prune } => {
                let flags = flag_mask(
                    common.backup,
                    common.clean,
                    common.force,
                    common.legacy,
                    *prune,
                );
                if let Some(g) = first_guard(flags, cli_rules::RUN_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Run {
                    prune: *prune,
                    legacy: common.legacy,
                    backup: common.backup,
                    clean: common.clean,
                    force: common.force,
                }))
            }

            Build { common, prune } => {
                let flags = flag_mask(
                    common.backup,
                    common.clean,
                    common.force,
                    common.legacy,
                    *prune,
                );
                if let Some(g) = first_guard(flags, cli_rules::BUILD_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Build {
                    legacy: common.legacy,
                    backup: common.backup,
                    clean: common.clean,
                    force: common.force,
                    prune: *prune,
                }))
            }

            Fetch {
                prune,
                legacy,
                backup,
                clean,
                force,
            } => {
                let flags = flag_mask(*backup, *clean, *force, *legacy, *prune);
                if let Some(g) = first_guard(flags, cli_rules::FETCH_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        let b = cli.common.backup;
        let c = cli.common.clean;
        let p = cli.prune;
        let f = cli.common.force;
        let l = cli.common.legacy;

        let flags = flag_mask(b, c, f, l, p);
        if let Some(g) = first_guard(flags, cli_rules::TOP_LEVEL_GUARDS) {
            return Err(keyed_err(g.key, g.message));
        }

        // No guard fired. Bare invocation shows help (main renders this as
        // top_level_no_args); otherwise -b/-c select the top-level action.
        if flags == 0 {
            return Ok(CliOutcome::ShowHelp);
        }
        if b {
            return Ok(CliOutcome::Action(Action::TopLevelBackup {
                clean: c,
                force: f,
                prune: p,
            }));
        }
        if c {
            return Ok(CliOutcome::Action(Action::TopLevelClean { force: f }));
        }
        Err(anyhow!("unsupported flag combination"))
    }
}

/// Exhaustive behavior snapshot of the CLI semantics layer.
///
/// Enumerates every flag combination per context, parses it the way `main` does
/// (`Cli::try_parse_argv`), runs `normalize_cli_to_action` (pure — no root, no
/// filesystem, no execution), and locks the outcome against a golden file. This
/// is the regression net for the spec-driven validator rewrite: the new
/// evaluator must reproduce this snapshot byte-for-byte. The spec examples
/// cannot serve this role (one canonical example per error case — see TODO
/// #92).
///
/// Regenerate after an *intended* behavior change:
///   cargo test regenerate_snapshot -- --ignored
#[cfg(test)]
mod snapshot {

    use super::*;

    /// All invocations as the args following the program name, per context.
    fn all_invocations() -> Vec<Vec<&'static str>> {
        let contexts: &[(&[&str], &[&str])] = &[
            (&[], &["-b", "-c", "-p", "-f", "-l"]),
            (&["fetch"], &["-p", "-b", "-c", "-f", "-l"]),
            (&["build"], &["-b", "-c", "-p", "-f", "-l"]),
            (&["run"], &["-b", "-c", "-p", "-f", "-l"]),
            (&["conf"], &["-d", "-s", "-e", "-c"]),
        ];
        let mut out = Vec::new();
        for (prefix, flags) in contexts {
            for mask in 0..(1u32 << flags.len()) {
                let mut argv = vec!["xtgeoip"];
                argv.extend_from_slice(prefix);
                for (i, flag) in flags.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        argv.push(flag);
                    }
                }
                out.push(argv);
            }
        }
        out
    }

    /// Canonical outcome string for one invocation.
    fn outcome(argv: &[&str]) -> String {
        match Cli::try_parse_argv(argv) {
            Err(_) => "PARSE_ERR".to_string(),
            Ok(cli) => match normalize_cli_to_action(&cli) {
                Ok(CliOutcome::ShowHelp) => "ShowHelp".to_string(),
                Ok(CliOutcome::Action(action)) => format!("{action:?}"),
                Err(e) => {
                    let s = e.to_string();
                    let key = s
                        .strip_prefix('[')
                        .and_then(|r| r.split_once(']'))
                        .map(|(k, _)| k)
                        .unwrap_or(s.as_str());
                    format!("Err({key})")
                }
            },
        }
    }

    fn snapshot() -> String {
        let mut lines: Vec<String> = all_invocations()
            .iter()
            .map(|argv| format!("{} => {}", argv.join(" "), outcome(argv)))
            .collect();
        lines.sort();
        lines.join("\n") + "\n"
    }

    #[test]
    fn cli_semantics_snapshot() {
        let actual = snapshot();
        let golden = include_str!("cli_snapshot.golden");
        for (i, (a, g)) in actual.lines().zip(golden.lines()).enumerate() {
            assert_eq!(
                a, g,
                "CLI semantics changed at line {i} — if intended, regenerate \
                 the snapshot (see module docs)"
            );
        }
        assert_eq!(
            actual.lines().count(),
            golden.lines().count(),
            "CLI invocation count changed — regenerate the snapshot"
        );
    }

    #[test]
    #[ignore = "writes the golden file; run explicitly after intended changes"]
    fn regenerate_snapshot() {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/cli_snapshot.golden");
        std::fs::write(path, snapshot()).unwrap();
    }
}

/// Contradiction checks over the spec (#92).
///
/// `snapshot` above proves *what the implementation does* across all 136 flag
/// combinations. These tests prove something different: that the spec does not
/// contradict itself or the implementation. Two distinct classes:
///
/// 1. **The spec's examples could lie.** `CLI_MATRIX` is generated from the
///    hand-written `examples:` in `cli.yaml`, and nothing checked them against
///    the parser. An example asserting `valid: true` for a combination the
///    guards reject would ship as documentation, a man-page entry, and a test
///    case — all wrong, all agreeing with each other. This is the shape of the
///    `p⊕f` leak recorded in #92.
/// 2. **A guard could be unreachable.** Guard order encodes precedence, so an
///    earlier guard can fully subsume a later one, which then never fires. The
///    dead guard's error message becomes unreachable while still appearing in
///    the spec and docs as if it were live.
#[cfg(test)]
mod contradiction {
    use clap::{CommandFactory, error::ErrorKind};

    use super::*;
    use crate::generated::{
        cli_matrix::CLI_MATRIX,
        cli_rules::{
            B, BUILD_GUARDS, C, F, FETCH_GUARDS, Guard, L, P, RUN_GUARDS,
            TOP_LEVEL_GUARDS,
        },
    };

    // ── global options (#1 residual) ─────────────────────────────────────

    /// Every global option clap knows about must appear in the man page.
    ///
    /// `--log-file`/`--no-log` are deliberately absent from `cli.yaml`'s
    /// `flags:` map — that is the universe the guard bitmask is built from,
    /// and `every_flag_is_referenced_by_some_guard` below would reject a bit
    /// no guard can mention. Being outside that map means they are also
    /// outside every check that reads it, so this is the one that keeps them
    /// documented. Derived from clap rather than from a hardcoded list, so a
    /// third global option is covered the day it is added.
    #[test]
    fn global_options_are_documented() {
        let man = std::fs::read_to_string("docs/generated/xtgeoip.1")
            .expect("docs/generated/xtgeoip.1 missing — run docgen");

        let cmd = Cli::command();
        let globals: Vec<String> = cmd
            .get_arguments()
            .filter(|a| a.is_global_set())
            .filter_map(|a| a.get_long().map(str::to_owned))
            .collect();

        assert!(
            !globals.is_empty(),
            "no global options found — has the flag model changed?"
        );

        for opt in &globals {
            // roff escapes every literal hyphen.
            let roff = format!("\\-\\-{}", opt.replace('-', "\\-"));
            assert!(
                man.contains(&roff),
                "--{opt} is not documented in the man page (looked for \
                 {roff:?} in OPTIONS)"
            );
        }
    }

    /// The two overrides are mutually exclusive, and the conflict is enforced
    /// by clap rather than by a hand-written guard — they carry no
    /// combination semantics with the other five flags, which is exactly why
    /// they are not in the mask.
    #[test]
    fn log_file_and_no_log_conflict() {
        assert!(
            Cli::try_parse_argv([
                "xtgeoip",
                "-b",
                "--log-file",
                "/tmp/x",
                "--no-log"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_argv(["xtgeoip", "-b", "--log-file", "/tmp/x"])
                .is_ok()
        );
        assert!(Cli::try_parse_argv(["xtgeoip", "-b", "--no-log"]).is_ok());
    }

    /// A value for `arg` on the command line: a flag takes none.
    fn argv_for(arg: &clap::Arg) -> Vec<String> {
        let long =
            format!("--{}", arg.get_long().expect("every option is long"));
        if arg.get_action().takes_values() {
            vec![long, "/tmp/x".to_owned()]
        } else {
            vec![long]
        }
    }

    /// Clap's argument surface must be exactly the surface `cli.yaml`
    /// declares, in both directions.
    ///
    /// The spec describes the whole CLI in three buckets: `flags` (the five
    /// carrying combination semantics, which form the guard bitmask),
    /// `global_options` (apply everywhere, constrain nothing) and
    /// `subcommand_options` (belong to one command). Until now nothing
    /// asserted that those three *together* are what clap actually parses.
    /// The existing checks each cover one part and none covers the union:
    /// `global_options_are_documented` pins the globals to the man page, and
    /// `every_flag_is_referenced_by_some_guard` pins `flags` to the guards. An
    /// argument added to `cli.rs` and forgotten in the spec — or renamed in
    /// one and not the other — was invisible to every one of them.
    ///
    /// Both directions are asserted separately because they are different
    /// mistakes: an argument clap has and the spec does not is undeclared
    /// surface, while one the spec has and clap does not is a rename or a
    /// deletion that the generated docs still advertise.
    ///
    /// **Join on the long name**, not the key: the spec's keys are short
    /// letters in `flags` and `subcommand_options` but snake-case names in
    /// `global_options`, whereas `long:` is present in all three and is what
    /// `Arg::get_long` returns.
    ///
    /// Two properties of clap this relies on, both confirmed by inspection
    /// rather than assumed. `Cli::command()` is deliberately *not*
    /// `build()`-ed: clap injects `--help`/`--version` during the build,
    /// and on an unbuilt command `get_arguments` yields only declared
    /// arguments, so there is nothing to filter out. (If a later change
    /// builds it first, those two will surface here as undeclared — which
    /// is the right failure, since filtering by "absent from the spec"
    /// would make this test vacuous.) And `hide = true` does not suppress
    /// an argument from `get_arguments`, which matters because `fetch`'s
    /// four rejected flags are declared hidden and must still be accounted
    /// for.
    #[test]
    fn clap_surface_matches_the_spec() {
        use std::collections::{BTreeMap, BTreeSet};

        use crate::spec::{FlagDef, Spec};

        fn longs(m: &BTreeMap<String, FlagDef>) -> BTreeSet<String> {
            m.values().map(|f| f.long.clone()).collect()
        }
        fn long_of(a: &clap::Arg) -> String {
            a.get_long()
                .expect("every argument has a long form")
                .to_owned()
        }

        let yaml = std::fs::read_to_string("docs/spec/cli.yaml")
            .expect("docs/spec/cli.yaml missing");
        let spec: Spec =
            serde_saphyr::from_str(&yaml).expect("cli.yaml does not parse");

        let flag_longs = longs(&spec.flags);
        let global_longs = longs(&spec.global_options);

        // Vacuity guard. A silently empty parse would satisfy both directions
        // below without checking anything at all.
        assert!(!flag_longs.is_empty(), "cli.yaml declares no flags");
        assert!(
            !global_longs.is_empty(),
            "cli.yaml declares no global_options"
        );
        assert!(
            !spec.subcommand_options.is_empty(),
            "cli.yaml declares no subcommand_options"
        );

        let cmd = Cli::command();

        // ── clap → spec ──────────────────────────────────────────────────
        let mut undeclared = Vec::new();
        for a in cmd.get_arguments() {
            let long = long_of(a);
            let (declared, kind) = if a.is_global_set() {
                (global_longs.contains(&long), "global option")
            } else {
                (flag_longs.contains(&long), "flag")
            };
            if !declared {
                undeclared.push(format!(
                    "  --{long}: clap parses it as a top-level {kind}, \
                     cli.yaml declares it nowhere"
                ));
            }
        }
        for sub in cmd.get_subcommands() {
            let name = sub.get_name();
            let own = spec
                .subcommand_options
                .get(name)
                .map(longs)
                .unwrap_or_default();
            for a in sub.get_arguments() {
                // Globals are checked once, above. Clap does not repeat them
                // on subcommands today; skipping keeps that from mattering.
                if a.is_global_set() {
                    continue;
                }
                let long = long_of(a);
                if !flag_longs.contains(&long) && !own.contains(&long) {
                    undeclared.push(format!(
                        "  {name} --{long}: clap parses it, cli.yaml declares \
                         it in neither flags nor subcommand_options.{name}"
                    ));
                }
            }
        }
        assert!(
            undeclared.is_empty(),
            "clap parses {} argument(s) the spec does not declare:\n{}",
            undeclared.len(),
            undeclared.join("\n")
        );

        // ── spec → clap ──────────────────────────────────────────────────
        let top_non_global: BTreeSet<String> = cmd
            .get_arguments()
            .filter(|a| !a.is_global_set())
            .map(long_of)
            .collect();
        let top_global: BTreeSet<String> = cmd
            .get_arguments()
            .filter(|a| a.is_global_set())
            .map(long_of)
            .collect();

        let mut missing = Vec::new();
        for long in &flag_longs {
            if !top_non_global.contains(long) {
                missing.push(format!(
                    "  --{long}: cli.yaml declares it in flags, clap has no \
                     such top-level argument"
                ));
            }
        }
        for long in &global_longs {
            if !top_global.contains(long) {
                missing.push(format!(
                    "  --{long}: cli.yaml declares it in global_options, clap \
                     has no such global argument"
                ));
            }
        }
        for (name, opts) in &spec.subcommand_options {
            match cmd
                .get_subcommands()
                .find(|s| s.get_name() == name.as_str())
            {
                None => missing.push(format!(
                    "  cli.yaml declares subcommand_options.{name}, clap has \
                     no {name} subcommand"
                )),
                Some(sub) => {
                    let have: BTreeSet<String> =
                        sub.get_arguments().map(long_of).collect();
                    for long in longs(opts) {
                        if !have.contains(&long) {
                            missing.push(format!(
                                "  {name} --{long}: cli.yaml declares it, \
                                 clap's {name} has no such argument"
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "cli.yaml declares {} argument(s) clap does not parse:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    /// Every global option works on either side of every subcommand, and
    /// reaches the same field either way.
    ///
    /// The position before the subcommand was rejected until 2026-09-13, by
    /// `args_conflicts_with_subcommands`, which clap applies to globals too —
    /// and it is the form a user types first. Derived from clap, so a new
    /// global is covered without editing this.
    #[test]
    fn global_options_go_either_side_of_the_subcommand() {
        let cmd = Cli::command();
        let globals: Vec<&clap::Arg> =
            cmd.get_arguments().filter(|a| a.is_global_set()).collect();
        assert!(globals.len() >= 4, "expected at least the four globals");

        for sub in ["build", "fetch", "run", "conf"] {
            for arg in &globals {
                let opt = argv_for(arg);
                let before: Vec<String> = ["xtgeoip".to_owned()]
                    .into_iter()
                    .chain(opt.iter().cloned())
                    .chain([sub.to_owned()])
                    .collect();
                let after: Vec<String> = ["xtgeoip".to_owned(), sub.to_owned()]
                    .into_iter()
                    .chain(opt.iter().cloned())
                    .collect();
                let render = |argv: &[String]| match Cli::try_parse_argv(argv) {
                    Ok(c) => format!(
                        "{:?} {:?} {:?} {}",
                        c.config, c.ca_file, c.log_file, c.no_log
                    ),
                    Err(e) => panic!("{argv:?} rejected: {e}"),
                };
                assert_eq!(
                    render(&before),
                    render(&after),
                    "{opt:?} with {sub}"
                );
            }
        }
    }

    /// Every non-global top-level argument still conflicts with a subcommand,
    /// as a clap `ArgumentConflict` — which `main` exits 2 on, as before.
    ///
    /// This is the rule `args_conflicts_with_subcommands` used to enforce and
    /// `Cli::try_parse_argv` now does. `xtgeoip -b build` must not quietly
    /// become either a top-level backup or a build with one. Derived from
    /// clap rather than listing `-b -c -p -f -l`, so a flag added later is
    /// held to it too. Confirmed failing with the check removed.
    #[test]
    fn top_level_flags_still_conflict_with_a_subcommand() {
        let cmd = Cli::command();
        let top_level: Vec<&clap::Arg> =
            cmd.get_arguments().filter(|a| !a.is_global_set()).collect();
        assert!(
            top_level.len() >= 5,
            "expected at least -b -c -p -f -l at the top level"
        );

        for sub in ["build", "fetch", "run", "conf"] {
            for arg in &top_level {
                let argv: Vec<String> = ["xtgeoip".to_owned()]
                    .into_iter()
                    .chain(argv_for(arg))
                    .chain([sub.to_owned()])
                    .collect();
                match Cli::try_parse_argv(&argv) {
                    Err(e) => assert_eq!(
                        e.kind(),
                        ErrorKind::ArgumentConflict,
                        "{argv:?}: {e}"
                    ),
                    Ok(_) => panic!("{argv:?} was accepted"),
                }
            }
        }
    }

    /// Every guard table, with the context name used in failure messages.
    fn all_contexts() -> [(&'static str, &'static [Guard]); 4] {
        [
            ("top-level", TOP_LEVEL_GUARDS),
            ("build", BUILD_GUARDS),
            ("fetch", FETCH_GUARDS),
            ("run", RUN_GUARDS),
        ]
    }

    /// Does the real CLI accept this invocation — i.e. would it exit 0?
    ///
    /// Two asymmetries make this less obvious than `is_ok()`:
    ///
    /// - **`-h` is an `Err`.** clap intercepts help/version before
    ///   `normalize_cli_to_action` ever runs and reports them as errors, so
    ///   they must be mapped back to accepted.
    /// - **`Ok(ShowHelp)` is a rejection.** Despite the name, `ShowHelp` is
    ///   produced at exactly one place — bare `xtgeoip` with no flags
    ///   (`cli.rs:267`) — and `main.rs:94` renders it as `Error
    ///   [top_level_no_args]` and exits non-zero. It is never the result of the
    ///   user *asking* for help; that path is the clap `Err` above. Treating it
    ///   as valid is what made this test fail on its first run against
    ///   `xtgeoip` with no arguments.
    fn accepted(argv: &[&str]) -> bool {
        match Cli::try_parse_argv(argv) {
            Err(e) => {
                matches!(
                    e.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                )
            }
            Ok(cli) => {
                matches!(
                    normalize_cli_to_action(&cli),
                    Ok(CliOutcome::Action(_))
                )
            }
        }
    }

    /// Class 1: every spec example must agree with the implementation.
    #[test]
    fn spec_examples_agree_with_parser() {
        let mut disagreements = Vec::new();
        for ex in CLI_MATRIX {
            let argv: Vec<&str> = ex.cmd.split_whitespace().collect();
            let actual = accepted(&argv);
            if actual != ex.valid {
                disagreements.push(format!(
                    "  {:?}: spec says valid={}, parser says valid={} \
                     (outcome: {:?})",
                    ex.cmd, ex.valid, actual, ex.outcome
                ));
            }
        }
        assert!(
            disagreements.is_empty(),
            "{} of {} spec examples contradict the parser:\n{}",
            disagreements.len(),
            CLI_MATRIX.len(),
            disagreements.join("\n")
        );
    }

    /// Class 2: every guard must be the first to fire for at least one flag
    /// combination. A guard that never wins is dead code whose error message
    /// can never be produced.
    #[test]
    fn every_guard_is_reachable() {
        let mut dead = Vec::new();
        for (ctx, guards) in all_contexts() {
            for (i, guard) in guards.iter().enumerate() {
                // 5 flag bits => 32 combinations, exhaustive.
                let reachable = (0u8..32).any(|flags| {
                    first_guard(flags, guards)
                        .is_some_and(|g| std::ptr::eq(g, guard))
                });
                if !reachable {
                    let shadow = (0u8..32)
                        .find(|&f| {
                            f & guard.require == guard.require
                                && f & guard.forbid == 0
                        })
                        .and_then(|f| first_guard(f, guards))
                        .map_or("nothing", |g| g.key);
                    dead.push(format!(
                        "  [{ctx}] guard #{i} {:?} never fires (shadowed by \
                         {shadow:?})",
                        guard.key
                    ));
                }
            }
        }
        assert!(
            dead.is_empty(),
            "unreachable guards — their error messages cannot be produced:\n{}",
            dead.join("\n")
        );
    }

    /// Guard keys are how errors are identified in output (`[key]: message`)
    /// and how `testcases.yaml` asserts which rule fired; duplicates within a
    /// context would make both ambiguous.
    #[test]
    fn guard_keys_are_unique_within_context() {
        for (ctx, guards) in all_contexts() {
            let keys: std::collections::BTreeSet<&str> =
                guards.iter().map(|g| g.key).collect();
            assert_eq!(keys.len(), guards.len(), "[{ctx}] duplicate guard key");
        }
    }

    /// Every flag in the universe must be constrained somewhere. A flag that
    /// appears in no guard is either unconstrained by design or an omission —
    /// this pins which, so adding a flag without rules fails loudly.
    #[test]
    fn every_flag_is_referenced_by_some_guard() {
        let used = all_contexts()
            .iter()
            .flat_map(|(_, guards)| guards.iter())
            .fold(0u8, |acc, g| acc | g.require | g.forbid);
        for (bit, name) in
            [(B, "-b"), (C, "-c"), (F, "-f"), (L, "-l"), (P, "-p")]
        {
            assert!(
                used & bit != 0,
                "flag {name} is declared but constrained by no guard"
            );
        }
    }
}
