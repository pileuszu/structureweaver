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

#[test]
fn saved_chart_point_colors_round_trip_directly_and_inside_drawing_groups() {
    use structureweaver::{
        domain::Document,
        drawings::{DrawingContent, GroupChild},
        ingest,
    };
    let mut document = ingest::read_document(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/visuals.xlsx"
    )))
    .unwrap();
    let drawings = &mut document.pages[0].grid.as_mut().unwrap().layout.drawings;
    let object = drawings
        .iter_mut()
        .find(|d| matches!(d.content, DrawingContent::Chart(_)))
        .unwrap();
    if let DrawingContent::Chart(chart) = &mut object.content {
        chart.groups[0].series[0]
            .point_colors
            .insert(1, "FF12AB34".into());
        chart.groups[0].series[0]
            .point_colors
            .insert(12, "FFABCDEF".into());
    }
    let mut grouped = object.clone();
    grouped.content = DrawingContent::Group {
        children: vec![GroupChild {
            rect: [0.0, 0.0, 1.0, 1.0],
            object: object.clone(),
        }],
    };
    drawings.push(grouped);
    let bytes = serde_json::to_vec(&document).unwrap();
    let restored: Document = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&document).unwrap()
    );
}
