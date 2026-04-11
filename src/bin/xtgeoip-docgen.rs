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
        summary: Option<String>,
        allowed_flags: Option<Vec<String>>,
        examples: Vec<Example>,
    },
    SelectorCommand {
        summary: Option<String>,
        usage: Option<String>,
        positional: Option<PositionalArg>,
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
    let yaml_path = "docs/spec/cli.yaml";
    let yaml_str = fs::read_to_string(yaml_path)?;

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

/* ------------------------- VALIDATION ------------------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used_error_cases = BTreeSet::new();
    let error_cases = spec.error_cases.as_ref();

    let mut validate_examples = |examples: &[Example], scope: &str| -> anyhow::Result<()> {
        for ex in examples {
            if let Some(reason) = &ex.reason {
                if !spec.reason_templates.contains_key(&reason.code) {
                    anyhow::bail!("Unknown reason code {} in {}", reason.code, scope);
                }
            }

            if !ex.valid {
                let maps_to = ex
                    .maps_to
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Missing maps_to in {}", ex.cmd))?;

                if let Some(ec) = error_cases {
                    if !ec.contains_key(maps_to) {
                        anyhow::bail!("maps_to '{}' not found", maps_to);
                    }
                }

                used_error_cases.insert(maps_to.clone());
            }
        }
        Ok(())
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. } => {
                validate_examples(examples, "top_level")?;
            }
            CommandSpec::SelectorCommand { examples, .. } => {
                validate_examples(examples, "top_level")?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { examples, .. } => {
                validate_examples(examples, name)?;
            }
            CommandSpec::SelectorCommand { examples, .. } => {
                validate_examples(examples, name)?;
            }
        }
    }

    if let Some(proof) = &spec.proof {
        if proof.full_branch_coverage.unwrap_or(false) {
            if let Some(ec) = error_cases {
                for k in ec.keys() {
                    if !used_error_cases.contains(k) {
                        anyhow::bail!("Error case '{}' defined but never used", k);
                    }
                }
            }
        }
    }

    Ok(())
}

/* ------------------------- REASON ------------------------- */

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

/* ------------------------- USAGE ------------------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    fn render(
        out: &mut String,
        spec: &Spec,
        exs: &[Example],
    ) -> anyhow::Result<()> {
        for ex in exs {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_else(|| "OK".into())
            } else if let Some(r) = &ex.reason {
                render_reason(spec, r)?
            } else {
                "(invalid)".into()
            };

            out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));

            if let Some(code) = ex.exit_status {
                out.push_str(&format!(" (exit {})", code));
            }

            out.push('\n');
        }
        out.push('\n');
        Ok(())
    }

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!(
                    "## top level\n{}\n\n",
                    summary.clone().unwrap_or_default()
                ));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!(
                    "## top level\n{}\nUsage: {}\n\n",
                    summary.clone().unwrap_or_default(),
                    usage.clone().unwrap_or_default()
                ));
                render(&mut out, spec, examples)?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!(
                    "## {}\n{}\n\n",
                    name,
                    summary.clone().unwrap_or_default()
                ));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!(
                    "## {}\n{}\nUsage: {}\n\n",
                    name,
                    summary.clone().unwrap_or_default(),
                    usage.clone().unwrap_or_default()
                ));
                render(&mut out, spec, examples)?;
            }
        }
    }

    Ok(out)
}

/* ------------------------- TLDR ------------------------- */

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    let mut add = |exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                out.push_str(&format!(
                    "- {}\n\n`{}`\n\n",
                    ex.outcome.clone().unwrap_or_else(|| "OK".into()),
                    ex.cmd
                ));
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

/* ------------------------- CLI MATRIX FIX ------------------------- */

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::from(
        "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\npub const CLI_MATRIX: &[CliExample] = &[\n"
    );

    let mut add = |exs: &[Example]| {
        for ex in exs {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_else(|| "OK".into())
            } else {
                ex.reason
                    .as_ref()
                    .map(|r| r.code.clone())
                    .unwrap_or_else(|| "ERROR".into())
            };

            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, outcome
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

/* ------------------------- SIMPLE GENERATORS ------------------------- */

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

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    for exs in spec
        .top_level
        .iter()
        .chain(spec.commands.values())
        .flat_map(|c| match c {
            CommandSpec::FlagCommand { examples, .. } => Some(examples),
            CommandSpec::SelectorCommand { examples, .. } => Some(examples),
        })
    {
        for ex in exs {
            testcases.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.into(),
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
            });
        }
    }

    Ok(serde_yaml::to_string(&testcases)?)
}

fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!(
        "xtgeoip v{}\n{}\n",
        spec.version, spec.meta.summary
    ))
}

fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!(
        ".TH {} 1\n.SH NAME\n{} - {}\n",
        spec.meta.program.to_uppercase(),
        spec.meta.program,
        spec.meta.summary
    ))
}
