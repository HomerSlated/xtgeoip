//! xtgeoip-docgen v3.1 (stable, schema-safe)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,

    #[serde(default)]
    pub proof: Option<Proof>,

    #[serde(default)]
    pub flags: BTreeMap<String, FlagDef>,

    #[serde(default)]
    pub error_cases: Option<BTreeMap<String, ErrorCase>>,

    #[serde(default)]
    pub top_level: Option<CommandSpec>,

    #[serde(default)]
    pub commands: BTreeMap<String, CommandSpec>,

    #[serde(default)]
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagDef {
    pub long: String,
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub unique_maps_to: Option<bool>,
    pub full_branch_coverage: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorCase {
    pub maps_to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub program: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum CommandSpec {
    FlagCommand {
        summary: String,
        allowed_flags: Vec<String>,
        #[serde(default)]
        reject: Vec<RejectSpec>,
        #[serde(default)]
        guards: Vec<GuardSpec>,
        examples: Vec<Example>,
    },
    SelectorCommand {
        summary: String,
        usage: String,
        selector_flags: SelectorFlags,
        constraints: Option<Constraints>,
        examples: Vec<Example>,
    },
}

/// A single combination guard: fires when every flag in `require` is present
/// AND every flag in `forbid` is absent. First firing guard (in declared order,
/// after lowered `reject` entries) wins → its `error` case is emitted.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardSpec {
    #[serde(default)]
    pub require: Vec<String>,
    #[serde(default)]
    pub forbid: Vec<String>,
    pub error: String,
}

/// A "flag not allowed in this context" rejection. Its `flag` set (across the
/// list) MUST equal the complement of `allowed_flags`; order is precedence and
/// is preserved. Lowered to a leading single-flag guard (`require:[flag]`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectSpec {
    pub flag: String,
    pub error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorFlags {
    pub choices: BTreeMap<String, ChoiceSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceSummary {
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    pub exactly_one_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub case_id: Option<String>,
    pub cmd: String,
    pub valid: bool,
    pub outcome: Option<String>,
    pub reason: Option<Reason>,
    pub exit_status: Option<i32>,
    pub note: Option<String>,
    pub maps_to: Option<String>,
    pub rebuild: Option<bool>,
    pub timeout_secs: Option<u64>,
    pub expected_stdout: Option<String>,
    pub expected_stderr: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    pub code: String,
    pub args: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonTemplate {
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManpageTemplate {
    description: String,
    commands: String,
    options: String,
    execution_order: String,
    legacy_mode: String,
    configuration: String,
    files: String,
    see_also: String,
    authors: String,
}

/// Schema version of the *output* file `docs/generated/testcases.yaml`.
///
/// Distinct from `SUPPORTED_SCHEMA_VERSION` ("3.1"), which versions the
/// *input* spec `docs/spec/cli.yaml`. Bump this only when the testcase file's
/// shape changes, and update the matching constant in `xtgeoip-tests.rs`,
/// which refuses to run against an unrecognised version.
const TESTCASES_SCHEMA_VERSION: u32 = 1;

/// Envelope for `testcases.yaml`: a version tag plus the case list.
///
/// The file was a bare YAML sequence before schema 1 (#77); wrapping it gives
/// the reader something to check before trusting the contents.
#[derive(Debug, Serialize, Deserialize)]
struct TestcaseFile {
    schema_version: u32,
    testcases: Vec<Testcase>,
}

// Deserialize is required for the round-trip self-check in
// generate_testcases_yaml, not just for emission.
#[derive(Debug, Serialize, Deserialize)]
struct Testcase {
    case_id: Option<String>,
    key: String,
    cmd: Vec<String>,
    maps_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_status: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rebuild: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_stderr: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let yaml_str = fs::read_to_string("docs/spec/cli.yaml")?;
    let spec: Spec = serde_saphyr::from_str(&yaml_str)?;

    const SUPPORTED_SCHEMA_VERSION: &str = "3.1";
    if spec.version != SUPPORTED_SCHEMA_VERSION {
        anyhow::bail!(
            "Unsupported spec schema version '{}' (expected '{}')",
            spec.version,
            SUPPORTED_SCHEMA_VERSION
        );
    }

    validate_spec(&spec)?;
    validate_examples(&spec)?;
    validate_rules(&spec)?;

    let toml_str = fs::read_to_string("docs/spec/manpage-template.toml")?;
    let tmpl: ManpageTemplate = toml::from_str(&toml_str)?;

    fs::create_dir_all("docs/generated")?;
    fs::create_dir_all("src/generated")?;

    fs::write("docs/generated/usage.md", generate_usage_md(&spec)?)?;
    fs::write("docs/generated/tldr.md", generate_tldr_md(&spec)?)?;
    fs::write("docs/generated/xtgeoip.1", generate_manpage(&spec, &tmpl)?)?;
    fs::write(
        "src/generated/mod.rs",
        "pub mod cli_matrix;\npub mod cli_rules;\npub mod error_text;\n",
    )?;
    fs::write(
        "src/generated/error_text.rs",
        generate_error_text_rs(&spec)?,
    )?;
    fs::write(
        "src/generated/cli_matrix.rs",
        generate_cli_matrix_rs(&spec)?,
    )?;
    fs::write("src/generated/cli_rules.rs", generate_cli_rules_rs(&spec)?)?;
    fs::write(
        "docs/generated/testcases.yaml",
        generate_testcases_yaml(&spec)?,
    )?;

    println!("Docs generated successfully.");
    Ok(())
}

/* ---------------- VALIDATION ---------------- */

/// Enforce the field invariant that `valid` implies (#76).
///
/// The spec has always followed a strict bimodal rule, but nothing checked
/// it, so a violation would have been absorbed by `resolve_outcome`'s old
/// `"OK"` / `"ERROR"` fallbacks and shipped as real-looking documentation:
///
/// | `valid` | `outcome` | `reason` | `maps_to` |
/// |---------|-----------|----------|-----------|
/// | `true`  | required  | rejected | rejected  |
/// | `false` | rejected  | required | required  |
///
/// A valid example describes what it *does*; an invalid one describes why it
/// is refused, and must name the error case it maps to so the integration
/// suite can assert the keyed error. Mixing the two is always a spec mistake.
fn validate_examples(spec: &Spec) -> anyhow::Result<()> {
    let mut problems: Vec<String> = Vec::new();

    let mut check = |scope: &str, ex: &Example| {
        let id = ex.case_id.as_deref().unwrap_or("<no case_id>");
        let where_ = format!("[{scope}] {id} ({:?})", ex.cmd);

        if ex.valid {
            if ex.outcome.is_none() {
                problems.push(format!("{where_}: valid, but no `outcome`"));
            }
            if ex.reason.is_some() {
                problems.push(format!("{where_}: valid, but has a `reason`"));
            }
            if ex.maps_to.is_some() {
                problems.push(format!("{where_}: valid, but has `maps_to`"));
            }
        } else {
            if ex.reason.is_none() {
                problems.push(format!("{where_}: invalid, but no `reason`"));
            }
            if ex.maps_to.is_none() {
                problems.push(format!("{where_}: invalid, but no `maps_to`"));
            }
            if ex.outcome.is_some() {
                problems.push(format!(
                    "{where_}: invalid, but has an `outcome` (the text comes \
                     from its reason template)"
                ));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        let (CommandSpec::FlagCommand { examples, .. }
        | CommandSpec::SelectorCommand { examples, .. }) = cmd;
        for ex in examples {
            check("top_level", ex);
        }
    }
    for (name, cmd) in &spec.commands {
        let (CommandSpec::FlagCommand { examples, .. }
        | CommandSpec::SelectorCommand { examples, .. }) = cmd;
        for ex in examples {
            check(name, ex);
        }
    }

    anyhow::ensure!(
        problems.is_empty(),
        "{} example(s) violate the valid/outcome/reason invariant:\n{}",
        problems.len(),
        problems.join("\n")
    );
    Ok(())
}

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used_error_cases: BTreeSet<String> = BTreeSet::new();
    let mut duplicate_maps_to: BTreeSet<String> = BTreeSet::new();

    let error_cases = spec.error_cases.as_ref();

    let check = |scope: &str,
                 ex: &Example,
                 used: &mut BTreeSet<String>,
                 dupes: &mut BTreeSet<String>|
     -> anyhow::Result<()> {
        if let Some(reason) = &ex.reason
            && !spec.reason_templates.contains_key(&reason.code)
        {
            anyhow::bail!("Unknown reason code {} in {}", reason.code, scope);
        }

        if !ex.valid {
            let maps_to = ex.maps_to.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Missing maps_to in invalid example {}", ex.cmd)
            })?;

            if let Some(ec) = error_cases
                && !ec.contains_key(maps_to)
            {
                anyhow::bail!("Unknown error case {}", maps_to);
            }

            if !used.insert(maps_to.clone()) {
                dupes.insert(maps_to.clone());
            }
        }

        Ok(())
    };

    let visit = |name: &str,
                 cmd: &CommandSpec,
                 used: &mut BTreeSet<String>,
                 dupes: &mut BTreeSet<String>|
     -> anyhow::Result<()> {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in exs {
            check(name, ex, used, dupes)?;
        }

        Ok(())
    };

    // IMPORTANT: top_level is a command too
    if let Some(cmd) = &spec.top_level {
        visit(
            "top_level",
            cmd,
            &mut used_error_cases,
            &mut duplicate_maps_to,
        )?;
    }

    for (name, cmd) in &spec.commands {
        visit(name, cmd, &mut used_error_cases, &mut duplicate_maps_to)?;
    }

    // UNIQUE MAPS_TO CHECK
    if spec
        .proof
        .as_ref()
        .and_then(|p| p.unique_maps_to)
        .unwrap_or(false)
        && !duplicate_maps_to.is_empty()
    {
        anyhow::bail!(
            "Duplicate maps_to references (proof.unique_maps_to violated): \
             {:?}",
            duplicate_maps_to
        );
    }

    // FULL COVERAGE CHECK
    if spec
        .proof
        .as_ref()
        .and_then(|p| p.full_branch_coverage)
        .unwrap_or(false)
    {
        let mut unused = Vec::new();

        if let Some(ec) = error_cases {
            for (key, case) in ec {
                if !used_error_cases.contains(&case.maps_to) {
                    unused.push(key.clone());
                }
            }
        }

        if !unused.is_empty() {
            anyhow::bail!(
                "Unused error cases (no invalid example maps_to reference): \
                 {:?}",
                unused
            );
        }
    }

    Ok(())
}

/* ---------------- RULE VALIDATION & CROSS-CHECK ---------------- */

/// A lowered guard (reject entry or combination guard) in evaluation order.
struct LoweredGuard {
    require: Vec<String>,
    forbid: Vec<String>,
    error: String,
}

/// Lower `reject` + `guards` into one ordered list: reject entries first (each
/// a single-flag `require`), then combination guards. This is the canonical
/// lowering the runtime evaluator must mirror.
fn lower_guards(
    reject: &[RejectSpec],
    guards: &[GuardSpec],
) -> Vec<LoweredGuard> {
    let mut out: Vec<LoweredGuard> = reject
        .iter()
        .map(|r| LoweredGuard {
            require: vec![r.flag.clone()],
            forbid: Vec::new(),
            error: r.error.clone(),
        })
        .collect();
    out.extend(guards.iter().map(|g| LoweredGuard {
        require: g.require.clone(),
        forbid: g.forbid.clone(),
        error: g.error.clone(),
    }));
    out
}

/// First guard that fires for `flags` (all `require` present, all `forbid`
/// absent). First-match = precedence.
fn first_guard<'a>(
    flags: &BTreeSet<String>,
    guards: &'a [LoweredGuard],
) -> Option<&'a str> {
    guards
        .iter()
        .find(|g| {
            g.require.iter().all(|r| flags.contains(r))
                && g.forbid.iter().all(|f| !flags.contains(f))
        })
        .map(|g| g.error.as_str())
}

/// Extract the flag set from an example `cmd` for `context`. Returns None when
/// the example is outside the guard model (uses `-h`, long flags, or any token
/// not a single short flag in `universe`).
fn example_flags(
    context: &str,
    cmd: &str,
    universe: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    let mut toks = cmd.split_whitespace();
    toks.next()?; // program name
    let mut rest: Vec<&str> = toks.collect();
    if context != "top_level" {
        match rest.first() {
            Some(&t) if t == context => {
                rest.remove(0);
            }
            _ => return None,
        }
    }
    let mut flags = BTreeSet::new();
    for t in rest {
        let f = t.strip_prefix('-')?;
        if f.len() != 1 || !universe.contains(f) {
            return None; // -h, long flags, etc.
        }
        flags.insert(f.to_string());
    }
    Some(flags)
}

/// Validate the `reject`/`guards` rules and cross-check that they reproduce
/// every example's documented outcome. This keeps the rules and the examples
/// provably consistent (the exhaustive snapshot test pins the full input space;
/// #92).
fn validate_rules(spec: &Spec) -> anyhow::Result<()> {
    let universe: BTreeSet<String> = spec.flags.keys().cloned().collect();
    let error_cases = spec.error_cases.as_ref();

    if let Some(cmd) = &spec.top_level {
        check_context("top_level", cmd, &universe, error_cases)?;
    }
    for (name, cmd) in &spec.commands {
        check_context(name, cmd, &universe, error_cases)?;
    }
    Ok(())
}

fn check_context(
    name: &str,
    cmd: &CommandSpec,
    universe: &BTreeSet<String>,
    error_cases: Option<&BTreeMap<String, ErrorCase>>,
) -> anyhow::Result<()> {
    // conf (SelectorCommand) is out of the guard model by design: clap's
    // ArgGroup already enforces exactly-one-of [-d/-s/-e] at parse time.
    let CommandSpec::FlagCommand {
        allowed_flags,
        reject,
        guards,
        examples,
        ..
    } = cmd
    else {
        return Ok(());
    };

    let allowed: BTreeSet<String> = allowed_flags.iter().cloned().collect();
    for f in &allowed {
        if !universe.contains(f) {
            anyhow::bail!("{name}: allowed_flags references unknown flag {f}");
        }
    }

    // reject's flag-set MUST equal the complement of allowed_flags (no
    // intra-spec duplication; allowed_flags stays the sole owner of the set).
    let complement: BTreeSet<String> =
        universe.difference(&allowed).cloned().collect();
    let reject_set: BTreeSet<String> =
        reject.iter().map(|r| r.flag.clone()).collect();
    if reject_set.len() != reject.len() {
        anyhow::bail!("{name}: duplicate flag in reject");
    }
    if reject_set != complement {
        anyhow::bail!(
            "{name}: reject set {reject_set:?} != complement of allowed_flags \
             {complement:?}"
        );
    }

    let valid_ec =
        |key: &str| error_cases.is_none_or(|ec| ec.contains_key(key));
    for r in reject {
        if !valid_ec(&r.error) {
            anyhow::bail!("{name}: unknown error case {} in reject", r.error);
        }
    }
    for g in guards {
        if !valid_ec(&g.error) {
            anyhow::bail!("{name}: unknown error case {} in guard", g.error);
        }
        for f in g.require.iter().chain(g.forbid.iter()) {
            if !allowed.contains(f) {
                anyhow::bail!(
                    "{name}: guard references flag {f} not in allowed_flags \
                     (use reject for disallowed flags)"
                );
            }
        }
    }

    // CROSS-CHECK: evaluate the lowered rules against every example.
    let lowered = lower_guards(reject, guards);
    for ex in examples {
        let Some(flags) = example_flags(name, &ex.cmd, universe) else {
            continue;
        };

        // Expected error from the rules, plus the top-level empty special case
        // (bare invocation -> ShowHelp, rendered by main as top_level_no_args).
        let expected: Option<&str> = match first_guard(&flags, &lowered) {
            Some(e) => Some(e),
            None if name == "top_level" && flags.is_empty() => {
                Some("top_level_no_args")
            }
            None => None,
        };

        match (ex.valid, expected) {
            (true, None) => {}
            (true, Some(e)) => anyhow::bail!(
                "{name}: example `{}` is valid but rules reject it ({e})",
                ex.cmd
            ),
            (false, Some(e)) => {
                let want = ex.maps_to.as_deref().unwrap_or("");
                if e != want {
                    anyhow::bail!(
                        "{name}: example `{}` maps_to {want} but rules \
                         produce {e}",
                        ex.cmd
                    );
                }
            }
            (false, None) => anyhow::bail!(
                "{name}: example `{}` is invalid ({:?}) but rules accept it",
                ex.cmd,
                ex.maps_to
            ),
        }
    }

    Ok(())
}

/* ---------------- CLI RULES (runtime guard table) ---------------- */

fn examples_of(cmd: &CommandSpec) -> &[Example] {
    match cmd {
        CommandSpec::FlagCommand { examples, .. }
        | CommandSpec::SelectorCommand { examples, .. } => examples,
    }
}

/// Render a flag-name list as an OR of generated bit constants (`B | C`), or
/// `0` for the empty set.
fn flag_bits(flags: &[String]) -> String {
    if flags.is_empty() {
        "0".to_string()
    } else {
        flags
            .iter()
            .map(|f| f.to_uppercase())
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Emit `src/generated/cli_rules.rs`: the per-context guard tables that the
/// runtime (`normalize_cli_to_action`) evaluates. Flags are encoded as a `u8`
/// bitmask in sorted flag-universe order; each `Guard` mirrors a `LoweredGuard`
/// (reject entries first as single-flag requires, then combination guards), so
/// the runtime evaluates the exact lowering this docgen cross-checks. Messages
/// are referenced from `error_text::NO_*` (resolved via the canonical example's
/// reason code) so each message literal lives in exactly one generated place.
fn generate_cli_rules_rs(spec: &Spec) -> anyhow::Result<String> {
    // Sorted flag universe (BTreeMap keys): the bit index is the position.
    let universe: Vec<&String> = spec.flags.keys().collect();

    // error_case key -> reason code, from the unique invalid example
    // (proof.unique_maps_to guarantees one; full_branch_coverage guarantees
    // every case is present). This is the only declared link between an error
    // key and its message text.
    let mut all_examples: Vec<&Example> = Vec::new();
    if let Some(cmd) = &spec.top_level {
        all_examples.extend(examples_of(cmd));
    }
    for cmd in spec.commands.values() {
        all_examples.extend(examples_of(cmd));
    }
    let mut reason_of: BTreeMap<&str, &str> = BTreeMap::new();
    for ex in all_examples {
        if !ex.valid
            && let (Some(mt), Some(r)) = (ex.maps_to.as_deref(), &ex.reason)
        {
            reason_of.insert(mt, r.code.as_str());
        }
    }

    let mut out = String::from(
        "// auto-generated\n#![allow(dead_code)]\nuse \
         crate::generated::error_text;\n\n",
    );

    out.push_str("// Flag bits, in sorted flag-universe order.\n");
    for (i, f) in universe.iter().enumerate() {
        out.push_str(&format!(
            "pub const {}: u8 = 1 << {i};\n",
            f.to_uppercase()
        ));
    }
    out.push_str(
        "\n/// One combination guard: fires when every `require` bit is \
         present and\n/// no `forbid` bit is. First firing guard per context \
         wins (= precedence).\npub struct Guard {\n    pub require: u8,\n    \
         pub forbid: u8,\n    pub key: &'static str,\n    pub message: \
         &'static str,\n}\n\n",
    );

    // One const array per FlagCommand context, in source order (top_level
    // first, then commands alphabetically). SelectorCommand (conf) is excluded.
    let mut contexts: Vec<(String, &Vec<RejectSpec>, &Vec<GuardSpec>)> =
        Vec::new();
    if let Some(CommandSpec::FlagCommand { reject, guards, .. }) =
        &spec.top_level
    {
        contexts.push(("TOP_LEVEL_GUARDS".to_string(), reject, guards));
    }
    for (name, cmd) in &spec.commands {
        if let CommandSpec::FlagCommand { reject, guards, .. } = cmd {
            contexts.push((
                format!("{}_GUARDS", name.to_uppercase()),
                reject,
                guards,
            ));
        }
    }

    for (const_name, reject, guards) in contexts {
        out.push_str(&format!("pub const {const_name}: &[Guard] = &[\n"));
        for g in lower_guards(reject, guards) {
            let code = reason_of.get(g.error.as_str()).ok_or_else(|| {
                anyhow::anyhow!("no example reason for error case {}", g.error)
            })?;
            out.push_str(&format!(
                "    Guard {{ require: {}, forbid: {}, key: \"{}\", message: \
                 error_text::{} }},\n",
                flag_bits(&g.require),
                flag_bits(&g.forbid),
                g.error,
                code.to_uppercase(),
            ));
        }
        out.push_str("];\n\n");
    }

    Ok(out)
}

/* ---------------- OUTCOME ---------------- */

/// Resolve an example's user-facing outcome text.
///
/// Fallible by design (#76). This previously returned `"OK"` for a valid
/// example with no `outcome` and `"ERROR"` for an invalid one with no usable
/// `reason` — placeholders that look like real output and would have shipped
/// into the man page, the markdown and `CLI_MATRIX` alike. Missing spec data
/// must not produce output, so both cases now fail generation.
///
/// `validate_examples` rejects these at spec-load time, so in practice
/// neither branch is reachable; they are the enforcement of last resort for a
/// caller that skipped validation.
fn resolve_outcome(spec: &Spec, ex: &Example) -> anyhow::Result<String> {
    if ex.valid {
        return ex.outcome.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "Example {:?} is valid but declares no `outcome`",
                ex.cmd
            )
        });
    }

    let reason = ex.reason.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Example {:?} is invalid but declares no `reason`",
            ex.cmd
        )
    })?;
    let t = spec.reason_templates.get(&reason.code).ok_or_else(|| {
        anyhow::anyhow!(
            "Example {:?} references unknown reason template {:?}",
            ex.cmd,
            reason.code
        )
    })?;

    let mut text = t.text.clone();
    if let Some(args) = &reason.args {
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), v);
        }
    }
    Ok(text)
}

