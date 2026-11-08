use std::path::Path;
use structureweaver::{domain::CellKind, ingest::read_document};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
fn relational_demo_preserves_all_three_sheets_and_ids() {
    let doc = read_document(&fixture("demo.xlsx")).unwrap();
    assert_eq!(doc.pages.len(), 3);
    assert_eq!(
        doc.pages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["Customers", "Orders", "LineItems"]
    );
    let orders = doc.pages[1].grid.as_ref().unwrap();
    assert_eq!(orders.cell(1, 2).unwrap().value, "customer_id");
    assert_eq!(orders.cell(3, 2).unwrap().value, "101");
    assert_eq!(orders.cell(3, 2).unwrap().kind, CellKind::Number);
    assert!(orders.cell(1, 2).unwrap().style.bold);
    assert_eq!(
        orders.cell(1, 2).unwrap().style.fill.as_deref(),
        Some("FFDCEAFE")
    );
    assert_eq!(doc.id, read_document(&fixture("demo.xlsx")).unwrap().id);
}

#[test]
fn all_sheets_include_hidden_empty_and_content_past_old_limits() {
    let doc = read_document(&fixture("coverage.xlsx")).unwrap();
    assert_eq!(doc.pages.len(), 6);
    let audit = &doc.pages[3];
    assert!(audit.hidden);
    let grid = audit.grid.as_ref().unwrap();
    assert_eq!((grid.rows, grid.cols), (450, 66));
    assert_eq!(
        grid.cell(450, 66).unwrap().value,
        "covered after old limits"
    );
    assert_eq!(grid.hidden_rows, [450]);
    assert_eq!(grid.hidden_cols, [65, 66]);
    assert!(doc.pages[5].hidden);
    assert!(doc.pages[5].grid.as_ref().unwrap().cells.is_empty());
}

#[test]
fn merges_styles_formulas_and_typed_values_survive_ingestion() {
    let doc = read_document(&fixture("coverage.xlsx")).unwrap();
    let matrix = doc.pages[2].grid.as_ref().unwrap();
    assert_eq!(matrix.merges.len(), 2);
    assert_eq!(matrix.resolved_cell(2, 3).unwrap().value, "FY2026");
    assert_eq!(matrix.cell(5, 1).unwrap().style.indent, 2);
    let grid = doc.pages[4].grid.as_ref().unwrap();
    assert_eq!(
        (grid.rows, grid.cols),
        (12, 7),
        "Merged extents must not be clipped to anchor cells"
    );
    assert_eq!(grid.cell(2, 2).unwrap().formula.as_deref(), Some("1+2"));
    assert_eq!(grid.cell(2, 2).unwrap().value, "3");
    assert_eq!(grid.cell(3, 2).unwrap().formula.as_deref(), Some("RAND()"));
    assert_eq!(grid.cell(3, 2).unwrap().kind, CellKind::Empty);
    assert_eq!(grid.cell(4, 2).unwrap().kind, CellKind::Date);
    assert!(grid.cell(4, 2).unwrap().value.starts_with("2024-01-01"));
    assert_eq!(grid.cell(5, 2).unwrap().value, "서울 & 부산");
    assert_eq!(grid.cell(6, 2).unwrap().kind, CellKind::Error);
    assert_eq!(grid.cell(7, 2).unwrap().kind, CellKind::Boolean);
    assert_eq!(grid.cell(8, 2).unwrap().value, "0");
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("without saved values"))
    );
}

#[test]
fn unsupported_inputs_and_resource_limits_are_explicit() {
    assert!(
        read_document(Path::new("unsupported.pdf"))
            .unwrap_err()
            .to_string()
            .contains("Unsupported format")
    );
    let error = read_document(&fixture("resource-limit.xlsx")).unwrap_err();
    assert!(format!("{error:#}").contains("resource budget"));
}

#[test]
fn worksheet_paths_follow_relationships_not_sheet_numbers() {
    use std::{
        fs::File,
        io::{Read, Write},
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("renamed.xlsx");
    let mut input = ZipArchive::new(File::open(fixture("demo.xlsx")).unwrap()).unwrap();
    let mut output = ZipWriter::new(File::create(&path).unwrap());
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        let name = if entry.name() == "xl/worksheets/sheet1.xml" {
            "xl/worksheets/custom-name.xml".to_owned()
        } else {
            entry.name().to_owned()
        };
        if name == "xl/_rels/workbook.xml.rels" {
            content = content.replace(
                "Target=\"worksheets/sheet1.xml\"",
                "Target=\"/xl/worksheets/custom-name.xml\"",
            );
        }
        if name == "[Content_Types].xml" {
            content = content.replace(
                "/xl/worksheets/sheet1.xml",
                "/xl/worksheets/custom-name.xml",
            );
        }
        output
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        output.write_all(content.as_bytes()).unwrap();
    }
    output.finish().unwrap();
    let doc = read_document(&path).unwrap();
    assert_eq!(
        doc.pages[0]
            .grid
            .as_ref()
            .unwrap()
            .cell(2, 2)
            .unwrap()
            .value,
        "Alice"
    );
}
