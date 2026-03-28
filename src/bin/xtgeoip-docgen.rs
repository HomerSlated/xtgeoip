use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SPEC_PATH: &str = "docs/cli_spec.yaml";
const OUT_DOCS_DIR: &str = "docs/generated";
const OUT_SRC_DIR: &str = "src/generated";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec_text = fs::read_to_string(SPEC_PATH)?;
    let spec: Spec = serde_yaml::from_str(&spec_text)?;

    validate_spec(&spec)?;

    fs::create_dir_all(OUT_DOCS_DIR)?;
    fs::create_dir_all(OUT_SRC_DIR)?;

    write_file(
        Path::new(OUT_DOCS_DIR).join("usage.md"),
        render_usage_md(&spec),
    )?;

    write_file(
        Path::new(OUT_DOCS_DIR).join("xtgeoip.tldr"),
        render_tldr(&spec),
    )?;

    write_file(
        Path::new(OUT_DOCS_DIR).join("xtgeoip.scd"),
        render_scd(&spec),
    )?;

    write_file(
        Path::new(OUT_SRC_DIR).join("error_text.rs"),
        render_error_text_rs(&spec),
    )?;

    write_file(
        Path::new(OUT_SRC_DIR).join("cli_matrix.rs"),
        render_cli_matrix_rs(&spec),
    )?;

    println!("Generated documentation and support files from {}", SPEC_PATH);
    Ok(())
}

fn write_file(path: PathBuf, contents: String) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(&path, contents)?;
    println!("wrote {}", path.display());
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Spec {
    version: u32,
    program: Program,
    flags: Vec<FlagDef>,
    commands: Vec<CommandDef>,
    examples: Option<Vec<Example>>,
    reasons: Option<Vec<ReasonTemplate>>,
}

