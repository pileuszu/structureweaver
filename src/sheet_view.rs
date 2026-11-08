//! Read-only worksheet canvas. Source formatting is independent of application chrome.
use crate::{domain::*, number_format};
use eframe::egui::{
    self, Align, Align2, Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

const GREEN: Color32 = Color32::from_rgb(0, 122, 86);
const GRID: Color32 = Color32::from_rgb(218, 222, 225);
const HEADER: Color32 = Color32::from_rgb(245, 247, 248);
const HEADER_W: f32 = 48.0;
const HEADER_H: f32 = 25.0;

// Excel for macOS displays document points in logical screen points. Windows and
// Linux worksheet coordinates use the conventional 96-dpi conversion instead.
const POINT_SCALE: f32 = if cfg!(target_os = "macos") {
    1.0
} else {
    96.0 / 72.0
};

pub struct SheetView {
    page: String,
    zoom: f32,
    show_hidden: bool,
    show_regions: bool,
    cells: HashMap<(u32, u32), usize>,
    rows: Axis,
    cols: Axis,
    digit_width: f32,
    previous_selection: (u32, u32),
    scroll_to_selection: bool,
    fonts: SourceFonts,
    conditional: crate::conditional::PreparedConditional,
    display_cells: HashMap<(u32, u32), Cell>,
    drawings: crate::drawings::DrawingRenderer,
}

impl SheetView {
    pub fn invalidate(&mut self) {
        self.page.clear();
    }

    pub fn new(fonts: egui::FontDefinitions) -> Self {
        Self {
            page: String::new(),
            zoom: 1.0,
            show_hidden: false,
            show_regions: false,
            cells: HashMap::new(),
            rows: Axis::default(),
            cols: Axis::default(),
            digit_width: if cfg!(target_os = "macos") { 6.0 } else { 7.0 },
            previous_selection: (0, 0),
            scroll_to_selection: false,
            fonts: SourceFonts::new(fonts),
            conditional: Default::default(),
            display_cells: HashMap::new(),
            drawings: Default::default(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        page: (&str, &str),
        grid: &Grid,
        selection: (&mut u32, &mut u32),
        tables: &[&Table],
        excel: &mut crate::excel_preview::ExcelPreview,
    ) {
        let (page_key, sheet_name) = page;
        let (selected_row, selected_col) = selection;
        let fresh = self.page != page_key;
        if fresh {
            self.page = page_key.into();
            self.zoom = (grid.layout.zoom_scale / 100.0).clamp(0.35, 2.0);
            self.cells = grid
                .cells
                .iter()
                .enumerate()
                .map(|(i, c)| ((c.row, c.col), i))
                .collect();
            self.conditional = crate::conditional::prepare(grid, &grid.layout.conditional_formats);
            self.display_cells.clear();
            self.drawings.clear();
            for (&(row, col), appearance) in &self.conditional.cells {
                let mut cell = self
                    .cells
                    .get(&(row, col))
                    .and_then(|&i| grid.cells.get(i))
                    .cloned()
                    .unwrap_or_else(|| Cell {
                        row,
                        col,
                        style: grid
                            .layout
                            .row_styles
                            .get(&row)
                            .or_else(|| grid.layout.column_styles.get(&col))
                            .unwrap_or(&grid.layout.default_style)
                            .clone(),
                        ..Default::default()
                    });
                appearance.style.apply(&mut cell.style);
                if appearance.style.number_format.is_some() {
                    cell.display_value = None;
                }
                self.display_cells.insert((row, col), cell);
            }
            self.show_hidden = false;
            self.rebuild_axes(grid);
            self.previous_selection = (0, 0);
            self.scroll_to_selection = true;
            self.fonts.prepare(ui.ctx(), grid, &self.display_cells);
        } else {
            self.fonts.pending = false;
            let normal_style = grid
                .layout
                .normal_style
                .as_ref()
                .unwrap_or(&grid.layout.default_style);
            let font = FontId::new(
                normal_style.font_size * POINT_SCALE,
                self.fonts.family(normal_style),
            );
            let measured = ('0'..='9')
                .map(|ch| {
                    ui.painter()
                        .layout_no_wrap(ch.to_string(), font.clone(), Color32::BLACK)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max)
                .round()
                .max(1.0);
            if (measured - self.digit_width).abs() > 0.1 {
                self.digit_width = measured;
                self.rebuild_axes(grid);
            }
        }
        if excel.enabled() && self.show_hidden {
            self.show_hidden = false;
            self.rebuild_axes(grid);
        }
        if self.previous_selection != (*selected_row, *selected_col) {
            self.scroll_to_selection = true;
            if !excel.enabled()
                && !self.show_hidden
                && (grid.hidden_rows.contains(selected_row)
                    || grid.hidden_cols.contains(selected_col))
            {
                self.show_hidden = true;
                self.rebuild_axes(grid);
            }
        }
        let mut jump = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("원본 시트").color(GREEN));
            ui.label("행");
            jump |= ui
                .add(
                    egui::DragValue::new(selected_row)
                        .range(1..=self.rows.count().max(1))
                        .speed(1),
                )
                .changed();
            ui.label("열");
            jump |= ui
                .add(
                    egui::DragValue::new(selected_col)
                        .range(1..=self.cols.count().max(1))
                        .speed(1),
                )
                .changed();
            ui.separator();
            let mut percent = (self.zoom * 100.0).round();
            if ui
                .add(
                    egui::DragValue::new(&mut percent)
                        .range(35.0..=200.0)
                        .suffix("%")
                        .speed(5.0),
                )
                .changed()
            {
                self.zoom = percent / 100.0;
            }
            if ui.small_button("100%").clicked() {
                self.zoom = 1.0;
            }
            if (!grid.hidden_rows.is_empty() || !grid.hidden_cols.is_empty())
                && ui
                    .add_enabled(!excel.enabled(), egui::Checkbox::new(&mut self.show_hidden, "숨김 영역 표시"))
                    .on_hover_text("숨김 영역을 펼치려면 Rust 미리보기를 선택하세요. Excel 개체는 원본 시트 좌표를 유지합니다.")
                    .changed()
            {
                self.rebuild_axes(grid);
                jump = true;
            }
            if !tables.is_empty() {
                ui.checkbox(&mut self.show_regions, "분석 영역 표시");
            }
        });
        if jump
            && !excel.enabled()
            && !self.show_hidden
            && (grid.hidden_rows.contains(selected_row) || grid.hidden_cols.contains(selected_col))
        {
            self.show_hidden = true;
            self.rebuild_axes(grid);
        }
        self.scroll_to_selection |= jump;
        let drawing_layers = excel.layers(sheet_name, &grid.layout.drawings);
        let mut extent = excel.extent(sheet_name) * POINT_SCALE;
        for object in &grid.layout.drawings {
            let rect = self.drawing_rect(&object.anchor, Pos2::ZERO, 1.0);
            let outset = object.visual_outset(rect.size(), 1.0);
            let rotation = if object.rotation == 0.0 {
                0.0
            } else {
                rect.size().length() * 0.5
            };
            extent = extent.max(rect.max.to_vec2() + Vec2::splat(outset + rotation));
        }
        self.rows.extend_until(extent.y, 1_048_576, |row| {
            if !self.show_hidden && grid.hidden_rows.contains(&row) {
                0.0
            } else {
                grid.layout
                    .row_heights
                    .get(&row)
                    .copied()
                    .unwrap_or(grid.layout.default_row_height)
                    .max(0.0)
                    * POINT_SCALE
            }
        });
        self.cols.extend_until(extent.x, 16_384, |col| {
            if !self.show_hidden && grid.hidden_cols.contains(&col) {
                0.0
            } else {
                column_pixels(
                    grid.layout
                        .column_widths
                        .get(&col)
                        .copied()
                        .unwrap_or(grid.layout.default_col_width),
                    self.digit_width,
                )
            }
        });
        let selected_bounds = merge_at(grid, *selected_row, *selected_col).unwrap_or(Bounds {
            top: *selected_row,
            left: *selected_col,
            bottom: *selected_row,
            right: *selected_col,
        });
        let cell = self.cell(grid, selected_bounds.top, selected_bounds.left);
        egui::Frame::NONE
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0_f32, GRID))
            .inner_margin(egui::Margin::symmetric(9, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [70.0, 20.0],
                        egui::Label::new(
                            RichText::new(address(selected_bounds.top, selected_bounds.left))
                                .monospace()
                                .color(GREEN),
                        ),
                    );
                    ui.separator();
                    ui.label(RichText::new("ƒx").color(Color32::GRAY));
                    let text = cell
                        .map(|c| {
                            c.formula
                                .as_ref()
                                .map(|f| format!("={f}"))
                                .unwrap_or_else(|| c.value.clone())
                        })
                        .unwrap_or_default();
                    ui.add(
                        egui::Label::new(if text.is_empty() {
                            " ".into()
                        } else {
                            text.clone()
                        })
                        .truncate(),
                    )
                    .on_hover_text(text);
                });
            });
        if !grid.layout.unsupported_features.is_empty()
            || !self.fonts.missing.is_empty()
            || !self.conditional.warnings.is_empty()
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("일부 원본 표시가 다를 수 있어요")
                        .size(11.5)
                        .color(Color32::from_rgb(145, 98, 18)),
                );
                ui.collapsing("표시 정보", |ui| {
                    for feature in &grid.layout.unsupported_features {
                        ui.label(feature);
                    }
                    for feature in &self.conditional.warnings {
                        ui.label(feature);
                    }
                    if !self.fonts.missing.is_empty() {
                        ui.label(format!(
                            "설치되지 않은 원본 글꼴을 대체했습니다: {}",
                            self.fonts
                                .missing
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                });
            });
        }
        ui.add_space(4.0);
        if grid.layout.chart_sheet {
            let (rect, _) =
                ui.allocate_exact_size(ui.available_size().max(Vec2::splat(50.0)), Sense::hover());
            let painter = ui.painter_at(rect);
            for layer in &drawing_layers {
                match *layer {
                    crate::excel_preview::Layer::Native(index) => self.drawings.paint_with_fonts(
                        &painter,
                        rect.shrink(10.0),
                        &grid.layout.drawings[index],
                        self.zoom,
                        &|run| self.fonts.family(&drawing_text_style(run)),
                    ),
                    crate::excel_preview::Layer::Excel(index) => excel.paint(
                        &painter,
                        index,
                        rect.min,
                        self.zoom,
                        Some(rect.shrink(10.0)),
                    ),
                }
            }
            return;
        }
        let available_height = ui.available_height().max(160.0);
        let z = self.zoom;
        let full = Vec2::new(
            HEADER_W + self.cols.total() * z,
            HEADER_H + self.rows.total() * z,
        );
        let scroll_id = ui.make_persistent_id(("worksheet-canvas", page_key));
        let mut scroll = egui::ScrollArea::both()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .max_height(available_height);
        if self.scroll_to_selection {
            let (x, _) = self.cols.interval(selected_bounds.left);
            let (y, _) = self.rows.interval(selected_bounds.top);
            let freeze_x = self.cols.offset_after(grid.layout.freeze_cols) * z;
            let freeze_y = self.rows.offset_after(grid.layout.freeze_rows) * z;
            scroll = scroll.scroll_offset(Vec2::new(
                if selected_bounds.left <= grid.layout.freeze_cols {
                    0.0
                } else {
                    (x * z - freeze_x - 30.0).max(0.0)
                },
                if selected_bounds.top <= grid.layout.freeze_rows {
                    0.0
                } else {
                    (y * z - freeze_y - 30.0).max(0.0)
                },
            ));
            self.scroll_to_selection = false;
        }
        let mut clicked = None;
        scroll.show_viewport(ui, |ui, viewport| {
            let (canvas, response) =
                ui.allocate_exact_size(full.max(ui.available_size()), Sense::click());
            let visible = ui.clip_rect().intersect(canvas);
            let painter = ui.painter_at(visible);
            painter.rect_filled(visible, 0.0, Color32::WHITE);
            let fx = (self.cols.offset_after(grid.layout.freeze_cols) * z)
                .min((visible.width() - HEADER_W - 35.0).max(0.0));
            let fy = (self.rows.offset_after(grid.layout.freeze_rows) * z)
                .min((visible.height() - HEADER_H - 35.0).max(0.0));
            let freeze = Vec2::new(fx, fy);
            let body = Rect::from_min_max(visible.min + Vec2::new(HEADER_W, HEADER_H), visible.max);
            let bands = [(false, false), (true, false), (false, true), (true, true)];
            for (frozen_col, frozen_row) in bands {
                if (frozen_col && fx == 0.0) || (frozen_row && fy == 0.0) {
                    continue;
                }
                let clip = Rect::from_min_max(
                    Pos2::new(
                        if frozen_col {
                            body.left()
                        } else {
                            body.left() + fx
                        },
                        if frozen_row {
                            body.top()
                        } else {
                            body.top() + fy
                        },
                    ),
                    Pos2::new(
                        if frozen_col {
                            body.left() + fx
                        } else {
                            body.right()
                        },
                        if frozen_row {
                            body.top() + fy
                        } else {
                            body.bottom()
                        },
                    ),
                );
                if !clip.is_positive() {
                    continue;
                }
                let translate = Vec2::new(
                    if frozen_col { viewport.min.x } else { 0.0 },
                    if frozen_row { viewport.min.y } else { 0.0 },
                );
                let origin = canvas.min + Vec2::new(HEADER_W, HEADER_H) + translate;
                let (r0, r1) = if frozen_row {
                    (1, grid.layout.freeze_rows.min(self.rows.count()))
                } else {
                    self.rows
                        .visible((clip.top() - origin.y) / z, (clip.bottom() - origin.y) / z)
                };
                let (c0, c1) = if frozen_col {
                    (1, grid.layout.freeze_cols.min(self.cols.count()))
                } else {
                    self.cols
                        .visible((clip.left() - origin.x) / z, (clip.right() - origin.x) / z)
                };
                let first_row = if frozen_row {
                    r0
                } else {
                    r0.max(grid.layout.freeze_rows + 1)
                };
                let first_col = if frozen_col {
                    c0
                } else {
                    c0.max(grid.layout.freeze_cols + 1)
                };
                if first_row > r1 || first_col > c1 {
                    continue;
                }
                let bounds = Bounds {
                    top: first_row,
                    left: first_col,
                    bottom: r1,
                    right: c1,
                };
                let merges: Vec<_> = grid
                    .merges
                    .iter()
                    .filter(|m| m.overlaps(bounds))
                    .copied()
                    .collect();
                let p = painter.with_clip_rect(clip);
                // Backgrounds and lines first; text overflow is painted after adjacent empty cells.
                let mut items = Vec::new();
                for row in first_row..=r1 {
                    if self.rows.size(row) == 0.0 {
                        continue;
                    }
                    for col in first_col..=c1 {
                        if self.cols.size(col) == 0.0 || merges.iter().any(|m| m.contains(row, col))
                        {
                            continue;
                        }
                        items.push(Bounds {
                            top: row,
                            left: col,
                            bottom: row,
                            right: col,
                        });
                    }
                }
                items.extend(merges);
                for b in &items {
                    let rect = self.cell_rect(*b, origin, z);
                    self.paint_cell_background(&p, grid, *b, rect, z);
                    if let Some(appearance) = self.conditional.cells.get(&(b.top, b.left)) {
                        paint_conditional_visuals(&p, rect, appearance, z);
                    }
                }
                for b in &items {
                    let rect = self.cell_rect(*b, origin, z);
                    self.paint_cell_text(&p, grid, *b, rect, origin, z);
                }
                if self.show_regions {
                    for table in tables {
                        for (b, c) in [
                            (Some(table.bounds), GREEN),
                            (table.column_header, Color32::from_rgb(113, 78, 190)),
                            (table.row_header, Color32::from_rgb(185, 123, 32)),
                        ] {
                            if let Some(b) = b.filter(|b| b.overlaps(bounds)) {
                                p.rect_stroke(
                                    self.cell_rect(b, origin, z).shrink(1.0),
                                    0.0,
                                    Stroke::new(1.5_f32, c),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    }
                }
                if selected_bounds.overlaps(bounds) {
                    p.rect_stroke(
                        self.cell_rect(selected_bounds, origin, z),
                        0.0,
                        Stroke::new(2.0_f32, GREEN),
                        egui::StrokeKind::Inside,
                    );
                }
                for layer in &drawing_layers {
                    let index = match *layer {
                        crate::excel_preview::Layer::Native(index) => index,
                        crate::excel_preview::Layer::Excel(index) => {
                            excel.paint(&p, index, origin, z * POINT_SCALE, None);
                            continue;
                        }
                    };
                    let object = &grid.layout.drawings[index];
                    let rect = self.drawing_rect(&object.anchor, origin, z);
                    // Rotation may extend past the anchor; the painter clips each frozen pane.
                    let radius = rect.size().length() / 2.0 + object.visual_outset(rect.size(), z);
                    if Rect::from_center_size(rect.center(), Vec2::splat(radius * 2.0))
                        .intersects(clip)
                    {
                        self.drawings.paint_with_fonts(&p, rect, object, z, &|run| {
                            self.fonts.family(&drawing_text_style(run))
                        });
                    }
                }
            }
            self.paint_headers(
                &painter,
                grid,
                canvas,
                visible,
                viewport.min,
                freeze,
                z,
                *selected_row,
                *selected_col,
            );
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
                && pos.x > body.left()
                && pos.y > body.top()
            {
                let x = if pos.x < body.left() + fx {
                    pos.x - body.left()
                } else {
                    pos.x - (canvas.left() + HEADER_W)
                };
                let y = if pos.y < body.top() + fy {
                    pos.y - body.top()
                } else {
                    pos.y - (canvas.top() + HEADER_H)
                };
                clicked = Some((self.rows.at(y / z), self.cols.at(x / z)));
            }
            if response.hovered()
                && let Some(pos) = response.hover_pos()
                && pos.x > body.left()
                && pos.y > body.top()
            {
                let x = if pos.x < body.left() + fx {
                    pos.x - body.left()
                } else {
                    pos.x - (canvas.left() + HEADER_W)
                };
                let y = if pos.y < body.top() + fy {
                    pos.y - body.top()
                } else {
                    pos.y - (canvas.top() + HEADER_H)
                };
                let row = self.rows.at(y / z);
                let col = self.cols.at(x / z);
                let b = merge_at(grid, row, col).unwrap_or(Bounds {
                    top: row,
                    left: col,
                    bottom: row,
                    right: col,
                });
                if let Some(cell) = self.cell(grid, b.top, b.left) {
                    response.clone().on_hover_text(format!(
                        "{}\n{}{}",
                        address(b.top, b.left),
                        number_format::display_value(cell, grid.layout.date1904),
                        cell.formula
                            .as_ref()
                            .map(|f| format!("\n={f}"))
                            .unwrap_or_default()
                    ));
                }
            }
        });
        if let Some((row, col)) = clicked {
            let b = merge_at(grid, row, col);
            *selected_row = b.map(|b| b.top).unwrap_or(row);
            *selected_col = b.map(|b| b.left).unwrap_or(col);
        }
        self.previous_selection = (*selected_row, *selected_col);
    }

    fn rebuild_axes(&mut self, grid: &Grid) {
        let hidden_rows: HashSet<_> = grid.hidden_rows.iter().copied().collect();
        let hidden_cols: HashSet<_> = grid.hidden_cols.iter().copied().collect();
        let (drawing_rows, drawing_cols) = drawing_extent(grid, self.digit_width);
        let conditional_rows = grid
            .layout
            .conditional_formats
            .iter()
            .flat_map(|r| &r.ranges)
            .map(|b| b.bottom)
            .max()
            .unwrap_or(0);
        let conditional_cols = grid
            .layout
            .conditional_formats
            .iter()
            .flat_map(|r| &r.ranges)
            .map(|b| b.right)
            .max()
            .unwrap_or(0);
        self.rows = Axis::new(
            grid.rows
                .max(40)
                .max(drawing_rows)
                .max(conditional_rows)
                .min(1_048_576),
            |row| {
                if !self.show_hidden && hidden_rows.contains(&row) {
                    0.0
                } else {
                    grid.layout
                        .row_heights
                        .get(&row)
                        .copied()
                        .unwrap_or(grid.layout.default_row_height)
                        .max(0.0)
                        * POINT_SCALE
                }
            },
        );
        self.cols = Axis::new(
            grid.cols
                .max(16)
                .max(drawing_cols)
                .max(conditional_cols)
                .min(16_384),
            |col| {
                if !self.show_hidden && hidden_cols.contains(&col) {
                    0.0
                } else {
                    column_pixels(
                        grid.layout
                            .column_widths
                            .get(&col)
                            .copied()
                            .unwrap_or(grid.layout.default_col_width),
                        self.digit_width,
                    )
                }
            },
        );
    }
    fn drawing_rect(&self, anchor: &crate::drawings::DrawingAnchor, origin: Pos2, z: f32) -> Rect {
        use crate::drawings::DrawingAnchor;
        let emu = |v: f64| (v / 12700.0) as f32 * POINT_SCALE;
        let marker = |m: &crate::drawings::Marker| {
            Vec2::new(
                self.cols.interval(m.col).0 + emu(m.col_offset_emu),
                self.rows.interval(m.row).0 + emu(m.row_offset_emu),
            )
        };
        match anchor {
            DrawingAnchor::TwoCell { from, to } => {
                Rect::from_two_pos(origin + marker(from) * z, origin + marker(to) * z)
            }
            DrawingAnchor::OneCell {
                from,
                width_emu,
                height_emu,
            } => Rect::from_min_size(
                origin + marker(from) * z,
                Vec2::new(emu(*width_emu), emu(*height_emu)) * z,
            ),
            DrawingAnchor::Absolute {
                x_emu,
                y_emu,
                width_emu,
                height_emu,
            } => Rect::from_min_size(
                origin + Vec2::new(emu(*x_emu), emu(*y_emu)) * z,
                Vec2::new(emu(*width_emu), emu(*height_emu)) * z,
            ),
        }
    }
    fn cell<'a>(&'a self, grid: &'a Grid, row: u32, col: u32) -> Option<&'a Cell> {
        self.display_cells
            .get(&(row, col))
            .or_else(|| self.cells.get(&(row, col)).and_then(|&i| grid.cells.get(i)))
    }
    fn style<'a>(&'a self, grid: &'a Grid, row: u32, col: u32) -> &'a CellStyle {
        self.cell(grid, row, col)
            .map(|c| &c.style)
            .or_else(|| grid.layout.row_styles.get(&row))
            .or_else(|| grid.layout.column_styles.get(&col))
            .unwrap_or(&grid.layout.default_style)
    }
    fn cell_rect(&self, b: Bounds, origin: Pos2, z: f32) -> Rect {
        let (x, _) = self.cols.interval(b.left);
        let (y, _) = self.rows.interval(b.top);
        Rect::from_min_max(
            origin + Vec2::new(x, y) * z,
            origin
                + Vec2::new(
                    self.cols.offset_after(b.right),
                    self.rows.offset_after(b.bottom),
                ) * z,
        )
    }
    fn paint_cell_background(&self, p: &egui::Painter, grid: &Grid, b: Bounds, rect: Rect, z: f32) {
        let style = self.style(grid, b.top, b.left);
        if style.fill.is_some() && (style.fill_pattern == "solid" || style.fill_pattern == "none") {
            if let Some(color) = parse_color(style.fill.as_deref()) {
                p.rect_filled(rect, 0.0, color);
            }
        } else if style.fill_pattern != "none" && !style.fill_pattern.is_empty() {
            pattern_fill(p, rect, style, z);
        }
        if grid.layout.show_grid_lines && style.fill.is_none() {
            p.rect_stroke(
                rect,
                0.0,
                Stroke::new(0.5_f32, GRID),
                egui::StrokeKind::Inside,
            );
        }
        borders(p, rect, &style.borders, z);
        if b.bottom > b.top || b.right > b.left {
            let (base_x, _) = self.cols.interval(b.left);
            let (base_y, _) = self.rows.interval(b.top);
            let (first_col, last_col) = self.cols.visible(
                base_x + (p.clip_rect().left() - rect.left()) / z,
                base_x + (p.clip_rect().right() - rect.left()) / z,
            );
            let (first_row, last_row) = self.rows.visible(
                base_y + (p.clip_rect().top() - rect.top()) / z,
                base_y + (p.clip_rect().bottom() - rect.top()) / z,
            );
            for col in b.left.max(first_col)..=b.right.min(last_col) {
                let (x, w) = self.cols.interval(col);
                let (base, _) = self.cols.interval(b.left);
                let left = rect.left() + (x - base) * z;
                let right = left + w * z;
                if let Some(c) = self.cell(grid, b.top, col) {
                    border_line(
                        p,
                        [Pos2::new(left, rect.top()), Pos2::new(right, rect.top())],
                        &c.style.borders.top,
                        z,
                    );
                }
                if let Some(c) = self.cell(grid, b.bottom, col) {
                    border_line(
                        p,
                        [
                            Pos2::new(left, rect.bottom()),
                            Pos2::new(right, rect.bottom()),
                        ],
                        &c.style.borders.bottom,
                        z,
                    );
                }
            }
            for row in b.top.max(first_row)..=b.bottom.min(last_row) {
                let (y, h) = self.rows.interval(row);
                let (base, _) = self.rows.interval(b.top);
                let top = rect.top() + (y - base) * z;
                let bottom = top + h * z;
                if let Some(c) = self.cell(grid, row, b.left) {
                    border_line(
                        p,
                        [Pos2::new(rect.left(), top), Pos2::new(rect.left(), bottom)],
                        &c.style.borders.left,
                        z,
                    );
                }
                if let Some(c) = self.cell(grid, row, b.right) {
                    border_line(
                        p,
                        [
                            Pos2::new(rect.right(), top),
                            Pos2::new(rect.right(), bottom),
                        ],
                        &c.style.borders.right,
                        z,
                    );
                }
            }
        }
    }
    fn paint_cell_text(
        &self,
        p: &egui::Painter,
        grid: &Grid,
        b: Bounds,
        rect: Rect,
        origin: Pos2,
        z: f32,
    ) {
        let Some(cell) = self.cell(grid, b.top, b.left) else {
            return;
        };
        let style = &cell.style;
        let conditional = self.conditional.cells.get(&(b.top, b.left));
        if conditional.is_some_and(|c| c.hide_value) {
            return;
        }
        let mut text = number_format::display_value(cell, grid.layout.date1904);
        if text.is_empty() {
            return;
        }
        let format_color = number_format::color_override(cell);
        let color = parse_color(
            conditional
                .and_then(|c| c.style.font_color.as_deref())
                .or(format_color.as_deref())
                .or(style.font_color.as_deref()),
        )
        .unwrap_or(Color32::from_rgb(24, 24, 24));
        let horizontal = match style.horizontal.as_str() {
            "center" | "centerContinuous" => Align::Center,
            "right" => Align::RIGHT,
            "general" if matches!(cell.kind, CellKind::Number | CellKind::Date) => Align::RIGHT,
            "general" if cell.kind == CellKind::Boolean => Align::Center,
            _ => Align::LEFT,
        };
        let pad = 3.0 * z;
        let icon_width = if conditional.is_some_and(|c| c.icon.is_some()) {
            18.0 * z
        } else {
            0.0
        };
        let indent = style.indent as f32 * 9.0 * z;
        let rect = Rect::from_min_max(rect.min + Vec2::new(icon_width, 0.0), rect.max);
        let mut clip = rect.shrink2(Vec2::new(1.0, 0.5));
        // Excel lets unwrapped text flow across blank neighbors, but never across occupied cells.
        if b.left == b.right
            && b.top == b.bottom
            && !style.wrap_text
            && !style.shrink_to_fit
            && style.text_rotation == 0
            && cell.kind == CellKind::Text
            && horizontal == Align::LEFT
        {
            let mut last = b.right;
            while last < self.cols.count() && last < b.right + 64 {
                let next = last + 1;
                if merge_at(grid, b.top, next).is_some()
                    || self.cell(grid, b.top, next).is_some_and(|c| {
                        !c.value.is_empty() || c.formula.is_some() || c.style.fill.is_some()
                    })
                {
                    break;
                }
                last = next;
                if origin.x + self.cols.offset_after(last) * z >= p.clip_rect().right() {
                    break;
                }
            }
            clip.max.x = origin.x + self.cols.offset_after(last) * z - 1.0;
        }
        let family = self.fonts.family(style);
        let size = style.font_size.clamp(1.0, 409.0) * POINT_SCALE * z;
        let mut font = FontId::new(size, family);
        let mut max_width = if style.wrap_text {
            (rect.width() - pad * 2.0 - indent).max(1.0)
        } else {
            f32::INFINITY
        };
        if style.text_rotation == 255 {
            text = text
                .chars()
                .filter(|c| *c != '\n')
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            max_width = f32::INFINITY;
        }
        let make_job = |font: FontId, text: String| {
            let mut job = egui::text::LayoutJob::simple(text, font, color, max_width);
            job.halign = horizontal;
            job.justify = style.horizontal == "justify" || style.horizontal == "distributed";
            job.sections[0].format.italics = style.italic && !self.fonts.has_italic(style);
            job.sections[0].format.underline =
                if style.underline != "none" && !style.underline.is_empty() {
                    Stroke::new((z * 0.8).max(0.5), color)
                } else {
                    Stroke::NONE
                };
            job.sections[0].format.strikethrough = if style.strike {
                Stroke::new((z * 0.8).max(0.5), color)
            } else {
                Stroke::NONE
            };
            job
        };
        let mut galley = p.layout_job(make_job(font.clone(), text.clone()));
        let inner_width = (rect.width() - pad * 2.0 - indent).max(1.0);
        if style.shrink_to_fit && !style.wrap_text && galley.size().x > inner_width {
            font.size = (font.size * inner_width / galley.size().x).max(POINT_SCALE * z);
            galley = p.layout_job(make_job(font, text.clone()));
        } else if matches!(cell.kind, CellKind::Number | CellKind::Date)
            && !style.wrap_text
            && style.text_rotation == 0
            && galley.size().x > inner_width
        {
            let count = (inner_width / (size * 0.55)).floor().max(1.0) as usize;
            text = "#".repeat(count.min(80));
            galley = p.layout_job(make_job(font, text));
        }
        let angle = match style.text_rotation {
            1..=90 => -(style.text_rotation as f32).to_radians(),
            91..=180 => (style.text_rotation as f32 - 90.0).to_radians(),
            _ => 0.0,
        };
        // Center/right-aligned galleys have a negative local x origin. Account for the
        // actual bounds before rotating, rather than applying alignment a second time.
        let (sin, cos) = angle.sin_cos();
        let rotate =
            |point: Pos2| Pos2::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos);
        let corners = [
            galley.rect.left_top(),
            galley.rect.right_top(),
            galley.rect.left_bottom(),
            galley.rect.right_bottom(),
        ]
        .map(rotate);
        let min = corners
            .iter()
            .fold(Pos2::new(f32::INFINITY, f32::INFINITY), |a, b| {
                Pos2::new(a.x.min(b.x), a.y.min(b.y))
            });
        let max = corners
            .iter()
            .fold(Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY), |a, b| {
                Pos2::new(a.x.max(b.x), a.y.max(b.y))
            });
        let rotated_size = max - min;
        let left = match horizontal {
            Align::Center => rect.center().x - rotated_size.x / 2.0,
            Align::RIGHT => rect.right() - pad - indent - rotated_size.x,
            Align::LEFT => rect.left() + pad + indent,
        };
        let top = match style.vertical.as_str() {
            "center" | "justify" | "distributed" => rect.center().y - rotated_size.y / 2.0,
            "top" => rect.top() + pad,
            _ => rect.bottom() - pad - rotated_size.y,
        };
        p.with_clip_rect(clip.intersect(p.clip_rect())).add(
            egui::epaint::TextShape::new(Pos2::new(left, top) - min.to_vec2(), galley, color)
                .with_angle(angle),
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_headers(
        &self,
        p: &egui::Painter,
        grid: &Grid,
        canvas: Rect,
        visible: Rect,
        scroll: Pos2,
        freeze: Vec2,
        z: f32,
        row: u32,
        col: u32,
    ) {
        let top = Rect::from_min_max(
            visible.min,
            Pos2::new(visible.right(), visible.top() + HEADER_H),
        );
        let left = Rect::from_min_max(
            visible.min,
            Pos2::new(visible.left() + HEADER_W, visible.bottom()),
        );
        p.rect_filled(top, 0.0, HEADER);
        p.rect_filled(left, 0.0, HEADER);
        for frozen in [false, true] {
            let start = if frozen {
                1
            } else {
                grid.layout.freeze_cols + 1
            };
            let (c0, c1) = if frozen {
                (1, grid.layout.freeze_cols.min(self.cols.count()))
            } else {
                self.cols
                    .visible(scroll.x / z, (scroll.x + visible.width()) / z)
            };
            let cp = p.with_clip_rect(Rect::from_min_max(
                Pos2::new(
                    visible.left() + HEADER_W + if frozen { 0.0 } else { freeze.x },
                    visible.top(),
                ),
                Pos2::new(
                    if frozen {
                        visible.left() + HEADER_W + freeze.x
                    } else {
                        visible.right()
                    },
                    visible.top() + HEADER_H,
                ),
            ));
            for c in c0.max(start)..=c1 {
                let (x, w) = self.cols.interval(c);
                if w == 0.0 {
                    continue;
                }
                let r = Rect::from_min_size(
                    Pos2::new(
                        canvas.left() + HEADER_W + x * z + if frozen { scroll.x } else { 0.0 },
                        visible.top(),
                    ),
                    Vec2::new(w * z, HEADER_H),
                );
                if c == col {
                    cp.rect_filled(r, 0.0, Color32::from_rgb(222, 243, 234));
                }
                cp.rect_stroke(r, 0.0, Stroke::new(0.5_f32, GRID), egui::StrokeKind::Inside);
                cp.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    column_name(c),
                    FontId::proportional(11.5),
                    if c == col {
                        GREEN
                    } else {
                        Color32::from_gray(95)
                    },
                );
            }
            let start = if frozen {
                1
            } else {
                grid.layout.freeze_rows + 1
            };
            let (r0, r1) = if frozen {
                (1, grid.layout.freeze_rows.min(self.rows.count()))
            } else {
                self.rows
                    .visible(scroll.y / z, (scroll.y + visible.height()) / z)
            };
            let rp = p.with_clip_rect(Rect::from_min_max(
                Pos2::new(
                    visible.left(),
                    visible.top() + HEADER_H + if frozen { 0.0 } else { freeze.y },
                ),
                Pos2::new(
                    visible.left() + HEADER_W,
                    if frozen {
                        visible.top() + HEADER_H + freeze.y
                    } else {
                        visible.bottom()
                    },
                ),
            ));
            for r in r0.max(start)..=r1 {
                let (y, h) = self.rows.interval(r);
                if h == 0.0 {
                    continue;
                }
                let rect = Rect::from_min_size(
                    Pos2::new(
                        visible.left(),
                        canvas.top() + HEADER_H + y * z + if frozen { scroll.y } else { 0.0 },
                    ),
                    Vec2::new(HEADER_W, h * z),
                );
                if r == row {
                    rp.rect_filled(rect, 0.0, Color32::from_rgb(222, 243, 234));
                }
                rp.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(0.5_f32, GRID),
                    egui::StrokeKind::Inside,
                );
                rp.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    r.to_string(),
                    FontId::proportional(11.0),
                    if r == row {
                        GREEN
                    } else {
                        Color32::from_gray(95)
                    },
                );
            }
        }
        p.rect_filled(
            Rect::from_min_size(visible.min, Vec2::new(HEADER_W, HEADER_H)),
            0.0,
            HEADER,
        );
        if freeze.x > 0.0 {
            p.line_segment(
                [
                    visible.min + Vec2::new(HEADER_W + freeze.x, HEADER_H),
                    Pos2::new(visible.left() + HEADER_W + freeze.x, visible.bottom()),
                ],
                Stroke::new(1.5_f32, Color32::from_gray(150)),
            );
        }
        if freeze.y > 0.0 {
            p.line_segment(
                [
                    visible.min + Vec2::new(HEADER_W, HEADER_H + freeze.y),
                    Pos2::new(visible.right(), visible.top() + HEADER_H + freeze.y),
                ],
                Stroke::new(1.5_f32, Color32::from_gray(150)),
            );
        }
    }
}

