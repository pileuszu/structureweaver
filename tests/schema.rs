use structureweaver::{domain::*, export, schema::infer_schema};

fn bounds(top: u32, left: u32, bottom: u32, right: u32) -> Bounds {
    Bounds {
        top,
        left,
        bottom,
        right,
    }
}
fn cell(row: u32, col: u32, value: &str, kind: CellKind) -> Cell {
    Cell {
        row,
        col,
        value: value.into(),
        kind,
        formula: None,
        style: CellStyle::default(),
        ..Cell::default()
    }
}
fn record(
    name: &str,
    headers: &[&str],
    records: &[Vec<Option<(&str, CellKind)>>],
) -> (Page, Table) {
    let mut cells: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(i, s)| cell(1, i as u32 + 1, s, CellKind::Text))
        .collect();
    for (r, row) in records.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            if let Some((v, k)) = value {
                cells.push(cell(r as u32 + 2, c as u32 + 1, v, k.clone()));
            }
        }
    }
    let last = records.len() as u32 + 1;
    let cols = headers.len() as u32;
    (
        Page {
            id: name.into(),
            name: name.into(),
            kind: PageKind::Sheet,
            hidden: false,
            grid: Some(Grid {
                rows: last,
                cols,
                cells,
                merges: vec![],
                hidden_rows: vec![],
                hidden_cols: vec![],
                layout: SheetLayout::default(),
            }),
            blocks: vec![],
        },
        Table {
            id: name.into(),
            name: name.into(),
            page_id: name.into(),
            bounds: bounds(1, 1, last, cols),
            title: None,
            column_header: Some(bounds(1, 1, 1, cols)),
            row_header: None,
            data: bounds(2, 1, last, cols),
            orientation: "records".into(),
            headers: vec![],
            evidence: "fixture".into(),
        },
    )
}
fn document(pages: Vec<Page>) -> Document {
    Document {
        id: "test".into(),
        name: "test.xlsx".into(),
        format: "xlsx".into(),
        pages,
        warnings: vec![],
    }
}
fn number(s: &str) -> Option<(&str, CellKind)> {
    Some((s, CellKind::Number))
}
fn text(s: &str) -> Option<(&str, CellKind)> {
    Some((s, CellKind::Text))
}

#[test]
fn cross_sheet_fk_requires_semantic_name_types_and_all_values() {
    let (customers, customer_table) = record(
        "Customers",
        &["customer_id", "name"],
        &[
            vec![number("1"), text("Kim")],
            vec![number("2"), text("Park")],
        ],
    );
    let (orders, order_table) = record(
        "Orders",
        &["order_id", "customer_id", "quantity"],
        &[
            vec![number("11"), number("1"), number("1")],
            vec![number("12"), number("1"), number("2")],
            vec![number("13"), number("2"), number("1")],
        ],
    );
    let schema = infer_schema(
        &document(vec![customers, orders]),
        &[customer_table, order_table],
    );
    assert_eq!(schema.relationships.len(), 1);
    let r = &schema.relationships[0];
    assert_eq!(r.from_attribute, "customer_id");
    assert_eq!(r.cardinality, "many_to_one");
    assert_eq!(r.status, "candidate");
    assert_eq!(
        schema.entities[1]
            .attributes
            .iter()
            .filter(|a| a.primary_key)
            .count(),
        1
    );
    assert!(schema.entities[1].attributes[0].primary_key);
}

#[test]
fn generic_ids_in_unrelated_tables_are_not_foreign_keys() {
    let (a, ta) = record(
        "Customers",
        &["id", "name"],
        &[vec![number("1"), text("A")], vec![number("2"), text("B")]],
    );
    let (b, tb) = record(
        "Products",
        &["id", "price"],
        &[
            vec![number("1"), number("1")],
            vec![number("2"), number("2")],
        ],
    );
    let schema = infer_schema(&document(vec![a, b]), &[ta, tb]);
    assert!(schema.relationships.is_empty());
    assert!(!schema.entities[1].attributes[1].primary_key);
}

#[test]
fn missing_or_wrong_type_or_orphan_values_prevent_fk() {
    let (a, ta) = record(
        "Customers",
        &["id"],
        &[vec![number("1")], vec![number("2")]],
    );
    let (b, tb) = record(
        "Orders",
        &["order_id", "customer_id"],
        &[vec![number("10"), text("1")], vec![number("11"), text("2")]],
    );
    let (c, tc) = record(
        "Invoices",
        &["invoice_id", "customer_id"],
        &[
            vec![number("20"), number("1")],
            vec![number("21"), number("3")],
        ],
    );
    let schema = infer_schema(&document(vec![a, b, c]), &[ta, tb, tc]);
    assert!(schema.relationships.is_empty());
}

