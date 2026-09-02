//! The `docs/spec/cli.yaml` data model.
//!
//! Lives in the library, not in `xtgeoip-docgen`, so that the generator and
//! the program it generates for can agree on one definition of the spec. Until
//! 2026-09-02 there was no `lib` target and docgen re-declared all fourteen of
//! these types privately, which meant nothing could check a spec claim against
//! the program's own semantics — the structural reason #92's generation-side
//! validator was impossible.
//!
//! `deny_unknown_fields` throughout is deliberate (#76): missing or unknown
//! spec data must be an error, never a silent default that ships into the man
//! page, the markdown and `CLI_MATRIX` alike.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,

    #[serde(default)]
    pub proof: Option<Proof>,

    #[serde(default)]
    pub flags: BTreeMap<String, FlagDef>,

    /// Options that apply to every command and carry no combination
    /// semantics. Kept out of `flags` on purpose: that map is the universe
    /// the guard bitmask is built from, and a bit no guard can reference
    /// would fail `every_flag_is_referenced_by_some_guard`.
    #[serde(default)]
    pub global_options: BTreeMap<String, FlagDef>,

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
    /// The execution plan this invocation produces, in order, as step names.
    ///
    /// Distinct from `outcome`, which is authored prose for humans. This is
    /// the machine-checkable half:
    /// `action::tests::spec_steps_agree_with_plan` drives each command
    /// through the real parser and `plan()` and compares. Three `outcome:`
    /// strings claimed clean-before-fetch for six weeks after #24 stage 1
    /// reversed exactly that, because nothing ever compared them to anything.
    ///
    /// Optional because not every valid invocation has a plan — `-h` never
    /// reaches `Action` at all. `conf` does, and declares `[]`.
    pub steps: Option<Vec<String>>,
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