#[derive(Default)]
struct Axis {
    offsets: Vec<f32>,
}
impl Axis {
    fn extend_until(&mut self, end: f32, limit: u32, size: impl Fn(u32) -> f32) {
        while self.total() < end && self.count() < limit {
            let next = size(self.count() + 1).max(0.0);
            self.offsets.push(self.total() + next);
        }
    }
    fn new(count: u32, size: impl Fn(u32) -> f32) -> Self {
        let mut offsets = Vec::with_capacity(count as usize + 1);
        offsets.push(0.0);
        for n in 1..=count {
            offsets.push(offsets.last().copied().unwrap_or(0.0) + size(n).max(0.0));
        }
        Self { offsets }
    }
    fn count(&self) -> u32 {
        self.offsets.len().saturating_sub(1) as u32
    }
    fn total(&self) -> f32 {
        *self.offsets.last().unwrap_or(&0.0)
    }
    fn offset_after(&self, index: u32) -> f32 {
        self.offsets
            .get(index as usize)
            .copied()
            .unwrap_or_else(|| self.total())
    }
    fn interval(&self, index: u32) -> (f32, f32) {
        let start = self.offset_after(index.saturating_sub(1));
        (start, (self.offset_after(index) - start).max(0.0))
    }
    fn size(&self, index: u32) -> f32 {
        self.interval(index).1
    }
    fn at(&self, position: f32) -> u32 {
        self.offsets
            .partition_point(|&x| x <= position.max(0.0))
            .clamp(1, self.count().max(1) as usize) as u32
    }
    fn visible(&self, min: f32, max: f32) -> (u32, u32) {
        (self.at(min), self.at(max).min(self.count()))
    }
}
fn column_pixels(width: f32, digit_width: f32) -> f32 {
    // OOXML stores a padded character width; adding padding again makes every column too wide.
    // ECMA-376 / Microsoft Column.Width conversion using the Normal style's maximum digit width.
    if width <= 0.0 || !width.is_finite() {
        return 0.0;
    }
    let digit_width = digit_width.max(1.0);
    (((256.0 * width + (128.0 / digit_width).floor()) / 256.0) * digit_width).floor()
}

