use std::path::Path;
use structureweaver::{
    drawings::{DrawingAnchor, DrawingContent},
    ingest,
};

#[test]
fn visual_fixture_retains_native_shapes_picture_and_all_chart_anchors() {
    let document = ingest::read_document(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/visuals.xlsx"
    )))
    .unwrap();
    let grid = document.pages[0].grid.as_ref().unwrap();
    // Anchored object positions depend on Normal font metrics even when cellXfs[0]
    // uses a different font. Both are independently preserved from this workbook.
    assert_eq!(
        grid.layout
            .normal_style
            .as_ref()
            .unwrap()
            .font_name
            .as_deref(),
        Some("Arial")
    );
    assert_eq!(
        grid.layout.default_style.font_name.as_deref(),
        Some("Calibri")
    );
    assert_eq!(grid.layout.drawings.len(), 8);
    assert_eq!(
        grid.layout
            .drawings
            .iter()
            .filter(|o| matches!(o.content, DrawingContent::Chart(_)))
            .count(),
        2
    );
    assert_eq!(
        grid.layout
            .drawings
            .iter()
            .filter(|o| matches!(o.content, DrawingContent::Shape(_)))
            .count(),
        5
    );
    let picture = grid
        .layout
        .drawings
        .iter()
        .find(|o| matches!(o.content, DrawingContent::Picture(_)))
        .unwrap();
    let DrawingAnchor::OneCell {
        from,
        width_emu,
        height_emu,
    } = &picture.anchor
    else {
        panic!("Picture lost its one-cell anchor")
    };
    assert_eq!((from.row, from.col), (18, 6));
    assert_eq!((*width_emu, *height_emu), (609600.0, 609600.0));
    let DrawingContent::Picture(picture) = &picture.content else {
        unreachable!()
    };
    assert!(!picture.base64.is_empty());
    let rotated = grid
        .layout
        .drawings
        .iter()
        .find(|o| o.name == "Rotated ellipse")
        .unwrap();
    assert_eq!(rotated.rotation, -12.0);
    let DrawingContent::Shape(shape) = &rotated.content else {
        panic!("Shape changed type")
    };
    assert_eq!(shape.geometry, "ellipse");
    assert_eq!(shape.paragraphs[0].runs[0].text, "Rotated");
    // Shapes on otherwise blank rows don't become source records or alter source cells.
    assert!(grid.cell(23, 1).is_none());
    let gallery = document.pages[2].grid.as_ref().unwrap();
    assert_eq!(gallery.layout.drawings.len(), 6);
    assert!(gallery.cells.is_empty());
    let chart_sheet = document
        .pages
        .iter()
        .find(|p| p.name == "Chart sheet")
        .unwrap()
        .grid
        .as_ref()
        .unwrap();
    assert!(chart_sheet.layout.chart_sheet);
    assert!(chart_sheet.cells.is_empty());
    assert_eq!(chart_sheet.layout.drawings.len(), 1);
    let DrawingContent::Chart(chart) = &chart_sheet.layout.drawings[0].content else {
        panic!("Chart sheet lost its chart")
    };
    assert_eq!(chart.groups[0].series[0].values.numbers[0], Some(15.0));
}
