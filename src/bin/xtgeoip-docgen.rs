//! xtgeoip-docgen v2
//! Generates documentation and test matrices from cli.yaml
use std::{collections::BTreeMap, fs};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,
    pub commands: BTreeMap<String, Command>,
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
}

#[derive(Debug, Deserialize)]
pub struct Meta {
    pub program: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct Command {
    pub summary: String,
    pub allowed_flags: Vec<String>,
    pub examples: Vec<Example>,
}

#[derive(Debug, Deserialize)]
pub struct Example {
    pub cmd: String,
    pub valid: bool,
    pub outcome: Option<String>,
    pub reason: Option<Reason>,
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

/// Ensure that all example flags exist
fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    for cmd in spec.commands.values() {
        for ex in &cmd.examples {
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
    Ok(())
}

/// Generate usage.md
fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("## {}\n{}\n\n", cmd_name, cmd.summary));
        for ex in &cmd.examples {
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
    let mut out =
        format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);
    for cmd in spec.commands.values() {
        for ex in &cmd.examples {
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
        out.push_str(&format!(
            "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
            cmd_name, cmd.summary, cmd.allowed_flags
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

/// Generate cli_matrix.rs and a tiny test module
fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: \
                   bool, pub outcome: &'static str }\n"
        .to_string();
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");
    for cmd in spec.commands.values() {
        for ex in &cmd.examples {
            let outcome = ex.outcome.clone().unwrap_or_default();
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" \
                 }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
    }
    out.push_str("];\n\n");

    out.push_str(
        "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn \
         test_matrix() {\n        assert!(!CLI_MATRIX.is_empty());\n    }\n}\n",
    );

    Ok(out)
}

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
            cmd.summary
        ));

        for ex in &cmd.examples {
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
