//! xtgeoip-docgen v3 (INSPECT MODE)
//! Instead of failing on schema mismatch, prints actual YAML structure.

use std::{collections::BTreeMap, fs};

use serde_yaml::Value;

fn main() -> anyhow::Result<()> {
    let path = "docs/spec/cli.yaml";

    println!("📄 Loading YAML from: {}", path);

    let raw = fs::read_to_string(path)?;
    println!("📦 file size: {} bytes", raw.len());

    let yaml: Value = serde_yaml::from_str(&raw)?;

    println!("\n🔎 FULL YAML STRUCTURE:\n{:#?}\n", yaml);

    inspect_root(&yaml);

    Ok(())
}

/* ------------------------- STRUCTURE INSPECTOR ------------------------- */

fn inspect_root(yaml: &Value) {
    println!("🧠 ROOT INSPECTION");

    match yaml {
        Value::Mapping(map) => {
            for (k, v) in map {
                let key = k.as_str().unwrap_or("<non-string key>");

                println!("\n📌 KEY: {}", key);

                match key {
                    "meta" => inspect_meta(v),
                    "commands" => inspect_commands(v),
                    "top_level" => println!("   (top_level present)"),
                    "flags" => inspect_flags(v),
                    other => {
                        println!("   ⚠️ Unknown section: {}", other);
                        println!("   Value: {:#?}", v);
                    }
                }
            }
        }
        _ => println!("❌ Root is not a mapping"),
    }
}

/* ------------------------- META ------------------------- */

fn inspect_meta(v: &Value) {
    println!("   🔍 META SECTION");

    if let Value::Mapping(map) = v {
        for (k, v) in map {
            let key = k.as_str().unwrap_or("<non-string>");
            println!("   - {}: {}", key, short(v));

            if key == "summary" {
                println!("     ✅ FOUND meta.summary");
            }
        }

        if !map.contains_key(&Value::String("summary".to_string())) {
            println!("     ❌ meta.summary MISSING (or not string key match)");
        }
    }
}

/* ------------------------- COMMANDS ------------------------- */

fn inspect_commands(v: &Value) {
    println!("   🔍 COMMANDS SECTION");

    if let Value::Mapping(cmds) = v {
        for (name, cmd) in cmds {
            println!("\n   🧩 COMMAND: {}", name.as_str().unwrap_or("?"));

            if let Value::Mapping(cmap) = cmd {
                for (k, v) in cmap {
                    let key = k.as_str().unwrap_or("<non-string>");
                    println!("     - {}: {}", key, short(v));
                }

                if let Some(kind) = cmap.get(&Value::String("kind".into())) {
                    println!("     🧠 kind = {}", short(kind));
                }
            }
        }
    }
}

/* ------------------------- FLAGS ------------------------- */

fn inspect_flags(v: &Value) {
    println!("   🔍 FLAGS SECTION");

    if let Value::Mapping(flags) = v {
        for (k, v) in flags {
            println!(
                "   - {} => {}",
                k.as_str().unwrap_or("?"),
                short(v)
            );
        }
    }
}

/* ------------------------- HELPERS ------------------------- */

fn short(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{}\"", s),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Sequence(_) => "[...]".into(),
        Value::Mapping(_) => "{...}".into(),
        Value::Null => "null".into(),
        other => format!("{:?}", other),
    }
}
