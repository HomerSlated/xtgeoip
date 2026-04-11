//! xtgeoip-docgen v3.1 (stable, schema-safe)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,

    #[serde(default)]
    pub proof: Option<Proof>,

    #[serde(default)]
    pub error_cases: Option<BTreeMap<String, ErrorCase>>,

    #[serde(default)]
    pub top_level: Option<CommandSpec>,

    #[serde(flatten)]
    pub commands: BTreeMap<String, CommandSpec>,

    #[serde(default)]
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
}

#[derive(Debug, Deserialize)]
pub struct Proof {
    pub unique_maps_to: Option<bool>,
    pub full_branch_coverage: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorCase {
    pub maps_to: String,
}

#[derive(Debug, Deserialize)]
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
        examples: Vec<Example>,
    },
    SelectorCommand {
        summary: String,
        usage: String,
        positional: PositionalArg,
        constraints: Option<Constraints>,
        examples: Vec<Example>,
    },
}

#[derive(Debug, Deserialize)]
pub struct PositionalArg {
    pub name: String,
    pub required: bool,
    pub choices: BTreeMap<String, ChoiceSummary>,
}

#[derive(Debug, Deserialize)]
pub struct ChoiceSummary {
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct Constraints {
    pub exactly_one_positional: bool,
}

#[derive(Debug, Deserialize)]
pub struct Example {
    pub case_id: Option<String>,
    pub cmd: String,
    pub valid: bool,
    pub outcome: Option<String>,
    pub reason: Option<Reason>,
    pub exit_status: Option<i32>,
    pub note: Option<String>,
    pub maps_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Reason {
    pub code: String,
    pub args: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct ReasonTemplate {
    pub text: String,
}

#[derive(Debug, Serialize)]
struct Testcase {
    case_id: Option<String>,
    key: String,
    cmd: String,
    maps_to: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let yaml_str = fs::read_to_string("docs/spec/cli.yaml")?;
    let spec: Spec = serde_yaml::from_str(&yaml_str)?;

    validate_spec(&spec)?;

    fs::create_dir_all("docs/generated")?;
    fs::create_dir_all("src/generated")?;

    fs::write("docs/generated/usage.md", generate_usage_md(&spec)?)?;
    fs::write("docs/generated/tldr.md", generate_tldr_md(&spec)?)?;
    fs::write("docs/generated/scd", generate_scd(&spec)?)?;
    fs::write("docs/generated/xtgeoip.1", generate_manpage(&spec)?)?;
    fs::write(
        "src/generated/error_text.rs",
        generate_error_text_rs(&spec)?,
    )?;
    fs::write(
        "src/generated/cli_matrix.rs",
        generate_cli_matrix_rs(&spec)?,
    )?;
    fs::write(
        "docs/generated/testcases.yaml",
        generate_testcases_yaml(&spec)?,
    )?;

    println!("Docs generated successfully.");
    Ok(())
}

/* ---------------- VALIDATION ---------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used_error_cases: BTreeSet<String> = BTreeSet::new();

    let error_cases = spec.error_cases.as_ref();

    let check = |ex: &Example,
                 scope: &str,
                 used: &mut BTreeSet<String>|
     -> anyhow::Result<()> {
        if let Some(reason) = &ex.reason {
            if !spec.reason_templates.contains_key(&reason.code) {
                anyhow::bail!(
                    "Unknown reason code {} in {}",
                    reason.code,
                    scope
                );
            }
        }

        if !ex.valid {
            let maps_to = ex.maps_to.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Missing maps_to in invalid example {}", ex.cmd)
            })?;

            if let Some(ec) = error_cases {
                if !ec.contains_key(maps_to) {
                    anyhow::bail!("Unknown error case {}", maps_to);
                }
            }

            used.insert(maps_to.clone());
        }

        Ok(())
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in exs {
            check(ex, "top_level", &mut used_error_cases)?;
        }
    }

    for (name, cmd) in &spec.commands {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in exs {
            check(ex, name, &mut used_error_cases)?;
        }
    }

    // FULL COVERAGE CHECK (FIXED LOGIC)
    if spec
        .proof
        .as_ref()
        .and_then(|p| p.full_branch_coverage)
        .unwrap_or(false)
    {
        if let Some(ec) = error_cases {
            let mut defined: BTreeSet<String> = BTreeSet::new();
            let mut unused = Vec::new();

            // collect all defined maps_to values
            for (_, case) in ec {
                defined.insert(case.maps_to.clone());
            }

            // check which ones were NOT used
            for maps_to in &defined {
                if !used_error_cases.contains(maps_to) {
                    unused.push(maps_to.clone());
                }
            }

            if !unused.is_empty() {
                anyhow::bail!(
                    "Unused error cases (no invalid example maps_to \
                     reference): {:?}",
                    unused
                );
            }
        }
    }

    Ok(())
}

/* ---------------- OUTCOME ---------------- */

fn resolve_outcome(spec: &Spec, ex: &Example) -> String {
    if ex.valid {
        return ex.outcome.clone().unwrap_or_else(|| "OK".into());
    }

    if let Some(reason) = &ex.reason {
        if let Some(t) = spec.reason_templates.get(&reason.code) {
            let mut text = t.text.clone();
            if let Some(args) = &reason.args {
                for (k, v) in args {
                    text = text.replace(&format!("{{{}}}", k), v);
                }
            }
            return text;
        }
    }

    "ERROR".into()
}

/* ---------------- ALL OTHER FUNCTIONS (UNCHANGED) ---------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    let render = |out: &mut String,
                  spec: &Spec,
                  exs: &[Example],
                  title: &str,
                  extra: Option<&str>| {
        out.push_str(&format!("## {}\n", title));

        if let Some(e) = extra {
            out.push_str(e);
            out.push('\n');
        }

        for ex in exs {
            let outcome = resolve_outcome(spec, ex);
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
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(summary));
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(usage));
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(summary));
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(usage));
            }
        }
    }

    Ok(out)
}

/* remaining functions unchanged from your version:
   generate_tldr_md
   generate_scd
   generate_error_text_rs
   generate_cli_matrix_rs
   generate_testcases_yaml
   generate_manpage
*/

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

/* ---------------- SCD ---------------- */

fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand {
                summary,
                allowed_flags,
                ..
            } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nFlags: {:?}\n\n",
                    summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { usage, .. } => {
                out.push_str(&format!(
                    "Command: top_level\nUsage: {}\n\n",
                    usage
                ));
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary,
                allowed_flags,
                ..
            } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
                    name, summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { usage, .. } => {
                out.push_str(&format!(
                    "Command: {}\nUsage: {}\n\n",
                    name, usage
                ));
            }
        }
    }

    Ok(out)
}

/* ---------------- ERROR TEXT ---------------- */

fn generate_error_text_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "// auto-generated\n".to_string();

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
        "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub \
         outcome: &'static str }\npub const CLI_MATRIX: &[CliExample] = &[\n",
    );

    let mut add = |exs: &[Example]| {
        for ex in exs {
            let outcome = resolve_outcome(spec, ex);
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" \
                 }},\n",
                ex.cmd, ex.valid, outcome
            ));
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
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
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

    Ok(serde_yaml::to_string(&testcases)?)
}

/* ---------------- MANPAGE ---------------- */

fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n.SH NAME\n{} {}\n",
        spec.meta.program.to_uppercase(),
        spec.version,
        spec.meta.program,
        spec.meta.summary
    ))
}