fn merge_at(grid: &Grid, row: u32, col: u32) -> Option<Bounds> {
    grid.merges.iter().find(|m| m.contains(row, col)).copied()
}
fn column_name(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}
fn address(row: u32, col: u32) -> String {
    format!("{}{row}", column_name(col))
}
fn parse_color(value: Option<&str>) -> Option<Color32> {
    let s = value?.trim_start_matches('#');
    let s = if s.len() == 8 { &s[2..] } else { s };
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
fn borders(p: &egui::Painter, r: Rect, b: &CellBorders, z: f32) {
    border_line(p, [r.left_top(), r.right_top()], &b.top, z);
    border_line(p, [r.left_bottom(), r.right_bottom()], &b.bottom, z);
    border_line(p, [r.left_top(), r.left_bottom()], &b.left, z);
    border_line(p, [r.right_top(), r.right_bottom()], &b.right, z);
    if b.diagonal_up {
        border_line(p, [r.left_bottom(), r.right_top()], &b.diagonal, z);
    }
    if b.diagonal_down {
        border_line(p, [r.left_top(), r.right_bottom()], &b.diagonal, z);
    }
}
fn border_line(p: &egui::Painter, line: [Pos2; 2], side: &BorderSide, z: f32) {
    if side.style.is_empty() || side.style == "none" {
        return;
    }
    let Some(line) = clipped_line(line, p.clip_rect().expand(4.0 * z)) else {
        return;
    };
    let color = parse_color(side.color.as_deref()).unwrap_or(Color32::BLACK);
    let width = match side.style.as_str() {
        "hair" => 0.5,
        "medium" | "mediumDashed" | "mediumDashDot" | "mediumDashDotDot" => 2.0,
        "thick" => 3.0,
        _ => 1.0,
    } * z;
    if side.style == "double" {
        let delta = line[1] - line[0];
        let normal = Vec2::new(-delta.y, delta.x).normalized() * 1.5 * z;
        p.line_segment([line[0] + normal, line[1] + normal], Stroke::new(z, color));
        p.line_segment([line[0] - normal, line[1] - normal], Stroke::new(z, color));
    } else if side.style.to_ascii_lowercase().contains("dash") || side.style == "dotted" {
        let delta = line[1] - line[0];
        let length = delta.length();
        let direction = delta.normalized();
        let dash = if side.style == "dotted" { z } else { 5.0 * z };
        let gap = 3.0 * z;
        let mut x = 0.0;
        while x < length {
            p.line_segment(
                [
                    line[0] + direction * x,
                    line[0] + direction * (x + dash).min(length),
                ],
                Stroke::new(width, color),
            );
            x += dash + gap;
        }
    } else {
        p.line_segment(line, Stroke::new(width, color));
    }
}
fn clipped_line(line: [Pos2; 2], r: Rect) -> Option<[Pos2; 2]> {
    let delta = line[1] - line[0];
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for (p, q) in [
        (-delta.x, line[0].x - r.left()),
        (delta.x, r.right() - line[0].x),
        (-delta.y, line[0].y - r.top()),
        (delta.y, r.bottom() - line[0].y),
    ] {
        if p.abs() < f32::EPSILON {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                low = low.max(t);
            } else {
                high = high.min(t);
            }
        }
    }
    if low > high {
        None
    } else {
        Some([line[0] + delta * low, line[0] + delta * high])
    }
}
fn pattern_fill(p: &egui::Painter, r: Rect, s: &CellStyle, z: f32) {
    let bg = parse_color(s.fill_background.as_deref()).unwrap_or(Color32::WHITE);
    let fg = parse_color(s.fill.as_deref()).unwrap_or(Color32::from_gray(160));
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    p.rect_filled(r, 0.0, bg);
    let visible = r.intersect(p.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let pattern = s.fill_pattern.to_ascii_lowercase();
    let step = if pattern.starts_with("dark") {
        4.0
    } else {
        8.0
    } * z;
    let stroke = Stroke::new(
        if pattern.starts_with("dark") {
            1.5 * z
        } else {
            z
        },
        fg,
    );
    let horizontal = pattern.contains("horizontal") || pattern.contains("grid");
    let vertical = pattern.contains("vertical") || pattern.contains("grid");
    if horizontal {
        let mut y = r.top() + ((visible.top() - r.top()) / step).floor() * step;
        while y <= visible.bottom() {
            p.line_segment(
                [Pos2::new(visible.left(), y), Pos2::new(visible.right(), y)],
                stroke,
            );
            y += step;
        }
    }
    if vertical {
        let mut x = r.left() + ((visible.left() - r.left()) / step).floor() * step;
        while x <= visible.right() {
            p.line_segment(
                [Pos2::new(x, visible.top()), Pos2::new(x, visible.bottom())],
                stroke,
            );
            x += step;
        }
    }
    let down = pattern.contains("down") || pattern.contains("trellis");
    let up = pattern.contains("up") || pattern.contains("trellis");
    for slope in [-1.0_f32, 1.0] {
        if (slope < 0.0 && !up) || (slope > 0.0 && !down) {
            continue;
        }
        // Lines y - slope*x = constant stay anchored to the cell while scrolling.
        let corners = [
            visible.left_top(),
            visible.right_top(),
            visible.left_bottom(),
            visible.right_bottom(),
        ];
        let values = corners.map(|v| (v.y - r.top()) - slope * (v.x - r.left()));
        let start = values.into_iter().fold(f32::INFINITY, f32::min);
        let end = values.into_iter().fold(f32::NEG_INFINITY, f32::max);
        let mut intercept = (start / step).floor() * step;
        while intercept <= end {
            let line = [
                Pos2::new(
                    visible.left(),
                    r.top() + slope * (visible.left() - r.left()) + intercept,
                ),
                Pos2::new(
                    visible.right(),
                    r.top() + slope * (visible.right() - r.left()) + intercept,
                ),
            ];
            if let Some(line) = clipped_line(line, visible) {
                p.line_segment(line, stroke);
            }
            intercept += step;
        }
    }
    if !horizontal && !vertical && !down && !up {
        let step = match pattern.as_str() {
            "darkgray" => 2.0,
            "mediumgray" => 3.0,
            "lightgray" => 4.0,
            "gray125" => 6.0,
            "gray0625" => 8.0,
            _ => 6.0,
        } * z;
        let mut row = ((visible.top() - r.top()) / step).floor() as i32;
        let mut y = r.top() + row as f32 * step;
        while y <= visible.bottom() {
            let shift = if row % 2 == 0 { 0.0 } else { step / 2.0 };
            let mut x = r.left() + ((visible.left() - r.left()) / step).floor() * step + shift;
            while x <= visible.right() {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, y), Vec2::splat(z)),
                    0.0,
                    fg,
                );
                x += step;
            }
            row += 1;
            y += step;
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct FontKey {
    name: String,
    bold: bool,
    italic: bool,
}
#[derive(Clone)]
struct FontFace {
    path: PathBuf,
    index: u32,
    names: HashSet<String>,
    weight: u16,
    italic: bool,
}
struct SourceFonts {
    definitions: egui::FontDefinitions,
    families: HashMap<FontKey, FontFamily>,
    actual_italics: HashSet<FontKey>,
    unavailable: HashSet<FontKey>,
    catalog: Option<Vec<FontFace>>,
    missing: BTreeSet<String>,
    pending: bool,
}
impl SourceFonts {
    fn new(definitions: egui::FontDefinitions) -> Self {
        Self {
            definitions,
            families: HashMap::new(),
            actual_italics: HashSet::new(),
            unavailable: HashSet::new(),
            catalog: None,
            missing: BTreeSet::new(),
            pending: false,
        }
    }
    fn key(s: &CellStyle) -> FontKey {
        FontKey {
            name: s.font_name.clone().unwrap_or_else(|| "Pretendard".into()),
            bold: s.bold,
            italic: s.italic,
        }
    }
    fn family(&self, s: &CellStyle) -> FontFamily {
        if self.pending {
            return fallback_family(s.bold);
        }
        self.families
            .get(&Self::key(s))
            .cloned()
            .unwrap_or_else(|| fallback_family(s.bold))
    }
    fn has_italic(&self, s: &CellStyle) -> bool {
        !self.pending && self.actual_italics.contains(&Self::key(s))
    }
    fn prepare(
        &mut self,
        ctx: &egui::Context,
        grid: &Grid,
        display_cells: &HashMap<(u32, u32), Cell>,
    ) {
        self.missing.clear();
        let mut keys: HashSet<_> = grid.cells.iter().map(|c| Self::key(&c.style)).collect();
        keys.extend(display_cells.values().map(|c| Self::key(&c.style)));
        for object in &grid.layout.drawings {
            drawing_font_keys(object, &mut keys);
        }
        keys.extend(
            grid.layout
                .row_styles
                .values()
                .chain(grid.layout.column_styles.values())
                .map(Self::key),
        );
        keys.insert(Self::key(&grid.layout.default_style));
        if let Some(normal_style) = &grid.layout.normal_style {
            keys.insert(Self::key(normal_style));
        }
        if self.catalog.is_none() {
            self.catalog = Some(font_catalog());
        }
        let mut changed = false;
        for key in keys {
            if self.families.contains_key(&key) {
                if self.unavailable.contains(&key) {
                    self.missing.insert(key.name.clone());
                }
                continue;
            }
            let wanted = normalize(&key.name);
            let target = if key.bold { 700 } else { 400 };
            let face = self
                .catalog
                .as_ref()
                .unwrap()
                .iter()
                .filter(|f| f.names.contains(&wanted))
                .min_by_key(|f| {
                    (i32::from(f.weight) - target).unsigned_abs()
                        + if f.italic == key.italic { 0 } else { 1000 }
                });
            if let Some(face) = face
                && let Ok(data) = std::fs::read(&face.path)
            {
                let name = format!("source-font-{}", self.families.len());
                let mut data = egui::FontData::from_owned(data);
                data.index = face.index;
                self.definitions.font_data.insert(name.clone(), data.into());
                let family = FontFamily::Name(name.clone().into());
                let mut fallback = self
                    .definitions
                    .families
                    .get(&fallback_family(key.bold))
                    .cloned()
                    .unwrap_or_default();
                fallback.insert(0, name);
                self.definitions.families.insert(family.clone(), fallback);
                if face.italic {
                    self.actual_italics.insert(key.clone());
                }
                self.families.insert(key, family);
                changed = true;
            } else {
                self.missing.insert(key.name.clone());
                self.unavailable.insert(key.clone());
                self.families.insert(key.clone(), fallback_family(key.bold));
            }
        }
        if changed {
            ctx.set_fonts(self.definitions.clone());
            ctx.request_repaint();
            self.pending = true;
        }
    }
}
fn fallback_family(bold: bool) -> FontFamily {
    if bold {
        FontFamily::Name("Pretendard SemiBold".into())
    } else {
        FontFamily::Proportional
    }
}
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}
fn font_catalog() -> Vec<FontFace> {
    let mut directories = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("C:/Windows/Fonts"),
        PathBuf::from("/Applications/Microsoft Excel.app/Contents/Resources/DFonts"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        directories.push(PathBuf::from(home).join("Library/Fonts"));
    }
    let mut files = vec![];
    while let Some(dir) = directories.pop() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    directories.push(path);
                } else if path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                    ["ttf", "otf", "ttc"]
                        .iter()
                        .any(|e| s.eq_ignore_ascii_case(e))
                }) {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files.dedup();
    let mut catalog = vec![];
    for path in files {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        for index in 0..ttf_parser::fonts_in_collection(&data).unwrap_or(1) {
            let Ok(face) = ttf_parser::Face::parse(&data, index) else {
                continue;
            };
            let names = face
                .names()
                .into_iter()
                .filter(|n| {
                    [
                        ttf_parser::name_id::FAMILY,
                        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
                        ttf_parser::name_id::FULL_NAME,
                    ]
                    .contains(&n.name_id)
                })
                .filter_map(|n| n.to_string())
                .map(|s| normalize(&s))
                .collect();
            catalog.push(FontFace {
                path: path.clone(),
                index,
                names,
                weight: face.weight().to_number(),
                italic: face.is_italic(),
            });
        }
    }
    catalog
}