/* ---------------- USAGE ---------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    let render = |out: &mut String,
                  spec: &Spec,
                  exs: &[Example],
                  title: &str,
                  extra: Option<&str>|
     -> anyhow::Result<()> {
        out.push_str(&format!("## {}\n", title));

        if let Some(e) = extra {
            out.push_str(e);
            out.push('\n');
        }

        for ex in exs {
            let outcome = resolve_outcome(spec, ex)?;
            out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));

            if let Some(s) = ex.exit_status {
                out.push_str(&format!(" (exit {})", s));
            }
            if let Some(n) = &ex.note {
                out.push_str(&format!(" — {}", n));
            }
            out.push('\n');
        }

        out.push('\n');
        Ok(())
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(summary))?;
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(usage))?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(summary))?;
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(usage))?;
            }
        }
    }

    Ok(out)
}

/* ---------------- TLDR ---------------- */

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    let mut add = |exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                out.push_str(&format!(
                    "- {}:\n\n`{}`\n\n",
                    ex.outcome.clone().unwrap_or_default(),
                    ex.cmd
                ));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    Ok(out)
}

/* ---------------- ERROR TEXT ---------------- */

fn generate_error_text_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "// auto-generated\n#![allow(dead_code)]\n".to_string();

    for (k, v) in &spec.reason_templates {
        out.push_str(&format!(
            "pub const {}: &str = r#\"{}\"#;\n",
            k.to_uppercase(),
            v.text
        ));
    }

    Ok(out)
}

