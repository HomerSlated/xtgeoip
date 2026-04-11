//! xtgeoip-docgen v3
//! Generates documentation and test matrices from cli.yaml (B-mode strict validation)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DocgenSpec {
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
    use std::path::PathBuf;

    let yaml_path = PathBuf::from("docs/spec/cli.yaml")
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to resolve cli.yaml path: {e}"))?;

    eprintln!("📄 xtgeoip-docgen loading spec from:");
    eprintln!("   → {}", yaml_path.display());

    let yaml_str = fs::read_to_string(&yaml_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", yaml_path.display()))?;

    eprintln!("📦 file size: {} bytes", yaml_str.len());

    eprintln!("📖 file preview (first 200 chars):\n{}\n",
        &yaml_str.chars().take(200).collect::<String>()
    );

    let spec = load_spec(&yaml_str, &yaml_path)?;

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

/* ------------------------- NEW: EXPLICIT YAML LOADER ------------------------- */

fn load_spec(yaml_str: &str, path: &std::path::Path) -> anyhow::Result<DocgenSpec> {
    serde_yaml::from_str::<DocgenSpec>(yaml_str).map_err(|e| {
        let location = e
            .location()
            .map(|l| format!("line {}, column {}", l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let path_display = path.display();

        let kind = match e.to_string().contains("missing field") {
            true => "Missing field error",
            false => "Serde deserialization error",
        };

        let msg = format!(
            "\n❌ xtgeoip-docgen YAML deserialization failed\n\
             \n📍 File: {}\n\
             📍 Location: {}\n\
             🔎 Error type: {}\n\
             🔎 Raw serde error: {}\n\
             \n🧠 Struct target: DocgenSpec\n\
             🧠 Likely causes:\n\
             - YAML structure mismatch (meta/version/top-level split)\n\
             - Wrong file being loaded\n\
             - Old compiled binary still running\n\
             - Hidden indentation or tab issue\n\
             \n📦 Debug hints:\n\
             - Check: `rg \"meta:\" -n {}`\n\
             - Check: `head -50 {}`\n",
            path_display,
            location,
            kind,
            e,
            path_display,
            path_display
        );

        anyhow::anyhow!(msg)
    })
}

/* ------------------------- STRICT B MODE VALIDATION ------------------------- */

fn validate_spec(spec: &DocgenSpec) -> anyhow::Result<()> {
    let mut used_error_cases = BTreeSet::new();
    let error_cases = spec.error_cases.as_ref();

    let mut validate_examples =
        |examples: &[Example], scope: &str| -> anyhow::Result<()> {
            for ex in examples {
                if let Some(reason) = &ex.reason {
                    if !spec.reason_templates.contains_key(&reason.code) {
                        anyhow::bail!(
                            "Unknown reason code '{}' in {} example '{}'",
                            reason.code,
                            scope,
                            ex.cmd
                        );
                    }
                }

                if !ex.valid {
                    let maps_to = ex.maps_to.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Missing maps_to in invalid example '{}'",
                            ex.cmd
                        )
                    })?;

                    if let Some(ec) = error_cases {
                        if !ec.contains_key(maps_to) {
                            anyhow::bail!(
                                "maps_to '{}' not found in error_cases (example: '{}')",
                                maps_to,
                                ex.cmd
                            );
                        }
                    }

                    used_error_cases.insert(maps_to.clone());
                }
            }
            Ok(())
        };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                if summary.trim().is_empty() {
                    anyhow::bail!("top_level FlagCommand missing summary");
                }
                validate_examples(examples, "top_level")?;
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                if summary.trim().is_empty() {
                    anyhow::bail!("top_level SelectorCommand missing summary");
                }
                validate_examples(examples, "top_level")?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                if summary.trim().is_empty() {
                    anyhow::bail!("command '{}' missing summary", name);
                }
                validate_examples(examples, name)?;
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                if summary.trim().is_empty() {
                    anyhow::bail!("command '{}' missing summary", name);
                }
                validate_examples(examples, name)?;
            }
        }
    }

    if let Some(proof) = &spec.proof {
        if proof.full_branch_coverage.unwrap_or(false) {
            if let Some(ec) = error_cases {
                for key in ec.keys() {
                    if !used_error_cases.contains(key) {
                        anyhow::bail!("Error case '{}' defined but never used", key);
                    }
                }
            }
        }
    }

    Ok(())
}

/* ------------------------- DOCUMENTATION GENERATORS ------------------------- */

fn render_reason(spec: &DocgenSpec, reason: &Reason) -> anyhow::Result<String> {
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

fn generate_usage_md(spec: &DocgenSpec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    fn render(out: &mut String, spec: &DocgenSpec, exs: &[Example]) -> anyhow::Result<()> {
        for ex in exs {
            let outcome = if ex.valid {
                ex.outcome.clone().unwrap_or_default()
            } else if let Some(r) = &ex.reason {
                render_reason(spec, r)?
            } else {
                "(invalid)".to_string()
            };

            out.push_str(&format!("- `{}` → {}", ex.cmd, outcome));

            if let Some(s) = ex.exit_status {
                out.push_str(&format!(" (exit {})", s));
            }
            if let Some(n) = &ex.note {
                out.push_str(&format!(" — {}", n));
            }

            out.push('\n');
        }
        out.push('\n');
        Ok(())
    }

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!("## top level\n{}\n\n", summary));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!("## top level\n{}\nUsage: {}\n\n", summary, usage));
                render(&mut out, spec, examples)?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                out.push_str(&format!("## {}\n{}\n\n", name, summary));
                render(&mut out, spec, examples)?;
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                out.push_str(&format!("## {}\n{}\nUsage: {}\n\n", name, summary, usage));
                render(&mut out, spec, examples)?;
            }
        }
    }

    Ok(out)
}

/* ------------------------- ALL OTHER FUNCTIONS UNCHANGED ------------------------- */

fn generate_tldr_md(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
fn generate_scd(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
fn generate_error_text_rs(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
fn generate_cli_matrix_rs(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
fn generate_testcases_yaml(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
fn generate_manpage(spec: &DocgenSpec) -> anyhow::Result<String> { /* unchanged */ unimplemented!() }
