//! xtgeoip-docgen v2
//! Generates documentation and test matrices from cli.yaml
use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,
    pub top_level: Option<CommandSpec>,
    pub commands: BTreeMap<String, CommandSpec>,
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
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
    pub cmd: String,
    pub valid: bool,
    pub outcome: Option<String>,
    pub reason: Option<Reason>,
    pub exit_status: Option<i32>,
    pub note: Option<String>,
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
    key: String,
    cmd: String,
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
    println!("Testcases YAML generated successfully.");
    println!("Documentation and generated code updated successfully.");
    Ok(())
}

/// Ensure that all example reason codes exist
fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    if let Some(reason) = &ex.reason
                        && !spec.reason_templates.contains_key(&reason.code)
                    {
                        anyhow::bail!(
                            "Unknown reason code {} in top_level example {}",
                            reason.code,
                            ex.cmd
                        );
                    }
                }
            }
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    if let Some(reason) = &ex.reason
                        && !spec.reason_templates.contains_key(&reason.code)
                    {
                        anyhow::bail!(
                            "Unknown reason code {} in command example {}",
                            reason.code,
                            ex.cmd
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Generate usage.md
fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                out.push_str(&format!("## top level\n{}\n\n", summary));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(r) = &ex.reason {
                        render_reason(spec, r)?
                    } else {
                        "(invalid)".to_string()
                    };
                    out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!(" (exit {})", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!(" — {}", note));
                    }
                    out.push('\n');
                }
                out.push('\n');
            }
            CommandSpec::SelectorCommand {
                summary,
                usage,
                examples,
                ..
            } => {
                out.push_str(&format!(
                    "## top level\n{}\nUsage: {}\n\n",
                    summary, usage
                ));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(r) = &ex.reason {
                        render_reason(spec, r)?
                    } else {
                        "(invalid)".to_string()
                    };
                    out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!(" (exit {})", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!(" — {}", note));
                    }
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }

    for (cmd_name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                out.push_str(&format!("## {}\n{}\n\n", cmd_name, summary));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(r) = &ex.reason {
                        render_reason(spec, r)?
                    } else {
                        "(invalid)".to_string()
                    };
                    out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!(" (exit {})", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!(" — {}", note));
                    }
                    out.push('\n');
                }
                out.push('\n');
            }
            CommandSpec::SelectorCommand {
                summary,
                usage,
                examples,
                ..
            } => {
                out.push_str(&format!(
                    "## {}\n{}\nUsage: {}\n\n",
                    cmd_name, summary, usage
                ));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(r) = &ex.reason {
                        render_reason(spec, r)?
                    } else {
                        "(invalid)".to_string()
                    };
                    out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!(" (exit {})", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!(" — {}", note));
                    }
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }

    Ok(out)
}

/// Generate TLDR Markdown
fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    if ex.valid {
                        let outcome = ex.outcome.clone().unwrap_or_default();
                        out.push_str(&format!(
                            "- {}:\n\n`{}`\n\n",
                            outcome, ex.cmd
                        ));
                    }
                }
            }
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    if ex.valid {
                        let outcome = ex.outcome.clone().unwrap_or_default();
                        out.push_str(&format!(
                            "- {}:\n\n`{}`\n\n",
                            outcome, ex.cmd
                        ));
                    }
                }
            }
        }
    }

    Ok(out)
}

/// Generate SCD (simplified command descriptor)
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
            CommandSpec::SelectorCommand {
                summary,
                usage,
                positional,
                constraints,
                ..
            } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nUsage: {}\nPositional: \
                     {:?}\nConstraints: {:?}\n\n",
                    summary, usage, positional.name, constraints
                ));
            }
        }
    }

    for (cmd_name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary,
                allowed_flags,
                ..
            } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
                    cmd_name, summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand {
                summary,
                usage,
                positional,
                constraints,
                ..
            } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nUsage: {}\nPositional: \
                     {:?}\nConstraints: {:?}\n\n",
                    cmd_name, summary, usage, positional.name, constraints
                ));
            }
        }
    }

    Ok(out)
}

/// Generate error_text.rs
fn generate_error_text_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "// Auto-generated error texts\n".to_string();
    for (code, r) in &spec.reason_templates {
        out.push_str(&format!(
            "pub const {}: &str = r#\"{}\"#;\n",
            code.to_uppercase(),
            r.text
        ));
    }
    Ok(out)
}