#[test]
fn null_identifier_is_not_primary_key() {
    let (a, ta) = record(
        "Customers",
        &["id", "name"],
        &[vec![number("1"), text("A")], vec![None, text("B")]],
    );
    let schema = infer_schema(&document(vec![a]), &[ta]);
    assert!(schema.entities[0].attributes[0].nullable);
    assert!(!schema.entities[0].attributes.iter().any(|a| a.primary_key));
}

#[test]
fn duplicate_headers_and_entity_names_get_stable_unique_names() {
    let (a, mut ta) = record(
        "A",
        &["Name", "name", "Name_2"],
        &[
            vec![text("A"), text("B"), text("C")],
            vec![text("D"), text("E"), text("F")],
        ],
    );
    let (b, mut tb) = record("B", &["id"], &[vec![number("1")], vec![number("2")]]);
    ta.name = "共有".into();
    tb.name = "共有".into();
    let schema = infer_schema(&document(vec![a, b]), &[ta, tb]);
    let names: Vec<_> = schema.entities[0]
        .attributes
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(names, vec!["Name", "name_2", "Name_2_2"]);
    assert_eq!(schema.entities[1].name, "共有_2");
}

#[test]
fn matrix_unpivots_headers_and_keeps_missing_cells_nullable() {
    let grid = Grid {
        rows: 4,
        cols: 3,
        cells: vec![
            cell(1, 2, "Sales", CellKind::Text),
            cell(2, 2, "2025", CellKind::Number),
            cell(2, 3, "2026", CellKind::Number),
            cell(3, 1, "Korea", CellKind::Text),
            cell(4, 1, "Japan", CellKind::Text),
            cell(3, 2, "10", CellKind::Number),
            cell(3, 3, "20", CellKind::Number),
            cell(4, 2, "30", CellKind::Number),
        ],
        merges: vec![bounds(1, 2, 1, 3)],
        hidden_rows: vec![],
        hidden_cols: vec![],
        layout: SheetLayout::default(),
    };
    let page = Page {
        id: "p".into(),
        name: "Sales".into(),
        kind: PageKind::Sheet,
        hidden: false,
        grid: Some(grid),
        blocks: vec![],
    };
    let table = Table {
        id: "sales".into(),
        name: "Sales".into(),
        page_id: "p".into(),
        bounds: bounds(1, 1, 4, 3),
        title: None,
        column_header: Some(bounds(1, 2, 2, 3)),
        row_header: Some(bounds(3, 1, 4, 1)),
        data: bounds(3, 2, 4, 3),
        orientation: "matrix".into(),
        headers: vec![],
        evidence: "fixture".into(),
    };
    let schema = infer_schema(&document(vec![page]), &[table]);
    assert_eq!(schema.entities.len(), 1);
    let e = &schema.entities[0];
    assert_eq!(e.row_count, 4);
    assert_eq!(e.attributes.len(), 4);
    assert_eq!(e.attributes[0].sample_values, vec!["Korea", "Japan"]);
    assert_eq!(e.attributes[1].sample_values, vec!["Sales"]);
    assert_eq!(e.attributes[2].sample_values, vec!["2025", "2026"]);
    assert_eq!(e.attributes[3].sample_values, vec!["10", "20", "30"]);
    assert!(e.attributes[3].nullable);
}

#[test]
fn export_handles_unicode_quotes_and_keeps_fk_in_comments() {
    let (p, mut t) = record(
        "고객",
        &["id", "이름\"\n}); DROP TABLE x;--"],
        &[
            vec![number("1"), text("Kim")],
            vec![number("2"), text("Park")],
        ],
    );
    t.name = "고객\"목록".into();
    let schema = infer_schema(&document(vec![p]), &[t]);
    let ddl = export::sql(&schema);
    assert!(ddl.contains("CREATE TABLE \"고객\"\"목록\""));
    assert!(ddl.contains("\"id\" INT NOT NULL PRIMARY KEY"));
    assert!(ddl.contains("\"이름\"\" }"));
    let mmd = export::mermaid(&schema);
    assert!(mmd.contains("고객#34;목록"));
    assert!(mmd.contains("field_2"));
    assert!(!mmd.contains("\"이름\""));
}