#[derive(Debug, Deserialize)]
struct Program {
    name: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct FlagDef {
    short: Option<String>,
    long: String,
    kind: String,
    value_name: Option<String>,
    scope: String, // "global" or "command"
    description: String,
    default: Option<String>,
    notes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CommandDef {
    name: String,
    summary: String,
    long: Option<String>,
    allowed_flags: Vec<String>,
    usage: Option<String>,
    examples: Option<Vec<Example>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Example {
    desc: String,
    command: String,
    reason: Option<ReasonRef>,
}

#[derive(Debug, Deserialize, Clone)]
struct ReasonRef {
    code: String,
    args: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ReasonTemplate {
    code: String,
    summary: String,
    detail: String,
}

/* =========================
 * Validation
 * ========================= */

fn validate_spec(spec: &Spec) -> Result<(), Box<dyn std::error::Error>> {
    if spec.version == 0 {
        return err("spec.version must be > 0");
    }

    if spec.program.name.trim().is_empty() {
        return err("program.name must not be empty");
    }

    let mut seen_longs = BTreeSet::new();
    let mut seen_shorts = BTreeSet::new();

    for flag in &spec.flags {
        if flag.long.trim().is_empty() {
            return err("flag.long must not be empty");
        }

        if !seen_longs.insert(flag.long.clone()) {
            return err(format!("duplicate flag.long '{}'", flag.long));
        }

        if let Some(s) = &flag.short {
            if s.trim().is_empty() {
                return err(format!("flag.short for --{} is empty", flag.long));
            }
            if !seen_shorts.insert(s.clone()) {
                return err(format!("duplicate flag.short '{}'", s));
            }
        }

        match flag.scope.as_str() {
            "global" | "command" => {}
            other => {
                return err(format!(
                    "flag --{} has invalid scope '{}'; expected 'global' or 'command'",
                    flag.long, other
                ))
            }
        }

        match flag.kind.as_str() {
            "switch" | "value" => {}
            other => {
                return err(format!(
                    "flag --{} has invalid kind '{}'; expected 'switch' or 'value'",
                    flag.long, other
                ))
            }
        }

        if flag.kind == "value" && flag.value_name.as_deref().unwrap_or("").trim().is_empty() {
            return err(format!(
                "flag --{} is kind=value but missing value_name",
                flag.long
            ));
        }

        if flag.kind == "switch" && flag.value_name.is_some() {
            return err(format!(
                "flag --{} is kind=switch but has value_name",
                flag.long
            ));
        }
    }

    let flag_names: BTreeSet<&str> = spec.flags.iter().map(|f| f.long.as_str()).collect();

    let mut seen_commands = BTreeSet::new();
    for cmd in &spec.commands {
        if cmd.name.trim().is_empty() {
            return err("command.name must not be empty");
        }

        if !seen_commands.insert(cmd.name.clone()) {
            return err(format!("duplicate command '{}'", cmd.name));
        }

        for af in &cmd.allowed_flags {
            if !flag_names.contains(af.as_str()) {
                return err(format!(
                    "command '{}' references unknown allowed_flag '{}'",
                    cmd.name, af
                ));
            }
        }

        if let Some(exs) = &cmd.examples {
            for ex in exs {
                validate_example_reason(spec, ex, Some(&cmd.name))?;
            }
        }
    }

    if let Some(exs) = &spec.examples {
        for ex in exs {
            validate_example_reason(spec, ex, None)?;
        }
    }

    if let Some(reasons) = &spec.reasons {
        let mut seen = BTreeSet::new();
        for r in reasons {
            if r.code.trim().is_empty() {
                return err("reason.code must not be empty");
            }
            if !seen.insert(r.code.clone()) {
                return err(format!("duplicate reason.code '{}'", r.code));
            }
        }
    }

    Ok(())
}

fn validate_example_reason(
    spec: &Spec,
    ex: &Example,
    owner: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(reason) = &ex.reason {
        let reasons = spec
            .reasons
            .as_ref()
            .ok_or_else(|| boxed_err("example references reason but spec.reasons is missing"))?;

        let template = reasons.iter().find(|r| r.code == reason.code).ok_or_else(|| {
            boxed_err(match owner {
                Some(cmd) => format!(
                    "command '{}' example '{}' references unknown reason.code '{}'",
                    cmd, ex.desc, reason.code
                ),
                None => format!(
                    "top-level example '{}' references unknown reason.code '{}'",
                    ex.desc, reason.code
                ),
            })
        })?;

        // Validate that all placeholders used in summary/detail exist in args.
        let placeholders = extract_placeholders(&template.summary)
            .into_iter()
            .chain(extract_placeholders(&template.detail))
            .collect::<BTreeSet<_>>();

        if !placeholders.is_empty() {
            let args = reason.args.as_ref().ok_or_else(|| {
                boxed_err(match owner {
                    Some(cmd) => format!(
                        "command '{}' example '{}' reason '{}' needs args {:?} but args missing",
                        cmd, ex.desc, reason.code, placeholders
                    ),
                    None => format!(
                        "top-level example '{}' reason '{}' needs args {:?} but args missing",
                        ex.desc, reason.code, placeholders
                    ),
                })
            })?;

            for ph in &placeholders {
                if !args.contains_key(ph) {
                    return err(match owner {
                        Some(cmd) => format!(
                            "command '{}' example '{}' reason '{}' missing arg '{}'",
                            cmd, ex.desc, reason.code, ph
                        ),
                        None => format!(
                            "top-level example '{}' reason '{}' missing arg '{}'",
                            ex.desc, reason.code, ph
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}

/* =========================
 * Rendering helpers
 * ========================= */

fn find_reason_template<'a>(spec: &'a Spec, code: &str) -> Option<&'a ReasonTemplate> {
    spec.reasons
        .as_ref()
        .and_then(|rs| rs.iter().find(|r| r.code == code))
}

fn render_reason_text(spec: &Spec, rr: &ReasonRef) -> String {
    let Some(tpl) = find_reason_template(spec, &rr.code) else {
        // Should never happen after validation.
        return format!("{}: {}", rr.code, rr.code);
    };

    let args = rr.args.as_ref();

    let summary = substitute_template(&tpl.summary, args);
    let detail = substitute_template(&tpl.detail, args);

    if detail.trim().is_empty() {
        summary
    } else {
        format!("{} {}", summary, detail)
    }
}

fn substitute_template(template: &str, args: Option<&BTreeMap<String, String>>) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '}') {
                let end = i + 1 + end;
                let key: String = chars[i + 1..end].iter().collect();
                if let Some(val) = args.and_then(|m| m.get(&key)) {
                    out.push_str(val);
                } else {
                    // Leave placeholder intact if missing; validation should prevent this.
                    out.push('{');
                    out.push_str(&key);
                    out.push('}');
                }
                i = end + 1;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

fn extract_placeholders(template: &str) -> Vec<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();

    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '}') {
                let end = i + 1 + end;
                let key: String = chars[i + 1..end].iter().collect();
                if !key.trim().is_empty() {
                    out.push(key);
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }

    out
}

fn flag_synopsis(flag: &FlagDef) -> String {
    let mut parts = Vec::new();

    if let Some(s) = &flag.short {
        if flag.kind == "switch" {
            parts.push(format!("-{}", s));
        } else {
            parts.push(format!(
                "-{} <{}>",
                s,
                flag.value_name.as_deref().unwrap_or("VALUE")
            ));
        }
    }

    if flag.kind == "switch" {
        parts.push(format!("--{}", flag.long));
    } else {
        parts.push(format!(
            "--{} <{}>",
            flag.long,
            flag.value_name.as_deref().unwrap_or("VALUE")
        ));
    }

    parts.join(", ")
}

fn usage_line(spec: &Spec, cmd: &CommandDef) -> String {
    if let Some(u) = &cmd.usage {
        return u.clone();
    }

    let mut s = format!("{} {}", spec.program.name, cmd.name);

    let globals: Vec<&FlagDef> = spec.flags.iter().filter(|f| f.scope == "global").collect();
    if !globals.is_empty() {
        s.push_str(" [global options]");
    }

    if !cmd.allowed_flags.is_empty() {
        s.push_str(" [command options]");
    }

    s
}

fn rust_string_lit(s: &str) -> String {
    format!("{:?}", s)
}

/* =========================
 * usage.md
 * ========================= */

fn render_usage_md(spec: &Spec) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", spec.program.name));
    out.push_str(&format!("{}\n\n", spec.program.summary));

    out.push_str("## Commands\n\n");
    for cmd in &spec.commands {
        out.push_str(&format!("### `{}`\n\n", cmd.name));
        out.push_str(&format!("{}\n\n", cmd.summary));

        if let Some(long) = &cmd.long {
            out.push_str(long);
            out.push_str("\n\n");
        }

        out.push_str("**Usage**\n\n");
        out.push_str("```text\n");
        out.push_str(&usage_line(spec, cmd));
        out.push('\n');
        out.push_str("```\n\n");

        let cmd_flags: Vec<&FlagDef> = spec
            .flags
            .iter()
            .filter(|f| cmd.allowed_flags.iter().any(|af| af == &f.long))
            .collect();

        if !cmd_flags.is_empty() {
            out.push_str("**Command options**\n\n");
            for f in cmd_flags {
                out.push_str(&format!(
                    "- `{}` — {}\n",
                    flag_synopsis(f),
                    f.description
                ));
                if let Some(def) = &f.default {
                    out.push_str(&format!("  - Default: `{}`\n", def));
                }
                if let Some(notes) = &f.notes {
                    for n in notes {
                        out.push_str(&format!("  - {}\n", n));
                    }
                }
            }
            out.push('\n');
        }

        if let Some(exs) = &cmd.examples {
            out.push_str("**Examples**\n\n");
            for ex in exs {
                out.push_str(&format!("- {}\n", ex.desc));
                out.push_str("  ```sh\n");
                out.push_str("  ");
                out.push_str(&ex.command);
                out.push('\n');
                out.push_str("  ```\n");
                if let Some(reason) = &ex.reason {
                    out.push_str(&format!(
                        "  - Why: {}\n",
                        render_reason_text(spec, reason)
                    ));
                }
            }
            out.push('\n');
        }
    }

    let globals: Vec<&FlagDef> = spec.flags.iter().filter(|f| f.scope == "global").collect();
    if !globals.is_empty() {
        out.push_str("## Global options\n\n");
        for f in globals {
            out.push_str(&format!(
                "- `{}` — {}\n",
                flag_synopsis(f),
                f.description
            ));
            if let Some(def) = &f.default {
                out.push_str(&format!("  - Default: `{}`\n", def));
            }
            if let Some(notes) = &f.notes {
                for n in notes {
                    out.push_str(&format!("  - {}\n", n));
                }
            }
        }
        out.push('\n');
    }

    if let Some(exs) = &spec.examples {
        out.push_str("## General examples\n\n");
        for ex in exs {
            out.push_str(&format!("- {}\n", ex.desc));
            out.push_str("  ```sh\n");
            out.push_str("  ");
            out.push_str(&ex.command);
            out.push('\n');
            out.push_str("  ```\n");
            if let Some(reason) = &ex.reason {
                out.push_str(&format!(
                    "  - Why: {}\n",
                    render_reason_text(spec, reason)
                ));
            }
        }
        out.push('\n');
    }

    out
}

/* =========================
 * tldr
 * ========================= */

fn render_tldr(spec: &Spec) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", spec.program.name));
    out.push_str(&format!("> {}.\n", spec.program.summary));
    out.push_str("> More information: <https://example.invalid/xtgeoip>.\n\n");

    // Prefer top-level examples; if absent, fall back to first example per command.
    if let Some(exs) = &spec.examples {
        for ex in exs {
            out.push_str(&format!("- {}:\n\n", ex.desc));
            out.push_str(&format!("`{}`\n\n", ex.command));
        }
    } else {
        for cmd in &spec.commands {
            if let Some(exs) = &cmd.examples {
                if let Some(ex) = exs.first() {
                    out.push_str(&format!("- {}:\n\n", ex.desc));
                    out.push_str(&format!("`{}`\n\n", ex.command));
                }
            }
        }
    }

    out
}

/* =========================
 * scd (manpage source)
 * ========================= */

fn render_scd(spec: &Spec) -> String {
    let mut out = String::new();

    let title = spec.program.name.to_uppercase();

    out.push_str(&format!("{}(1)\n\n", title));
    out.push_str("# NAME\n\n");
    out.push_str(&format!("{} - {}\n\n", spec.program.name, spec.program.summary));

    out.push_str("# SYNOPSIS\n\n");
    for cmd in &spec.commands {
        out.push_str(&format!("*{}*\n", usage_line(spec, cmd)));
    }
    out.push('\n');

    out.push_str("# DESCRIPTION\n\n");
    out.push_str(&spec.program.summary);
    out.push_str("\n\n");

    let globals: Vec<&FlagDef> = spec.flags.iter().filter(|f| f.scope == "global").collect();
    if !globals.is_empty() {
        out.push_str("# GLOBAL OPTIONS\n\n");
        for f in globals {
            out.push_str(&format!("*{}*\n", scd_flag_synopsis(f)));
            out.push_str(&format!(": {}\n", f.description));
            if let Some(def) = &f.default {
                out.push_str(&format!("  Default: _{}_\n", def));
            }
            if let Some(notes) = &f.notes {
                for n in notes {
                    out.push_str(&format!("  {}\n", n));
                }
            }
            out.push('\n');
        }
    }

    for cmd in &spec.commands {
        out.push_str(&format!("# COMMAND {}\n\n", cmd.name.to_uppercase()));
        out.push_str(&format!("{}\n\n", cmd.summary));

        if let Some(long) = &cmd.long {
            out.push_str(long);
            out.push_str("\n\n");
        }

        out.push_str("Usage:\n");
        out.push_str(&format!("*{}*\n\n", usage_line(spec, cmd)));

        let cmd_flags: Vec<&FlagDef> = spec
            .flags
            .iter()
            .filter(|f| cmd.allowed_flags.iter().any(|af| af == &f.long))
            .collect();

        if !cmd_flags.is_empty() {
            out.push_str("Options:\n\n");
            for f in cmd_flags {
                out.push_str(&format!("*{}*\n", scd_flag_synopsis(f)));
                out.push_str(&format!(": {}\n", f.description));
                if let Some(def) = &f.default {
                    out.push_str(&format!("  Default: _{}_\n", def));
                }
                if let Some(notes) = &f.notes {
                    for n in notes {
                        out.push_str(&format!("  {}\n", n));
                    }
                }
                out.push('\n');
            }
        }

        if let Some(exs) = &cmd.examples {
            out.push_str("Examples:\n\n");
            for ex in exs {
                out.push_str(&format!("- {}\n", ex.desc));
                out.push_str(&format!("  `{}`\n", ex.command));
                if let Some(reason) = &ex.reason {
                    out.push_str(&format!("  {}\n", render_reason_text(spec, reason)));
                }
                out.push('\n');
            }
        }
    }

    out
}

fn scd_flag_synopsis(flag: &FlagDef) -> String {
    let mut parts = Vec::new();

    if let Some(s) = &flag.short {
        if flag.kind == "switch" {
            parts.push(format!("-{}", s));
        } else {
            parts.push(format!(
                "-{} <{}>",
                s,
                flag.value_name.as_deref().unwrap_or("VALUE")
            ));
        }
    }

    if flag.kind == "switch" {
        parts.push(format!("--{}", flag.long));
    } else {
        parts.push(format!(
            "--{} <{}>",
            flag.long,
            flag.value_name.as_deref().unwrap_or("VALUE")
        ));
    }

    parts.join(", ")
}

/* =========================
 * src/generated/error_text.rs
 * ========================= */

fn render_error_text_rs(spec: &Spec) -> String {
    let mut out = String::new();

    out.push_str("// @generated by xtgeoip-docgen; DO NOT EDIT.\n");
    out.push_str("// Regenerate with: cargo run --bin xtgeoip-docgen\n\n");

    out.push_str("pub fn error_reason_text(code: &str, args: &[(&str, &str)]) -> Option<String> {\n");
    out.push_str("    let map: std::collections::BTreeMap<&str, &str> = args.iter().copied().collect();\n");
    out.push_str("    match code {\n");

    if let Some(reasons) = &spec.reasons {
        for r in reasons {
            let rendered = render_runtime_template_expr(&r.summary, &r.detail);
            out.push_str(&format!(
                "        {} => Some({}),\n",
                rust_string_lit(&r.code),
                rendered
            ));
        }
    }

    out.push_str("        _ => None,\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

fn render_runtime_template_expr(summary: &str, detail: &str) -> String {
    let summary_expr = runtime_subst_expr(summary);
    let detail_expr = runtime_subst_expr(detail);

    format!(
        "{{ let summary = {}; let detail = {}; if detail.trim().is_empty() {{ summary }} else {{ format!(\"{{}} {{}}\", summary, detail) }} }}",
        summary_expr, detail_expr
    )
}

fn runtime_subst_expr(template: &str) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut pieces: Vec<String> = Vec::new();

    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '}') {
                let end = i + 1 + end;
                let key: String = chars[i + 1..end].iter().collect();
                pieces.push(format!(
                    "map.get({k}).copied().unwrap_or(concat!(\"{{\", {k}, \"}}\")).to_string()",
                    k = rust_string_lit(&key)
                ));
                i = end + 1;
                continue;
            }
        }

        let start = i;
        while i < chars.len() {
            if chars[i] == '{' {
                break;
            }
            i += 1;
        }
        let literal: String = chars[start..i].iter().collect();
        if !literal.is_empty() {
            pieces.push(format!("{}.to_string()", rust_string_lit(&literal)));
        }
    }

    if pieces.is_empty() {
        "\"\".to_string()".to_string()
    } else {
        format!("vec![{}].concat()", pieces.join(", "))
    }
}

/* =========================
 * src/generated/cli_matrix.rs
 * ========================= */

fn render_cli_matrix_rs(spec: &Spec) -> String {
    let mut out = String::new();

    out.push_str("// @generated by xtgeoip-docgen; DO NOT EDIT.\n");
    out.push_str("// Regenerate with: cargo run --bin xtgeoip-docgen\n\n");

    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    out.push_str("pub struct CommandMatrix {\n");
    out.push_str("    pub name: &'static str,\n");
    out.push_str("    pub allowed_flags: &'static [&'static str],\n");
    out.push_str("}\n\n");

    out.push_str("pub const GLOBAL_FLAGS: &[&str] = &[\n");
    for f in spec.flags.iter().filter(|f| f.scope == "global") {
        out.push_str(&format!("    {},\n", rust_string_lit(&f.long)));
    }
    out.push_str("];\n\n");

    out.push_str("pub const COMMANDS: &[CommandMatrix] = &[\n");
    for cmd in &spec.commands {
        out.push_str("    CommandMatrix {\n");
        out.push_str(&format!("        name: {},\n", rust_string_lit(&cmd.name)));
        out.push_str("        allowed_flags: &[\n");
        for af in &cmd.allowed_flags {
            out.push_str(&format!("            {},\n", rust_string_lit(af)));
        }
        out.push_str("        ],\n");
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");

    out.push_str("pub fn command_allowed_flags(cmd: &str) -> Option<&'static [&'static str]> {\n");
    out.push_str("    COMMANDS.iter().find(|c| c.name == cmd).map(|c| c.allowed_flags)\n");
    out.push_str("}\n\n");

    out.push_str("#[cfg(test)]\n");
    out.push_str("mod tests {\n");
    out.push_str("    use super::*;\n\n");

    out.push_str("    #[test]\n");
    out.push_str("    fn every_command_is_unique() {\n");
    out.push_str("        let mut seen = std::collections::BTreeSet::new();\n");
    out.push_str("        for c in COMMANDS {\n");
    out.push_str("            assert!(seen.insert(c.name), \"duplicate command in matrix: {}\", c.name);\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    out.push_str("    #[test]\n");
    out.push_str("    fn no_duplicate_flags_per_command() {\n");
    out.push_str("        for c in COMMANDS {\n");
    out.push_str("            let mut seen = std::collections::BTreeSet::new();\n");
    out.push_str("            for f in c.allowed_flags {\n");
    out.push_str("                assert!(seen.insert(*f), \"duplicate flag '{}' for command '{}'\", f, c.name);\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    out.push_str("    #[test]\n");
    out.push_str("    fn lookup_round_trips_known_commands() {\n");
    out.push_str("        for c in COMMANDS {\n");
    out.push_str("            let found = command_allowed_flags(c.name).expect(\"command missing from lookup\");\n");
    out.push_str("            assert_eq!(found, c.allowed_flags);\n");
    out.push_str("        }\n");
    out.push_str("    }\n");

    out.push_str("}\n");

    out
}

/* =========================
 * Utilities
 * ========================= */

fn err<T>(msg: impl Into<String>) -> Result<T, Box<dyn std::error::Error>> {
    Err(boxed_err(msg))
}

fn boxed_err(msg: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(SimpleError(msg.into()))
}

#[derive(Debug)]
struct SimpleError(String);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SimpleError {}
