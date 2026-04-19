//! xtgeoip-docgen v3.1 (stable, schema-safe)

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub meta: Meta,
    pub version: String,

    #[serde(default)]
    pub proof: Option<Proof>,

    #[serde(default)]
    pub flags: BTreeMap<String, FlagDef>,

    #[serde(default)]
    pub error_cases: Option<BTreeMap<String, ErrorCase>>,

    #[serde(default)]
    pub top_level: Option<CommandSpec>,

    #[serde(default)]
    pub commands: BTreeMap<String, CommandSpec>,

    #[serde(default)]
    pub reason_templates: BTreeMap<String, ReasonTemplate>,
}

#[derive(Debug, Deserialize)]
pub struct FlagDef {
    pub long: String,
    pub kind: String,
    pub summary: String,
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

#[derive(Debug, Deserialize)]
struct ManpageTemplate {
    description: String,
    commands: String,
    options: String,
    execution_order: String,
    legacy_mode: String,
    configuration: String,
    files: String,
    see_also: String,
    authors: String,
}

#[derive(Debug, Serialize)]
struct Testcase {
    case_id: Option<String>,
    key: String,
    cmd: String,
    maps_to: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let yaml_str = fs::read_to_string("docs/spec/cli.yaml")?;
    let spec: Spec = serde_yaml::from_str(&yaml_str)?;

    validate_spec(&spec)?;

    let toml_str = fs::read_to_string("docs/spec/manpage-template.toml")?;
    let tmpl: ManpageTemplate = toml::from_str(&toml_str)?;

    fs::create_dir_all("docs/generated")?;
    fs::create_dir_all("src/generated")?;

    fs::write("docs/generated/usage.md", generate_usage_md(&spec)?)?;
    fs::write("docs/generated/tldr.md", generate_tldr_md(&spec)?)?;
    fs::write("docs/generated/xtgeoip.1", generate_manpage(&spec, &tmpl)?)?;
    fs::write(
        "src/generated/error_text.rs",
        generate_error_text_rs(&spec)?,
    )?;
    fs::write(
        "src/generated/cli_matrix.rs",
        generate_cli_matrix_rs(&spec)?,
    )?;
    fs::write(
        "docs/generated/testcases.yaml",
        generate_testcases_yaml(&spec)?,
    )?;

    println!("Docs generated successfully.");
    Ok(())
}

/* ---------------- VALIDATION ---------------- */

fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
    let mut used_error_cases: BTreeSet<String> = BTreeSet::new();

    let error_cases = spec.error_cases.as_ref();

    let check = |scope: &str,
                 ex: &Example,
                 used: &mut BTreeSet<String>|
     -> anyhow::Result<()> {
        if let Some(reason) = &ex.reason
            && !spec.reason_templates.contains_key(&reason.code) {
                anyhow::bail!(
                    "Unknown reason code {} in {}",
                    reason.code,
                    scope
                );
            }

        if !ex.valid {
            let maps_to = ex.maps_to.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Missing maps_to in invalid example {}", ex.cmd)
            })?;

            if let Some(ec) = error_cases
                && !ec.contains_key(maps_to) {
                    anyhow::bail!("Unknown error case {}", maps_to);
                }

            used.insert(maps_to.clone());
        }

        Ok(())
    };

    let visit = |name: &str,
                 cmd: &CommandSpec,
                 used: &mut BTreeSet<String>|
     -> anyhow::Result<()> {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };

        for ex in exs {
            check(name, ex, used)?;
        }

        Ok(())
    };

    // IMPORTANT: top_level is a command too
    if let Some(cmd) = &spec.top_level {
        visit("top_level", cmd, &mut used_error_cases)?;
    }

    // These are ALL real commands in your YAML
    if let Some(cmd) = spec.commands.get("fetch") {
        visit("fetch", cmd, &mut used_error_cases)?;
    }

    if let Some(cmd) = spec.commands.get("build") {
        visit("build", cmd, &mut used_error_cases)?;
    }

    if let Some(cmd) = spec.commands.get("run") {
        visit("run", cmd, &mut used_error_cases)?;
    }

    if let Some(cmd) = spec.commands.get("conf") {
        visit("conf", cmd, &mut used_error_cases)?;
    }

    // FULL COVERAGE CHECK
    if spec
        .proof
        .as_ref()
        .and_then(|p| p.full_branch_coverage)
        .unwrap_or(false)
    {
        let mut unused = Vec::new();

        if let Some(ec) = error_cases {
            for (key, case) in ec {
                if !used_error_cases.contains(&case.maps_to) {
                    unused.push(key.clone());
                }
            }
        }

        if !unused.is_empty() {
            anyhow::bail!(
                "Unused error cases (no invalid example maps_to reference): \
                 {:?}",
                unused
            );
        }
    }

    Ok(())
}

/* ---------------- OUTCOME ---------------- */

fn resolve_outcome(spec: &Spec, ex: &Example) -> String {
    if ex.valid {
        return ex.outcome.clone().unwrap_or_else(|| "OK".into());
    }

    if let Some(reason) = &ex.reason
        && let Some(t) = spec.reason_templates.get(&reason.code) {
            let mut text = t.text.clone();
            if let Some(args) = &reason.args {
                for (k, v) in args {
                    text = text.replace(&format!("{{{}}}", k), v);
                }
            }
            return text;
        }

    "ERROR".into()
}

/* ---------------- USAGE ---------------- */

