use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Spec {
    version: u32,
    meta: Meta,
    flags: BTreeMap<String, FlagDef>,
    reason_codes: BTreeMap<String, ReasonTemplate>,
    commands: BTreeMap<String, CommandDef>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    command: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct FlagDef {
    long: String,
    kind: String,
    meaning: String,
}

#[derive(Debug, Deserialize)]
struct ReasonTemplate {
    template: String,
}

#[derive(Debug, Deserialize)]
struct CommandDef {
    name: String,
    synopsis: String,
    description: String,
    allowed_flags: Vec<String>,
    examples: Vec<Example>,
}

#[derive(Debug, Deserialize)]
struct Example {
    argv: Vec<String>,
    valid: bool,
    note: String,
    reason: Option<ReasonRef>,
}

#[derive(Debug, Deserialize)]
struct ReasonRef {
    code: String,
    args: Option<BTreeMap<String, String>>,
}

fn main() -> Result<()> {
    let spec_path = "doc/xtgeoip-usage.yaml";
    let spec: Spec = serde_yaml::from_str(
        &fs::read_to_string(spec_path).with_context(|| format!("reading {}", spec_path))?,
    )
    .with_context(|| format!("parsing {}", spec_path))?;

    fs::create_dir_all("doc/tldr")?;
    fs::create_dir_all("src/generated")?;

    fs::write("doc/usage.md", render_usage_md(&spec))?;
    fs::write("doc/tldr/xtgeoip.md", render_tldr_md(&spec))?;
    fs::write("doc/xtgeoip.1.scd", render_scd(&spec))?;
    fs::write(
        "src/generated/help_text.json",
        serde_json::to_string_pretty(&build_help_json(&spec))?,
    )?;
    fs::write(
        "src/generated/error_text.json",
        serde_json::to_string_pretty(&build_error_json(&spec))?,
    )?;

    println!("Generated:");
    println!("  doc/usage.md");
    println!("  doc/tldr/xtgeoip.md");
    println!("  doc/xtgeoip.1.scd");
    println!("  src/generated/help_text.json");
    println!("  src/generated/error_text.json");

    Ok(())
}

fn render_usage_md(spec: &Spec) -> String {
    let mut out = String::new();
    out.push_str("<!-- Generated from doc/xtgeoip-usage.yaml. Do not edit manually. -->\n\n");
    out.push_str("# Usage\n\n");

    for key in ["root", "build", "fetch", "run", "conf"] {
        if let Some(cmd) = spec.commands.get(key) {
            let heading = if key == "root" { "Top level" } else { key };
            out.push_str(&format!("## {}\n\n", heading));

            for ex in &cmd.examples {
                let cmdline = format_cmdline(spec, key, &ex.argv);
                out.push_str(&format!("- `{}` — {}\n", cmdline, ex.note));
            }

            out.push('\n');
        }
    }

    out
}

fn render_tldr_md(spec: &Spec) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {}\n\n> {}.\n> More information: `man {}`.\n\n",
        spec.meta.command, spec.meta.summary, spec.meta.command
    ));

    // Curated representative valid examples
    let picks = [
        ("root", vec!["b"]),
        ("root", vec!["b", "c", "f"]),
        ("build", vec![]),
        ("build", vec!["b", "p"]),
        ("fetch", vec!["p"]),
        ("run", vec![]),
        ("run", vec!["c", "p"]),
        ("conf", vec!["s"]),
        ("conf", vec!["e"]),
    ];

    for (cmd, argv) in picks {
        if let Some(example) = find_example(spec, cmd, &argv) {
            out.push_str(&format!("- {}:\n", capitalize_first(&example.note)));
            out.push_str(&format!("`{}`\n\n", format_cmdline(spec, cmd, &example.argv)));
        }
    }

    out
}

fn render_scd(spec: &Spec) -> String {
    let mut out = String::new();

    out.push_str("xtgeoip(1)\n\n");
    out.push_str("# NAME\n\n");
    out.push_str(&format!("xtgeoip - {}\n\n", spec.meta.summary));

    out.push_str("# SYNOPSIS\n\n");
    for key in ["root", "build", "fetch", "run", "conf"] {
        if let Some(cmd) = spec.commands.get(key) {
            out.push_str(&format!("*{}*\n\n", cmd.synopsis));
        }
    }

    out.push_str("# DESCRIPTION\n\n");
    out.push_str("xtgeoip manages xt_geoip binary data, local backups, and archived MaxMind CSV downloads.\n\n");

    out.push_str("# COMMANDS\n\n");
    for key in ["root", "build", "fetch", "run", "conf"] {
        if let Some(cmd) = spec.commands.get(key) {
            let title = if key == "root" { "top level" } else { key };
            out.push_str(&format!("## {}\n\n", title));
            out.push_str(&format!("{}\n\n", cmd.description));
        }
    }

    out.push_str("# OPTIONS\n\n");
    let mut seen = BTreeSet::new();
    for (short, flag) in &spec.flags {
        if seen.insert(short.clone()) {
            out.push_str(&format!("*-{}*, *--{}*\n", short, flag.long));
            out.push_str(&format!(": {}\n\n", flag.meaning));
        }
    }

    out.push_str("# INVALID COMBINATIONS\n\n");
    out.push_str("The following rules are enforced:\n\n");
    out.push_str("- *--force* may be used only with *--backup* and/or *--clean*.\n");
    out.push_str("- *--prune* may not be combined with *--force*.\n");
    out.push_str("- In *run*, *--backup* with *--prune* is rejected because prune would be ambiguous between backups and CSV archives.\n");
    out.push_str("- In top-level mode, *--prune* requires *--backup*.\n");
    out.push_str("- In *build*, *--prune* requires *--backup*.\n\n");

    out.push_str("# EXAMPLES\n\n");
    for key in ["root", "build", "fetch", "run", "conf"] {
        if let Some(cmd) = spec.commands.get(key) {
            out.push_str(&format!("## {}\n\n", if key == "root" { "top level" } else { key }));
            for ex in cmd.examples.iter().filter(|e| e.valid) {
                out.push_str(&format!("*{}*\n", format_cmdline(spec, key, &ex.argv)));
                out.push_str(&format!(": {}\n\n", ex.note));
            }
        }
    }

    out.push_str("# FILES\n\n");
    out.push_str("*/usr/share/xt_geoip/*\n");
    out.push_str(": Installed xt_geoip binary data.\n\n");
    out.push_str("*/var/lib/xt_geoip/*\n");
    out.push_str(": Local backups and archived CSV downloads.\n\n");

    out.push_str("# SEE ALSO\n\n");
    out.push_str("*iptables*(8), *ip6tables*(8)\n");

    out
}

fn build_help_json(spec: &Spec) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (key, cmd) in &spec.commands {
        map.insert(key.clone(), cmd.description.clone());
    }
    map
}

fn build_error_json(spec: &Spec) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (code, tmpl) in &spec.reason_codes {
        map.insert(code.clone(), tmpl.template.clone());
    }
    map
}

fn find_example<'a>(spec: &'a Spec, cmd: &str, argv: &[&str]) -> Option<&'a Example> {
    let wanted: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    spec.commands
        .get(cmd)?
        .examples
        .iter()
        .find(|ex| ex.valid && ex.argv == wanted)
}

fn format_cmdline(spec: &Spec, cmd_key: &str, argv: &[String]) -> String {
    let mut parts = vec![spec.meta.command.clone()];
    if cmd_key != "root" {
        parts.push(cmd_key.to_string());
    }
    for flag in argv {
        parts.push(format!("-{}", flag));
    }
    parts.join(" ")
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
