//! xtgeoip-docgen v3
//! Generates documentation and test matrices from cli.yaml (B-mode strict validation)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,

    pub proof: Option<Proof>,
    pub error_cases: Option<BTreeMap<String, ErrorCase>>,

    pub top_level: Option<CommandSpec>,
    pub commands: BTreeMap<String, CommandSpec>,
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
    fs::write("src/generated/error_text.rs", generate_error_text_rs(&spec)?)?;
    fs::write("src/generated/cli_matrix.rs", generate_cli_matrix_rs(&spec)?)?;
    fs::write("docs/generated/testcases.yaml", generate_testcases_yaml(&spec)?)?;

    println!("Docs generated successfully.");
    Ok(())
}

/* ------------------------- VALIDATION (STRICT B MODE) ------------------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used_error_cases = BTreeSet::new();
    let error_cases = spec.error_cases.as_ref();

    let mut validate_examples =
        |examples: &[Example], scope: &str| -> anyhow::Result<()> {
            for ex in examples {
                // reason code validity
                if let Some(reason) = &ex.reason {
                    if !spec.reason_templates.contains_key(&reason.code) {
                        anyhow::bail!(
                            "Unknown reason code {} in {} example {}",
                            reason.code,
                            scope,
                            ex.cmd
                        );
                    }
                }

                // invalid examples MUST map_to error_cases
                if !ex.valid {
                    let maps_to = ex.maps_to.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("Missing maps_to in invalid example {}", ex.cmd)
                    })?;

                    if let Some(ec) = error_cases {
                        if !ec.contains_key(maps_to) {
                            anyhow::bail!(
                                "maps_to '{}' not found in error_cases (example: {})",
                                maps_to,
                                ex.cmd
                            );
                        }
                    }

                    used_error_cases.insert(maps_to.clone());
                }
            }
            Ok(())
        };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                validate_examples(examples, "top_level")?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                validate_examples(examples, name)?;
            }
        }
    }

    // Proof-table coverage enforcement
    if let Some(proof) = &spec.proof {
        if proof.full_branch_coverage.unwrap_or(false) {
            if let Some(ec) = error_cases {
                for key in ec.keys() {
                    if !used_error_cases.contains(key) {
                        anyhow::bail!(
                            "Error case '{}' defined but never used",
                            key
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/* ------------------------- DOCUMENTATION GENERATORS ------------------------- */

fn render_reason(spec: &Spec, reason: &Reason) -> anyhow::Result<String> {
    let template = spec
        .reason_templates
        .get(&reason.code)
        .ok_or_else(|| anyhow::anyhow!("Unknown reason code: {}", reason.code))?;

    let mut text = template.text.clone();

    if let Some(args) = &reason.args {
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), v);
        }
    }

    Ok(text)
}

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    fn render(out: &mut String, spec: &Spec, exs: &[Example]) -> anyhow::Result<()> {
        for ex in exs {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_default()
            } else if let Some(r) = &ex.reason {
                render_reason(spec, r)?
            } else {
                "(invalid)".to_string()
            };

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
    }

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!("## top level\n{}\n\n", summary));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!("## top level\n{}\nUsage: {}\n\n", summary, usage));
                render(&mut out, spec, examples)?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!("## {}\n{}\n\n", name, summary));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!("## {}\n{}\nUsage: {}\n\n", name, summary, usage));
                render(&mut out, spec, examples)?;
            }
        }
    }

    Ok(out)
}

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    let mut add = |exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                out.push_str(&format!("- {}:\n\n`{}`\n\n",
                    ex.outcome.clone().unwrap_or_default(),
                    ex.cmd));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    Ok(out)
}

fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, allowed_flags, .. } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nFlags: {:?}\n\n",
                    summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { summary, usage, positional, constraints, .. } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nUsage: {}\nPositional: {:?}\nConstraints: {:?}\n\n",
                    summary, usage, positional.name, constraints
                ));
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, allowed_flags, .. } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
                    name, summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { summary, usage, positional, constraints, .. } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nUsage: {}\nPositional: {:?}\nConstraints: {:?}\n\n",
                    name, summary, usage, positional.name, constraints
                ));
            }
        }
    }

    Ok(out)
}

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

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::from(
        "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\npub const CLI_MATRIX: &[CliExample] = &[\n"
    );

    let mut add = |exs: &[Example]| {
        for ex in exs {
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, ex.outcome.clone().unwrap_or_default()
            ));
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    out.push_str("];\n");
    Ok(out)
}

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    let mut add = |exs: &[Example]| {
        for ex in exs {
            testcases.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.to_string(),
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
            });
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    Ok(serde_yaml::to_string(&testcases)?)
}

fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n.SH NAME\n{}\n\t{}\n",
        spec.meta.program.to_uppercase(),
        spec.version,
        spec.meta.program,
        spec.meta.summary
    ))
}