#[test]
fn leading_zero_text_ids_remain_text_and_distinct() {
    let (p, t) = record(
        "Codes",
        &["id", "label"],
        &[
            vec![text("01"), text("first")],
            vec![text("1"), text("second")],
        ],
    );
    let schema = infer_schema(&document(vec![p]), &[t]);
    let a = &schema.entities[0].attributes[0];
    assert_eq!(a.data_type, "TEXT");
    assert!(a.primary_key);
    assert_eq!(a.sample_values, vec!["01", "1"]);
}

#[test]
fn real_values_are_not_candidate_primary_keys() {
    let (p, t) = record(
        "Observations",
        &["id"],
        &[vec![number("1.1")], vec![number("2.2")]],
    );
    let schema = infer_schema(&document(vec![p]), &[t]);
    assert_eq!(schema.entities[0].attributes[0].data_type, "REAL");
    assert!(!schema.entities[0].attributes[0].primary_key);
}

#[test]
fn matrix_uses_explicit_row_parent_paths_and_never_fills_unknown_blanks() {
    let grid = Grid {
        rows: 4,
        cols: 2,
        cells: vec![
            cell(1, 2, "2026", CellKind::Number),
            cell(2, 1, "Asia", CellKind::Text),
            cell(3, 1, "Korea", CellKind::Text),
            cell(3, 2, "100", CellKind::Number),
            cell(4, 2, "50", CellKind::Number),
        ],
        merges: vec![],
        hidden_rows: vec![],
        hidden_cols: vec![],
        layout: SheetLayout::default(),
    };
    let page = Page {
        id: "p".into(),
        name: "Sales".into(),
        kind: PageKind::Sheet,
        hidden: false,
        grid: Some(grid),
        blocks: vec![],
    };
    let table = Table {
        id: "t".into(),
        name: "Sales".into(),
        page_id: "p".into(),
        bounds: bounds(1, 1, 4, 2),
        title: None,
        column_header: Some(bounds(1, 2, 1, 2)),
        row_header: Some(bounds(2, 1, 4, 1)),
        data: bounds(2, 2, 4, 2),
        orientation: "matrix".into(),
        headers: vec![
            HeaderNode {
                id: "asia".into(),
                label: "Asia".into(),
                axis: "row".into(),
                bounds: bounds(2, 1, 2, 1),
                parent_id: None,
            },
            HeaderNode {
                id: "korea".into(),
                label: "Korea".into(),
                axis: "row".into(),
                bounds: bounds(3, 1, 3, 1),
                parent_id: Some("asia".into()),
            },
        ],
        evidence: "fixture".into(),
    };
    let schema = infer_schema(&document(vec![page]), &[table]);
    assert_eq!(
        schema.entities[0].attributes[0].sample_values,
        vec!["Asia", "Asia / Korea"]
    );
    assert!(schema.entities[0].attributes[0].nullable);
    assert_eq!(schema.entities[0].row_count, 3);
}

#[test]
fn singleton_and_error_ids_are_not_inferred_primary_keys() {
    let (a, ta) = record("Singleton", &["id"], &[vec![number("1")]]);
    let (b, tb) = record(
        "Errors",
        &["id"],
        &[vec![Some(("#VALUE!", CellKind::Error))], vec![text("ID2")]],
    );
    let schema = infer_schema(&document(vec![a, b]), &[ta, tb]);
    assert!(
        schema
            .entities
            .iter()
            .all(|e| !e.attributes.iter().any(|a| a.primary_key))
    );
}

#[test]
fn inferred_foreign_keys_are_review_comments_in_executable_ddl() {
    let (a, ta) = record(
        "Customers",
        &["id"],
        &[vec![number("1")], vec![number("2")]],
    );
    let (b, tb) = record(
        "Orders",
        &["order_id", "customer_id"],
        &[
            vec![number("10"), number("1")],
            vec![number("11"), number("1")],
            vec![number("12"), number("2")],
        ],
    );
    let schema = infer_schema(&document(vec![a, b]), &[ta, tb]);
    assert_eq!(schema.relationships.len(), 1);
    let ddl = export::sql(&schema);
    assert!(ddl.contains("-- Candidate only: FOREIGN KEY"));
    let executable: Vec<_> = ddl
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect();
    assert!(!executable.join("\n").contains("FOREIGN KEY"));
    let mmd = export::mermaid(&schema);
    assert!(mmd.contains("||..o{"));
    assert!(mmd.contains("(candidate)"));
}
