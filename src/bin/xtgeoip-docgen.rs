//! xtgeoip-docgen v2
//! Generates documentation and test matrices from cli.yaml

use std::{collections::BTreeMap, fs};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub meta: Meta,
    pub version: u32,
    pub flags: BTreeMap<String, FlagSpec>,
    pub commands: BTreeMap<String, CommandSpec>,
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub program: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagSpec {
    pub long: String,
    pub kind: FlagKind,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    Bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandSpec {
    FlagCommand(FlagCommand),
    SelectorCommand(SelectorCommand),
}

impl CommandSpec {
    pub fn summary(&self) -> &str {
        match self {
            CommandSpec::FlagCommand(cmd) => &cmd.summary,
            CommandSpec::SelectorCommand(cmd) => &cmd.summary,
        }
    }

    pub fn examples(&self) -> &[Example] {
        match self {
            CommandSpec::FlagCommand(cmd) => &cmd.examples,
            CommandSpec::SelectorCommand(cmd) => &cmd.examples,
        }
    }

    pub fn allowed_flags(&self) -> Option<&[String]> {
        match self {
            CommandSpec::FlagCommand(cmd) => Some(&cmd.allowed_flags),
            CommandSpec::SelectorCommand(_) => None,
        }
    }

    pub fn usage<'a>(&'a self, cmd_name: &'a str, program: &'a str) -> String {
        match self {
            CommandSpec::FlagCommand(_) => format!("{program} {cmd_name} [OPTIONS]"),
            CommandSpec::SelectorCommand(cmd) => cmd.usage.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagCommand {
    pub summary: String,
    pub allowed_flags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<Example>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorCommand {
    pub summary: String,
    pub usage: String,
    pub positional: PositionalSpec,
    #[serde(default)]
    pub constraints: Option<Constraints>,
    #[serde(default)]
    pub examples: Vec<Example>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionalSpec {
    pub name: String,
    pub required: bool,
    pub choices: BTreeMap<String, ChoiceSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceSpec {
    pub summary: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    #[serde(default)]
    pub exactly_one_positional: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub cmd: String,
    pub valid: bool,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub reason: Option<Reason>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    pub code: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonTemplate {
    pub text: String,
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

    println!("Documentation and generated code updated successfully.");
    Ok(())
}

/// Validate structural and referential integrity of the spec.
fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    // Validate global flags table keys
    for (short, flag) in &spec.flags {
        if short.len() != 1 {
            anyhow::bail!("Flag key '{}' must be exactly one character", short);
        }

        if !short.chars().all(|c| c.is_ascii_alphanumeric()) {
            anyhow::bail!("Flag key '{}' must be ASCII alphanumeric", short);
        }

        if flag.long.trim().is_empty() {
            anyhow::bail!("Flag '{}' has empty long name", short);
        }
    }

    for (cmd_name, cmd) in &spec.commands {
        // Command-specific validation
        match cmd {
            CommandSpec::FlagCommand(flag_cmd) => {
                for flag in &flag_cmd.allowed_flags {
                    if !spec.flags.contains_key(flag) {
                        anyhow::bail!(
                            "Command '{}' references unknown allowed flag '{}'",
                            cmd_name,
                            flag
                        );
                    }
                }
            }

            CommandSpec::SelectorCommand(selector_cmd) => {
                if selector_cmd.positional.name.trim().is_empty() {
                    anyhow::bail!(
                        "Command '{}' has positional with empty name",
                        cmd_name
                    );
                }

                if selector_cmd.positional.choices.is_empty() {
                    anyhow::bail!(
                        "Command '{}' selector positional '{}' has no choices",
                        cmd_name,
                        selector_cmd.positional.name
                    );
                }

                for (choice, choice_spec) in &selector_cmd.positional.choices {
                    if choice.trim().is_empty() {
                        anyhow::bail!(
                            "Command '{}' has empty selector choice",
                            cmd_name
                        );
                    }

                    if choice_spec.summary.trim().is_empty() {
                        anyhow::bail!(
                            "Command '{}' selector choice '{}' has empty summary",
                            cmd_name,
                            choice
                        );
                    }
                }

                if selector_cmd.usage.trim().is_empty() {
                    anyhow::bail!("Command '{}' has empty usage", cmd_name);
                }
            }
        }

        // Example validation
        for ex in cmd.examples() {
            // reason code must exist if present
            if let Some(reason) = &ex.reason
                && !spec.reason_templates.contains_key(&reason.code)
            {
                anyhow::bail!(
                    "Unknown reason code '{}' in command '{}' example '{}'",
                    reason.code,
                    cmd_name,
                    ex.cmd
                );
            }

            // Valid examples should generally have outcome and not reason
            if ex.valid {
                if ex.reason.is_some() {
                    anyhow::bail!(
                        "Valid example '{}' in command '{}' must not include reason",
                        ex.cmd,
                        cmd_name
                    );
                }
            } else {
                if ex.reason.is_none() {
                    anyhow::bail!(
                        "Invalid example '{}' in command '{}' must include reason",
                        ex.cmd,
                        cmd_name
                    );
                }
            }
        }
    }

    Ok(())
}

/// Generate usage.md
fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("## {}\n{}\n\n", cmd_name, cmd.summary()));
        for ex in cmd.examples() {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_default()
            } else if let Some(r) = &ex.reason {
                format!("({})", r.code)
            } else {
                "(invalid)".to_string()
            };
            out.push_str(&format!("- `{}` → {}\n", ex.cmd, outcome));
        }
        out.push('\n');
    }
    Ok(out)
}

/// Generate TLDR Markdown
fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);
    for cmd in spec.commands.values() {
        for ex in cmd.examples() {
            if ex.valid {
                let outcome = ex.outcome.clone().unwrap_or_default();
                out.push_str(&format!("- {}:\n\n`{}`\n\n", outcome, ex.cmd));
            }
        }
    }
    Ok(out)
}

/// Generate SCD (simplified command descriptor)
fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();
    for (cmd_name, cmd) in &spec.commands {
        let flags = cmd.allowed_flags().unwrap_or(&[]);
        out.push_str(&format!(
            "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
            cmd_name,
            cmd.summary(),
            flags
        ));
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

/// Generate cli_matrix.rs
fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\n".to_string();
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");
    for cmd in spec.commands.values() {
        for ex in cmd.examples() {
            let outcome = ex.outcome.clone().unwrap_or_default();
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
    }
    out.push_str("];\n\n");

    out.push_str(
        "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_matrix() {\n        assert!(!CLI_MATRIX.is_empty());\n    }\n}\n",
    );

    Ok(out)
}

/// Generate man page
fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    // Header
    out.push_str(&format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n",
        spec.meta.program.to_uppercase(),
        spec.version
    ));
    out.push_str(&format!(
        ".SH NAME\n{}\n\t{}\n\n",
        spec.meta.program, spec.meta.summary
    ));

    // Commands
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!(
            ".SH {}\n{}\n",
            cmd_name.to_uppercase(),
            cmd.summary()
        ));

        for ex in cmd.examples() {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_default()
            } else if let Some(reason) = &ex.reason {
                format!("{{{{ {} }}}}", reason.code.to_uppercase())
            } else {
                String::new()
            };
            out.push_str(&format!(".TP\n{}\n", ex.cmd));
            if !outcome.is_empty() {
                out.push_str(&format!("{}\n", outcome));
            }
        }
    }

    Ok(out)
}