fn generate_usage_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n{}\n\n", spec.meta.program, spec.meta.summary);

    let render = |out: &mut String,
                  spec: &Spec,
                  exs: &[Example],
                  title: &str,
                  extra: Option<&str>| {
        out.push_str(&format!("## {}\n", title));

        if let Some(e) = extra {
            out.push_str(e);
            out.push('\n');
        }

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
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(summary));
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, "top level", Some(usage));
            }
        }
    }

    for (name, cmd) in &spec.commands {
        match cmd {
            CommandSpec::FlagCommand {
                summary, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(summary));
            }
            CommandSpec::SelectorCommand {
                usage, examples, ..
            } => {
                render(&mut out, spec, examples, name, Some(usage));
            }
        }
    }

    Ok(out)
}

/* ---------------- TLDR ---------------- */

fn generate_tldr_md(spec: &Spec) -> anyhow::Result<String> {
    let mut out =
        format!("# {}\n\n> {}\n\n", spec.meta.program, spec.meta.summary);

    let mut add = |exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                out.push_str(&format!(
                    "- {}:\n\n`{}`\n\n",
                    ex.outcome.clone().unwrap_or_default(),
                    ex.cmd
                ));
            }
        }
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    Ok(out)
}

/* ---------------- ERROR TEXT ---------------- */

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

/* ---------------- CLI MATRIX ---------------- */

fn generate_cli_matrix_rs(spec: &Spec) -> anyhow::Result<String> {
    let mut out = String::from(
        "pub struct CliExample { pub cmd: &'static str, pub valid: bool, pub \
         outcome: &'static str }\npub const CLI_MATRIX: &[CliExample] = &[\n",
    );

    let mut add = |exs: &[Example]| {
        for ex in exs {
            let outcome = resolve_outcome(spec, ex);
            out.push_str(&format!(
                "    CliExample {{ cmd: \"{}\", valid: {}, outcome: \"{}\" \
                 }},\n",
                ex.cmd, ex.valid, outcome
            ));
        }
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    out.push_str("];\n");
    Ok(out)
}

/* ---------------- TESTCASES ---------------- */

fn generate_testcases_yaml(spec: &Spec) -> anyhow::Result<String> {
    let mut testcases = Vec::new();

    let mut add = |exs: &[Example]| {
        for ex in exs {
            testcases.push(Testcase {
                case_id: ex.case_id.clone(),
                key: if ex.valid { "p" } else { "f" }.into(),
                cmd: ex.cmd.clone(),
                maps_to: ex.maps_to.clone(),
            });
        }
    };

    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        add(exs);
    }

    Ok(serde_yaml::to_string(&testcases)?)
}

/* ---------------- MANPAGE ---------------- */

fn generate_manpage(spec: &Spec, tmpl: &ManpageTemplate) -> anyhow::Result<String> {
    let prog = &spec.meta.program;
    let version = env!("CARGO_PKG_VERSION");
    let mut out = String::new();

    let push_section = |out: &mut String, heading: &str, body: &str| {
        out.push_str(&format!(".SH {}\n", heading));
        out.push_str(body.trim_end_matches('\n'));
        out.push('\n');
    };

    // Header
    out.push_str(&format!(
        ".TH {} 1 \"\" \"{} {}\" \"User Commands\"\n",
        prog.to_uppercase(),
        prog,
        version
    ));

    // NAME (from spec meta)
    push_section(&mut out, "NAME", &format!("{} \\- {}\n", prog, spec.meta.summary));

    // SYNOPSIS (from spec top_level flags + command names)
    out.push_str(".SH SYNOPSIS\n");
    if let Some(cmd) = &spec.top_level
        && let CommandSpec::FlagCommand { allowed_flags, .. } = cmd
    {
        let flags: String = allowed_flags
            .iter()
            .map(|f| format!("[\\fB\\-{}\\fR]", f))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(".B {}\n{}\n.br\n", prog, flags));
    }
    out.push_str(&format!(".B {}\n\\fIcommand\\fR [\\fIoptions\\fR]\n", prog));

    // DESCRIPTION, COMMANDS, OPTIONS, EXECUTION ORDER, LEGACY MODE,
    // CONFIGURATION from template
    push_section(&mut out, "DESCRIPTION", &tmpl.description);
    push_section(&mut out, "COMMANDS", &tmpl.commands);
    push_section(&mut out, "OPTIONS", &tmpl.options);
    push_section(&mut out, "EXECUTION ORDER", &tmpl.execution_order);
    push_section(&mut out, "LEGACY MODE", &tmpl.legacy_mode);
    push_section(&mut out, "CONFIGURATION", &tmpl.configuration);

    // EXAMPLES (from spec valid examples)
    out.push_str(".SH EXAMPLES\n");
    let emit_valid = |out: &mut String, exs: &[Example]| {
        for ex in exs {
            if ex.valid {
                let desc = ex.outcome.as_deref().unwrap_or("");
                out.push_str(&format!(".TP\n.B {}\n{}\n", ex.cmd, desc));
            }
        }
    };
    if let Some(cmd) = &spec.top_level {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs);
    }
    for cmd in spec.commands.values() {
        let exs = match cmd {
            CommandSpec::FlagCommand { examples, .. }
            | CommandSpec::SelectorCommand { examples, .. } => examples,
        };
        emit_valid(&mut out, exs);
    }

    // FILES, SEE ALSO, AUTHORS from template
    push_section(&mut out, "FILES", &tmpl.files);
    push_section(&mut out, "SEE ALSO", &tmpl.see_also);
    push_section(&mut out, "AUTHORS", &tmpl.authors);

    Ok(out)
}
