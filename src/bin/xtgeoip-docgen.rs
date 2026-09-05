//! xtgeoip-docgen v3.1 (stable, schema-safe)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
// The spec model is shared with the library so the generator and the
// program it generates for cannot drift apart on what the spec means.
use xtgeoip::spec::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManpageTemplate {
    description: String,
    commands: String,
    options: String,
    execution_order: String,
    /// The manifest-based ownership model (#98). General, so it precedes
    /// LEGACY MODE, which is one specific case of stale-owned files.
    file_ownership: String,
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
    validate_plan(&spec)?;

    let toml_str = fs::read_to_string("docs/spec/manpage-template.toml")?;
    let tmpl: ManpageTemplate = toml::from_str(&toml_str)?;
    validate_manpage_template(&spec, &tmpl)?;

    let outputs = render_outputs(&spec, &tmpl)?;

    fs::create_dir_all("docs/generated")?;
    fs::create_dir_all("src/generated")?;
    for (path, body) in outputs {
        fs::write(path, body)?;
    }

    println!("Docs generated successfully.");
    Ok(())
}

/// Render every generated file into memory, before any of them is written.
///
/// F-007. The eight outputs were generated and written one at a time, so an
/// emitter failing part-way left the tree half-regenerated: new docs and a new
/// CLI matrix beside an old `plan.rs`. The reachable trigger is a new entry in
/// `plan.steps` — every validator passes, since they only check declaredness
/// *within* the spec, and then `step_ctor` bails with "has no Step variant";
/// same for an unknown `fetch_mode` or an unknown step `param`. docgen exited
/// non-zero, but `cargo test` then compared a new `CLI_MATRIX` against an old
/// planner and failed somewhere unrelated, and the partial regeneration was
/// easy to commit by accident.
///
/// #92 moved the validators ahead of all writes; this closes the remaining gap,
/// for the errors the validators cannot see. Every fallible step now happens
/// here, and this function touches no files at all, so a failure leaves the
/// tree exactly as it was. What is left is an IO error inside the write loop —
/// a different and much rarer failure.
fn render_outputs(
    spec: &Spec,
    tmpl: &ManpageTemplate,
) -> anyhow::Result<Vec<(&'static str, String)>> {
    Ok(vec![
        ("docs/generated/usage.md", generate_usage_md(spec)?),
        ("docs/generated/tldr.md", generate_tldr_md(spec)?),
        ("docs/generated/xtgeoip.1", generate_manpage(spec, tmpl)?),
        (
            "src/generated/mod.rs",
            "pub mod cli_matrix;\npub mod cli_rules;\npub mod \
             error_text;\npub mod plan;\n"
                .to_string(),
        ),
        ("src/generated/error_text.rs", generate_error_text_rs(spec)?),
        ("src/generated/cli_matrix.rs", generate_cli_matrix_rs(spec)?),
        ("src/generated/cli_rules.rs", generate_cli_rules_rs(spec)?),
        ("src/generated/plan.rs", generate_plan_rs(spec)?),
        (
            "docs/generated/testcases.yaml",
            generate_testcases_yaml(spec)?,
        ),
    ])
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

/// Spec-internal consistency of the `plan:` section (#92, generation side).
///
/// **Why these checks and not others.** docgen links the library, and the
/// library is built from the *previously generated* sources — so anything that
/// compares the spec against the program's behaviour is inherently one
/// generation behind: change a guard, and docgen validates the new spec
/// against the old rules. Those comparisons therefore stay at test time
/// (`spec_examples_agree_with_parser`, `spec_steps_agree_with_plan`), which
/// runs after compilation and cannot lag.
///
/// What *is* sound here is everything decidable from the spec alone. That is
/// the boundary: **generation time owns spec-internal contradictions, test
/// time owns spec-versus-program agreement.**
fn validate_plan(spec: &Spec) -> anyhow::Result<()> {
    let Some(plan) = &spec.plan else {
        anyhow::bail!("cli.yaml has no `plan:` section");
    };

    let mut problems: Vec<String> = Vec::new();

    // Ranks must be unique: two steps at one rank leave the order between them
    // undefined, and the generator would pick arbitrarily.
    let mut by_rank: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for (name, step) in &plan.steps {
        by_rank.entry(step.rank).or_default().push(name);
        if step.why.trim().is_empty() {
            problems.push(format!("step `{name}` has an empty `why:`"));
        }
    }
    for (rank, names) in &by_rank {
        if names.len() > 1 {
            problems.push(format!(
                "rank {rank} is shared by {names:?} — the order between them \
                 is undefined"
            ));
        }
    }

    let mut used: BTreeSet<&str> = BTreeSet::new();
    for (ctx_name, ctx) in &plan.contexts {
        let mut runs: BTreeSet<&str> = BTreeSet::new();
        for name in &ctx.always {
            runs.insert(name.as_str());
        }
        for (letter, name) in &ctx.selects {
            runs.insert(name.as_str());
            if !spec.flags.contains_key(letter) {
                problems.push(format!(
                    "context `{ctx_name}` selects on flag `{letter}`, which \
                     is not in `flags:`"
                ));
            }
        }
        for name in &runs {
            if !plan.steps.contains_key(*name) {
                problems.push(format!(
                    "context `{ctx_name}` runs step `{name}`, which is not \
                     declared in `plan.steps`"
                ));
            }
            used.insert(name);
        }

        // The spec-level half of Fetch-before-Build. The type system enforces
        // it in the generated code (`Plan::Pipeline` cannot be named without a
        // fetch), so a violation here fails to compile rather than misbehaving
        // — but failing in the generator, by name, beats failing in rustc.
        if runs.contains("build") {
            if !runs.contains("fetch") {
                problems.push(format!(
                    "context `{ctx_name}` runs `build` without `fetch`; a \
                     build consumes a fetch result"
                ));
            }
            if ctx.fetch_mode.is_none() {
                problems.push(format!(
                    "context `{ctx_name}` builds but declares no `fetch_mode`"
                ));
            }
        }
        match ctx.fetch_mode.as_deref() {
            None => {
                if runs.contains("fetch") {
                    problems.push(format!(
                        "context `{ctx_name}` fetches but declares no \
                         `fetch_mode`"
                    ));
                }
            }
            Some("remote" | "local") => {
                if !runs.contains("fetch") {
                    problems.push(format!(
                        "context `{ctx_name}` declares a `fetch_mode` but \
                         never fetches"
                    ));
                }
            }
            Some(other) => problems.push(format!(
                "context `{ctx_name}` has fetch_mode `{other}`; expected \
                 `remote` or `local`"
            )),
        }
    }

    // A declared step no context runs is dead: it can never appear in a plan,
    // yet its rank still participates in the ordering. The analogue of
    // `every_flag_is_referenced_by_some_guard` for the plan model.
    for name in plan.steps.keys() {
        if !used.contains(name.as_str()) {
            problems.push(format!(
                "step `{name}` is declared but no context runs it"
            ));
        }
    }

    // Example step lists must use declared step names.
    let mut check_examples = |exs: &[Example]| {
        for ex in exs {
            let Some(steps) = &ex.steps else { continue };
            if !ex.valid {
                problems.push(format!(
                    "invalid example {:?} declares `steps:`; an invocation \
                     that is rejected has no plan",
                    ex.cmd
                ));
            }
            for s in steps {
                if !plan.steps.contains_key(s) {
                    problems.push(format!(
                        "example {:?} names step `{s}`, which is not declared \
                         in `plan.steps`",
                        ex.cmd
                    ));
                }
            }
        }
    };
    if let Some(cmd) = &spec.top_level {
        check_examples(examples_of(cmd));
    }
    for cmd in spec.commands.values() {
        check_examples(examples_of(cmd));
    }

    if problems.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "{} contradiction(s) in the `plan:` section:\n{}",
        problems.len(),
        problems
            .iter()
            .map(|p| format!("  * {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
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

/// What an example *means*, independent of how any format displays it (#75).
///
/// Resolution and presentation were previously fused: `resolve_outcome`
/// returned a bare `String` that every generator interpolated raw. That is
/// why two separate escaping defects went unnoticed — each format has its own
/// metacharacters, and a plain `String` carries no signal that escaping is
/// owed. Callers now receive this type and must pass it through a `render_*`
/// function, which is where escaping lives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedOutcome {
    /// A valid invocation: what it does, from the example's `outcome`.
    Succeeds { description: String },
    /// A rejected invocation: why, from its reason template with `args`
    /// substituted.
    Fails { reason: String },
}

impl ResolvedOutcome {
    /// The unescaped human-readable text.
    ///
    /// Deliberately not `Display`: rendering must be an explicit choice of
    /// target format, so that interpolating an outcome without escaping it
    /// requires visibly reaching past the renderers.
    fn text(&self) -> &str {
        match self {
            Self::Succeeds { description } => description,
            Self::Fails { reason } => reason,
        }
    }

    fn succeeded(&self) -> bool {
        matches!(self, Self::Succeeds { .. })
    }
}

/// Semantic resolution: spec data → meaning. No formatting, no fallbacks.
///
/// Fallible by design (#76). This once returned `"OK"` for a valid example
/// with no `outcome` and `"ERROR"` for an invalid one with no usable
/// `reason` — placeholders that look like real output and would have shipped
/// into the man page, the markdown and `CLI_MATRIX` alike. Missing spec data
/// must not produce output.
///
/// `validate_examples` rejects these at spec-load time, so neither error is
/// reachable in practice; they are the enforcement of last resort for a
/// caller that skipped validation.
fn resolve_outcome(
    spec: &Spec,
    ex: &Example,
) -> anyhow::Result<ResolvedOutcome> {
    if ex.valid {
        let description = ex.outcome.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "Example {:?} is valid but declares no `outcome`",
                ex.cmd
            )
        })?;
        return Ok(ResolvedOutcome::Succeeds { description });
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

    // Template substitution is resolution, not presentation: the result is
    // the same message regardless of the target format.
    let mut text = t.text.clone();
    if let Some(args) = &reason.args {
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), v);
        }
    }
    Ok(ResolvedOutcome::Fails { reason: text })
}

