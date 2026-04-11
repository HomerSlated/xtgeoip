//! structure-errors
//! Generates structured error enum + formatter from cli.yaml

use std::{collections::BTreeMap, fs};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ErrorSpec {
    reason_templates: BTreeMap<String, ReasonTemplate>,
    error_cases: BTreeMap<String, ErrorCase>,
}

#[derive(Debug, Deserialize)]
struct ReasonTemplate {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ErrorCase {
    maps_to: String,
}

fn main() -> anyhow::Result<()> {
    let yaml = fs::read_to_string("docs/spec/cli.yaml")?;
    let spec: ErrorSpec = serde_yaml::from_str(&yaml)?;

    let output = generate_errors_rs(&spec)?;

    fs::create_dir_all("src/generated")?;
    fs::write("src/generated/errors.rs.in", output)?;

    println!("Generated src/generated/errors.rs.in");

    Ok(())
}

/// Generate full Rust module
fn generate_errors_rs(spec: &ErrorSpec) -> anyhow::Result<String> {
    let mut out = String::new();

    out.push_str("// Auto-generated from cli.yaml — DO NOT EDIT\n\n");

    // --- Enum ---
    out.push_str("#[derive(Debug, Clone)]\n");
    out.push_str("pub enum CliError {\n");

    for (case_name, case) in &spec.error_cases {
        let template =
            spec.reason_templates.get(&case.maps_to).ok_or_else(|| {
                anyhow::anyhow!(
                    "error_case '{}' refers to unknown template '{}'",
                    case_name,
                    case.maps_to
                )
            })?;

        let fields = extract_placeholders(&template.text);

        if fields.is_empty() {
            out.push_str(&format!("    {},\n", to_camel(case_name)));
        } else {
            out.push_str(&format!("    {} {{ ", to_camel(case_name)));
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("{}: String", f));
            }
            out.push_str(" },\n");
        }
    }

    out.push_str("}\n\n");

    // --- impl block ---
    out.push_str("impl CliError {\n");
    out.push_str("    pub fn render(&self) -> String {\n");
    out.push_str("        match self {\n");

    for (case_name, case) in &spec.error_cases {
        let variant = to_camel(case_name);
        let template = &spec.reason_templates[&case.maps_to].text;
        let fields = extract_placeholders(template);

        if fields.is_empty() {
            out.push_str(&format!(
                "            CliError::{} => \"{}\".to_string(),\n",
                variant, template
            ));
        } else {
            out.push_str(&format!(
                "            CliError::{} {{ {} }} => {{\n",
                variant,
                fields.join(", ")
            ));

            out.push_str(&format!(
                "                let mut s = \"{}\".to_string();\n",
                template
            ));

            for f in &fields {
                out.push_str(&format!(
                    "                s = s.replace(\"{{{}}}\", {});\n",
                    f, f
                ));
            }

            out.push_str("                s\n");
            out.push_str("            }\n");
        }
    }

    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    Ok(out)
}

/// Extract `{placeholders}` from template
fn extract_placeholders(template: &str) -> Vec<String> {
    let mut fields = Vec::new();

    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == '}' {
                    break;
                }
                name.push(next);
            }

            if !name.is_empty() && !fields.contains(&name) {
                fields.push(name);
            }
        }
    }

    fields
}

/// Convert snake_case → CamelCase
fn to_camel(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + c.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}