fn drawing_text_style(run: &crate::drawings::TextRun) -> CellStyle {
    CellStyle {
        font_name: run.font_name.clone(),
        font_size: run.size,
        bold: run.bold,
        italic: run.italic,
        ..Default::default()
    }
}
fn drawing_font_keys(object: &crate::drawings::DrawingObject, keys: &mut HashSet<FontKey>) {
    match &object.content {
        crate::drawings::DrawingContent::Shape(shape) => {
            for run in shape.paragraphs.iter().flat_map(|p| &p.runs) {
                keys.insert(SourceFonts::key(&drawing_text_style(run)));
            }
        }
        crate::drawings::DrawingContent::Group { children } => {
            for child in children {
                drawing_font_keys(&child.object, keys);
            }
        }
        _ => {}
    }
}
fn drawing_extent(grid: &Grid, digit_width: f32) -> (u32, u32) {
    use crate::drawings::DrawingAnchor;
    let h = (grid.layout.default_row_height * POINT_SCALE).max(1.0);
    let w = column_pixels(grid.layout.default_col_width, digit_width).max(1.0);
    let extra =
        |v: f64, size: f32| ((v / 12700.0) as f32 * POINT_SCALE / size).ceil().max(0.0) as u32;
    let mut rows = 0;
    let mut cols = 0;
    for object in &grid.layout.drawings {
        let (r, c) = match &object.anchor {
            DrawingAnchor::TwoCell { from, to } => (
                from.row.max(to.row) + extra(to.row_offset_emu, h) + 2,
                from.col.max(to.col) + extra(to.col_offset_emu, w) + 2,
            ),
            DrawingAnchor::OneCell {
                from,
                width_emu,
                height_emu,
            } => (
                from.row
                    .saturating_add(extra(from.row_offset_emu + height_emu, h))
                    .saturating_add(2),
                from.col
                    .saturating_add(extra(from.col_offset_emu + width_emu, w))
                    .saturating_add(2),
            ),
            DrawingAnchor::Absolute {
                x_emu,
                y_emu,
                width_emu,
                height_emu,
            } => (
                extra(y_emu + height_emu, h).saturating_add(2),
                extra(x_emu + width_emu, w).saturating_add(2),
            ),
        };
        rows = rows.max(r);
        cols = cols.max(c);
    }
    (rows.min(1_048_576), cols.min(16_384))
}
fn paint_conditional_visuals(
    p: &egui::Painter,
    rect: Rect,
    cell: &crate::conditional::ConditionalCell,
    z: f32,
) {
    if let Some(bar) = &cell.data_bar {
        let r = rect.shrink2(Vec2::new(2.0 * z, 3.0 * z));
        let x = |v: f32| r.left() + r.width() * v.clamp(0.0, 1.0);
        let (left, right) = (x(bar.start.min(bar.end)), x(bar.start.max(bar.end)));
        let color = parse_color(Some(&bar.color)).unwrap_or(GREEN);
        if right > left && r.height() > 0.0 {
            let area = Rect::from_min_max(Pos2::new(left, r.top()), Pos2::new(right, r.bottom()));
            if bar.gradient {
                let mut mesh = egui::Mesh::default();
                let pale = Color32::from_rgb(
                    (color.r() as f32 * 0.15 + 255.0 * 0.85) as u8,
                    (color.g() as f32 * 0.15 + 255.0 * 0.85) as u8,
                    (color.b() as f32 * 0.15 + 255.0 * 0.85) as u8,
                );
                let (a, b) = if bar.start <= bar.end {
                    (color, pale)
                } else {
                    (pale, color)
                };
                for (pos, color) in [
                    (area.left_top(), a),
                    (area.right_top(), b),
                    (area.right_bottom(), b),
                    (area.left_bottom(), a),
                ] {
                    mesh.colored_vertex(pos, color);
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                p.add(egui::Shape::mesh(mesh));
            } else {
                p.rect_filled(area, 0.0, color);
            }
        }
        if let Some(zero) = bar.zero {
            let x = x(zero);
            p.line_segment(
                [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
                Stroke::new(
                    z.max(0.5),
                    parse_color(Some(&bar.axis_color)).unwrap_or(Color32::GRAY),
                ),
            );
        }
    }
    if let Some(icon) = &cell.icon {
        let size = (14.0 * z).min((rect.height() - 2.0 * z).max(1.0));
        crate::conditional_icons::paint(
            p,
            Pos2::new(rect.left() + 9.0 * z, rect.center().y),
            size,
            icon,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_column_width_uses_ooxml_pixel_formula_without_duplicate_padding() {
        assert_eq!(column_pixels(8.7109375, 7.0), 61.0);
        assert_eq!(column_pixels(9.140625, 7.0), 64.0);
        assert_eq!(column_pixels(0.0, 7.0), 0.0);
    }
    #[test]
    fn hidden_axes_keep_source_coordinates_and_skip_zero_width_hits() {
        let axis = Axis::new(4, |n| if n == 2 { 0.0 } else { 20.0 });
        assert_eq!(axis.at(21.0), 3);
        assert_eq!(axis.interval(3), (20.0, 20.0));
        assert_eq!(axis.total(), 60.0);
    }
    #[test]
    fn border_segments_are_clipped_before_generating_dashes() {
        let clip = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0));
        assert_eq!(
            clipped_line(
                [Pos2::new(-1_000_000.0, 50.0), Pos2::new(1_000_000.0, 50.0)],
                clip
            ),
            Some([Pos2::new(0.0, 50.0), Pos2::new(100.0, 50.0)])
        );
        assert!(clipped_line([Pos2::new(-3.0, -3.0), Pos2::new(-1.0, -1.0)], clip).is_none());
    }
}
