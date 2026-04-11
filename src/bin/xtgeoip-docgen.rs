//! xtgeoip-docgen v3
//! Generates documentation and test matrices from cli.yaml (B-mode strict validation)

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

/* ------------------------- MAIN ------------------------- */

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
    fs::write("docs/generated/testcases.yaml", generate_testcases_yaml(&spec)?)?;

    println!("Docs generated successfully.");
    Ok(())
}

/* ------------------------- CORE HELPERS ------------------------- */

fn resolve_outcome(spec: &Spec, ex: &Example) -> String {
    if ex.valid {
        ex.outcome.clone().unwrap_or_else(|| "OK".into())
    } else if let Some(r) = &ex.reason {
        r.code.clone()
    } else {
        "(invalid)".into()
    }
}

fn render_reason(spec: &Spec, reason: &Reason) -> anyhow::Result<String> {
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

/* ------------------------- VALIDATION ------------------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let error_cases = spec.error_cases.as_ref();

    let mut validate =
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
                                "maps_to '{}' not found in error_cases (example '{}')",
                                maps_to,
                                ex.cmd
                            );
                        }
                    }
                }
            }
            Ok(())
        };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                validate(examples, "top_level")?;
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => {
                validate(examples, name)?;
            }
        }
    }

    Ok(())
}

/* ------------------------- USAGE ------------------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!(
        "# {}\n\n{}\n\n",
        spec.meta.program, spec.meta.summary
    );

    let mut render = |name: &str, summary: &str, usage: Option<&str>, exs: &[Example]| {
        out.push_str(&format!("## {}\n{}\n", name, summary));
        if let Some(u) = usage {
            out.push_str(&format!("Usage: {}\n", u));
        }
        out.push('\n');

        for ex in exs {
            let outcome = resolve_outcome(spec, ex);
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
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render("top level", summary, None, examples);
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                render("top level", summary, Some(usage), examples);
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render(name, summary, None, examples);
            }
            CommandSpec::SelectorCommand { summary, usage, examples, .. } => {
                render(name, summary, Some(usage), examples);
            }
        }
    }

    Ok(out)
}

/* ------------------------- TLDR (FIXED) ------------------------- */

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out = format!(
        "# {}\n\n> {}\n\n",
        spec.meta.program, spec.meta.summary
    );

    let mut render = |name: &str, summary: &str, exs: &[Example]| {
        let mut header = false;

        for ex in exs {
            if ex.valid {
                if !header {
                    out.push_str(&format!("## {}\n{}\n\n", name, summary));
                    header = true;
                }

                let outcome = ex.outcome.clone().unwrap_or_else(|| "OK".into());

                out.push_str(&format!("- {}\n\n  `{}`\n\n", outcome, ex.cmd));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render("xtgeoip", summary, examples);
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                render("xtgeoip", summary, examples);
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render(name, summary, examples);
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                render(name, summary, examples);
            }
        }
    }

    Ok(out)
}

/* ------------------------- SCD ------------------------- */

fn generate_scd(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, allowed_flags, .. } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nFlags: {:?}\n\n",
                    summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { summary, usage, positional, constraints, .. } => {
                out.push_str(&format!(
                    "Command: top_level\nSummary: {}\nUsage: {}\nPositional: {:?}\nConstraints: {:?}\n\n",
                    summary, usage, positional.name, constraints
                ));
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, allowed_flags, .. } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nFlags: {:?}\n\n",
                    name, summary, allowed_flags
                ));
            }
            CommandSpec::SelectorCommand { summary, usage, positional, constraints, .. } => {
                out.push_str(&format!(
                    "Command: {}\nSummary: {}\nUsage: {}\nPositional: {:?}\nConstraints: {:?}\n\n",
                    name, summary, usage, positional.name, constraints
                ));
            }
        }
    }

    Ok(out)
}

/* ------------------------- ERROR TEXT ------------------------- */

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

/* ------------------------- CLI MATRIX ------------------------- */

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub outcome: &'static str }\n".to_string();
    out.push_str("pub const CLI_MATRIX: &[CliExample] = &[\n");

    let mut add = |exs: &[Example]| {
        for ex in exs {
            let outcome = resolve_outcome(spec, ex);
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    out.push_str("];\n");
    Ok(out)
}

/* ------------------------- TESTCASES ------------------------- */

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    let mut add = |exs: &[Example]| {
        for ex in exs {
            testcases.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.to_string(),
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
            });
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    for cmd in spec.commands.values() {
        match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => add(examples),
        }
    }

    Ok(serde_yaml::to_string(&testcases)?)
}

/* ------------------------- MANPAGE ------------------------- */

fn generate_manpage(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::new();

    out.push_str(&format!(
        ".TH {} 1 \"\" \"xtgeoip {}\" \"User Commands\"\n",
        spec.meta.program.to_uppercase(),
        spec.version
    ));

    out.push_str(&format!(
        ".SH NAME\n{}\n{}\n\n",
        spec.meta.program, spec.meta.summary
    ));

    out.push_str(".SH COMMANDS\n");

    let mut render_cmd = |name: &str, summary: &str, exs: &[Example]| {
        out.push_str(&format!(".SS {}\n{}\n\n", name, summary));

        for ex in exs {
            let outcome = resolve_outcome(spec, ex);

            out.push_str(&format!(".TP\n\\fB{}\\fR\n", ex.cmd));
            out.push_str(&format!("{}\n", outcome));

            if let Some(s) = ex.exit_status {
                out.push_str(&format!("Exit status: {}\n", s));
            }
            if let Some(n) = &ex.note {
                out.push_str(&format!("{}\n", n));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render_cmd("top level", summary, examples);
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                render_cmd("top level", summary, examples);
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand { summary, examples, .. } => {
                render_cmd(name, summary, examples);
            }
            CommandSpec::SelectorCommand { summary, examples, .. } => {
                render_cmd(name, summary, examples);
            }
        }
    }

    Ok(out)
}
