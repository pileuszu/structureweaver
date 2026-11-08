use structureweaver::{
    analysis::{Decision, header_nodes, tiles, validate_decision},
    domain::*,
};
fn b(top: u32, left: u32, bottom: u32, right: u32) -> Bounds {
    Bounds {
        top,
        left,
        bottom,
        right,
    }
}
fn cell(row: u32, col: u32, value: &str) -> Cell {
    Cell {
        row,
        col,
        value: value.into(),
        kind: CellKind::Text,
        formula: None,
        style: CellStyle::default(),
    }
}
fn grid() -> Grid {
    Grid {
        rows: 501,
        cols: 80,
        cells: vec![
            cell(1, 1, "id"),
            cell(2, 1, "1"),
            cell(450, 66, "late hidden"),
            cell(501, 80, "end"),
        ],
        merges: vec![],
        hidden_rows: vec![450],
        hidden_cols: vec![66],
    }
}
#[test]
fn all_populated_cells_are_tiled_once_including_hidden_and_late_coordinates() {
    let g = grid();
    let ts = tiles(&g);
    for c in &g.cells {
        assert_eq!(ts.iter().filter(|t| t.contains(c.row, c.col)).count(), 1);
    }
    assert_eq!(ts.len(), 3);
}
#[test]
fn rejects_hallucinated_and_overlapping_regions() {
    let g = grid();
    let mut d = Decision {
        name: "test".into(),
        bounds: b(1, 1, 2, 1),
        title: None,
        column_header: Some(b(1, 1, 1, 1)),
        row_header: None,
        data: b(2, 1, 2, 1),
        orientation: "records".into(),
    };
    assert!(validate_decision(&d, b(1, 1, 32, 10), &g).is_ok());
    d.data = b(1, 1, 2, 1);
    assert!(validate_decision(&d, b(1, 1, 32, 10), &g).is_err());
    d.data = b(33, 1, 34, 1);
    assert!(validate_decision(&d, b(1, 1, 32, 10), &g).is_err());
}
#[test]
fn column_hierarchy_follows_merged_spans() {
    let g = Grid {
        rows: 3,
        cols: 2,
        cells: vec![
            cell(1, 1, "2026"),
            cell(2, 1, "Revenue"),
            cell(2, 2, "Cost"),
            cell(3, 1, "3"),
        ],
        merges: vec![b(1, 1, 1, 2)],
        hidden_rows: vec![],
        hidden_cols: vec![],
    };
    let t = Table {
        id: "t".into(),
        name: "x".into(),
        page_id: "s".into(),
        bounds: b(1, 1, 3, 2),
        title: None,
        column_header: Some(b(1, 1, 2, 2)),
        row_header: None,
        data: b(3, 1, 3, 2),
        orientation: "records".into(),
        headers: vec![],
        evidence: String::new(),
    };
    let nodes = header_nodes(&g, &t);
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1].parent_id, Some(nodes[0].id.clone()));
    assert_eq!(nodes[2].parent_id, Some(nodes[0].id.clone()));
}
#[test]
fn records_never_drop_leading_text_as_row_headers() {
    let g = grid();
    let d = Decision {
        name: "test".into(),
        bounds: b(1, 1, 2, 2),
        title: None,
        column_header: Some(b(1, 2, 1, 2)),
        row_header: Some(b(2, 1, 2, 1)),
        data: b(2, 2, 2, 2),
        orientation: "records".into(),
    };
    assert!(validate_decision(&d, b(1, 1, 32, 10), &g).is_err());
}

