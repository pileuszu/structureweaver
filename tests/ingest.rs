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

fn styled_workbook() -> (tempfile::TempDir, std::path::PathBuf) {
    use std::{
        fs::File,
        io::{Read, Write},
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("styled.xlsx");
    let mut input = ZipArchive::new(File::open(fixture("demo.xlsx")).unwrap()).unwrap();
    let mut output = ZipWriter::new(File::create(&path).unwrap());
    let styles = r##"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <numFmts count="2"><numFmt numFmtId="164" formatCode="#,##0.00"/><numFmt numFmtId="165" formatCode="0.0%"/></numFmts>
      <fonts count="3">
        <font><sz val="11"/><name val="Arial"/><color indexed="8"/></font>
        <font><b/><i/><u val="double"/><strike/><sz val="18"/><name val="Pretendard"/><color theme="4" tint="0.5"/></font>
        <font><sz val="10"/><name val="Fallback"/><scheme val="minor"/><color indexed="2"/></font>
      </fonts>
      <fills count="3"><fill><patternFill patternType="none"/></fill>
        <fill><patternFill patternType="solid"><fgColor theme="5" tint="-0.25"/><bgColor rgb="FF112233"/></patternFill></fill>
        <fill><patternFill patternType="darkHorizontal"><fgColor indexed="11"/><bgColor rgb="FFFAFAFA"/></patternFill></fill>
      </fills>
      <borders count="2"><border/><border diagonalUp="1" diagonalDown="1">
        <left style="thin"><color rgb="FF102030"/></left><right style="double"><color indexed="2"/></right>
        <top style="dashed"><color theme="4"/></top><bottom style="medium"><color auto="1"/></bottom>
        <diagonal style="dotted"><color rgb="FF334455"/></diagonal>
      </border></borders>
      <cellStyleXfs count="2"><xf fontId="0" fillId="0" borderId="0" numFmtId="0"/>
        <xf fontId="1" fillId="1" borderId="1" numFmtId="164"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
      </cellStyleXfs>
      <cellXfs count="4"><xf xfId="0" fontId="0" fillId="0" borderId="0" numFmtId="0"/>
        <xf xfId="1" fontId="0" fillId="0" borderId="0" numFmtId="0" applyFont="0" applyFill="0" applyBorder="0" applyNumberFormat="0" applyAlignment="0"><alignment horizontal="left"/></xf>
        <xf xfId="0" fontId="2" fillId="2" borderId="0" numFmtId="165"><alignment horizontal="right" vertical="top" shrinkToFit="1" textRotation="45" indent="2"/></xf>
        <xf xfId="0" fontId="0" fillId="0" borderId="0" numFmtId="14"/>
      </cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
    </styleSheet>"##;
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <sheetViews><sheetView workbookViewId="0" showGridLines="0" zoomScale="125"><pane state="frozen" xSplit="1" ySplit="2"/></sheetView></sheetViews>
      <sheetFormatPr defaultRowHeight="18" defaultColWidth="12"/>
      <cols><col min="1" max="1" width="8" style="2"/><col min="2" max="3" width="20" style="2" hidden="1"/></cols>
      <sheetData>
        <row r="1" s="1" customFormat="1" ht="30"><c r="A1" t="inlineStr"><is><t>보고서</t></is></c><c r="B1" s="2"><v>0.125</v></c></row>
        <row r="2"><c r="A2" t="inlineStr"><is><t>column style</t></is></c><c r="B2" s="1"/><c r="C2" s="0"><v>1234.5</v></c></row>
        <row r="3" hidden="1"><c r="A3" s="3"><v>45292</v></c></row>
        <row r="4" ht="40"><c r="A4" s="1" t="inlineStr"><is><t>merged title</t></is></c></row>
        <row r="5"><c r="A5" t="inlineStr"><is><r><rPr><b/></rPr><t>mixed</t></r></is></c></row>
      </sheetData><mergeCells count="1"><mergeCell ref="A4:C4"/></mergeCells>
      <conditionalFormatting sqref="C2"><cfRule type="cellIs" operator="greaterThan" priority="1"><formula>1000</formula></cfRule></conditionalFormatting>
      <drawing r:id="drawing1"/><tableParts count="1"><tablePart r:id="table1"/></tableParts>
    </worksheet>"#;
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        match entry.name() {
            "xl/styles.xml" => content = styles.to_owned(),
            "xl/worksheets/sheet1.xml" => content = sheet.to_owned(),
            "xl/_rels/workbook.xml.rels" => content = content.replace("</Relationships>", r#"<Relationship Id="theme" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>"#),
            _ => {}
        }
        output
            .start_file(entry.name(), SimpleFileOptions::default())
            .unwrap();
        output.write_all(content.as_bytes()).unwrap();
    }
    output
        .start_file("xl/theme/theme1.xml", SimpleFileOptions::default())
        .unwrap();
    output.write_all(br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements>
      <a:clrScheme name="custom"><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk1><a:srgbClr val="111111"/></a:dk1><a:accent1><a:srgbClr val="336699"/></a:accent1><a:accent2><a:srgbClr val="CC6633"/></a:accent2></a:clrScheme>
      <a:fontScheme name="custom"><a:majorFont><a:latin typeface="Arial"/></a:majorFont><a:minorFont><a:latin typeface="Pretendard"/></a:minorFont></a:fontScheme>
    </a:themeElements></a:theme>"#).unwrap();
    output.finish().unwrap();
    (directory, path)
}

#[test]
fn complete_cell_styles_resolve_theme_tint_and_base_style_inheritance() {
    let (_directory, path) = styled_workbook();
    let doc = read_document(&path).unwrap();
    let grid = doc.pages[0].grid.as_ref().unwrap();
    let title = &grid.cell(1, 1).unwrap().style;
    assert_eq!(title.font_name.as_deref(), Some("Pretendard"));
    assert_eq!(title.font_size, 18.0);
    assert!(title.bold && title.italic && title.strike && title.wrap_text);
    assert_eq!(title.underline, "double");
    assert_eq!(title.font_color.as_deref(), Some("FF8CB3D9"));
    assert_eq!(title.fill.as_deref(), Some("FF994D26"));
    assert_eq!(title.fill_background.as_deref(), Some("FF112233"));
    assert_eq!(title.horizontal, "center");
    assert_eq!(title.vertical, "center");
    assert_eq!(title.number_format.as_deref(), Some("#,##0.00"));
    assert_eq!(title.borders.left.style, "thin");
    assert_eq!(title.borders.left.color.as_deref(), Some("FF102030"));
    assert_eq!(title.borders.right.style, "double");
    assert_eq!(title.borders.right.color.as_deref(), Some("FFFF0000"));
    assert_eq!(title.borders.top.color.as_deref(), Some("FF336699"));
    assert_eq!(title.borders.bottom.color.as_deref(), Some("FF000000"));
    assert!(title.borders.diagonal_up && title.borders.diagonal_down);
    let explicit = &grid.cell(1, 2).unwrap().style;
    assert_eq!(
        explicit.font_size, 10.0,
        "explicit cell style wins over row style"
    );
    assert_eq!(
        explicit.font_name.as_deref(),
        Some("Pretendard"),
        "theme font scheme is resolved"
    );
    assert_eq!(explicit.fill.as_deref(), Some("FF00FF00"));
    assert_eq!(explicit.fill_pattern, "darkHorizontal");
    assert_eq!(explicit.text_rotation, 45);
    assert!(explicit.shrink_to_fit);
    assert_eq!(explicit.indent, 2);
    assert_eq!(
        grid.cell(2, 1).unwrap().style.font_size,
        10.0,
        "column style fills absent cell/row style"
    );
}

#[test]
fn sheet_geometry_blank_styles_and_raw_numbers_survive() {
    let (_directory, path) = styled_workbook();
    let doc = read_document(&path).unwrap();
    let grid = doc.pages[0].grid.as_ref().unwrap();
    assert_eq!(grid.layout.default_row_height, 18.0);
    assert_eq!(grid.layout.default_col_width, 12.0);
    assert_eq!(grid.layout.row_heights.get(&1), Some(&30.0));
    assert_eq!(grid.layout.column_widths.get(&3), Some(&20.0));
    assert!(!grid.layout.show_grid_lines);
    assert_eq!(grid.layout.zoom_scale, 125.0);
    assert_eq!((grid.layout.freeze_rows, grid.layout.freeze_cols), (2, 1));
    assert_eq!(grid.hidden_rows, [3]);
    assert_eq!(grid.hidden_cols, [2, 3]);
    let blank = grid.cell(2, 2).expect("styled blank must be kept");
    assert_eq!(blank.kind, CellKind::Empty);
    assert_eq!(blank.style.fill.as_deref(), Some("FF994D26"));
    assert_eq!(grid.cell(1, 2).unwrap().raw_number, Some(0.125));
    assert_eq!(
        grid.cell(1, 2).unwrap().value,
        "0.125",
        "display formatting must not change analytical values"
    );
    assert_eq!(
        grid.cell(1, 2).unwrap().display_value.as_deref(),
        Some("12.5%")
    );
    assert_eq!(grid.cell(3, 1).unwrap().raw_number, Some(45292.0));
    assert_eq!(grid.cell(3, 1).unwrap().kind, CellKind::Date);
    assert_eq!(
        grid.layout.default_style.font_name.as_deref(),
        Some("Arial")
    );
}

#[test]
fn unsupported_visual_features_are_recorded_instead_of_silently_claimed() {
    let (_directory, path) = styled_workbook();
    let doc = read_document(&path).unwrap();
    let unsupported = &doc.pages[0]
        .grid
        .as_ref()
        .unwrap()
        .layout
        .unsupported_features;
    for feature in [
        "누락되거나 외부에 연결된 그리기 레이어",
        "Excel 표의 자동 스타일",
        "셀 내부의 혼합 글꼴",
    ] {
        assert!(
            unsupported.iter().any(|v| v == feature),
            "missing {feature}"
        );
    }
    assert_eq!(
        doc.pages[0]
            .grid
            .as_ref()
            .unwrap()
            .layout
            .conditional_formats
            .len(),
        1
    );
    assert!(!unsupported.iter().any(|v| v == "조건부 서식"));
}

#[test]
fn drawing_extension_coordinates_are_not_worksheet_rows_columns_or_cells() {
    use std::{
        fs::File,
        io::{Read, Write},
    };
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing-coordinates.xlsx");
    let mut input = ZipArchive::new(File::open(fixture("demo.xlsx")).unwrap()).unwrap();
    let mut output = ZipWriter::new(File::create(&path).unwrap());
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        if entry.name() == "xl/worksheets/sheet1.xml" {
            content = content.replace("</worksheet>", r#"
              <extLst><ext uri="synthetic-drawing-regression">
                <xdr:twoCellAnchor xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing">
                  <xdr:from><xdr:col>4</xdr:col><xdr:row>9</xdr:row></xdr:from>
                  <xdr:to><xdr:col>7</xdr:col><xdr:row>15</xdr:row></xdr:to>
                  <xdr:c/><xdr:cols><xdr:col/></xdr:cols><xdr:sheetData><xdr:row><xdr:c/></xdr:row></xdr:sheetData>
                </xdr:twoCellAnchor>
                <col/><row/><c/><v>999</v>
              </ext></extLst></worksheet>"#);
        }
        output
            .start_file(entry.name(), SimpleFileOptions::default())
            .unwrap();
        output.write_all(content.as_bytes()).unwrap();
    }
    output.finish().unwrap();
    let parsed = read_document(&path).unwrap();
    let baseline = read_document(&fixture("demo.xlsx")).unwrap();
    let grid = parsed.pages[0].grid.as_ref().unwrap();
    let original = baseline.pages[0].grid.as_ref().unwrap();
    assert_eq!((grid.rows, grid.cols), (original.rows, original.cols));
    assert_eq!(grid.cells.len(), original.cells.len());
    assert!(grid.layout.row_heights.is_empty());
    assert!(grid.layout.column_widths.is_empty());
    assert_eq!(grid.cell(2, 1).unwrap().raw_number, Some(101.0));
    assert!(
        grid.layout
            .unsupported_features
            .iter()
            .any(|feature| feature == "시트 확장에 포함된 DrawingML 개체")
    );
}