/* ---------------- CLI MATRIX ---------------- */

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::from(
        "// auto-generated\n#![allow(dead_code)]\npub struct CliExample { pub \
         cmd: &'static str, pub valid: bool, pub outcome: &'static str }\npub \
         const CLI_MATRIX: &[CliExample] = &[\n",
    );

    let mut add = |exs: &[Example]| -> anyhow::Result<()> {
        for ex in exs {
            let outcome = resolve_outcome(spec, ex)?;
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" \
                 }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
        Ok(())
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs)?;
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs)?;
    }

    out.push_str("];\n");
    Ok(out)
}

/* ---------------- TESTCASES ---------------- */

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    let mut add = |exs: &[Example]| {
        for ex in exs {
            testcases.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.into(),
                cmd: ex.cmd.split_whitespace().map(String::from).collect(),
                maps_to: ex.maps_to.clone(),
                exit_status: ex.exit_status,
                rebuild: ex.rebuild,
                timeout_secs: ex.timeout_secs,
                expected_stdout: ex.expected_stdout.clone(),
                expected_stderr: ex.expected_stderr.clone(),
            });
        }
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    let file = TestcaseFile {
        schema_version: TESTCASES_SCHEMA_VERSION,
        testcases,
    };
    let yaml = serde_saphyr::to_string(&file)?;

    // Round-trip self-check (#77c): parse the emitted YAML back and re-emit
    // it. Any field the emitter writes but the parser cannot read — or reads
    // differently — shows up here, at generation time, rather than as a
    // confusing failure inside the integration suite that consumes this file.
    let reparsed: TestcaseFile = serde_saphyr::from_str(&yaml).context(
        "Generated testcases.yaml could not be parsed back — emitter and \
         parser disagree",
    )?;
    let reemitted = serde_saphyr::to_string(&reparsed)?;
    anyhow::ensure!(
        yaml == reemitted,
        "testcases.yaml is not round-trip stable: re-emitting the parsed file \
         produced different output"
    );

    Ok(yaml)
}

