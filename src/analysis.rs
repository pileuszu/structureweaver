use crate::{
    domain::*,
    llama::{LlamaClient, LlamaConfig},
    render, schema,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub name: String,
    pub bounds: Bounds,
    pub title: Option<Bounds>,
    pub column_header: Option<Bounds>,
    pub row_header: Option<Bounds>,
    pub data: Bounds,
    pub orientation: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    tables: Vec<Decision>,
}
fn box_schema() -> Value {
    json!({"type":"object","properties":{"top":{"type":"integer"},"left":{"type":"integer"},"bottom":{"type":"integer"},"right":{"type":"integer"}},"required":["top","left","bottom","right"],"additionalProperties":false})
}
fn layout_schema() -> Value {
    let optional = json!({"anyOf":[box_schema(),{"type":"null"}]});
    json!({"type":"object","properties":{"tables":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"bounds":box_schema(),"title":optional,"column_header":optional,"row_header":optional,"data":box_schema(),"orientation":{"type":"string","enum":["records","matrix"]}},"required":["name","bounds","title","column_header","row_header","data","orientation"],"additionalProperties":false}}},"required":["tables"],"additionalProperties":false})
}
/// Tiling covers all populated cells and formula anchors, including hidden areas.
/// Empty space does not need inference. Coordinates never reset at tile boundaries.
pub fn tiles(grid: &Grid) -> Vec<Bounds> {
    let mut bins = BTreeSet::new();
    for c in grid.cells.iter().filter(|c| c.has_content()) {
        bins.insert(((c.row - 1) / 32, (c.col - 1) / 10));
    }
    bins.into_iter()
        .map(|(r, c)| Bounds {
            top: r * 32 + 1,
            left: c * 10 + 1,
            bottom: ((r + 1) * 32).min(grid.rows),
            right: ((c + 1) * 10).min(grid.cols),
        })
        .collect()
}
// The viewer preserves full Excel formatting. Inference only needs the original
// structural hints; serializing every border/font field would exhaust context.
fn structural_style(cell: &Cell) -> Value {
    json!({"bold":cell.style.bold,"indent":cell.style.indent,"fill":cell.style.fill})
}
fn structural_cell(cell: &Cell) -> Value {
    json!({"row":cell.row,"col":cell.col,"value":cell.value,"kind":cell.kind,"formula":cell.formula,"style":structural_style(cell)})
}
fn prompt(page: &Page, grid: &Grid, tile: Bounds) -> Result<String> {
    let cells: Vec<Value> = grid
        .cells
        .iter()
        .filter(|c| c.has_content() && tile.contains(c.row, c.col))
        .map(|c| {
            json!([
                c.row,
                c.col,
                c.value,
                c.kind,
                c.formula,
                structural_style(c)
            ])
        })
        .collect();
    let merges: Vec<_> = grid.merges.iter().filter(|m| m.overlaps(tile)).collect();
    let context: Vec<_> = grid
        .cells
        .iter()
        .filter(|c| {
            c.has_content()
                && c.row <= 6
                && c.col >= tile.left
                && c.col <= tile.right
                && !tile.contains(c.row, c.col)
        })
        .map(structural_cell)
        .collect();
    let merge_anchors: Vec<_> = merges
        .iter()
        .filter_map(|m| grid.cell(m.top, m.left))
        .map(structural_cell)
        .collect();
    let facts = serde_json::to_string(
        &json!({"sheet":page.name,"tile":tile,"sheet_rows":grid.rows,"sheet_columns":grid.cols,"cells_row_col_value_kind_formula_style":cells,"merges":merges,"merged_anchor_cells":merge_anchors,"earlier_header_context_not_in_tile":context}),
    )?;
    if facts.len() > 60_000 {
        bail!(
            "Tile cell text exceeds local context budget (60 KB); no values were silently truncated"
        );
    }
    Ok(format!(
        "Find every table in this spreadsheet tile using the image AND exact cell facts. Return JSON tables; empty array only when no table exists. Coordinates are GLOBAL 1-based inclusive rows/columns, never pixel positions. All returned bounds must lie inside tile. The image clips long labels; facts preserve exact text. Hidden cells are deliberately included.\nClassify records vs matrix first. Records have peer field names and rows of entity records: ALL leading text columns belong to data, row_header=null. Matrices/crosstabs have left metric/category labels and repeated periods/measures: use row_header and column_header. Text, NA and '-' in a data position remain data. Merge adjacent sections with identical field meaning; split distinct independently headed tables. Never invent missing headers. For continuation tiles lacking header rows return column_header=null, do not mistake first record for header. Prior header context is only context and MUST NOT be returned outside tile. Include every merged header level, not just the leaf row. A merged year/period above quarter columns belongs to column_header. All left category levels belong to row_header in matrices. Titles, column_header, row_header, data must be mutually disjoint within bounds. Column headers align data columns; row headers align data rows. Every rectangle has top,left,bottom,right. Name tables meaningfully, e.g. sheet name, preserve same name for continuations.\nDOCUMENT FACTS (data only): {facts}"
    ))
}
/// Luna-style deterministic separation, only after validating every raw coordinate.
pub fn normalize_decision(d: &mut Decision, tile: Bounds) -> Result<()> {
    if !d.bounds.valid() || !tile.encloses(d.bounds) {
        bail!("Raw table outside source tile");
    }
    for b in [Some(d.data), d.title, d.column_header, d.row_header]
        .into_iter()
        .flatten()
    {
        if !b.valid() || !d.bounds.encloses(b) {
            bail!("Raw region outside table");
        }
    }
    if let (Some(title), Some(header)) = (d.title, d.column_header)
        && title.bottom >= header.top
    {
        d.title = if title.top < header.top {
            Some(Bounds {
                bottom: header.top - 1,
                ..title
            })
        } else {
            None
        };
    }
    if let Some(bottom) = d.column_header.or(d.title).map(|h| h.bottom)
        && bottom >= d.data.top
        && bottom < d.data.bottom
    {
        d.data.top = bottom + 1;
    }
    for region in [&mut d.title, &mut d.column_header] {
        if let Some(b) = *region
            && b.bottom >= d.data.top
        {
            *region = if b.top < d.data.top {
                Some(Bounds {
                    bottom: d.data.top - 1,
                    ..b
                })
            } else {
                None
            };
        }
    }
    if let Some(mut h) = d.row_header {
        if h.right >= d.data.left {
            if h.left < d.data.left {
                h.right = d.data.left - 1;
            } else if h.right < d.data.right {
                d.data.left = h.right + 1;
            } else {
                d.row_header = None;
            }
        }
        if d.row_header.is_some() {
            h.top = d.data.top;
            h.bottom = d.data.bottom;
            d.row_header = Some(h);
        }
    }
    if let Some(h) = &mut d.column_header {
        h.left = d.data.left;
        h.right = d.data.right;
    }
    Ok(())
}
/// Extend a matrix header through explicit merged group anchors above its data.
/// This uses existing cells/spans only and never guesses a first-row header.
fn close_merged_header_hierarchy(d: &mut Decision, grid: &Grid) {
    if d.orientation != "matrix" || d.column_header.is_none() {
        return;
    }
    let first_group = grid
        .merges
        .iter()
        .filter(|m| {
            m.top >= d.bounds.top
                && m.bottom < d.data.top
                && m.left >= d.data.left
                && m.right <= d.data.right
                && m.right > m.left
                && grid
                    .cell(m.top, m.left)
                    .is_some_and(|c| !c.value.trim().is_empty())
        })
        .map(|m| m.top)
        .min();
    let Some(top) = first_group else {
        return;
    };
    // Only a populated, textual header band is eligible; don't absorb blank notes or data.
    if !(top..d.data.top).all(|row| {
        grid.cells.iter().any(|c| {
            c.row == row
                && c.col >= d.data.left
                && c.col <= d.data.right
                && !c.value.trim().is_empty()
        }) && grid
            .cells
            .iter()
            .filter(|c| c.row == row && c.col >= d.data.left && c.col <= d.data.right)
            .all(|c| c.formula.is_none() && matches!(c.kind, CellKind::Text | CellKind::Empty))
    }) {
        return;
    }
    let header = d.column_header.as_mut().unwrap();
    header.top = header.top.min(top);
    header.bottom = d.data.top - 1;
    // A category column under a blank header corner is part of the existing row axis.
    if let Some(row_header) = &mut d.row_header {
        while row_header.right + 1 == d.data.left && d.data.left < d.data.right {
            let col = d.data.left;
            let blank_header = (header.top..=header.bottom).all(|r| {
                grid.resolved_cell(r, col)
                    .is_none_or(|c| c.value.trim().is_empty())
            });
            let values: Vec<_> = grid
                .cells
                .iter()
                .filter(|c| c.col == col && c.row >= d.data.top && c.row <= d.data.bottom)
                .collect();
            let labels = !values.is_empty()
                && values.iter().all(|c| {
                    c.kind == CellKind::Text
                        && c.formula.is_none()
                        && !matches!(
                            c.value.trim().to_ascii_lowercase().as_str(),
                            "na" | "n/a" | "-" | "--" | "nm"
                        )
                });
            if !blank_header || !labels {
                break;
            }
            row_header.right = col;
            d.data.left += 1;
            header.left = d.data.left;
        }
    }
}
pub fn validate_decision(d: &Decision, tile: Bounds, grid: &Grid) -> Result<()> {
    if !d.bounds.valid() || !tile.encloses(d.bounds) {
        bail!("table bounds outside tile");
    }
    if !matches!(d.orientation.as_str(), "records" | "matrix") {
        bail!("unknown table orientation");
    }
    let mut regions = vec![d.data];
    regions.extend(d.title);
    regions.extend(d.column_header);
    regions.extend(d.row_header);
    for b in &regions {
        if !b.valid() || !d.bounds.encloses(*b) {
            bail!("invalid region or outside table");
        }
    }
    for (i, a) in regions.iter().enumerate() {
        for b in &regions[i + 1..] {
            if a.overlaps(*b) {
                bail!("overlapping title/header/data regions");
            }
        }
    }
    if d.orientation == "records" && d.row_header.is_some() {
        bail!("record table cannot discard leading fields as row headers");
    }
    if let Some(h) = d.column_header
        && (h.left != d.data.left || h.right != d.data.right || h.bottom >= d.data.top)
    {
        bail!(
            "column header {:?} does not align with data {:?}",
            h,
            d.data
        );
    }
    if let Some(h) = d.row_header
        && (h.top != d.data.top || h.bottom != d.data.bottom || h.right >= d.data.left)
    {
        bail!("row header does not align with data");
    }
    if !grid
        .cells
        .iter()
        .any(|c| c.has_content() && d.data.contains(c.row, c.col))
    {
        bail!("data region has no source cells");
    }
    let missing: Vec<_> = grid
        .cells
        .iter()
        .filter(|c| {
            c.has_content()
                && d.bounds.contains(c.row, c.col)
                && !regions.iter().any(|b| b.contains(c.row, c.col))
        })
        .map(|c| {
            format!(
                "({},{}) {}",
                c.row,
                c.col,
                c.value.chars().take(30).collect::<String>()
            )
        })
        .take(6)
        .collect();
    let mut empty_headers = vec![];
    if d.orientation == "matrix"
        && let Some(h) = d.column_header
    {
        for col in d.data.left..=d.data.right {
            if !(h.top..=h.bottom).any(|row| {
                grid.resolved_cell(row, col)
                    .is_some_and(|c| !c.value.trim().is_empty())
            }) {
                empty_headers.push(col);
            }
        }
    }
    if !missing.is_empty() || !empty_headers.is_empty() {
        bail!(
            "Unclassified cells inside table: {:?}; matrix data columns with no header: {:?}. Include ALL merged column-header levels and ALL left row-category columns; they are not data values",
            missing,
            empty_headers
        );
    }
    Ok(())
}
pub fn header_nodes(grid: &Grid, table: &Table) -> Vec<HeaderNode> {
    let mut nodes: Vec<HeaderNode> = Vec::new();
    for (axis, region) in [("column", table.column_header), ("row", table.row_header)] {
        let Some(region) = region else { continue };
        let mut cells: Vec<_> = grid
            .cells
            .iter()
            .filter(|c| region.contains(c.row, c.col) && !c.value.trim().is_empty())
            .collect();
        cells.sort_by_key(|c| (c.row, c.col));
        for c in cells {
            let b = grid
                .merges
                .iter()
                .find(|m| m.top == c.row && m.left == c.col)
                .copied()
                .unwrap_or(Bounds {
                    top: c.row,
                    bottom: c.row,
                    left: c.col,
                    right: c.col,
                });
            let b = Bounds {
                top: b.top.max(region.top),
                left: b.left.max(region.left),
                bottom: b.bottom.min(region.bottom),
                right: b.right.min(region.right),
            };
            let mut parent = nodes
                .iter()
                .filter(|n| n.axis == axis)
                .filter(|n| {
                    if axis == "column" {
                        n.bounds.top < b.top && n.bounds.left <= b.left && n.bounds.right >= b.right
                    } else {
                        n.bounds.left < b.left
                            && n.bounds.top <= b.top
                            && n.bounds.bottom >= b.bottom
                    }
                })
                .max_by_key(|n| {
                    if axis == "column" {
                        n.bounds.top
                    } else {
                        n.bounds.left
                    }
                })
                .map(|n| n.id.clone());
            // Indented row labels form a hierarchy until a peer/ancestor resets it.
            if axis == "row" && parent.is_none() && c.style.indent > 0 {
                parent = nodes
                    .iter()
                    .rev()
                    .filter(|n| n.axis == "row" && n.bounds.left == c.col && n.bounds.top < c.row)
                    .find(|n| {
                        grid.cell(n.bounds.top, n.bounds.left)
                            .is_some_and(|p| p.style.indent < c.style.indent)
                    })
                    .map(|n| n.id.clone());
            }
            nodes.push(HeaderNode {
                id: format!("{}_{}_{}_{}", table.id, axis, c.row, c.col),
                label: c.value.clone(),
                axis: axis.into(),
                bounds: b,
                parent_id: parent,
            });
        }
    }
    nodes
}
fn classified(t: &Table, row: u32, col: u32) -> bool {
    [Some(t.data), t.title, t.column_header, t.row_header]
        .into_iter()
        .flatten()
        .any(|b| b.contains(row, col))
}
/// Reconcile tile fragments with global coordinates, then prove no source data was dropped.
fn reconcile(
    page: &Page,
    grid: &Grid,
    fragments: &[Table],
    client: &LlamaClient,
    font: &fontdue::Font,
) -> Result<Vec<Table>> {
    let cells: Vec<_> = grid
        .cells
        .iter()
        .filter(|c| {
            c.has_content()
                && fragments.iter().any(|t| {
                    t.column_header.is_some_and(|b| b.contains(c.row, c.col))
                        || t.title.is_some_and(|b| b.contains(c.row, c.col))
                        || (t.bounds.contains(c.row, c.col) && c.row < t.data.top.saturating_add(2))
                })
        })
        .map(structural_cell)
        .collect();
    let facts = serde_json::to_string(
        &json!({"sheet":page.name,"sheet_bounds":grid.bounds(),"fragments":fragments,"header_and_sample_cells":cells,"merges":grid.merges}),
    )?;
    if facts.len() > 60000 {
        bail!("Global reconciliation exceeds 60 KB context budget; fragments retained for review");
    }
    let preview = tiles(grid).first().copied().context("No source tile")?;
    let png = render::tile_png(grid, preview, font)?;
    let instruction = format!(
        "Reconcile previously analyzed spreadsheet tile fragments into complete logical tables. The image is only the first tile preview; use GLOBAL source coordinates and the supplied fragments and cell facts for the entire sheet. Adjacent horizontal/vertical fragments of ONE table must become one full rectangle, preserving all data columns and rows. Independent tables with separate titles or distinct headers remain separate. Do not merge just because columns align. For records, leading text fields are DATA and row_header=null. For matrices, preserve left row dimensions and all top header levels even when right-side tiles omitted them. Names should stay stable and meaningful. Output all table rectangles within sheet_bounds, with disjoint title/column_header/row_header/data. No observed data may be discarded. Return JSON only. DOCUMENT FACTS: {facts}"
    );
    let mut layout: Layout =
        serde_json::from_value(client.structure(&instruction, &png, layout_schema())?)
            .context("Invalid reconciliation contract")?;
    for d in &mut layout.tables {
        normalize_decision(d, grid.bounds())?;
        close_merged_header_hierarchy(d, grid);
        normalize_decision(d, grid.bounds())?;
        validate_decision(d, grid.bounds(), grid)?;
    }
    for (i, a) in layout.tables.iter().enumerate() {
        for b in &layout.tables[i + 1..] {
            if a.bounds.overlaps(b.bounds) {
                bail!("Reconciled tables overlap");
            }
        }
    }
    for cell in grid.cells.iter().filter(|c| c.has_content()) {
        if fragments
            .iter()
            .any(|t| t.data.contains(cell.row, cell.col))
            && !layout
                .tables
                .iter()
                .any(|t| t.data.contains(cell.row, cell.col))
        {
            bail!(
                "Reconciliation would discard data at row {}, column {}",
                cell.row,
                cell.col
            );
        }
    }
    Ok(layout
        .tables
        .into_iter()
        .enumerate()
        .map(|(i, d)| Table {
            id: format!("{}_table_{}", page.id, i + 1),
            name: d.name,
            page_id: page.id.clone(),
            bounds: d.bounds,
            title: d.title,
            column_header: d.column_header,
            row_header: d.row_header,
            data: d.data,
            orientation: d.orientation,
            headers: vec![],
            evidence:
                "Llama tile analysis + sheet-wide reconciliation; source data coverage validated"
                    .into(),
        })
        .collect())
}
pub fn analyze(
    document: Document,
    mut config: LlamaConfig,
    progress: impl Fn(String) + Send,
    cancel: Arc<AtomicBool>,
) -> Result<Analysis> {
    let initial = LlamaClient::new(config.clone())?;
    let models = initial.models()?;
    if config.model.is_empty() {
        config.model = models
            .first()
            .context("Llama has no available models")?
            .clone();
    }
    if !models.contains(&config.model) {
        bail!("Selected model is unavailable in Llama: {}", config.model);
    }
    let client = LlamaClient::new(config.clone())?;
    let font = render::load_font()?;
    let mut tables = vec![];
    let mut pages = vec![];
    let mut warnings = document.warnings.clone();
    for page in &document.pages {
        if cancel.load(Ordering::Relaxed) {
            bail!("Analysis cancelled");
        }
        let Some(grid) = &page.grid else {
            pages.push(PageStatus {
                page_id: page.id.clone(),
                page_name: page.name.clone(),
                state: "unsupported".into(),
                tiles_total: 0,
                tiles_completed: 0,
                message: "No grid adapter for this page".into(),
            });
            continue;
        };
        let chunks = tiles(grid);
        let mut completed = 0;
        let mut errors = vec![];
        for (index, tile) in chunks.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("Analysis cancelled");
            }
            progress(format!(
                "{}{} · tile {}/{} · rows {}–{}, columns {}–{}",
                page.name,
                if page.hidden { " (hidden)" } else { "" },
                index + 1,
                chunks.len(),
                tile.top,
                tile.bottom,
                tile.left,
                tile.right
            ));
            let result = (|| -> Result<Vec<Decision>> {
                let base = prompt(page, grid, *tile)?;
                let png = render::tile_png(grid, *tile, &font)?;
                let mut feedback = String::new();
                for attempt in 0..2 {
                    if cancel.load(Ordering::Relaxed) {
                        bail!("Analysis cancelled");
                    }
                    let answer =
                        client.structure(&format!("{base}{feedback}"), &png, layout_schema());
                    let previous = answer
                        .as_ref()
                        .ok()
                        .map(Value::to_string)
                        .unwrap_or_default();
                    let parsed = answer.and_then(|v| {
                        serde_json::from_value::<Layout>(v).context("Invalid layout contract")
                    });
                    let checked = parsed.and_then(|mut layout| {
                        for d in &mut layout.tables {
                            normalize_decision(d, *tile)?;
                            close_merged_header_hierarchy(d, grid);
                            normalize_decision(d, *tile)?;
                            validate_decision(d, *tile, grid)?;
                        }
                        for (i, a) in layout.tables.iter().enumerate() {
                            for b in &layout.tables[i + 1..] {
                                if a.bounds.overlaps(b.bounds) {
                                    bail!("Detected tables overlap");
                                }
                            }
                        }
                        Ok(layout.tables)
                    });
                    match checked {
                        Ok(t) => return Ok(t),
                        Err(e) => {
                            if attempt == 1 {
                                return Err(e);
                            }
                            feedback = format!(
                                "\nPrevious response was invalid: {e}. Previous JSON: {previous}. Correct it using the exact coordinates."
                            );
                            progress(format!("{} · retrying invalid tile: {e}", page.name));
                        }
                    }
                }
                unreachable!()
            })();
            match result {
                Ok(decisions) => {
                    completed += 1;
                    for d in decisions {
                        let id = format!("{}_t{}", page.id, tables.len() + 1);
                        tables.push(Table {
                            id,
                            name: d.name,
                            page_id: page.id.clone(),
                            bounds: d.bounds,
                            title: d.title,
                            column_header: d.column_header,
                            row_header: d.row_header,
                            data: d.data,
                            orientation: d.orientation,
                            headers: vec![],
                            evidence: format!(
                                "Llama vision + exact cell facts + Luna region normalization and merged-header closure, tile {}",
                                index + 1
                            ),
                        });
                    }
                }
                Err(e) => errors.push(format!("tile {}: {e:#}", index + 1)),
            }
        }
        if chunks.len() > 1 && tables.iter().any(|t| t.page_id == page.id) {
            if cancel.load(Ordering::Relaxed) {
                bail!("Analysis cancelled");
            }
            progress(format!(
                "{} · reconciling {} tiles into complete tables",
                page.name,
                chunks.len()
            ));
            let fragments: Vec<_> = tables
                .iter()
                .filter(|t| t.page_id == page.id)
                .cloned()
                .collect();
            match reconcile(page, grid, &fragments, &client, &font) {
                Ok(complete) => {
                    tables.retain(|t| t.page_id != page.id);
                    tables.extend(complete);
                }
                Err(e) => errors.push(format!(
                    "Cross-tile structure unresolved: {e:#}. Fragment entities need review"
                )),
            }
        }
        let covered: HashSet<_> = grid
            .cells
            .iter()
            .filter(|c| {
                c.has_content()
                    && tables
                        .iter()
                        .any(|t| t.page_id == page.id && classified(t, c.row, c.col))
            })
            .map(|c| (c.row, c.col))
            .collect();
        let uncovered = grid.cells.iter().filter(|c| c.has_content()).count() - covered.len();
        let state = if chunks.is_empty() {
            "empty"
        } else if completed == 0 {
            "failed"
        } else if !errors.is_empty() || uncovered > 0 {
            "partial"
        } else {
            "complete"
        };
        if uncovered > 0 {
            errors.push(format!("{uncovered} populated cells are outside classified table regions (notes or unresolved structure); original cells retained"));
        }
        let message = if errors.is_empty() {
            "All populated tiles processed".into()
        } else {
            errors.join("; ")
        };
        if state == "partial" || state == "failed" {
            warnings.push(format!("{}: {}", page.name, message));
        }
        pages.push(PageStatus {
            page_id: page.id.clone(),
            page_name: page.name.clone(),
            state: state.into(),
            tiles_total: chunks.len(),
            tiles_completed: completed,
            message,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        bail!("Analysis cancelled");
    }
    for t in &mut tables {
        if let Some(grid) = document
            .pages
            .iter()
            .find(|p| p.id == t.page_id)
            .and_then(|p| p.grid.as_ref())
        {
            t.headers = header_nodes(grid, t);
        }
    }
    progress("Profiling all source values and generating ERD candidates".into());
    let schema = schema::infer_schema(&document, &tables);
    Ok(Analysis {
        version: "0.1.0".into(),
        document,
        tables,
        schema,
        pages,
        warnings,
        model: config.model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_viewer_styles_do_not_inflate_the_model_context() {
        let mut grid = Grid {
            rows: 32,
            cols: 10,
            ..Default::default()
        };
        for row in 1..=32 {
            for col in 1..=10 {
                grid.cells.push(Cell {
                    row,
                    col,
                    value: "123".into(),
                    kind: CellKind::Number,
                    style: CellStyle {
                        font_name: Some("A very long installed font name".repeat(20)),
                        number_format: Some("#,##0.00".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
        }
        let page = Page {
            id: "s1".into(),
            name: "Source".into(),
            kind: PageKind::Sheet,
            hidden: false,
            grid: None,
            blocks: vec![],
        };
        let text = prompt(&page, &grid, grid.bounds()).unwrap();
        assert!(text.contains("123"));
        assert!(!text.contains("A very long installed font name"));
        assert!(text.len() < 60_000);
    }
    #[test]
    fn source_merge_spans_recover_matrix_parent_levels_and_category_axis() {
        let doc = crate::ingest::read_document(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/matrix.xlsx"
        )))
        .unwrap();
        let grid = doc.pages[0].grid.as_ref().unwrap();
        let mut d = Decision {
            name: "Revenue".into(),
            bounds: grid.bounds(),
            title: None,
            column_header: Some(Bounds {
                top: 3,
                left: 2,
                bottom: 3,
                right: 6,
            }),
            row_header: Some(Bounds {
                top: 4,
                left: 1,
                bottom: 7,
                right: 1,
            }),
            data: Bounds {
                top: 4,
                left: 2,
                bottom: 7,
                right: 6,
            },
            orientation: "matrix".into(),
        };
        close_merged_header_hierarchy(&mut d, grid);
        normalize_decision(&mut d, grid.bounds()).unwrap();
        validate_decision(&d, grid.bounds(), grid).unwrap();
        assert_eq!(
            d.column_header,
            Some(Bounds {
                top: 1,
                left: 3,
                bottom: 3,
                right: 6
            })
        );
        assert_eq!(
            d.row_header,
            Some(Bounds {
                top: 4,
                left: 1,
                bottom: 7,
                right: 2
            })
        );
        assert_eq!(
            d.data,
            Bounds {
                top: 4,
                left: 3,
                bottom: 7,
                right: 6
            }
        );
    }
}
