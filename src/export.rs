//! Portable exports. Foreign keys are proposals, never silently applied constraints.
use crate::domain::{Analysis, Schema};
use anyhow::{Context, Result};
use std::{collections::HashMap, fmt::Write as _, fs, path::Path};

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\0', "").replace('"', "\"\""))
}
fn single_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn mermaid_label(s: &str) -> String {
    // Mermaid text entities retain labels without allowing syntax/HTML injection.
    single_line(s)
        .replace('&', "#38;")
        .replace('"', "#34;")
        .replace('<', "#60;")
        .replace('>', "#62;")
}
fn safe_type(s: &str) -> &str {
    match s {
        "INTEGER" => "INTEGER",
        "REAL" => "REAL",
        "BOOLEAN" => "BOOLEAN",
        "DATE" => "DATE",
        _ => "TEXT",
    }
}

pub fn mermaid(schema: &Schema) -> String {
    let mut out = String::from(
        "erDiagram\n    direction LR\n    %% Inferred draft. Dashed relationships are candidates requiring review.\n",
    );
    let ids: HashMap<_, _> = schema
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), format!("E{}", i + 1)))
        .collect();
    for e in &schema.entities {
        let id = &ids[e.id.as_str()];
        writeln!(out, "    {id}[\"{}\"] {{", mermaid_label(&e.name)).unwrap();
        for (i, a) in e.attributes.iter().enumerate() {
            let fk = schema
                .relationships
                .iter()
                .any(|r| r.from_entity == e.id && r.from_attribute == a.name);
            let keys = match (a.primary_key, fk) {
                (true, true) => " PK, FK",
                (true, false) => " PK",
                (false, true) => " FK",
                _ => "",
            };
            writeln!(
                out,
                "        {} field_{}{} \"{}{}\"",
                safe_type(&a.data_type),
                i + 1,
                keys,
                mermaid_label(&a.name),
                if a.nullable { " (nullable)" } else { "" }
            )
            .unwrap();
        }
        out.push_str("    }\n");
    }
    for r in &schema.relationships {
        let (Some(from), Some(to)) = (
            ids.get(r.from_entity.as_str()),
            ids.get(r.to_entity.as_str()),
        ) else {
            continue;
        };
        let nullable = schema
            .entities
            .iter()
            .find(|e| e.id == r.from_entity)
            .and_then(|e| e.attributes.iter().find(|a| a.name == r.from_attribute))
            .is_some_and(|a| a.nullable);
        let left = if nullable { "|o" } else { "||" };
        let right = if r.cardinality == "one_to_one" {
            "o|"
        } else {
            "o{"
        };
        writeln!(
            out,
            "    {to} {left}..{right} {from} : \"{} to {} (candidate)\"",
            mermaid_label(&r.from_attribute),
            mermaid_label(&r.to_attribute)
        )
        .unwrap();
    }
    out
}

pub fn sql(schema: &Schema) -> String {
    let mut out = String::from(
        "-- StructureWeaver inferred SQLite schema draft. Review before applying.\n-- PKs are inferred from observed unique identifiers; FK candidates are comments only.\n\n",
    );
    let entities: HashMap<_, _> = schema.entities.iter().map(|e| (e.id.as_str(), e)).collect();
    for e in &schema.entities {
        if e.attributes.is_empty() {
            continue;
        }
        writeln!(
            out,
            "-- Source table: {} ({} observed records)",
            single_line(&e.table_id),
            e.row_count
        )
        .unwrap();
        writeln!(out, "CREATE TABLE {} (", quoted(&e.name)).unwrap();
        for (i, a) in e.attributes.iter().enumerate() {
            // INT avoids SQLite INTEGER PRIMARY KEY rowid/autoincrement semantics.
            let ty = if a.primary_key && a.data_type == "INTEGER" {
                "INT"
            } else {
                safe_type(&a.data_type)
            };
            write!(
                out,
                "    {} {}{}{}",
                quoted(&a.name),
                ty,
                if !a.nullable { " NOT NULL" } else { "" },
                if a.primary_key { " PRIMARY KEY" } else { "" }
            )
            .unwrap();
            out.push_str(if i + 1 < e.attributes.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        out.push_str(");\n");
        for r in schema
            .relationships
            .iter()
            .filter(|r| r.from_entity == e.id)
        {
            if let Some(target) = entities.get(r.to_entity.as_str()) {
                writeln!(
                    out,
                    "-- Candidate only: FOREIGN KEY ({}) REFERENCES {} ({});",
                    single_line(&quoted(&r.from_attribute)),
                    single_line(&quoted(&target.name)),
                    single_line(&quoted(&r.to_attribute))
                )
                .unwrap();
                writeln!(out, "-- Evidence: {}", single_line(&r.evidence)).unwrap();
            }
        }
        out.push('\n');
    }
    out
}

pub fn write_outputs(analysis: &Analysis, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("Cannot create output directory {}", dir.display()))?;
    fs::write(
        dir.join("analysis.json"),
        serde_json::to_vec_pretty(analysis)?,
    )?;
    fs::write(dir.join("schema.mmd"), mermaid(&analysis.schema))?;
    fs::write(dir.join("schema.sql"), sql(&analysis.schema))?;
    Ok(())
}
