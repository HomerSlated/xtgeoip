//! xtgeoip-docgen v3 (fixed)
//! Restores missing outcome data across all generators

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

    #[serde(skip_serializing_if = "Option::is_none")]
    maps_to: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let yaml_str = fs::read_to_string("docs/spec/cli.yaml")?;
    let spec: Spec = serde_yaml::from_str(&yaml_str)?;

    let validation = validate_spec(&spec);

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

    if let Err(e) = validation {
        eprintln!("\n⚠️ Validation failed:\n{}\n", e);
        std::process::exit(1);
    }

    Ok(())
}

/* ------------------------- VALIDATION ------------------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used = BTreeSet::new();
    let error_cases = spec.error_cases.as_ref();

    let mut check = |examples: &[Example]| -> anyhow::Result<()> {
        for ex in examples {
            if !ex.valid {
                let m = ex.maps_to.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Missing maps_to in '{}'", ex.cmd)
                })?;

                if let Some(ec) = error_cases {
                    if !ec.contains_key(m) {
                        anyhow::bail!("Unknown error_case '{}'", m);
                    }
                }

                used.insert(m.clone());
            }
        }
        Ok(())
    };

    if let Some(c) = &spec.top_level {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => check(examples)?,
        }
    }

    for c in spec.commands.values() {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => check(examples)?,
        }
    }

    if let Some(p) = &spec.proof {
        if p.full_branch_coverage.unwrap_or(false) {
            if let Some(ec) = error_cases {
                for k in ec.keys() {
                    if !used.contains(k) {
                        anyhow::bail!("Error case '{}' defined but never used", k);
                    }
                }
            }
        }
    }

    Ok(())
}

/* ------------------------- CORE FIX ------------------------- */

fn resolve_outcome(spec: &Spec, ex: &Example) -> anyhow::Result<String> {
    if ex.valid {
        Ok(ex.outcome.clone().unwrap_or_default())
    } else if let Some(r) = &ex.reason {
        render_reason(spec, r)
    } else {
        Ok("(invalid)".into())
    }
}

fn render_reason(spec: &Spec, reason: &Reason) -> anyhow::Result<String> {
    let t = spec.reason_templates.get(&reason.code)
        .ok_or_else(|| anyhow::anyhow!("Unknown reason: {}", reason.code))?;

    let mut text = t.text.clone();

    if let Some(args) = &reason.args {
        for (k, v) in args {
            text = text.replace(&format!("{{{}}}", k), v);
        }
    }

    Ok(text)
}

/* ------------------------- GENERATORS ------------------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    let mut render = |examples: &[Example]| -> anyhow::Result<()> {
        for ex in examples {
            let o = resolve_outcome(spec, ex)?;
            out.push_str(&format!("- `{}` → {}", ex.cmd, o));

            if let Some(s) = ex.exit_status {
                out.push_str(&format!(" (exit {})", s));
            }

            out.push('\n');
        }
        out.push('\n');
        Ok(())
    };

    if let Some(c) = &spec.top_level {
        match c {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!("## top level\n{}\n\n", summary));
                render(examples)?;
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                out.push_str(&format!("## top level\n{}\n\n", summary));
                render(examples)?;
            }
        }
    }

    for (n, c) in &spec.commands {
        match c {
            CommandSpec::FlagCommand { summary, examples, .. }
            | CommandSpec::SelectorCommand { summary, examples, .. } => {
                out.push_str(&format!("## {}\n{}\n\n", n, summary));
                render(examples)?;
            }
        }
    }

    Ok(out)
}

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    let mut add = |examples: &[Example]| -> anyhow::Result<()> {
        for ex in examples {
            if ex.valid {
                let o = resolve_outcome(spec, ex)?;
                if !o.is_empty() {
                    out.push_str(&format!("- {}:\n\n`{}`\n\n", o, ex.cmd));
                }
            }
        }
        Ok(())
    };

    if let Some(c) = &spec.top_level {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples)?,
        }
    }

    for c in spec.commands.values() {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples)?,
        }
    }

    Ok(out)
}

fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!("{} - {}\n", spec.meta.program, spec.meta.summary))
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
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\npub const CLI_MATRIX: &[CliExample] = &[\n".to_string();

    let mut add = |examples: &[Example]| -> anyhow::Result<()> {
        for ex in examples {
            let o = resolve_outcome(spec, ex)?;
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, o.replace('"', "\\\"")
            ));
        }
        Ok(())
    };

    if let Some(c) = &spec.top_level {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples)?,
        }
    }

    for c in spec.commands.values() {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples)?,
        }
    }

    out.push_str("];\n");
    Ok(out)
}

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut v = Vec::new();

    let mut add = |examples: &[Example]| {
        for ex in examples {
            v.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.into(),
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
            });
        }
    };

    if let Some(c) = &spec.top_level {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for c in spec.commands.values() {
        match c {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    Ok(serde_yaml::to_string(&v)?)
}

fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    Ok(format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n\
         .SH NAME\n\
         {}\n\t{}\n\
         .SH DESCRIPTION\n{}\n",
        spec.meta.program.to_uppercase(),
        spec.version,
        spec.meta.program,
        spec.meta.summary,
        spec.meta.summary
    ))
}
