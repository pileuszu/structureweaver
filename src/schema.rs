//! Evidence-based relational candidates. Structural headers are not foreign keys.
use crate::domain::*;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone)]
struct Datum {
    value: String,
    kind: CellKind,
}
struct Profile {
    values: Vec<Option<Datum>>,
}
struct Inferred {
    entity: Entity,
    profiles: Vec<Profile>,
}
struct GridView<'a> {
    grid: &'a Grid,
    cells: HashMap<(u32, u32), &'a Cell>,
}
impl<'a> GridView<'a> {
    fn new(grid: &'a Grid) -> Self {
        Self {
            grid,
            cells: grid.cells.iter().map(|c| ((c.row, c.col), c)).collect(),
        }
    }
    fn cell(&self, r: u32, c: u32) -> Option<&'a Cell> {
        self.cells.get(&(r, c)).copied()
    }
    fn resolved(&self, r: u32, c: u32) -> Option<&'a Cell> {
        if let Some(m) = self.grid.merges.iter().find(|m| m.contains(r, c)) {
            self.cell(m.top, m.left)
        } else {
            self.cell(r, c)
        }
    }
}
fn datum(cell: Option<&Cell>) -> Option<Datum> {
    cell.filter(|c| c.kind != CellKind::Empty && !c.value.trim().is_empty())
        .map(|c| Datum {
            value: c.value.clone(),
            kind: c.kind.clone(),
        })
}
fn source(page: &str, bounds: Bounds) -> SourceRef {
    SourceRef {
        page_id: page.into(),
        region: SourceRegion::Cells { bounds },
    }
}
fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}
fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    let base = clean_name(base);
    let base = if base.is_empty() {
        "unnamed".to_owned()
    } else {
        base
    };
    let mut candidate = base.clone();
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{base}_{n}");
        n += 1;
    }
    candidate
}
fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(c.to_lowercase());
            prev_lower = c.is_lowercase() || c.is_numeric();
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    out.trim_matches('_').to_owned()
}
fn singular(s: &str) -> String {
    let s = snake(s);
    if let Some(stem) = s.strip_suffix("ies") {
        format!("{stem}y")
    } else if s.ends_with('s') && !s.ends_with("ss") {
        s[..s.len() - 1].to_owned()
    } else {
        s
    }
}
fn data_type(values: &[Option<Datum>]) -> String {
    let nonempty: Vec<_> = values.iter().flatten().collect();
    if nonempty.is_empty() {
        return "TEXT".into();
    }
    if nonempty.iter().all(|d| d.kind == CellKind::Number) {
        if nonempty.iter().all(|d| d.value.parse::<i64>().is_ok()) {
            "INTEGER"
        } else {
            "REAL"
        }
    } else if nonempty.iter().all(|d| d.kind == CellKind::Boolean) {
        "BOOLEAN"
    } else if nonempty.iter().all(|d| d.kind == CellKind::Date) {
        "DATE"
    } else {
        "TEXT"
    }
    .into()
}
fn canonical(d: &Datum, ty: &str) -> String {
    match ty {
        "INTEGER" => d
            .value
            .parse::<i64>()
            .map(|v| v.to_string())
            .unwrap_or_else(|_| d.value.clone()),
        "REAL" => d
            .value
            .parse::<f64>()
            .map(|v| if v == 0.0 { "0".into() } else { v.to_string() })
            .unwrap_or_else(|_| d.value.clone()),
        "BOOLEAN" => d.value.to_lowercase(),
        _ => d.value.clone(),
    }
}
fn attribute(name: String, values: &[Option<Datum>], source: SourceRef) -> Attribute {
    let mut seen = HashSet::new();
    Attribute {
        name,
        data_type: data_type(values),
        nullable: values.is_empty() || values.iter().any(Option::is_none),
        primary_key: false,
        source,
        sample_values: values
            .iter()
            .flatten()
            .filter_map(|v| {
                if seen.insert(v.value.clone()) {
                    Some(v.value.clone())
                } else {
                    None
                }
            })
            .take(5)
            .collect(),
    }
}
fn header_path(view: &GridView<'_>, table: &Table, col: u32) -> Vec<String> {
    let Some(h) = table.column_header else {
        return vec![];
    };
    let mut labels = Vec::new();
    let mut anchors = HashSet::new();
    for row in h.top..=h.bottom {
        if let Some(c) = view.resolved(row, col)
            && !c.value.trim().is_empty()
            && anchors.insert((c.row, c.col))
        {
            labels.push(c.value.clone());
        }
    }
    labels
}
fn record_table(view: &GridView<'_>, table: &Table) -> Inferred {
    let rows: Vec<_> = (table.data.top..=table.data.bottom)
        .filter(|&r| (table.data.left..=table.data.right).any(|c| datum(view.cell(r, c)).is_some()))
        .collect();
    let mut used = HashSet::new();
    let mut attrs = Vec::new();
    let mut profiles = Vec::new();
    for col in table.data.left..=table.data.right {
        let path = header_path(view, table, col);
        let label = if path.is_empty() {
            format!("column_{col}")
        } else {
            path.join(" / ")
        };
        let values: Vec<_> = rows.iter().map(|&r| datum(view.cell(r, col))).collect();
        attrs.push(attribute(
            unique_name(&label, &mut used),
            &values,
            source(
                &table.page_id,
                Bounds {
                    top: table.data.top,
                    bottom: table.data.bottom,
                    left: col,
                    right: col,
                },
            ),
        ));
        profiles.push(Profile { values });
    }
    Inferred {
        entity: Entity {
            id: format!("entity_{}", table.id),
            name: table.name.clone(),
            table_id: table.id.clone(),
            attributes: attrs,
            row_count: rows.len(),
        },
        profiles,
    }
}
fn row_dimension(view: &GridView<'_>, table: &Table, row: u32, col: u32) -> Option<Datum> {
    let raw = datum(view.resolved(row, col))?;
    let Some(node) = table
        .headers
        .iter()
        .filter(|n| n.axis == "row" && n.bounds.contains(row, col))
        .max_by_key(|n| n.bounds.top)
    else {
        return Some(raw);
    };
    let by_id: HashMap<_, _> = table.headers.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut path = vec![node.label.clone()];
    let mut seen = HashSet::from([node.id.as_str()]);
    let mut parent = node.parent_id.as_deref();
    while let Some(id) = parent {
        if !seen.insert(id) {
            break;
        }
        let Some(n) = by_id.get(id) else {
            break;
        };
        if n.axis != "row" {
            break;
        }
        path.push(n.label.clone());
        parent = n.parent_id.as_deref();
    }
    path.reverse();
    if path.len() > 1 {
        Some(Datum {
            value: path.join(" / "),
            kind: CellKind::Text,
        })
    } else {
        Some(raw)
    }
}
fn matrix_table(view: &GridView<'_>, table: &Table) -> Inferred {
    // Each cell is an observation: row dimensions + column hierarchy dimensions + value.
    let row_cols: Vec<_> = table
        .row_header
        .map(|b| (b.left..=b.right).collect())
        .unwrap_or_default();
    let header_rows: Vec<_> = table
        .column_header
        .map(|b| (b.top..=b.bottom).collect())
        .unwrap_or_default();
    let row_dims = row_cols.len().max(1);
    let col_dims = header_rows.len().max(1);
    let mut profiles: Vec<_> = (0..row_dims + col_dims + 1)
        .map(|_| Profile { values: Vec::new() })
        .collect();
    let active_rows: Vec<_> = (table.data.top..=table.data.bottom)
        .filter(|&r| {
            (table.data.left..=table.data.right).any(|c| datum(view.cell(r, c)).is_some())
                || row_cols
                    .iter()
                    .any(|&c| datum(view.resolved(r, c)).is_some())
        })
        .collect();
    for &row in &active_rows {
        for col in table.data.left..=table.data.right {
            if row_cols.is_empty() {
                profiles[0].values.push(Some(Datum {
                    value: row.to_string(),
                    kind: CellKind::Number,
                }));
            } else {
                for (i, &c) in row_cols.iter().enumerate() {
                    profiles[i].values.push(row_dimension(view, table, row, c));
                }
            }
            if header_rows.is_empty() {
                profiles[row_dims].values.push(Some(Datum {
                    value: col.to_string(),
                    kind: CellKind::Number,
                }));
            } else {
                for (i, &r) in header_rows.iter().enumerate() {
                    profiles[row_dims + i]
                        .values
                        .push(datum(view.resolved(r, col)));
                }
            }
            profiles[row_dims + col_dims]
                .values
                .push(datum(view.cell(row, col)));
        }
    }
    let mut attrs = Vec::new();
    let mut used = HashSet::new();
    for (i, profile) in profiles.iter().take(row_dims).enumerate() {
        let col = row_cols.get(i).copied().unwrap_or(table.data.left);
        let label = if row_cols.is_empty() {
            "source_row".to_owned()
        } else {
            format!("row_header_{}", i + 1)
        };
        attrs.push(attribute(
            unique_name(&label, &mut used),
            &profile.values,
            source(
                &table.page_id,
                Bounds {
                    top: table.data.top,
                    bottom: table.data.bottom,
                    left: col,
                    right: col,
                },
            ),
        ));
    }
    for (i, profile) in profiles.iter().skip(row_dims).take(col_dims).enumerate() {
        let row = header_rows.get(i).copied().unwrap_or(table.data.top);
        let label = if header_rows.is_empty() {
            "source_column".to_owned()
        } else {
            format!("column_header_{}", i + 1)
        };
        attrs.push(attribute(
            unique_name(&label, &mut used),
            &profile.values,
            source(
                &table.page_id,
                Bounds {
                    top: row,
                    bottom: row,
                    left: table.data.left,
                    right: table.data.right,
                },
            ),
        ));
    }
    attrs.push(attribute(
        unique_name("value", &mut used),
        &profiles[row_dims + col_dims].values,
        source(&table.page_id, table.data),
    ));
    let row_count = profiles.first().map_or(0, |p| p.values.len());
    Inferred {
        entity: Entity {
            id: format!("entity_{}", table.id),
            name: table.name.clone(),
            table_id: table.id.clone(),
            attributes: attrs,
            row_count,
        },
        profiles,
    }
}
fn key_score(name: &str, entity: &str) -> u8 {
    let n = snake(name);
    let entity = singular(entity);
    if n == "id" || n == format!("{entity}_id") {
        4
    } else if n == format!("{entity}_code") {
        3
    } else if n.ends_with("_id")
        || n.ends_with("_code")
        || n.ends_with("식별자")
        || n.ends_with("번호")
    {
        2
    } else if n == "code" {
        1
    } else {
        0
    }
}
fn assign_primary_key(item: &mut Inferred, warnings: &mut Vec<String>) {
    if item.entity.row_count < 2 {
        warnings.push(format!(
            "{}: fewer than two records; no primary key inferred.",
            item.entity.name
        ));
        return;
    }
    let mut candidates = Vec::new();
    for (i, (attr, p)) in item
        .entity
        .attributes
        .iter()
        .zip(&item.profiles)
        .enumerate()
    {
        let score = key_score(&attr.name, &item.entity.name);
        if score == 0
            || attr.nullable
            || !matches!(attr.data_type.as_str(), "TEXT" | "INTEGER")
            || p.values
                .iter()
                .flatten()
                .any(|d| !matches!(d.kind, CellKind::Text | CellKind::Number))
        {
            continue;
        }
        let keys: HashSet<_> = p
            .values
            .iter()
            .flatten()
            .map(|d| canonical(d, &attr.data_type))
            .collect();
        if keys.len() == item.entity.row_count {
            candidates.push((score, i));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    if let Some(&(score, index)) = candidates.first() {
        if candidates.get(1).is_some_and(|c| c.0 == score) {
            warnings.push(format!(
                "{}: several equally plausible unique ID columns; primary key needs review.",
                item.entity.name
            ));
        } else {
            item.entity.attributes[index].primary_key = true;
        }
    } else {
        warnings.push(format!(
            "{}: no complete unique identifier; primary key unresolved (no synthetic ID invented).",
            item.entity.name
        ));
    }
}
fn name_matches(source: &str, target_attr: &str, target_entity: &str) -> bool {
    let s = snake(source);
    let t = snake(target_attr);
    let e = singular(target_entity);
    // Generic `id` columns in unrelated tables are never linked just because values coincide.
    if s == "id" || s == "code" {
        return false;
    }
    (s == t && key_score(target_attr, target_entity) > 0)
        || s == format!("{e}_id")
        || s == format!("{e}_code")
}
fn relationships(items: &[Inferred], warnings: &mut Vec<String>) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    for (from_index, from) in items.iter().enumerate() {
        for (attr_index, attr) in from.entity.attributes.iter().enumerate() {
            let mut matches = Vec::new();
            for (to_index, to) in items.iter().enumerate() {
                if from_index == to_index {
                    continue;
                }
                for (target_index, target_attr) in to.entity.attributes.iter().enumerate() {
                    if !target_attr.primary_key
                        || attr.data_type != target_attr.data_type
                        || !name_matches(&attr.name, &target_attr.name, &to.entity.name)
                    {
                        continue;
                    }
                    let target_keys: HashSet<_> = to.profiles[target_index]
                        .values
                        .iter()
                        .flatten()
                        .map(|d| canonical(d, &target_attr.data_type))
                        .collect();
                    let source_values: Vec<_> = from.profiles[attr_index]
                        .values
                        .iter()
                        .flatten()
                        .map(|d| canonical(d, &attr.data_type))
                        .collect();
                    let distinct: HashSet<_> = source_values.iter().cloned().collect();
                    if distinct.len() < 2 || !distinct.is_subset(&target_keys) {
                        continue;
                    }
                    let unique = source_values.len() == distinct.len();
                    matches.push(Relationship {
                        from_entity:from.entity.id.clone(),from_attribute:attr.name.clone(),to_entity:to.entity.id.clone(),to_attribute:target_attr.name.clone(),
                        cardinality:if unique{"one_to_one"}else{"many_to_one"}.into(),confidence:if source_values.len()>=5{0.9}else{0.75},status:"candidate".into(),
                        evidence:format!("Compatible {} types and ID naming; all {} observed non-null values ({} distinct) reference the unique {}.{} key. {} Source key {}. Observed data only; requires review.",attr.data_type,source_values.len(),distinct.len(),to.entity.name,target_attr.name,if unique{"Source values are unique."}else{"Source values repeat."},if attr.nullable{"allows nulls"}else{"has no nulls"}),
                    });
                }
            }
            if matches.len() > 1 {
                warnings.push(format!("{}.{} matches multiple parent keys; relation omitted until target is resolved.",from.entity.name,attr.name));
            } else {
                relationships.extend(matches);
            }
        }
    }
    relationships
}

pub fn infer_schema(document: &Document, tables: &[Table]) -> Schema {
    let mut warnings = Vec::new();
    let mut items = Vec::new();
    let mut names = HashSet::new();
    let pages: BTreeMap<_, _> = document.pages.iter().map(|p| (p.id.as_str(), p)).collect();
    for table in tables {
        let Some(page) = pages.get(table.page_id.as_str()) else {
            warnings.push(format!("{}: source page missing.", table.name));
            continue;
        };
        let Some(grid) = page.grid.as_ref() else {
            warnings.push(format!(
                "{}: relational extraction for this source type is not implemented yet.",
                table.name
            ));
            continue;
        };
        if !table.data.valid()
            || !grid.bounds().encloses(table.data)
            || [table.title, table.column_header, table.row_header]
                .into_iter()
                .flatten()
                .any(|b| !b.valid() || !grid.bounds().encloses(b))
        {
            warnings.push(format!(
                "{}: invalid data bounds; table excluded.",
                table.name
            ));
            continue;
        }
        let view = GridView::new(grid);
        let matrix = matches!(
            table.orientation.to_lowercase().as_str(),
            "matrix" | "crosstab" | "cross_tab"
        );
        let mut item = if matrix {
            matrix_table(&view, table)
        } else {
            record_table(&view, table)
        };
        item.entity.name = unique_name(
            if table.name.trim().is_empty() {
                &page.name
            } else {
                &table.name
            },
            &mut names,
        );
        if matrix {
            warnings.push(format!("{}: matrix unpivoted into header dimensions and value; composite key and dimension entities require review.",item.entity.name));
        } else {
            assign_primary_key(&mut item, &mut warnings);
        }
        if grid
            .cells
            .iter()
            .any(|c| table.data.contains(c.row, c.col) && c.kind == CellKind::Error)
        {
            warnings.push(format!(
                "{}: formula/error cells are preserved as text; inspect original cell evidence.",
                item.entity.name
            ));
        }
        items.push(item);
    }
    let relations = relationships(&items, &mut warnings);
    Schema {
        entities: items.into_iter().map(|i| i.entity).collect(),
        relationships: relations,
        warnings,
    }
}