/* ---------------- PRESENTATION ---------------- */

/// Markdown / plain-text targets (`usage.md`, `tldr.md`).
///
/// The text is emitted inside prose, not inside a code span or literal, so
/// no metacharacter has structural meaning here.
fn render_plain(outcome: &ResolvedOutcome) -> String {
    outcome.text().to_string()
}

/// Rust source target (`cli_matrix.rs`).
///
/// The text lands inside a `&'static str` literal, so `"` and `\` would
/// otherwise emit code that does not compile. `{:?}` on a `str` produces a
/// correctly escaped Rust literal *including* the surrounding quotes, and —
/// unlike `escape_default` — leaves printable non-ASCII intact, which matters
/// because the error messages contain em-dashes.
fn render_rust_literal(outcome: &ResolvedOutcome) -> String {
    format!("{:?}", outcome.text())
}

/// roff target (`xtgeoip.1`).
///
/// In roff a line beginning with `.` or `'` is a control line, and `\` starts
/// an escape sequence. Text interpolated into a man page must neutralise
/// both or it silently corrupts the rendered output.
fn render_roff(outcome: &ResolvedOutcome) -> String {
    let escaped = outcome.text().replace('\\', "\\e");
    match escaped.chars().next() {
        // `\&` is roff's zero-width character: it stops the line being read
        // as a control line without displaying anything.
        Some('.') | Some('\'') => format!("\\&{escaped}"),
        _ => escaped,
    }
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
            let outcome = render_plain(&resolve_outcome(spec, ex)?);
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

    // Previously read ex.outcome directly with an unwrap_or_default(),
    // bypassing resolution entirely (#75/#76). Routing through
    // resolve_outcome means the missing-data guarantee applies here too.
    let mut add = |exs: &[Example]| -> anyhow::Result<()> {
        for ex in exs {
            let outcome = resolve_outcome(spec, ex)?;
            if outcome.succeeded() {
                out.push_str(&format!(
                    "- {}:\n\n`{}`\n\n",
                    render_plain(&outcome),
                    ex.cmd
                ));
            }
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

/* ---------------- EXECUTION PLAN (#26/#27) ---------------- */

/// How each `Action` variant yields a context and a value for each flag.
///
/// This is the one piece of knowledge that lives in the emitter rather than in
/// the spec, and deliberately so: it is a fact about the Rust `Action` type,
/// not about `cli.yaml`. See §4 of docs/design/26-spec-derived-planning.md.
/// `top_level` appears twice because the variant split (`TopLevelBackup` /
/// `TopLevelClean`) is exactly what the rank model unifies — the first has `b`
/// true by construction, the second has `c` true by construction.
/// A flag letter and the Rust expression yielding its value in a match arm.
type FlagBinding = (&'static str, &'static str);

/// `(match pattern, spec context name, flag bindings)`.
type ActionBinding = (&'static str, &'static str, &'static [FlagBinding]);

const ACTION_BINDINGS: &[ActionBinding] = &[
    (
        "Action::TopLevelBackup { clean, force, prune }",
        "top_level",
        &[
            ("b", "true"),
            ("c", "*clean"),
            ("p", "*prune"),
            ("f", "*force"),
        ],
    ),
    (
        "Action::TopLevelClean { force }",
        "top_level",
        &[
            ("b", "false"),
            ("c", "true"),
            ("p", "false"),
            ("f", "*force"),
        ],
    ),
    ("Action::Fetch { prune }", "fetch", &[("p", "*prune")]),
    (
        "Action::Run { backup, clean, force, prune, legacy }",
        "run",
        &[
            ("b", "*backup"),
            ("c", "*clean"),
            ("p", "*prune"),
            ("f", "*force"),
            ("l", "*legacy"),
        ],
    ),
    (
        "Action::Build { backup, clean, force, prune, legacy }",
        "build",
        &[
            ("b", "*backup"),
            ("c", "*clean"),
            ("p", "*prune"),
            ("f", "*force"),
            ("l", "*legacy"),
        ],
    ),
    ("Action::Conf(_)", "conf", &[]),
];

/// The `Step` variant a spec step name constructs, with its parameter.
fn step_ctor(name: &str, param: Option<&str>) -> anyhow::Result<String> {
    let arg = match param {
        Some("backup_mode") => " { mode }",
        Some(other) => anyhow::bail!("unknown step param {other:?}"),
        None => "",
    };
    Ok(match name {
        "backup" => format!("Step::Backup{arg}"),
        "clean" => format!("Step::Clean{arg}"),
        "prune_bin" => "Step::PruneBin".to_string(),
        "prune_csv" => "Step::PruneCsv".to_string(),
        other => anyhow::bail!("step {other:?} has no Step variant"),
    })
}

/// Emit `src/generated/plan.rs`: `plan_generated(&Action) -> Plan`.
///
/// Rust, not a data table, and that is the whole point. `Plan::Pipeline`
/// cannot be constructed without naming the fetch that feeds the build, so a
/// spec that selected `build` without `fetch` would emit code that does not
/// compile. A runtime-evaluated table could express that combination and would
/// downgrade a compile-time guarantee to a runtime check.
fn generate_plan_rs(spec: &Spec) -> anyhow::Result<String> {
    let plan = spec
        .plan
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("cli.yaml has no `plan:` section"))?;

    let rank = |name: &str| -> anyhow::Result<u32> {
        plan.steps
            .get(name)
            .map(|s| s.rank)
            .ok_or_else(|| anyhow::anyhow!("step {name:?} has no rank"))
    };
    let fetch_rank = rank("fetch")?;
    let build_rank = rank("build")?;

    // A raw string: rustfmt's `format_strings` reflows ordinary literals and
    // would bake its own indentation into the file this emits.
    let mut out = String::from(
        r#"// auto-generated — see docs/design/26-spec-derived-planning.md
//
// Ordering comes from `plan.steps[*].rank` in docs/spec/cli.yaml; membership
// from `plan.contexts`. Each step's `why` is carried through from the spec, so
// the reasoning survives the migration instead of becoming a bare integer.

use crate::{
    action::{Action, Plan, Step, backup_mode},
    fetch::FetchMode,
};

pub(crate) fn plan_generated(action: &Action) -> Plan {
    match action {
"#,
    );

    for (pattern, ctx_name, bindings) in ACTION_BINDINGS {
        let ctx = plan.contexts.get(*ctx_name).ok_or_else(|| {
            anyhow::anyhow!("plan.contexts has no {ctx_name:?}")
        })?;
        let flag = |letter: &str| -> Option<&str> {
            bindings.iter().find(|(l, _)| l == &letter).map(|(_, e)| *e)
        };

        // Every step this context can run, as (rank, name, guard-expression).
        let mut selected: Vec<(u32, &str, Option<String>)> = Vec::new();
        for name in &ctx.always {
            selected.push((rank(name)?, name.as_str(), None));
        }
        for (letter, name) in &ctx.selects {
            // No `else`: an unbound letter used to drop the step from the
            // emitted planner with docgen still exiting 0, so the spec
            // declared a step the program would never run. `validate_plan`
            // checks the letter against `spec.flags` but not against this
            // binding table, and generation time is precisely where
            // spec-internal contradictions are supposed to be owned.
            let expr = flag(letter).ok_or_else(|| {
                anyhow::anyhow!(
                    "plan.contexts.{ctx_name}.selects binds {letter:?} to \
                     step {name:?}, but ACTION_BINDINGS has no expression for \
                     {letter:?} in this context — the step would be silently \
                     omitted from the generated planner"
                )
            })?;
            selected.push((rank(name)?, name.as_str(), Some(expr.into())));
        }
        selected.sort_by_key(|(r, _, _)| *r);

        let builds = selected.iter().any(|(_, n, _)| *n == "build");
        let needs_mode = selected.iter().any(|(_, n, _)| {
            plan.steps.get(*n).and_then(|s| s.param.as_deref())
                == Some("backup_mode")
        });

        out.push_str(&format!(
            "        {pattern} => {{
"
        ));
        if needs_mode {
            let f = flag("f").unwrap_or("false");
            out.push_str(&format!(
                "            let mode = backup_mode({f});
"
            ));
        }

        let emit = |out: &mut String,
                    list: &[(u32, &str, Option<String>)],
                    var: &str|
         -> anyhow::Result<()> {
            let m = if list.is_empty() { "" } else { "mut " };
            out.push_str(&format!(
                "            let {m}{var} = Vec::new();
"
            ));
            for (_, name, guard) in list {
                let why = &plan.steps[*name].why;
                for line in why.trim().lines() {
                    out.push_str(&format!(
                        "            // {name}: {}
",
                        line.trim()
                    ));
                }
                let ctor = step_ctor(name, plan.steps[*name].param.as_deref())?;
                match guard {
                    Some(g) => out.push_str(&format!(
                        "            if {g} {{ {var}.push({ctor}); }}
"
                    )),
                    None => out.push_str(&format!(
                        "            {var}.push({ctor});
"
                    )),
                }
            }
            Ok(())
        };

        if builds {
            let mode = ctx.fetch_mode.as_deref().ok_or_else(|| {
                anyhow::anyhow!("{ctx_name} builds but declares no fetch_mode")
            })?;
            let fetch_variant = match mode {
                "remote" => "FetchMode::Remote",
                "local" => "FetchMode::Local",
                other => anyhow::bail!("unknown fetch_mode {other:?}"),
            };
            let pre: Vec<_> = selected
                .iter()
                .filter(|(r, _, _)| *r < fetch_rank)
                .cloned()
                .collect();
            let mid: Vec<_> = selected
                .iter()
                .filter(|(r, _, _)| *r > fetch_rank && *r < build_rank)
                .cloned()
                .collect();
            // `pre` and `mid` are open windows either side of the fetch,
            // so a step ranked at or after `build` (or exactly at the
            // fetch) falls into neither and is discarded without a word.
            // Unreachable while `build` holds the maximum rank; the rank
            // model's own caveat (docs/design/26-spec-derived-planning.md)
            // says that assumption is the thing most likely to be broken
            // next, so fail loudly rather than emit a short plan. The +2 is
            // fetch and build themselves, which are in neither window.
            anyhow::ensure!(
                pre.len() + mid.len() + 2 == selected.len(),
                "{ctx_name}: {} of {} plan steps fall outside the \
                 pre/fetch/mid/build windows and would be dropped from the \
                 generated planner — a step ranked at or after `build` cannot \
                 be placed by the rank model",
                selected.len() - (pre.len() + mid.len() + 2),
                selected.len()
            );
            emit(&mut out, &pre, "pre")?;
            emit(&mut out, &mid, "mid")?;
            let legacy = flag("l").unwrap_or("false");
            out.push_str(&format!(
                "            Plan::Pipeline {{ pre, fetch: {fetch_variant}, \
                 mid, legacy: {legacy} }}
"
            ));
        } else {
            let simple: Vec<_> = selected
                .iter()
                .filter(|(_, n, _)| *n != "build")
                .cloned()
                .collect();
            // A context that fetches without building keeps the fetch inline;
            // nothing consumes its result, so it is an ordinary step.
            let mut body = Vec::new();
            for item in &simple {
                if item.1 == "fetch" {
                    let mode = ctx.fetch_mode.as_deref().unwrap_or("remote");
                    let v = if mode == "local" {
                        "FetchMode::Local"
                    } else {
                        "FetchMode::Remote"
                    };
                    body.push((item.0, "fetch", item.2.clone(), v.to_string()));
                } else {
                    body.push((item.0, item.1, item.2.clone(), String::new()));
                }
            }
            let m = if body.is_empty() { "" } else { "mut " };
            out.push_str(&format!("            let {m}steps = Vec::new();\n"));
            for (_, name, guard, fetch_variant) in &body {
                let why = &plan.steps[*name].why;
                for line in why.trim().lines() {
                    out.push_str(&format!(
                        "            // {name}: {}
",
                        line.trim()
                    ));
                }
                let ctor = if *name == "fetch" {
                    format!("Step::Fetch {{ mode: {fetch_variant} }}")
                } else {
                    step_ctor(name, plan.steps[*name].param.as_deref())?
                };
                match guard {
                    Some(g) => out.push_str(&format!(
                        "            if {g} {{ steps.push({ctor}); }}
"
                    )),
                    None => out.push_str(&format!(
                        "            steps.push({ctor});
"
                    )),
                }
            }
            out.push_str(
                "            Plan::Simple(steps)
",
            );
        }
        out.push_str(
            "        }

",
        );
    }

    out.push_str(
        "    }
}
",
    );
    Ok(out)
}

/* ---------------- CLI MATRIX ---------------- */

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::from(
        "// auto-generated\n#![allow(dead_code)]\npub struct CliExample { pub \
         cmd: &'static str, pub valid: bool, pub outcome: &'static str, pub \
         steps: Option<&'static [&'static str]> }\npub const CLI_MATRIX: \
         &[CliExample] = &[\n",
    );

    let mut add = |exs: &[Example]| -> anyhow::Result<()> {
        for ex in exs {
            // Both fields are Rust literals: escape via the renderer, and
            // `{:?}` supplies the surrounding quotes.
            let outcome = render_rust_literal(&resolve_outcome(spec, ex)?);
            let steps = match &ex.steps {
                Some(steps) => {
                    let names: Vec<String> =
                        steps.iter().map(|s| format!("{s:?}")).collect();
                    format!("Some(&[{}])", names.join(", "))
                }
                None => "None".to_string(),
            };
            out.push_str(&format!(
                "    CliExample {{ cmd: {:?}, valid: {}, outcome: {}, steps: \
                 {} }},\n",
                ex.cmd, ex.valid, outcome, steps
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

    // DESCRIPTION, COMMANDS, OPTIONS, EXECUTION ORDER, FILE OWNERSHIP,
    // LEGACY MODE, CONFIGURATION from template
    push_section(&mut out, "DESCRIPTION", &tmpl.description);
    push_section(&mut out, "COMMANDS", &tmpl.commands);
    push_section(&mut out, "OPTIONS", &tmpl.options);
    push_section(&mut out, "EXECUTION ORDER", &tmpl.execution_order);
    push_section(&mut out, "FILE OWNERSHIP", &tmpl.file_ownership);
    push_section(&mut out, "LEGACY MODE", &tmpl.legacy_mode);
    push_section(&mut out, "CONFIGURATION", &tmpl.configuration);

    // EXAMPLES (from spec valid examples)
    out.push_str(".SH EXAMPLES\n");
    // roff-escaped via render_roff: an outcome starting with `.` or `'`
    // would otherwise be read as a control line (#75).
    let emit_valid =
        |out: &mut String, exs: &[Example]| -> anyhow::Result<()> {
            for ex in exs {
                let outcome = resolve_outcome(spec, ex)?;
                if outcome.succeeded() {
                    out.push_str(&format!(
                        ".TP\n.B {}\n{}\n",
                        ex.cmd,
                        render_roff(&outcome)
                    ));
                }
            }
            Ok(())
        };
    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs)?;
    }
    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs)?;
    }

    // FILES, SEE ALSO, AUTHORS from template
    push_section(&mut out, "FILES", &tmpl.files);
    push_section(&mut out, "SEE ALSO", &tmpl.see_also);
    push_section(&mut out, "AUTHORS", &tmpl.authors);

    Ok(out)
}

/// Strip roff font escapes (`\fB`, `\fI`, `\fR`, `\fP`) so they cannot mask an
/// option boundary.
///
/// This is not cosmetic. `\fB\-\-legacy\fR` ends in `B` immediately before the
/// `\-`, and the boundary rule below rejects an option preceded by an
/// alphanumeric — so without stripping, that token is read as the short flag
/// `-legacy` rather than the long `--legacy`. Observed on the real template.
fn strip_roff_fonts(body: &str) -> String {
    let c: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\'
            && i + 2 < c.len()
            && c[i + 1] == 'f'
            && matches!(c[i + 2], 'B' | 'I' | 'R' | 'P')
        {
            i += 3;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Every option token a roff body names, as `-b` / `--backup`.
///
/// The discriminating rule is the one preceding character: roff writes a
/// literal hyphen as `\-` too, so `stale\-owned` and `spec\-driven` look
/// exactly like options to a naive scan. An option's `\-` is never preceded by
/// an alphanumeric; a prose hyphen always is. On the real template that single
/// rule is the difference between 6 spurious tokens and none.
///
/// A short name is not constrained to one character on purpose: `\-legacy`
/// (a plausible typo for `\-\-legacy`) should be reported as unknown rather
/// than silently truncated to `-l`, which exists.
fn roff_option_tokens(body: &str) -> BTreeSet<String> {
    let c: Vec<char> = strip_roff_fonts(body).chars().collect();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i + 1 < c.len() {
        if c[i] != '\\' || c[i + 1] != '-' {
            i += 1;
            continue;
        }
        if i > 0 && c[i - 1].is_ascii_alphanumeric() {
            i += 2;
            continue;
        }
        let mut j = i + 2;
        let long = j + 1 < c.len() && c[j] == '\\' && c[j + 1] == '-';
        if long {
            j += 2;
        }
        if j >= c.len() || !c[j].is_ascii_alphabetic() {
            i += 2;
            continue;
        }
        let mut name = String::new();
        while j < c.len() {
            if c[j].is_ascii_alphanumeric() {
                name.push(c[j]);
                j += 1;
            } else if c[j] == '\\'
                && j + 2 < c.len()
                && c[j + 1] == '-'
                && c[j + 2].is_ascii_alphanumeric()
            {
                name.push('-');
                j += 2;
            } else {
                break;
            }
        }
        found.insert(if long {
            format!("--{name}")
        } else {
            format!("-{name}")
        });
        i = j;
    }
    found
}

/// The man-page template may name no option or command the spec does not
/// declare (#92, generation-time half).
///
/// The five man-page checks that landed 2026-09-03 all live at *test* time and
/// compare a documented *claim* against the program's behaviour. This is the
/// other half, and it is purely spec-internal, which is what makes it
/// admissible here: it needs nothing from the program's semantics, only two
/// spec sections and a template. The boundary #92 settled — generation time
/// owns spec-internal contradictions, test time owns spec-versus-program
/// agreement — is what puts it on this side.
///
/// Scope, stated so it is not mistaken for more than it is: this catches a
/// **rename or a typo**, not a false claim. `-c` is `--clean` under `build`
/// and `--set-credentials` under `conf`, and the check deliberately does not
/// model per-command validity — only that a name exists somewhere in the
/// declared surface. Modelling context would duplicate the guard table, which
/// test time already checks against the parser.
///
/// `description` is excluded. It is prose about the problem domain and the one
/// section that legitimately names a *different* program's options —
/// `xt_geoip`'s `--src-cc`, an iptables match option this tool never accepts.
/// Every other section documents this program's own interface, so a foreign
/// flag appearing in one would itself be a defect.
fn validate_manpage_template(
    spec: &Spec,
    tmpl: &ManpageTemplate,
) -> anyhow::Result<()> {
    let mut known: BTreeSet<String> = BTreeSet::new();
    for (short, def) in &spec.flags {
        known.insert(format!("-{short}"));
        known.insert(format!("--{}", def.long));
    }
    for def in spec.global_options.values() {
        known.insert(format!("--{}", def.long));
    }
    for per_command in spec.subcommand_options.values() {
        for (short, def) in per_command {
            known.insert(format!("-{short}"));
            known.insert(format!("--{}", def.long));
        }
    }
    // clap supplies these and no spec map declares them, by design.
    for builtin in ["-h", "--help", "-V", "--version"] {
        known.insert(builtin.to_string());
    }

    let sections: [(&str, &str); 9] = [
        ("commands", &tmpl.commands),
        ("options", &tmpl.options),
        ("execution_order", &tmpl.execution_order),
        ("file_ownership", &tmpl.file_ownership),
        ("legacy_mode", &tmpl.legacy_mode),
        ("configuration", &tmpl.configuration),
        ("files", &tmpl.files),
        ("see_also", &tmpl.see_also),
        ("authors", &tmpl.authors),
    ];

    let mut problems: Vec<String> = Vec::new();
    for (name, body) in sections {
        for token in roff_option_tokens(body) {
            if !known.contains(&token) {
                problems.push(format!(
                    "manpage-template.toml `{name}` names option `{token}`, \
                     which no spec section declares (checked: flags, \
                     global_options, subcommand_options, and clap's built-ins)"
                ));
            }
        }
    }

    // Commands, both directions. `.BI "<name> "` is the template's own
    // anchor for a command entry, so this reads the structure rather than
    // guessing at prose.
    let mut documented: BTreeSet<&str> = BTreeSet::new();
    for line in tmpl.commands.lines() {
        if let Some(rest) = line.strip_prefix(".BI \"")
            && let Some(name) = rest.split('"').next()
            && !name.trim().is_empty()
        {
            documented.insert(name.trim());
        }
    }
    for name in &documented {
        if !spec.commands.contains_key(*name) {
            problems.push(format!(
                "manpage-template.toml `commands` documents `{name}`, which \
                 cli.yaml `commands:` does not declare"
            ));
        }
    }
    for name in spec.commands.keys() {
        if !documented.contains(name.as_str()) {
            problems.push(format!(
                "cli.yaml declares command `{name}`, which \
                 manpage-template.toml `commands` does not document"
            ));
        }
    }

    if !problems.is_empty() {
        anyhow::bail!(
            "manpage template disagrees with the spec:\n  - {}",
            problems.join("\n  - ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    // ── manpage template vs spec (#92, generation-time half) ──────────────

    /// The shipped template and the shipped spec must agree. This is the
    /// check docgen runs; asserting it here means `cargo test` reports the
    /// disagreement directly rather than as a docgen failure during a build.
    #[test]
    fn shipped_manpage_template_agrees_with_the_spec() {
        let yaml = std::fs::read_to_string("docs/spec/cli.yaml")
            .expect("docs/spec/cli.yaml missing");
        let spec: Spec =
            serde_saphyr::from_str(&yaml).expect("cli.yaml does not parse");
        let toml_str =
            std::fs::read_to_string("docs/spec/manpage-template.toml")
                .expect("docs/spec/manpage-template.toml missing");
        let tmpl: ManpageTemplate =
            toml::from_str(&toml_str).expect("template does not parse");
        validate_manpage_template(&spec, &tmpl)
            .expect("shipped template disagrees with shipped spec");
    }

    /// The discriminating case for the whole scan. roff writes a literal
    /// hyphen as `\-`, so prose compounds are indistinguishable from options
    /// to a naive reading — on the real template a bare scan yields six
    /// spurious tokens (`\-owned`, `\-src`, `\-cc`, `\-Z`, `\-no`,
    /// `\-file`). The preceding-character rule is what removes them.
    #[test]
    fn prose_hyphens_are_not_read_as_options() {
        let body = "Files are stale\\-owned under a spec\\-driven model, \
                    matching [A\\-Z0\\-9] only.";
        assert!(
            roff_option_tokens(body).is_empty(),
            "prose hyphens leaked in: {:?}",
            roff_option_tokens(body)
        );
    }

    /// `\fB` ends in an alphanumeric, so without font stripping the boundary
    /// rule rejects the first `\-` and the token is misread as the short
    /// flag `-legacy` instead of the long `--legacy`. Observed on the real
    /// template before `strip_roff_fonts` existed.
    #[test]
    fn font_escapes_do_not_mask_a_long_option() {
        let toks = roff_option_tokens("ran with \\fB\\-\\-legacy\\fR and");
        assert!(toks.contains("--legacy"), "expected --legacy, got {toks:?}");
        assert!(
            !toks.contains("-legacy"),
            "font escape masked the long form: {toks:?}"
        );
    }

    /// Both option forms are recognised, and a short flag is not truncated:
    /// `\-legacy` (a plausible typo for `\-\-legacy`) must surface as
    /// unknown rather than silently reading as `-l`, which exists.
    #[test]
    fn short_and_long_forms_are_both_recognised() {
        let toks = roff_option_tokens(".BR \\-b \", \" \\-\\-backup");
        assert!(toks.contains("-b"), "{toks:?}");
        assert!(toks.contains("--backup"), "{toks:?}");

        let typo = roff_option_tokens("run with \\-legacy");
        assert!(
            typo.contains("-legacy"),
            "a mistyped long option must not truncate to -l: {typo:?}"
        );
    }
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
            steps: None,
        }
    }

    // ── plan validation (#92, generation side) ───────────────────────────

    fn step(rank: u32) -> PlanStep {
        PlanStep {
            rank,
            param: None,
            why: "because".into(),
        }
    }

    /// A minimal but *consistent* plan, so each test can break exactly one
    /// thing and name the fault it is about.
    fn sound_plan() -> PlanSpec {
        PlanSpec {
            steps: BTreeMap::from([
                ("backup".to_string(), step(10)),
                ("fetch".to_string(), step(30)),
                ("build".to_string(), step(60)),
            ]),
            contexts: BTreeMap::from([(
                "build".to_string(),
                PlanContext {
                    always: vec!["fetch".into(), "build".into()],
                    selects: BTreeMap::from([(
                        "b".to_string(),
                        "backup".to_string(),
                    )]),
                    fetch_mode: Some("local".into()),
                },
            )]),
        }
    }

    fn spec_with_plan(plan: PlanSpec) -> Spec {
        let mut spec = spec_with(conforming_valid());
        spec.flags.insert(
            "b".to_string(),
            FlagDef {
                long: "backup".into(),
                kind: "bool".into(),
                summary: "b".into(),
            },
        );
        spec.plan = Some(plan);
        spec
    }

    #[test]
    fn a_sound_plan_validates() {
        assert!(validate_plan(&spec_with_plan(sound_plan())).is_ok());
    }

    /// Two steps at one rank leave the order between them undefined, and the
    /// generator would pick arbitrarily.
    #[test]
    fn duplicate_ranks_are_rejected() {
        let mut plan = sound_plan();
        plan.steps.insert("fetch".to_string(), step(10));
        let err = validate_plan(&spec_with_plan(plan))
            .unwrap_err()
            .to_string();
        assert!(err.contains("rank 10 is shared"), "{err}");
    }

    /// The spec-level half of Fetch-before-Build. The generated code could not
    /// express this anyway — `Plan::Pipeline` cannot be named without a fetch —
    /// but failing here names the context instead of failing in rustc.
    #[test]
    fn a_context_that_builds_must_fetch() {
        let mut plan = sound_plan();
        plan.contexts.get_mut("build").unwrap().always =
            vec!["build".to_string()];
        let err = validate_plan(&spec_with_plan(plan))
            .unwrap_err()
            .to_string();
        assert!(err.contains("without `fetch`"), "{err}");
    }

    /// A step no context runs can never appear in a plan, yet its rank still
    /// participates in the ordering. The plan-model analogue of
    /// `every_flag_is_referenced_by_some_guard`.
    #[test]
    fn a_step_no_context_runs_is_rejected() {
        let mut plan = sound_plan();
        plan.steps.insert("prune_csv".to_string(), step(50));
        let err = validate_plan(&spec_with_plan(plan))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no context runs it"), "{err}");
    }

    #[test]
    fn selecting_on_an_undeclared_flag_is_rejected() {
        let mut plan = sound_plan();
        plan.contexts.get_mut("build").unwrap().selects =
            BTreeMap::from([("z".to_string(), "backup".to_string())]);
        let err = validate_plan(&spec_with_plan(plan))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not in `flags:`"), "{err}");
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
            global_options: BTreeMap::new(),
            subcommand_options: BTreeMap::new(),
            plan: None,
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

    // ── resolution vs presentation (#75) ─────────────────────────────────

    fn succeeds(text: &str) -> ResolvedOutcome {
        ResolvedOutcome::Succeeds {
            description: text.into(),
        }
    }

    #[test]
    fn resolution_distinguishes_success_from_failure() {
        let spec = spec_with(conforming_valid());
        assert!(
            resolve_outcome(&spec, &conforming_valid())
                .unwrap()
                .succeeded()
        );

        let mut spec = spec_with(conforming_invalid());
        spec.reason_templates.insert(
            "some_code".into(),
            ReasonTemplate {
                text: "because {why}".into(),
            },
        );
        let out = resolve_outcome(&spec, &conforming_invalid()).unwrap();
        assert!(!out.succeeded());
        assert_eq!(out.text(), "because {why}");
    }

    #[test]
    fn template_args_are_substituted_during_resolution() {
        let ex = Example {
            reason: Some(Reason {
                code: "c".into(),
                args: Some(BTreeMap::from([(
                    "why".into(),
                    "it is bad".into(),
                )])),
            }),
            maps_to: Some("m".into()),
            ..example(false)
        };
        let mut spec = spec_with(conforming_valid());
        spec.reason_templates.insert(
            "c".into(),
            ReasonTemplate {
                text: "refused because {why}".into(),
            },
        );
        assert_eq!(
            resolve_outcome(&spec, &ex).unwrap().text(),
            "refused because it is bad"
        );
    }

    /// The defect that motivated the split: an outcome containing a quote or
    /// backslash was interpolated raw into a Rust `&'static str` literal,
    /// emitting code that does not compile.
    #[test]
    fn rust_literal_escapes_quotes_and_backslashes() {
        assert_eq!(
            render_rust_literal(&succeeds(r#"he said "hi""#)),
            r#""he said \"hi\"""#
        );
        assert_eq!(render_rust_literal(&succeeds(r"a\b")), r#""a\\b""#);
    }

    /// Messages contain em-dashes; escaping must not mangle printable
    /// non-ASCII the way `escape_default` would.
    #[test]
    fn rust_literal_preserves_non_ascii() {
        assert_eq!(
            render_rust_literal(&succeeds("a — b")),
            "\"a — b\"",
            "em-dash must survive escaping"
        );
    }

    /// In roff a line starting with `.` or `'` is a control line.
    #[test]
    fn roff_neutralises_leading_control_characters() {
        assert_eq!(
            render_roff(&succeeds(".B not a command")),
            "\\&.B not a command"
        );
        assert_eq!(render_roff(&succeeds("'quoted")), "\\&'quoted");
        assert_eq!(render_roff(&succeeds("safe text")), "safe text");
    }

    #[test]
    fn roff_escapes_backslashes() {
        assert_eq!(render_roff(&succeeds(r"a\b")), r"a\eb");
    }

    /// Plain targets embed the text in prose, so it passes through as-is.
    #[test]
    fn plain_render_is_verbatim() {
        assert_eq!(render_plain(&succeeds(r#".a\b"c"#)), r#".a\b"c"#);
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
