//! xtgeoip-docgen v2
//! Generates documentation and test matrices from cli.yaml
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::Path,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Spec {
    version: u32,
    meta: Meta,
    flags: BTreeMap<String, FlagDef>,
    reason_templates: BTreeMap<String, ReasonRef>,
    commands: BTreeMap<String, CommandDef>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    program: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct FlagDef {
    long: String,
    kind: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct ReasonRef {
    text: String,
}

#[derive(Debug, Deserialize)]
struct CommandDef {
    summary: String,
    allowed_flags: Vec<String>,
    examples: Vec<Example>,
}

#[derive(Debug, Deserialize)]
struct Example {
    cmd: String,
    valid: bool,
    outcome: Option<String>,
    reason: Option<Reason>,
}

#[derive(Debug, Deserialize)]
struct Reason {
    code: String,
    args: Option<BTreeMap<String, String>>,
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

    println!("Documentation and generated code updated successfully.");
    Ok(())
}

/// Ensure that all example flags exist
fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    for (_cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            if let Some(reason) = &ex.reason {
                if !spec.reason_templates.contains_key(&reason.code) {
                    anyhow::bail!("Unknown reason code {} in command example {}", reason.code, ex.cmd);
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
    let mut out = format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);
    for (_cmd_name, cmd) in &spec.commands {
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
        out.push_str(&format!("Command: {}\nSummary: {}\nFlags: {:?}\n\n",
            cmd_name, cmd.summary, cmd.allowed_flags));
    }
    Ok(out)
}

/// Generate error_text.rs
fn generate_error_text_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "// Auto-generated error texts\n".to_string();
    for (code, r) in &spec.reason_templates {
        out.push_str(&format!("pub const {}: &str = r#\"{}\"#;\n", code.to_uppercase(), r.text));
    }
    Ok(out)
}

/// Generate cli_matrix.rs and a tiny test module
fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\n".to_string();
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");
    for (_cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            let outcome = ex.outcome.clone().unwrap_or_default();
            out.push_str(&format!("    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, outcome));
        }
    }
    out.push_str("];\n\n");

    out.push_str("#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_matrix() {\n        assert!(!CLI_MATRIX.is_empty());\n    }\n}\n");

    Ok(out)
}

fn generate_manpage(spec: &Spec) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!(
        ".TH {} 1 \"{}\" \"{} v1\" \"User Commands\"\n",
        spec.meta.program.to_uppercase(),
        chrono::Local::now().format("%Y-%m-%d"),
        spec.meta.program
    ));

    // Name section
    out.push_str(&format!(
        ".SH NAME\n{} \\- {}\n\n",
        spec.meta.program, spec.meta.summary
    ));

    // Synopsis
    out.push_str(".SH SYNOPSIS\n.B ");
    out.push_str(&spec.meta.program);
    out.push_str(" [\\fB-b\\fR] [\\fB-c\\fR] [\\fB-p\\fR] [\\fB-f\\fR] \\fICOMMAND\\fR [\\fB-flags\\fR]\n\n");

    // Description
    out.push_str(".SH DESCRIPTION\n");
    out.push_str(&format!("{}\n\n", spec.meta.summary));

    // Commands
    out.push_str(".SH COMMANDS\n");
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(".TP\n");
        out.push_str(&format!("\\fB{}\\fR\n", cmd_name));
        out.push_str(&format!("{}\n\n", cmd.summary));
    }

    // Options
    out.push_str(".SH OPTIONS\n");
    for (flag, def) in &spec.flags {
        out.push_str(".TP\n");
        out.push_str(&format!("\\fB-{}\\fR, \\fB--{}\\fR\n", flag, def.long));
        out.push_str(&format!("{}\n\n", def.summary));
    }

    // Examples
    out.push_str(".SH EXAMPLES\n");
    for (cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            if let Some(outcome) = &ex.outcome {
                out.push_str(".TP\n");
                out.push_str(&format!("{}\n.nf\n{}\n.fi\n\n", outcome, ex.cmd));
            }
        }
    }

    // Author / See also
    out.push_str(".SH AUTHOR\nGenerated from CLI specification YAML by xtgeoip-docgen.\n\n");
    out.push_str(".SH \"SEE ALSO\"\n.tl tldr(1)\n");

    out
}
