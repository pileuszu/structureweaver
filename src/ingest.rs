//! Excel adapter: typed cached values from calamine, geometry and style from OOXML.
//! Every worksheet is included, including hidden and very-hidden sheets. Unsupported
//! formats and resource limits are explicit errors, never silent clipping.
use crate::{
    domain::*,
    xlsx_styles::{self, Styles, Theme},
};
use anyhow::{Context, Result, bail, ensure};
use calamine::{Data, Reader as ExcelReader, Xlsx, open_workbook};
use quick_xml::{
    NsReader, Reader,
    events::{BytesStart, Event},
    name::ResolveResult,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::Read,
    path::Path,
};
use zip::ZipArchive;

const MAX_DENSE_CELLS: u64 = 5_000_000;
const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;

pub trait DocumentAdapter {
    fn supports(&self, path: &Path) -> bool;
    fn read(&self, path: &Path) -> Result<Document>;
}

pub struct ExcelAdapter;

pub fn read_document(path: &Path) -> Result<Document> {
    let adapter = ExcelAdapter;
    ensure!(
        adapter.supports(path),
        "Unsupported format. MVP supports .xlsx and .xlsm; PDF, PowerPoint and legacy .xls need separate adapters."
    );
    adapter.read(path)
}

#[derive(Default)]
struct CellMeta {
    style: Option<usize>,
    raw_number: Option<f64>,
}
#[derive(Default)]
struct SheetMeta {
    cells: BTreeMap<(u32, u32), CellMeta>,
    merges: Vec<Bounds>,
    hidden_rows: Vec<u32>,
    hidden_cols: Vec<u32>,
    rows: u32,
    cols: u32,
    layout: SheetLayout,
    shared_string_ids: BTreeSet<u32>,
}

#[derive(Clone)]
struct SheetInfo {
    name: String,
    relationship: String,
    hidden: bool,
}

struct RelationshipPart {
    path: String,
    kind: String,
}

