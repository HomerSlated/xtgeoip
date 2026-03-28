// src/bin/xtgeoip-docgen.rs
use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use tera::{Context, Tera};

#[derive(Debug, Deserialize)]
struct Spec {
    version: u32,
    meta: Meta,
    flags: BTreeMap<String, FlagDef>,
    reason_templates: BTreeMap<String, ReasonTemplate>,
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
struct ReasonTemplate {
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
    let path = Path::new("docs/spec/cli.yaml");
    let raw = fs::read_to_string(path)?;
    let spec: Spec = serde_yaml::from_str(&raw)?;

    validate_spec(&spec)?;

    // Generate usage.md
    let usage_md = generate_usage_md(&spec);
    fs::write("docs/usage.md", usage_md)?;

    // Generate tldr.md
    let tldr_md = generate_tldr_md(&spec);
    fs::write("docs/tldr.md", tldr_md)?;

    // Generate scd.md
    let scd_md = generate_scd_md(&spec);
    fs::write("docs/scd.md", scd_md)?;

    // Generate error_text.rs
    let error_rs = generate_error_text_rs(&spec);
    fs::create_dir_all("src/generated")?;
    fs::write("src/generated/error_text.rs", error_rs)?;

    // Generate cli_matrix.rs
    let cli_matrix_rs = generate_cli_matrix_rs(&spec);
    fs::write("src/generated/cli_matrix.rs", cli_matrix_rs)?;

    // Generate xtgeoip.1
    let manpage = generate_manpage(&spec);
    fs::write("docs/xtgeoip.1", manpage)?;

    Ok(())
}

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    // Ensure all allowed_flags exist in spec.flags
    for (cmd_name, cmd) in &spec.commands {
        for flag in &cmd.allowed_flags {
            if !spec.flags.contains_key(flag) {
                anyhow::bail!("Command '{}' allows unknown flag '{}'", cmd_name, flag);
            }
        }
    }
    Ok(())
}

fn render_reason_text(spec: &Spec, r: &ReasonRef) -> String {
    let template = spec.reason_templates.get(&r.code).expect("unknown reason code");
    let mut text = template.text.clone();
    if let Some(args) = &r.args {
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), v);
        }
    }
    text
}

fn generate_usage_md(spec: &Spec) -> String {
    let mut out = format!("# {}\n\n{}\n\n## Flags\n\n", spec.meta.program, spec.meta.summary);
    for (short, flag) in &spec.flags {
        out.push_str(&format!("-`-{}` / `--{}` ({})\n", short, flag.long, flag.summary));
    }
    out.push_str("\n## Commands\n\n");
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("### {}\n{}\n\n", cmd_name, cmd.summary));
        for ex in &cmd.examples {
            if ex.valid {
                let outcome = ex.outcome.clone().unwrap_or_default();
                out.push_str(&format!("- `{}` → {}\n", ex.cmd, outcome));
            } else if let Some(reason) = &ex.reason {
                let text = render_reason_text(spec, reason);
                out.push_str(&format!("- `{}` → {}\n", ex.cmd, text));
            }
        }
        out.push('\n');
    }
    out
}

fn generate_tldr_md(spec: &Spec) -> String {
    let mut out = String::new();
    for (cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            if ex.valid {
                out.push_str(&format!("{}: {}\n", ex.cmd, ex.outcome.clone().unwrap_or_default()));
            }
        }
    }
    out
}

fn generate_scd_md(spec: &Spec) -> String {
    let mut out = String::new();
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!("Command: {}\n  Summary: {}\n  Allowed Flags: {:?}\n", cmd_name, cmd.summary, cmd.allowed_flags));
    }
    out
}

fn generate_error_text_rs(spec: &Spec) -> String {
    let mut out = "// Generated error texts\n\n".to_string();
    out.push_str("pub fn error_text(code: &str) -> &'static str {\n    match code {\n");
    for (code, tmpl) in &spec.reason_templates {
        out.push_str(&format!("        \"{}\" => \"{}\",\n", code, tmpl.text));
    }
    out.push_str("        _ => \"unknown error\",\n    }\n}\n");
    out
}

fn generate_cli_matrix_rs(spec: &Spec) -> String {
    let mut out = "// Generated CLI matrix for tests\n\n";
    out.push_str("pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\n");
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");
    for (cmd_name, cmd) in &spec.commands {
        for ex in &cmd.examples {
            let outcome = ex.outcome.clone().unwrap_or_default();
            out.push_str(&format!("    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n", ex.cmd, ex.valid, outcome));
        }
    }
    out.push_str("];\n");

    // Tiny test module
    out.push_str(
        "\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn sanity() {\n        assert!(!CLI_MATRIX.is_empty());\n    }\n}\n",
    );
    out
}

fn generate_manpage(spec: &Spec) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!(
        ".TH {} 1 \"\" \"{}\" \"{} manual\"\n",
        spec.meta.program.to_uppercase(),
        chrono::Local::now().format("%Y-%m-%d"),
        spec.meta.program
    ));
    out.push_str(&format!(".SH NAME\n{} \\- {}\n", spec.meta.program, spec.meta.summary));

    // Synopsis
    out.push_str(".SH SYNOPSIS\n");
    out.push_str(&format!("{} [OPTIONS] <COMMAND>\n\n", spec.meta.program));

    // Options
    out.push_str(".SH OPTIONS\n");
    for (short, flag) in &spec.flags {
        out.push_str(&format!(
            ".TP\n\\fB-{0}\\fR, \\fB--{1}\\fR\n{2}\n",
            short, flag.long, flag.summary
        ));
    }

    // Commands
    out.push_str(".SH COMMANDS\n");
    for (cmd_name, cmd) in &spec.commands {
        out.push_str(&format!(".TP\n\\fB{}\\fR\n{}\n", cmd_name, cmd.summary));

        // Show allowed flags for command
        if !cmd.allowed_flags.is_empty() {
            let allowed: Vec<String> = cmd
                .allowed_flags
                .iter()
                .map(|f| format!("--{}", spec.flags[f].long))
                .collect();
            out.push_str(&format!("Allowed flags: {}\n", allowed.join(", ")));
        }

        // Examples
        for ex in &cmd.examples {
            if ex.valid {
                let outcome = ex.outcome.clone().unwrap_or_default();
                out.push_str(&format!("Example: {} → {}\n", ex.cmd, outcome));
            } else if let Some(reason) = &ex.reason {
                let text = render_reason_text(spec, reason);
                out.push_str(&format!("Example: {} → {}\n", ex.cmd, text));
            }
        }
    }

    // Footer
    out.push_str(".SH AUTHOR\nGenerated by xtgeoip-docgen\n");

    out
}