/// Generate cli_matrix.rs and a tiny test module
fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: \
                   bool, pub outcome: &'static str }\n"
        .to_string();
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    let outcome = ex.outcome.clone().unwrap_or_default();
                    out.push_str(&format!(
                        "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \
                         \"{}\" }},\n",
                        ex.cmd, ex.valid, outcome
                    ));
                }
            }
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                for ex in examples {
                    let outcome = ex.outcome.clone().unwrap_or_default();
                    out.push_str(&format!(
                        "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \
                         \"{}\" }},\n",
                        ex.cmd, ex.valid, outcome
                    ));
                }
            }
        }
    }

    out.push_str("];\n\n");
    out.push_str(
        "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn \
         test_matrix() {\n        assert!(!CLI_MATRIX.is_empty());\n    }\n}\n",
    );

    Ok(out)
}

fn render_reason(spec: &Spec, reason: &Reason) -> anyhow::Result<String> {
    let template =
        spec.reason_templates.get(&reason.code).ok_or_else(|| {
            anyhow::anyhow!("Unknown reason code: {}", reason.code)
        })?;

    let mut text = template.text.clone();

    if let Some(args) = &reason.args {
        for (key, value) in args {
            let placeholder = format!("{{{}}}", key);
            text = text.replace(&placeholder, value);
        }
    }

    Ok(text)
}

/// Generate manpage
fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    out.push_str(&format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n",
        spec.meta.program.to_uppercase(),
        spec.version
    ));

    out.push_str(&format!(
        ".SH NAME\n{}\n\t{}\n\n",
        spec.meta.program, spec.meta.summary
    ));

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                out.push_str(&format!(".SH TOP LEVEL\n{}\n", summary));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(reason) = &ex.reason {
                        render_reason(spec, reason)?
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(".TP\n{}\n", ex.cmd));
                    if !outcome.is_empty() {
                        out.push_str(&format!("{}\n", outcome));
                    }
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!("Exit status: {}\n", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!("{}\n", note));
                    }
                }
            }
            CommandSpec::SelectorCommand {
                summary,
                usage,
                examples,
                ..
            } => {
                out.push_str(&format!(
                    ".SH TOP LEVEL\n{}\nUsage: {}\n",
                    summary, usage
                ));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(reason) = &ex.reason {
                        render_reason(spec, reason)?
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(".TP\n{}\n", ex.cmd));
                    if !outcome.is_empty() {
                        out.push_str(&format!("{}\n", outcome));
                    }
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!("Exit status: {}\n", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!("{}\n", note));
                    }
                }
            }
        }
    }

    for (cmd_name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                out.push_str(&format!(
                    ".SH {}\n{}\n",
                    cmd_name.to_uppercase(),
                    summary
                ));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(reason) = &ex.reason {
                        render_reason(spec, reason)?
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(".TP\n{}\n", ex.cmd));
                    if !outcome.is_empty() {
                        out.push_str(&format!("{}\n", outcome));
                    }
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!("Exit status: {}\n", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!("{}\n", note));
                    }
                }
            }
            CommandSpec::SelectorCommand {
                summary,
                usage,
                examples,
                ..
            } => {
                out.push_str(&format!(
                    ".SH {}\n{}\nUsage: {}\n",
                    cmd_name.to_uppercase(),
                    summary,
                    usage
                ));
                for ex in examples {
                    let outcome = if ex.valid {
                        ex.outcome.clone().unwrap_or_default()
                    } else if let Some(reason) = &ex.reason {
                        render_reason(spec, reason)?
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(".TP\n{}\n", ex.cmd));
                    if !outcome.is_empty() {
                        out.push_str(&format!("{}\n", outcome));
                    }
                    if let Some(status) = ex.exit_status {
                        out.push_str(&format!("Exit status: {}\n", status));
                    }
                    if let Some(note) = &ex.note {
                        out.push_str(&format!("{}\n", note));
                    }
                }
            }
        }
    }

    Ok(out)
}

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    if let Some(cmd) = &spec.top_level {
        let examples = match cmd {
            CommandSpec::FlagCommand { examples, .. } => examples,
            CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in examples {
            let key = if ex.valid { "p" } else { "f" };
            testcases.push(Testcase {
                key: key.to_string(),
                cmd: ex.cmd.clone(),
            });
        }
    }

    for cmd in spec.commands.values() {
        let examples = match cmd {
            CommandSpec::FlagCommand { examples, .. } => examples,
            CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in examples {
            let key = if ex.valid { "p" } else { "f" };
            testcases.push(Testcase {
                key: key.to_string(),
                cmd: ex.cmd.clone(),
            });
        }
    }

    let yaml = serde_yaml::to_string(&testcases)?;
    Ok(yaml)
}