fn attributes(e: &BytesStart<'_>) -> Result<HashMap<String, String>> {
    e.attributes()
        .map(|a| {
            let a = a?;
            let key = std::str::from_utf8(a.key.as_ref())?.to_owned();
            let value = quick_xml::escape::unescape(std::str::from_utf8(&a.value)?)?.into_owned();
            Ok((key, value))
        })
        .collect()
}
fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|&b| b == b':').next().unwrap_or(name)
}
fn number(attrs: &HashMap<String, String>, key: &str) -> Result<u32> {
    attrs
        .get(key)
        .with_context(|| format!("Missing XML attribute {key}"))?
        .parse()
        .with_context(|| format!("Invalid integer attribute {key}"))
}
fn truth(value: Option<&String>) -> bool {
    value.is_some_and(|v| v == "1" || v == "true")
}
fn xml_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<String> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("Missing workbook part {name}"))?;
    ensure!(
        entry.size() <= MAX_XML_BYTES,
        "Workbook part {name} exceeds the 64 MiB XML resource limit"
    );
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .with_context(|| format!("Cannot read UTF-8 XML part {name}"))?;
    Ok(content)
}
fn coordinate(value: &str) -> Result<(u32, u32)> {
    let mut col = 0u32;
    let mut row_text = String::new();
    for b in value.bytes().filter(|&b| b != b'$') {
        if b.is_ascii_alphabetic() && row_text.is_empty() {
            col = col
                .checked_mul(26)
                .and_then(|v| v.checked_add(u32::from(b.to_ascii_uppercase() - b'A' + 1)))
                .context("Cell coordinate overflow")?;
        } else if b.is_ascii_digit() {
            row_text.push(char::from(b));
        } else {
            bail!("Invalid Excel coordinate {value}");
        }
    }
    let row: u32 = row_text
        .parse()
        .with_context(|| format!("Invalid Excel coordinate {value}"))?;
    ensure!(
        row > 0 && row <= 1_048_576 && col > 0 && col <= 16_384,
        "Excel coordinate out of bounds: {value}"
    );
    Ok((row, col))
}
fn cell_bounds(value: &str) -> Result<Bounds> {
    let mut parts = value.split(':');
    let first = parts.next().context("Empty cell range")?;
    let last = parts.next().unwrap_or(first);
    ensure!(parts.next().is_none(), "Invalid range {value}");
    let (top, left) = coordinate(first)?;
    let (bottom, right) = coordinate(last)?;
    let bounds = Bounds {
        top,
        left,
        bottom,
        right,
    };
    ensure!(bounds.valid(), "Inverted range {value}");
    Ok(bounds)
}
fn sheet_inventory(xml: &str) -> Result<Vec<SheetInfo>> {
    let mut reader = Reader::from_str(xml);
    let mut sheets = vec![];
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if local_name(e.name().as_ref()) == b"sheet" => {
                let a = attributes(&e)?;
                sheets.push(SheetInfo {
                    name: a.get("name").context("Worksheet without name")?.clone(),
                    relationship: a
                        .get("r:id")
                        .or_else(|| a.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v))
                        .context("Worksheet without relationship")?
                        .clone(),
                    hidden: a.get("state").is_some_and(|s| s != "visible"),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(sheets)
}
fn relationships(xml: &str) -> Result<HashMap<String, RelationshipPart>> {
    let mut reader = Reader::from_str(xml);
    let mut paths = HashMap::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e)
                if local_name(e.name().as_ref()) == b"Relationship" =>
            {
                let a = attributes(&e)?;
                if a.get("TargetMode").is_some_and(|m| m == "External") {
                    continue;
                }
                if let (Some(id), Some(target)) = (a.get("Id"), a.get("Target")) {
                    let raw = if target.starts_with('/') {
                        target.trim_start_matches('/').to_owned()
                    } else {
                        format!("xl/{target}")
                    };
                    let mut normalized = vec![];
                    for piece in raw.split('/') {
                        match piece {
                            "" | "." => {}
                            ".." => {
                                ensure!(
                                    normalized.pop().is_some(),
                                    "Invalid workbook relationship path"
                                );
                            }
                            _ => normalized.push(piece),
                        }
                    }
                    paths.insert(
                        id.clone(),
                        RelationshipPart {
                            path: normalized.join("/"),
                            kind: a.get("Type").cloned().unwrap_or_default(),
                        },
                    );
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paths)
}
fn style_at(styles: &Styles, index: usize) -> CellStyle {
    styles
        .cells
        .get(index)
        .cloned()
        .unwrap_or_else(|| styles.default_style.clone())
}
fn decimal(attrs: &HashMap<String, String>, key: &str) -> Option<f32> {
    attrs
        .get(key)
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
}
fn sheet_metadata(xml: &str, styles: &Styles, date1904: bool) -> Result<SheetMeta> {
    let mut reader = NsReader::from_str(xml);
    let mut meta = SheetMeta::default();
    meta.layout.default_style = styles.default_style.clone();
    meta.layout.date1904 = date1904;
    let mut current_cell = None;
    let mut cell_type = String::new();
    let mut in_value = false;
    let mut unsupported = BTreeSet::new();
    // DrawingML and extension parts reuse names such as col, row, c and v.
    // Only SpreadsheetML elements at their worksheet schema locations are cells.
    let mut path: Vec<(String, bool)> = Vec::new();
    loop {
        let (namespace, event) = reader.read_resolved_event()?;
        let spreadsheet = match namespace {
            ResolveResult::Bound(namespace) => matches!(
                namespace.as_ref(),
                b"http://schemas.openxmlformats.org/spreadsheetml/2006/main"
                    | b"http://purl.oclc.org/ooxml/spreadsheetml/main"
            ),
            ResolveResult::Unbound => true,
            ResolveResult::Unknown(_) => false,
        };
        let empty = matches!(&event, Event::Empty(_));
        let opening = if let Event::Start(element) = &event {
            Some((
                std::str::from_utf8(local_name(element.name().as_ref()))?.to_owned(),
                spreadsheet,
            ))
        } else {
            None
        };
        let at = |parents: &[&str]| {
            path.len() == parents.len()
                && path
                    .iter()
                    .zip(parents)
                    .all(|((name, valid), expected)| *valid && name == expected)
        };
        match event {
            Event::Start(e) | Event::Empty(e) if spreadsheet => {
                let a = attributes(&e)?;
                match local_name(e.name().as_ref()) {
                    b"c" if at(&["worksheet", "sheetData", "row"]) => {
                        let position =
                            coordinate(a.get("r").context("Cell without explicit coordinate")?)?;
                        let style = a.get("s").map(|s| s.parse::<usize>()).transpose()?;
                        meta.cells.insert(
                            position,
                            CellMeta {
                                style,
                                raw_number: None,
                            },
                        );
                        meta.rows = meta.rows.max(position.0);
                        meta.cols = meta.cols.max(position.1);
                        current_cell = (!empty).then_some(position);
                        cell_type = a.get("t").cloned().unwrap_or_default();
                    }
                    b"v" if current_cell.is_some()
                        && at(&["worksheet", "sheetData", "row", "c"]) =>
                    {
                        in_value = !empty
                    }
                    b"r" if current_cell.is_some() => {
                        unsupported.insert("셀 내부의 혼합 글꼴".to_owned());
                    }
                    b"mergeCell" if at(&["worksheet", "mergeCells"]) => {
                        let bounds = cell_bounds(a.get("ref").context("Merge without range")?)?;
                        meta.rows = meta.rows.max(bounds.bottom);
                        meta.cols = meta.cols.max(bounds.right);
                        meta.merges.push(bounds);
                    }
                    b"sheetFormatPr" if at(&["worksheet"]) => {
                        if let Some(value) = decimal(&a, "defaultRowHeight") {
                            meta.layout.default_row_height = value;
                        }
                        if let Some(value) = decimal(&a, "defaultColWidth") {
                            meta.layout.default_col_width = value;
                        }
                        if truth(a.get("zeroHeight")) {
                            unsupported.insert("기본 숨김 행 설정".to_owned());
                        }
                    }
                    b"sheetView" if at(&["worksheet", "sheetViews"]) => {
                        if a.contains_key("showGridLines") {
                            meta.layout.show_grid_lines = truth(a.get("showGridLines"));
                        }
                        if let Some(value) = decimal(&a, "zoomScale") {
                            meta.layout.zoom_scale = value.clamp(10.0, 400.0);
                        }
                        if truth(a.get("rightToLeft")) {
                            unsupported.insert("오른쪽에서 왼쪽으로 시트 배치".to_owned());
                        }
                        if a.get("view").is_some_and(|v| v != "normal") {
                            unsupported.insert("페이지 레이아웃 보기".to_owned());
                        }
                    }
                    b"pane" if at(&["worksheet", "sheetViews", "sheetView"]) => {
                        if a.get("state")
                            .is_some_and(|v| v == "frozen" || v == "frozenSplit")
                        {
                            meta.layout.freeze_rows = decimal(&a, "ySplit").unwrap_or(0.0) as u32;
                            meta.layout.freeze_cols = decimal(&a, "xSplit").unwrap_or(0.0) as u32;
                        } else if a.contains_key("xSplit") || a.contains_key("ySplit") {
                            unsupported.insert("분할 창".to_owned());
                        }
                    }
                    b"row" if at(&["worksheet", "sheetData"]) => {
                        let row = number(&a, "r")?;
                        ensure!(
                            (1..=1_048_576).contains(&row),
                            "Row is outside Excel bounds"
                        );
                        if truth(a.get("hidden")) {
                            meta.hidden_rows.push(row);
                        }
                        if let Some(value) = decimal(&a, "ht") {
                            meta.layout.row_heights.insert(row, value);
                        }
                        if let Some(style) = a.get("s").map(|v| v.parse::<usize>()).transpose()?
                            && a.get("customFormat")
                                .is_none_or(|v| v != "0" && v != "false")
                        {
                            meta.layout.row_styles.insert(row, style_at(styles, style));
                        }
                    }
                    b"col" if at(&["worksheet", "cols"]) => {
                        let min = number(&a, "min")?;
                        let max = number(&a, "max")?;
                        ensure!(
                            min > 0 && min <= max && max <= 16_384,
                            "Columns are outside Excel bounds"
                        );
                        if truth(a.get("hidden")) {
                            meta.hidden_cols.extend(min..=max);
                        }
                        if let Some(value) = decimal(&a, "width") {
                            for col in min..=max {
                                meta.layout.column_widths.insert(col, value);
                            }
                        }
                        if let Some(style) =
                            a.get("style").map(|v| v.parse::<usize>()).transpose()?
                        {
                            for col in min..=max {
                                meta.layout
                                    .column_styles
                                    .insert(col, style_at(styles, style));
                            }
                        }
                    }
                    b"conditionalFormatting" => {
                        unsupported.insert("조건부 서식".to_owned());
                    }
                    b"drawing" | b"legacyDrawing" | b"legacyDrawingHF" | b"picture" => {
                        unsupported.insert("그림·차트·도형".to_owned());
                    }
                    b"tableParts" => {
                        unsupported.insert("Excel 표의 자동 스타일".to_owned());
                    }
                    _ => {}
                }
            }
            Event::Start(e) | Event::Empty(e) => match local_name(e.name().as_ref()) {
                b"conditionalFormatting" | b"dataBar" | b"colorScale" | b"iconSet" => {
                    unsupported.insert("조건부 서식".to_owned());
                }
                b"drawing" | b"sp" | b"graphicFrame" | b"pic" | b"twoCellAnchor"
                | b"oneCellAnchor" | b"absoluteAnchor" => {
                    unsupported.insert("그림·차트·도형".to_owned());
                }
                _ => {}
            },
            Event::Text(text) if in_value => {
                if let Some(position) = current_cell {
                    let raw = quick_xml::escape::unescape(std::str::from_utf8(text.as_ref())?)?;
                    if cell_type.is_empty() || cell_type == "n" {
                        meta.cells
                            .get_mut(&position)
                            .context("Missing current cell")?
                            .raw_number = raw.parse::<f64>().ok().filter(|v| v.is_finite());
                    } else if cell_type == "s"
                        && let Ok(index) = raw.parse::<u32>()
                    {
                        meta.shared_string_ids.insert(index);
                    }
                }
            }
            Event::End(e) => {
                match local_name(e.name().as_ref()) {
                    b"c" if spreadsheet && at(&["worksheet", "sheetData", "row", "c"]) => {
                        current_cell = None;
                        in_value = false;
                    }
                    b"v" if spreadsheet && at(&["worksheet", "sheetData", "row", "c", "v"]) => {
                        in_value = false
                    }
                    _ => {}
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        if let Some(element) = opening {
            ensure!(path.len() < 128, "Worksheet XML nesting limit exceeded");
            path.push(element);
        }
    }
    meta.layout.unsupported_features = unsupported.into_iter().collect();
    meta.hidden_rows.sort_unstable();
    meta.hidden_rows.dedup();
    meta.hidden_cols.sort_unstable();
    meta.hidden_cols.dedup();
    ensure!(
        u64::from(meta.rows) * u64::from(meta.cols) <= MAX_DENSE_CELLS,
        "Sheet extent {} × {} exceeds the MVP resource budget of {MAX_DENSE_CELLS} cells. Split the workbook; no content was silently truncated.",
        meta.rows,
        meta.cols
    );
    Ok(meta)
}
fn rich_shared_strings(xml: &str) -> Result<BTreeSet<u32>> {
    let mut reader = Reader::from_str(xml);
    let mut current = 0u32;
    let mut inside = false;
    let mut rich = BTreeSet::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if local_name(e.name().as_ref()) == b"si" => inside = true,
            Event::Empty(e) if local_name(e.name().as_ref()) == b"si" => current += 1,
            Event::Start(e) | Event::Empty(e)
                if local_name(e.name().as_ref()) == b"r" && inside =>
            {
                rich.insert(current);
            }
            Event::End(e) if local_name(e.name().as_ref()) == b"si" => {
                inside = false;
                current += 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(rich)
}
fn uses_1904_dates(xml: &str) -> Result<bool> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if local_name(e.name().as_ref()) == b"workbookPr" => {
                return Ok(truth(attributes(&e)?.get("date1904")));
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}
fn typed_value(value: &Data) -> (String, CellKind) {
    match value {
        Data::Empty => (String::new(), CellKind::Empty),
        Data::String(s) => (s.clone(), CellKind::Text),
        Data::Int(i) => (i.to_string(), CellKind::Number),
        Data::Float(f) => (f.to_string(), CellKind::Number),
        Data::Bool(b) => (b.to_string(), CellKind::Boolean),
        Data::DateTime(d) => (
            d.as_datetime()
                .map(|v| v.to_string())
                .unwrap_or_else(|| d.to_string()),
            CellKind::Date,
        ),
        Data::DateTimeIso(d) => (d.clone(), CellKind::Date),
        Data::DurationIso(d) => (d.clone(), CellKind::Text),
        Data::Error(e) => (e.to_string(), CellKind::Error),
    }
}

impl DocumentAdapter for ExcelAdapter {
    fn supports(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xlsx") || e.eq_ignore_ascii_case("xlsm"))
    }
    fn read(&self, path: &Path) -> Result<Document> {
        ensure!(
            self.supports(path),
            "Excel adapter supports .xlsx and .xlsm only"
        );
        let mut file =
            File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let id = format!("{:x}", hasher.finalize());
        let mut zip =
            ZipArchive::new(File::open(path)?).context("Not a valid OOXML Excel ZIP archive")?;
        let mut expanded = 0u64;
        for i in 0..zip.len() {
            expanded = expanded
                .checked_add(zip.by_index(i)?.size())
                .context("ZIP size overflow")?;
        }
        ensure!(
            expanded <= MAX_EXPANDED_BYTES,
            "Workbook expands beyond the 256 MiB MVP resource limit"
        );
        let workbook_xml = xml_entry(&mut zip, "xl/workbook.xml")?;
        let sheets = sheet_inventory(&workbook_xml)?;
        let date1904 = uses_1904_dates(&workbook_xml)?;
        let relationships = relationships(&xml_entry(&mut zip, "xl/_rels/workbook.xml.rels")?)?;
        // calamine expects these two standard part names. Reject unusual layouts rather
        // than silently decode dates as numbers or lose shared strings.
        for part in relationships.values() {
            if part.kind.ends_with("/styles") {
                ensure!(
                    part.path == "xl/styles.xml",
                    "Nonstandard styles part {} is not supported by the MVP Excel decoder",
                    part.path
                );
            }
            if part.kind.ends_with("/sharedStrings") {
                ensure!(
                    part.path == "xl/sharedStrings.xml",
                    "Nonstandard shared strings part {} is not supported by the MVP Excel decoder",
                    part.path
                );
            }
        }
        let theme = if let Some(part) = relationships.values().find(|p| p.kind.ends_with("/theme"))
        {
            Theme::from_xml(&xml_entry(&mut zip, &part.path)?)?
        } else {
            Theme::default()
        };
        let style_data = if zip.index_for_name("xl/styles.xml").is_some() {
            xlsx_styles::styles(&xml_entry(&mut zip, "xl/styles.xml")?, &theme)?
        } else {
            Styles::default()
        };
        let rich_strings = if zip.index_for_name("xl/sharedStrings.xml").is_some() {
            rich_shared_strings(&xml_entry(&mut zip, "xl/sharedStrings.xml")?)?
        } else {
            BTreeSet::new()
        };
        let mut metadata = Vec::new();
        for sheet in &sheets {
            let part = relationships
                .get(&sheet.relationship)
                .with_context(|| format!("No worksheet relationship for {}", sheet.name))?;
            // Chart sheets are not worksheets and have no cell grid; retain them explicitly.
            if !part.kind.ends_with("/worksheet") {
                metadata.push(None);
                continue;
            }
            let mut meta = sheet_metadata(&xml_entry(&mut zip, &part.path)?, &style_data, date1904)
                .with_context(|| format!("Worksheet {}", sheet.name))?;
            if !meta.shared_string_ids.is_disjoint(&rich_strings)
                && !meta
                    .layout
                    .unsupported_features
                    .iter()
                    .any(|v| v == "셀 내부의 혼합 글꼴")
            {
                meta.layout
                    .unsupported_features
                    .push("셀 내부의 혼합 글꼴".into());
            }
            metadata.push(Some(meta));
        }
        drop(zip);
        let mut workbook: Xlsx<_> = open_workbook(path).context("Cannot decode Excel workbook")?;
        let mut pages = Vec::new();
        let mut warnings = vec!["Formula values are Excel's saved cached results; formulas are preserved but are not recalculated.".to_owned()];
        for (index, (sheet, metadata)) in sheets.iter().zip(metadata).enumerate() {
            let page_id = format!("sheet-{}", index + 1);
            let Some(mut meta) = metadata else {
                warnings.push(format!("{} is a non-worksheet sheet (for example a chart sheet); it is listed without a cell grid.", sheet.name));
                pages.push(Page {
                    id: page_id,
                    name: sheet.name.clone(),
                    kind: PageKind::Sheet,
                    hidden: sheet.hidden,
                    grid: None,
                    blocks: vec![],
                });
                continue;
            };
            let values = workbook
                .worksheet_range(&sheet.name)
                .with_context(|| format!("Read values in {}", sheet.name))?;
            let formulas = workbook
                .worksheet_formula(&sheet.name)
                .with_context(|| format!("Read formulas in {}", sheet.name))?;
            let mut cells = Vec::new();
            let mut missing_formula_cache = 0usize;
            for ((row, col), cell_meta) in meta.cells {
                let position = (row - 1, col - 1);
                let data = values.get_value(position).unwrap_or(&Data::Empty);
                let (value, kind) = typed_value(data);
                let formula = formulas
                    .get_value(position)
                    .filter(|s| !s.is_empty())
                    .cloned();
                if formula.is_some() && kind == CellKind::Empty {
                    missing_formula_cache += 1;
                }
                let style = cell_meta
                    .style
                    .map(|index| style_at(&style_data, index))
                    .or_else(|| meta.layout.row_styles.get(&row).cloned())
                    .or_else(|| meta.layout.column_styles.get(&col).cloned())
                    .unwrap_or_else(|| meta.layout.default_style.clone());
                if style.fill_pattern == "gradient"
                    && !meta
                        .layout
                        .unsupported_features
                        .iter()
                        .any(|s| s == "그라데이션 채우기")
                {
                    meta.layout
                        .unsupported_features
                        .push("그라데이션 채우기".into());
                }
                let mut cell = Cell {
                    row,
                    col,
                    value,
                    kind,
                    formula,
                    style,
                    raw_number: cell_meta.raw_number,
                    display_value: None,
                };
                cell.display_value = Some(crate::number_format::display_value(
                    &cell,
                    meta.layout.date1904,
                ));
                cells.push(cell);
            }
            if missing_formula_cache > 0 {
                warnings.push(format!("{} has {missing_formula_cache} formulas without saved values; these cells remain unresolved.", sheet.name));
            }
            pages.push(Page {
                id: page_id,
                name: sheet.name.clone(),
                kind: PageKind::Sheet,
                hidden: sheet.hidden,
                grid: Some(Grid {
                    rows: meta.rows,
                    cols: meta.cols,
                    cells,
                    merges: meta.merges,
                    hidden_rows: meta.hidden_rows,
                    hidden_cols: meta.hidden_cols,
                    layout: meta.layout,
                }),
                blocks: vec![],
            });
        }
        Ok(Document {
            id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            format: path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase(),
            pages,
            warnings,
        })
    }
}
