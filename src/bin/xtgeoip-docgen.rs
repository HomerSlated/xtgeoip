//! xtgeoip-docgen v2
//! Generates documentation and test matrices from cli.yaml

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Spec {
    version: u32,
    meta: Meta,
    flags: BTreeMap<String, FlagDef>,
    reason_templates: BTreeMap<String, ReasonDef>,
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
struct ReasonDef {
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
    reason: Option<ReasonRef>,
}

#[derive(Debug, Deserialize)]
struct ReasonRef {
    code: String,
    args: Option<BTreeMap<String, String>>,
}

fn main() -> anyhow::Result<()> {
    let spec_path = Path::new("docs/spec/cli.yaml");
    let spec_str = fs::read_to_string(spec_path)?;
    let spec: Spec = serde_yaml::from_str(&spec_str)?;
    validate_spec(&spec)?;

    fs::write("docs/generated/usage.md", generate_usage_md(&spec))?;
    fs::write("docs/generated/tldr", generate_tldr(&spec))?;
    fs::write("docs/generated/scd", generate_scd(&spec))?;

    fs::create_dir_all("src/generated")?;
    fs::write("src/generated/error_text.rs", generate_error_text_rs(&spec))?;
    fs::write("src/generated/cli_matrix.rs", generate_cli_matrix_rs(&spec))?;

    println!("Documentation and generated code updated successfully.");
    Ok(())
}

// --- Validation ---
fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    for (cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            for ch in extract_flags(&ex.cmd) {
                if !cmd.allowed_flags.contains(&ch) && ex.valid {
                    anyhow::bail!(
                        "Spec error: example '{}' uses flag '{}' not allowed by '{}'",
                        ex.cmd,
                        ch,
                        cmd_name
                    );
                }
            }
        }
    }
    Ok(())
}

fn extract_flags(cmd: &str) -> Vec<String> {
    cmd.split_whitespace()
        .filter_map(|part| {
            if part.starts_with('-') && part.len() > 1 {
                Some(part.trim_start_matches('-').to_string())
            } else {
                None
            }
        })
        .collect()
}

// --- Generate usage.md ---
fn generate_usage_md(spec: &Spec) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary));
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("## {}\n{}\n\n", cmd_name, cmd.summary));
        for ex in &cmd.examples {
            let reason = if !ex.valid {
                Some(render_reason(&spec, ex.reason.as_ref()))
            } else {
                None
            };
            out.push_str(&format!(
                "- `{}`{}{}\n",
                ex.cmd,
                if let Some(r) = reason { format!(" → {}", r) } else { "".to_string() },
                if let Some(outcome) = &ex.outcome { format!(" → {}", outcome) } else { "".to_string() }
            ));
        }
        out.push('\n');
    }
    out
}

// --- Render reason ---
fn render_reason(spec: &Spec, r: Option<&ReasonRef>) -> String {
    if let Some(r) = r {
        let template = spec
            .reason_templates
            .get(&r.code)
            .map(|t| &t.text)
            .unwrap_or(&"unknown reason".to_string())
            .clone();
        if let Some(args) = &r.args {
            let mut out = template.clone();
            for (k, v) in args {
                out = out.replace(&format!("{{{}}}", k), v);
            }
            out
        } else {
            template
        }
    } else {
        "".to_string()
    }
}

// --- Generate tldr ---
fn generate_tldr(spec: &Spec) -> String {
    let mut out = String::new();
    for (_cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            if ex.valid {
                out.push_str(&format!("{}\n", ex.cmd));
            }
        }
    }
    out
}

// --- Generate scd ---
fn generate_scd(spec: &Spec) -> String {
    let mut out = String::new();
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("Command: {}\nSummary: {}\nFlags: {:?}\n\n",
            cmd_name, cmd.summary, cmd.allowed_flags));
    }
    out
}

// --- Generate error_text.rs ---
fn generate_error_text_rs(spec: &Spec) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated error texts\n");
    for (code, r) in &spec.reason_templates {
        out.push_str(&format!(
            "pub const {}: &str = r#\"{}\"#;\n",
            code.to_uppercase(),
            r.text
        ));
    }
    out
}

// --- Generate cli_matrix.rs ---
fn generate_cli_matrix_rs(spec: &Spec) -> String {
    let mut out = String::new();
    out.push_str("pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\n");
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");
    for cmd in spec.commands.values() {
        for ex in &cmd.examples {
            let outcome = ex.outcome.clone().unwrap_or_default();
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
    }
    out.push_str("];\n");

    out.push_str(
        r#"
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cli_matrix_valid() {
        for ex in CLI_MATRIX {
            println!("{} → {} → {}", ex.cmd, ex.valid, ex.outcome);
        }
    }
}
"#,
    );

    out
}
