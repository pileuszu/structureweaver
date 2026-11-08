//! Excel adapter: typed cached values from calamine, geometry and style from OOXML.
//! Every worksheet is included, including hidden and very-hidden sheets. Unsupported
//! formats and resource limits are explicit errors, never silent clipping.
use crate::domain::*;
use anyhow::{Context, Result, bail, ensure};
use calamine::{Data, Reader as ExcelReader, Xlsx, open_workbook};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
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
struct SheetMeta {
    cells: BTreeMap<(u32, u32), usize>,
    merges: Vec<Bounds>,
    hidden_rows: Vec<u32>,
    hidden_cols: Vec<u32>,
    rows: u32,
    cols: u32,
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
fn styles(xml: &str) -> Result<Vec<CellStyle>> {
    let mut reader = Reader::from_str(xml);
    let mut fonts = Vec::new();
    let mut fills = Vec::new();
    let mut result = Vec::new();
    let mut in_fonts = false;
    let mut in_font = false;
    let mut bold = false;
    let mut in_fills = false;
    let mut in_fill = false;
    let mut fill = None;
    let mut in_xfs = false;
    let mut current: Option<CellStyle> = None;
    loop {
        let event = reader.read_event()?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let a = attributes(&e)?;
                match local_name(e.name().as_ref()) {
                    b"fonts" => in_fonts = true,
                    b"font" if in_fonts => {
                        in_font = !empty;
                        bold = false;
                        if empty {
                            fonts.push(false);
                        }
                    }
                    b"b" if in_font => bold = a.get("val").is_none_or(|v| v != "0" && v != "false"),
                    b"fills" => in_fills = true,
                    b"fill" if in_fills => {
                        in_fill = !empty;
                        fill = None;
                        if empty {
                            fills.push(None);
                        }
                    }
                    b"fgColor" if in_fill => {
                        fill = a
                            .get("rgb")
                            .cloned()
                            .or_else(|| a.get("indexed").map(|v| format!("indexed:{v}")))
                            .or_else(|| a.get("theme").map(|v| format!("theme:{v}")))
                    }
                    b"cellXfs" => in_xfs = true,
                    b"xf" if in_xfs => {
                        if let Some(style) = current.take() {
                            result.push(style);
                        }
                        let font_id = a
                            .get("fontId")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let fill_id = a
                            .get("fillId")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        current = Some(CellStyle {
                            bold: fonts.get(font_id).copied().unwrap_or(false),
                            indent: 0,
                            fill: fills.get(fill_id).cloned().flatten(),
                        });
                    }
                    b"alignment" if in_xfs => {
                        if let Some(style) = current.as_mut() {
                            style.indent =
                                a.get("indent").map(|v| v.parse()).transpose()?.unwrap_or(0);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match local_name(e.name().as_ref()) {
                b"font" if in_font => {
                    fonts.push(bold);
                    in_font = false;
                }
                b"fonts" => in_fonts = false,
                b"fill" if in_fill => {
                    fills.push(fill.take());
                    in_fill = false;
                }
                b"fills" => in_fills = false,
                b"cellXfs" => {
                    if let Some(style) = current.take() {
                        result.push(style);
                    }
                    in_xfs = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}
fn sheet_metadata(xml: &str) -> Result<SheetMeta> {
    let mut reader = Reader::from_str(xml);
    let mut meta = SheetMeta::default();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => {
                let a = attributes(&e)?;
                match local_name(e.name().as_ref()) {
                    b"c" => {
                        let (row, col) =
                            coordinate(a.get("r").context("Cell without explicit coordinate")?)?;
                        let style = a
                            .get("s")
                            .map(|s| s.parse::<usize>())
                            .transpose()?
                            .unwrap_or(0);
                        meta.cells.insert((row, col), style);
                        meta.rows = meta.rows.max(row);
                        meta.cols = meta.cols.max(col);
                    }
                    b"mergeCell" => {
                        let bounds = cell_bounds(a.get("ref").context("Merge without range")?)?;
                        meta.rows = meta.rows.max(bounds.bottom);
                        meta.cols = meta.cols.max(bounds.right);
                        meta.merges.push(bounds);
                    }
                    b"row" if truth(a.get("hidden")) => {
                        let row = number(&a, "r")?;
                        ensure!(
                            (1..=1_048_576).contains(&row),
                            "Hidden row is outside Excel bounds"
                        );
                        meta.hidden_rows.push(row);
                    }
                    b"col" if truth(a.get("hidden")) => {
                        let min = number(&a, "min")?;
                        let max = number(&a, "max")?;
                        ensure!(
                            min > 0 && min <= max && max <= 16_384,
                            "Hidden columns are outside Excel bounds"
                        );
                        meta.hidden_cols.extend(min..=max);
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
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
        let sheets = sheet_inventory(&xml_entry(&mut zip, "xl/workbook.xml")?)?;
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
        let style_data = if zip.index_for_name("xl/styles.xml").is_some() {
            styles(&xml_entry(&mut zip, "xl/styles.xml")?)?
        } else {
            vec![CellStyle::default()]
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
            metadata.push(Some(
                sheet_metadata(&xml_entry(&mut zip, &part.path)?)
                    .with_context(|| format!("Worksheet {}", sheet.name))?,
            ));
        }
        drop(zip);
        let mut workbook: Xlsx<_> = open_workbook(path).context("Cannot decode Excel workbook")?;
        let mut pages = Vec::new();
        let mut warnings = vec!["Formula values are Excel's saved cached results; formulas are preserved but are not recalculated.".to_owned()];
        for (index, (sheet, metadata)) in sheets.iter().zip(metadata).enumerate() {
            let page_id = format!("sheet-{}", index + 1);
            let Some(meta) = metadata else {
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
            for ((row, col), style_index) in meta.cells {
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
                cells.push(Cell {
                    row,
                    col,
                    value,
                    kind,
                    formula,
                    style: style_data.get(style_index).cloned().unwrap_or_default(),
                });
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
