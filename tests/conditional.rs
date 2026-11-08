use std::path::Path;
use structureweaver::{conditional, ingest};

#[test]
fn workbook_conditional_rules_reproduce_visuals_without_changing_source_cells() {
    let document = ingest::read_document(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/visuals.xlsx"
    )))
    .unwrap();
    let grid = document
        .pages
        .iter()
        .find(|p| p.name == "Visuals")
        .unwrap()
        .grid
        .as_ref()
        .unwrap();
    assert_eq!(grid.layout.conditional_formats.len(), 7);
    let original = serde_json::to_value(&grid.cells).unwrap();
    let result = conditional::prepare(grid, &grid.layout.conditional_formats);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let fill = |row, col| {
        result.cells[&(row, col)]
            .style
            .fill
            .as_deref()
            .unwrap()
            .get(2..)
            .unwrap()
    };
    assert_eq!(fill(5, 1), "FECACA"); // Negative value: red.
    assert_eq!(fill(12, 1), "A7F3D0"); // Value >= 70: green.
    assert_eq!(fill(5, 2), "DDD6FE"); // Duplicate text A.
    assert!(!result.cells.contains_key(&(6, 2))); // B occurs only once.
    assert_eq!(fill(8, 3), "FEF3C7"); // Relative A reference and even ROW.
    assert_eq!(result.cells[&(8, 3)].style.bold, Some(true));
    assert!(!result.cells.contains_key(&(6, 3))); // Even row alone is insufficient.
    let low_bar = result.cells[&(5, 4)].data_bar.as_ref().unwrap();
    let high_bar = result.cells[&(14, 4)].data_bar.as_ref().unwrap();
    assert!(low_bar.end < high_bar.end);
    assert_eq!(low_bar.start, 0.0);
    assert!(low_bar.zero.is_none()); // Legacy bars do not introduce an x14 zero axis.
    assert_eq!(fill(5, 5), "F8696B");
    assert_eq!(fill(14, 5), "63BE7B");
    assert_eq!(result.cells[&(5, 6)].icon.as_ref().unwrap().index, 0);
    assert_eq!(result.cells[&(14, 6)].icon.as_ref().unwrap().index, 2);
    assert_eq!(serde_json::to_value(&grid.cells).unwrap(), original);
}