#[test]
fn luna_normalization_separates_blank_matrix_corner_and_duplicate_title() {
    use structureweaver::analysis::normalize_decision;
    let g = Grid {
        rows: 7,
        cols: 6,
        cells: vec![cell(1, 3, "FY2026"), cell(4, 3, "100")],
        merges: vec![b(1, 3, 1, 6)],
        hidden_rows: vec![],
        hidden_cols: vec![],
    };
    let mut d = Decision {
        name: "matrix".into(),
        bounds: b(1, 1, 7, 6),
        title: Some(b(1, 1, 3, 6)),
        column_header: Some(b(1, 1, 3, 6)),
        row_header: Some(b(1, 1, 7, 2)),
        data: b(4, 3, 7, 6),
        orientation: "matrix".into(),
    };
    normalize_decision(&mut d, g.bounds()).unwrap();
    validate_decision(&d, g.bounds(), &g).unwrap();
    assert_eq!(d.title, None);
    assert_eq!(d.column_header, Some(b(1, 3, 3, 6)));
    assert_eq!(d.row_header, Some(b(4, 1, 7, 2)));
    assert_eq!(d.data, b(4, 3, 7, 6));
}
#[test]
fn normalization_never_clamps_out_of_bounds_hallucinations() {
    use structureweaver::analysis::normalize_decision;
    let mut d = Decision {
        name: "bad".into(),
        bounds: b(1, 1, 7, 6),
        title: None,
        column_header: Some(b(1, 1, 3, 700)),
        row_header: None,
        data: b(4, 1, 7, 6),
        orientation: "records".into(),
    };
    assert!(normalize_decision(&mut d, b(1, 1, 7, 6)).is_err());
}
#[test]
fn indentation_builds_same_column_row_parent_context() {
    let mut child = cell(3, 1, "Laptops");
    child.style.indent = 1;
    let g = Grid {
        rows: 3,
        cols: 2,
        cells: vec![
            cell(1, 2, "Revenue"),
            cell(2, 1, "Hardware"),
            child,
            cell(2, 2, "10"),
            cell(3, 2, "7"),
        ],
        merges: vec![],
        hidden_rows: vec![],
        hidden_cols: vec![],
    };
    let t = Table {
        id: "t".into(),
        name: "x".into(),
        page_id: "s".into(),
        bounds: b(1, 1, 3, 2),
        title: None,
        column_header: Some(b(1, 2, 1, 2)),
        row_header: Some(b(2, 1, 3, 1)),
        data: b(2, 2, 3, 2),
        orientation: "matrix".into(),
        headers: vec![],
        evidence: String::new(),
    };
    let nodes = header_nodes(&g, &t);
    let child = nodes.iter().find(|n| n.label == "Laptops").unwrap();
    let parent = nodes.iter().find(|n| n.label == "Hardware").unwrap();
    assert_eq!(child.parent_id, Some(parent.id.clone()));
}

#[test]
fn valid_bounding_box_does_not_hide_unclassified_cells() {
    let g = Grid {
        rows: 3,
        cols: 1,
        cells: vec![cell(1, 1, "id"), cell(2, 1, "omitted"), cell(3, 1, "1")],
        merges: vec![],
        hidden_rows: vec![],
        hidden_cols: vec![],
    };
    let d = Decision {
        name: "gap".into(),
        bounds: g.bounds(),
        title: None,
        column_header: Some(b(1, 1, 1, 1)),
        row_header: None,
        data: b(3, 1, 3, 1),
        orientation: "records".into(),
    };
    assert!(
        validate_decision(&d, g.bounds(), &g)
            .unwrap_err()
            .to_string()
            .contains("Unclassified")
    );
}
#[test]
fn matrix_rejects_label_column_without_measure_header() {
    let g = Grid {
        rows: 2,
        cols: 3,
        cells: vec![
            cell(1, 3, "Q1"),
            cell(2, 1, "Hardware"),
            cell(2, 2, "Laptops"),
            cell(2, 3, "100"),
        ],
        merges: vec![],
        hidden_rows: vec![],
        hidden_cols: vec![],
    };
    let d = Decision {
        name: "matrix".into(),
        bounds: g.bounds(),
        title: None,
        column_header: Some(b(1, 2, 1, 3)),
        row_header: Some(b(2, 1, 2, 1)),
        data: b(2, 2, 2, 3),
        orientation: "matrix".into(),
    };
    assert!(
        validate_decision(&d, g.bounds(), &g)
            .unwrap_err()
            .to_string()
            .contains("no header: [2]")
    );
}