/* ---------------- MANPAGE ---------------- */

fn generate_manpage(
    spec: &Spec,
    tmpl: &ManpageTemplate,
) -> anyhow::Result<String> {
    let prog = &spec.meta.program;
    let version = env!("CARGO_PKG_VERSION");
    let mut out = String::new();

    let push_section = |out: &mut String, heading: &str, body: &str| {
        out.push_str(&format!(".SH {}\n", heading));
        out.push_str(body.trim_end_matches('\n'));
        out.push('\n');
    };

    // Header
    out.push_str(&format!(
        ".TH {} 1 \"\" \"{} {}\" \"User Commands\"\n",
        prog.to_uppercase(),
        prog,
        version
    ));

    // NAME (from spec meta)
    push_section(
        &mut out,
        "NAME",
        &format!("{} \\- {}\n", prog, spec.meta.summary),
    );

    // SYNOPSIS (from spec top_level flags + command names)
    out.push_str(".SH SYNOPSIS\n");
    if let Some(cmd) = &spec.top_level
        && let CommandSpec::FlagCommand { allowed_flags, .. } = cmd
    {
        let flags: String = allowed_flags
            .iter()
            .map(|f| format!("[\\fB\\-{}\\fR]", f))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(".B {}\n{}\n.br\n", prog, flags));
    }
    out.push_str(&format!(".B {}\n\\fIcommand\\fR [\\fIoptions\\fR]\n", prog));

    // DESCRIPTION, COMMANDS, OPTIONS, EXECUTION ORDER, LEGACY MODE,
    // CONFIGURATION from template
    push_section(&mut out, "DESCRIPTION", &tmpl.description);
    push_section(&mut out, "COMMANDS", &tmpl.commands);
    push_section(&mut out, "OPTIONS", &tmpl.options);
    push_section(&mut out, "EXECUTION ORDER", &tmpl.execution_order);
    push_section(&mut out, "LEGACY MODE", &tmpl.legacy_mode);
    push_section(&mut out, "CONFIGURATION", &tmpl.configuration);

    // EXAMPLES (from spec valid examples)
    out.push_str(".SH EXAMPLES\n");
    let emit_valid = |out: &mut String, exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                let desc = ex.outcome.as_deref().unwrap_or("");
                out.push_str(&format!(".TP\n.B {}\n{}\n", ex.cmd, desc));
            }
        }
    };
    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs);
    }
    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs);
    }

    // FILES, SEE ALSO, AUTHORS from template
    push_section(&mut out, "FILES", &tmpl.files);
    push_section(&mut out, "SEE ALSO", &tmpl.see_also);
    push_section(&mut out, "AUTHORS", &tmpl.authors);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(valid: bool) -> Example {
        Example {
            case_id: Some("X-001".into()),
            cmd: "xtgeoip -x".into(),
            valid,
            outcome: None,
            reason: None,
            exit_status: None,
            note: None,
            maps_to: None,
            rebuild: None,
            timeout_secs: None,
            expected_stdout: None,
            expected_stderr: None,
        }
    }

    /// Minimal spec carrying one example in the top-level command.
    fn spec_with(ex: Example) -> Spec {
        Spec {
            meta: Meta {
                program: "xtgeoip".into(),
                summary: "test".into(),
            },
            version: "3.1".into(),
            proof: None,
            flags: BTreeMap::new(),
            error_cases: None,
            top_level: Some(CommandSpec::FlagCommand {
                summary: "test".into(),
                allowed_flags: vec![],
                reject: vec![],
                guards: vec![],
                examples: vec![ex],
            }),
            commands: BTreeMap::new(),
            reason_templates: BTreeMap::new(),
        }
    }

    fn conforming_valid() -> Example {
        Example {
            outcome: Some("does a thing".into()),
            ..example(true)
        }
    }

    fn conforming_invalid() -> Example {
        Example {
            reason: Some(Reason {
                code: "some_code".into(),
                args: None,
            }),
            maps_to: Some("some_case".into()),
            ..example(false)
        }
    }

    #[test]
    fn conforming_examples_pass() {
        assert!(validate_examples(&spec_with(conforming_valid())).is_ok());
        assert!(validate_examples(&spec_with(conforming_invalid())).is_ok());
    }

    #[test]
    fn valid_without_outcome_is_rejected() {
        let err = validate_examples(&spec_with(example(true)))
            .expect_err("must reject");
        assert!(
            err.to_string().contains("valid, but no `outcome`"),
            "unhelpful: {err}"
        );
    }

    #[test]
    fn valid_with_reason_is_rejected() {
        let ex = Example {
            reason: Some(Reason {
                code: "c".into(),
                args: None,
            }),
            ..conforming_valid()
        };
        assert!(
            validate_examples(&spec_with(ex))
                .expect_err("must reject")
                .to_string()
                .contains("valid, but has a `reason`")
        );
    }

    #[test]
    fn invalid_without_reason_is_rejected() {
        let err = validate_examples(&spec_with(example(false)))
            .expect_err("must reject");
        let msg = err.to_string();
        assert!(msg.contains("invalid, but no `reason`"), "unhelpful: {msg}");
        assert!(
            msg.contains("invalid, but no `maps_to`"),
            "unhelpful: {msg}"
        );
    }

    #[test]
    fn invalid_with_outcome_is_rejected() {
        let ex = Example {
            outcome: Some("text".into()),
            ..conforming_invalid()
        };
        assert!(
            validate_examples(&spec_with(ex))
                .expect_err("must reject")
                .to_string()
                .contains("invalid, but has an `outcome`")
        );
    }

    /// The failure message must name the offending case, or a spec author
    /// gets "something is wrong" with 51 candidates.
    #[test]
    fn rejection_names_the_case() {
        let msg = validate_examples(&spec_with(example(true)))
            .expect_err("must reject")
            .to_string();
        assert!(msg.contains("X-001"), "case_id missing from: {msg}");
        assert!(msg.contains("xtgeoip -x"), "cmd missing from: {msg}");
    }

    /// `resolve_outcome` is the enforcement of last resort for a caller that
    /// skipped validation: it must error, not emit a plausible placeholder.
    #[test]
    fn resolve_outcome_refuses_to_invent_text() {
        let spec = spec_with(conforming_valid());
        let err = resolve_outcome(&spec, &example(true))
            .expect_err("must not return \"OK\"");
        assert!(err.to_string().contains("declares no `outcome`"));

        let err = resolve_outcome(&spec, &example(false))
            .expect_err("must not return \"ERROR\"");
        assert!(err.to_string().contains("declares no `reason`"));
    }
}
