use std::path::Path;
use structureweaver::{drawings::DrawingContent, ingest};

#[test]
fn advanced_fixture_preserves_camera_layout_effects_and_original_data() {
    let document = ingest::read_document(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/fidelity.xlsx"
    )))
    .unwrap();
    assert_eq!(document.pages.len(), 4);
    let grid = document.pages[1].grid.as_ref().unwrap();
    assert_eq!(grid.layout.drawings.len(), 6);
    for object in &grid.layout.drawings {
        let DrawingContent::Chart(chart) = &object.content else {
            panic!("Expected 3D chart")
        };
        assert!(chart.groups[0].three_dimensional);
        assert_eq!(chart.view_3d.rotation_y, 30.0);
        assert_eq!(chart.view_3d.depth_percent, 120.0);
        assert!(!chart.view_3d.right_angle_axes);
        assert_eq!(chart.groups[0].series[0].values.numbers[2], Some(-6.0));
    }
    let layout = document.pages[2].grid.as_ref().unwrap();
    assert_eq!(layout.layout.drawings.len(), 4);
    let DrawingContent::Chart(manual) = &layout.layout.drawings[1].content else {
        panic!("Expected manual chart")
    };
    assert_eq!(manual.plot_layout.as_ref().unwrap().x, Some(0.14));
    assert_eq!(manual.title_layout.as_ref().unwrap().y, Some(0.03));
    assert_eq!(manual.legend_layout.as_ref().unwrap().y, Some(0.86));
    let DrawingContent::Chart(rotated) = &layout.layout.drawings[2].content else {
        panic!("Expected dense category chart")
    };
    let axis = rotated
        .axes
        .iter()
        .find(|axis| axis.kind == "catAx")
        .unwrap();
    assert_eq!(axis.label_rotation, Some(-45.0));
    assert_eq!(axis.label_font_size, Some(11.0));
    assert_eq!(axis.label_skip, Some(2));
    assert_eq!(rotated.groups[0].series[0].categories.labels.len(), 12);
    let effects = document.pages[3].grid.as_ref().unwrap();
    let shape = |name| {
        let object = effects
            .layout
            .drawings
            .iter()
            .find(|o| o.name == name)
            .unwrap();
        let DrawingContent::Shape(shape) = &object.content else {
            panic!("Expected effect shape")
        };
        shape
    };
    assert_eq!(
        shape("Outer shadow")
            .effects
            .outer_shadow
            .as_ref()
            .unwrap()
            .blur,
        6.0
    );
    assert!(shape("Inner shadow").effects.inner_shadow.is_some());
    assert!(shape("Glow").effects.glow.is_some());
    assert_eq!(shape("Soft edges").effects.soft_edge, 6.0);
    assert!(shape("Reflection").effects.reflection.is_some());
    assert!(shape("Bevel").effects.scene.is_some());
    assert_eq!(
        shape("Extrusion").effects.scene.as_ref().unwrap().extrusion,
        20.0
    );
    let text = shape("Text reflection");
    assert!(text.fill.is_none());
    assert!(text.effects.reflection.is_some());
    assert_eq!(text.paragraphs[0].runs[0].text, "Text reflection");
    let combined = &shape("3D with effects").effects;
    assert!(combined.scene.is_some());
    assert!(combined.reflection.is_some());
    assert!(combined.outer_shadow.is_some());
    for (name, material) in [
        ("Matte lighting", "matte"),
        ("Plastic lighting", "plastic"),
        ("Metal lighting", "metal"),
    ] {
        assert_eq!(
            shape(name).effects.scene.as_ref().unwrap().material,
            material
        );
    }
    assert_eq!(
        document.pages[0]
            .grid
            .as_ref()
            .unwrap()
            .cell(4, 2)
            .unwrap()
            .value,
        "-6"
    );
    // Native presentation metadata must survive saving/reopening an analysis.
    let saved = serde_json::to_vec(&document).unwrap();
    let reopened: structureweaver::domain::Document = serde_json::from_slice(&saved).unwrap();
    assert_eq!(saved, serde_json::to_vec(&reopened).unwrap());
}
