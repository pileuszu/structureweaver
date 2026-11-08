use structureweaver::domain::Analysis;

#[test]
fn older_saved_analysis_retains_values_and_gets_safe_style_defaults() {
    let analysis: Analysis =
        serde_json::from_str(include_str!("../docs/verification/live-demo/analysis.json")).unwrap();
    let grid = analysis.document.pages[0].grid.as_ref().unwrap();
    assert_eq!(grid.cell(2, 1).unwrap().value, "101");
    assert!(grid.layout.show_grid_lines);
    assert_eq!(grid.layout.default_row_height, 15.0);
    assert_eq!(grid.cell(1, 1).unwrap().style.font_size, 11.0);
    assert_eq!(analysis.schema.entities.len(), 3);
}
