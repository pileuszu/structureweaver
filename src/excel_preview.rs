//! Optional in-memory Excel-rendered object previews. Source cells stay editable
//! only in the source application; no preview images are sent to the model.
use crate::{drawings::DrawingObject, excel_render};
use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, Vec2};
use std::{collections::HashMap, path::Path, sync::mpsc, thread};

#[derive(Clone, Copy)]
pub enum Layer {
    Native(usize),
    Excel(usize),
}

#[derive(Default)]
pub struct ExcelPreview {
    document_id: String,
    workbook: Option<excel_render::RenderedWorkbook>,
    receiver: Option<mpsc::Receiver<Result<excel_render::RenderedWorkbook, String>>>,
    textures: HashMap<usize, TextureHandle>,
    placement: HashMap<usize, Rect>,
    enabled: bool,
    error: Option<String>,
}

impl ExcelPreview {
    pub fn controls(&mut self, ui: &mut egui::Ui, document_id: &str, source: Option<&Path>) {
        if !cfg!(feature = "excel-render-experimental") {
            return;
        }
        if self.document_id != document_id {
            *self = Self {
                document_id: document_id.into(),
                ..Default::default()
            };
        }
        if let Some(result) = self.receiver.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.receiver = None;
            match result {
                Ok(workbook) if workbook.objects.is_empty() => {
                    self.error = Some(format!(
                        "Excel에서 내보낼 수 있는 차트·도형이 없습니다. {}",
                        workbook.warnings.join("\n")
                    ));
                }
                Ok(workbook) => {
                    self.workbook = Some(workbook);
                    self.textures.clear();
                    self.placement.clear();
                    self.enabled = true;
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.enabled, false, "Rust 미리보기");
            if self.workbook.is_some() {
                ui.selectable_value(&mut self.enabled, true, "Excel 원본 개체");
            }
            let busy = self.receiver.is_some();
            let title = if self.workbook.is_some() { "Excel 개체 새로고침" } else { "Excel 원본 개체 불러오기 · 실험" };
            if ui.add_enabled(!busy && excel_render::available(), egui::Button::new(title))
                .on_hover_text("설치된 Microsoft Excel이 차트와 도형을 직접 그립니다. 임시 복사본을 사용하며 원본은 저장하지 않습니다.")
                .clicked()
            {
                let path = source.map(Path::to_path_buf).or_else(|| {
                    rfd::FileDialog::new().set_title("이 분석의 원본 Excel 파일 선택")
                        .add_filter("Excel", &["xlsx", "xlsm"]).pick_file()
                });
                if let Some(path) = path {
                    let document_id = document_id.to_owned();
                    let (tx, rx) = mpsc::channel();
                    self.receiver = Some(rx);
                    self.error = None;
                    let ctx = ui.ctx().clone();
                    thread::spawn(move || {
                        let result = std::panic::catch_unwind(|| excel_render::render(&path, &document_id))
                            .map_err(|_| "Excel 미리보기를 생성하는 중 오류가 발생했습니다.".to_owned())
                            .and_then(|r| r.map_err(|e| format!("{e:#}")));
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    });
                }
            }
            if busy {
                ui.spinner();
                ui.label("Excel에서 차트·도형을 그리고 있습니다…");
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            if self.enabled {
                ui.label(egui::RichText::new("차트·도형: Excel · 셀: Rust").small().color(Color32::DARK_GRAY));
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(
                Color32::from_rgb(157, 101, 0),
                "Excel 원본 개체를 불러오지 못했습니다. Rust 미리보기를 계속 사용할 수 있습니다.",
            );
            ui.collapsing("오류 자세히", |ui| {
                ui.label(error);
            });
        }
        if let Some(workbook) = &self.workbook
            && !workbook.warnings.is_empty()
        {
            ui.collapsing("Excel 표시 정보", |ui| {
                for warning in &workbook.warnings {
                    ui.label(warning);
                }
            });
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled && self.workbook.is_some()
    }

    pub fn extent(&self, sheet: &str) -> Vec2 {
        self.workbook
            .as_ref()
            .filter(|_| self.enabled)
            .map_or(Vec2::ZERO, |workbook| {
                workbook
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.sheet_name == sheet)
                    .fold(Vec2::ZERO, |extent, (index, o)| {
                        extent.max(
                            self.placement
                                .get(&index)
                                .copied()
                                .unwrap_or_else(|| image_rect(o, None))
                                .max
                                .to_vec2(),
                        )
                    })
            })
    }

    /// Preserve source drawing order while substituting successful Excel exports.
    /// Missing exports keep their native object and never disappear silently.
    pub fn layers(&mut self, sheet: &str, native: &[DrawingObject]) -> Vec<Layer> {
        let Some(workbook) = self.workbook.as_ref().filter(|_| self.enabled) else {
            return (0..native.len()).map(Layer::Native).collect();
        };
        let objects: Vec<_> = workbook
            .objects
            .iter()
            .enumerate()
            .filter(|(_, object)| object.sheet_name == sheet)
            .collect();
        for (index, object) in &objects {
            self.placement.entry(*index).or_insert_with(|| {
                let source = native.iter().find(|n| n.name == object.object_name);
                image_rect(object, source)
            });
        }
        let mut layers: Vec<_> = objects
            .iter()
            .map(|(index, object)| (object.z_order as f64, Layer::Excel(*index)))
            .collect();
        for (index, object) in native.iter().enumerate() {
            if objects
                .iter()
                .any(|(_, exported)| exported.object_name == object.name)
            {
                continue;
            }
            let before = native[..index].iter().rev().find_map(|n| {
                objects
                    .iter()
                    .find(|(_, o)| o.object_name == n.name)
                    .map(|(_, o)| o.z_order)
            });
            let after = native[index + 1..].iter().find_map(|n| {
                objects
                    .iter()
                    .find(|(_, o)| o.object_name == n.name)
                    .map(|(_, o)| o.z_order)
            });
            let order = before
                .map(|v| v as f64 + 0.001 * (index + 1) as f64)
                .or_else(|| after.map(|v| v as f64 - 0.5 + 0.001 * index as f64))
                .unwrap_or(index as f64 + 1.0);
            layers.push((order, Layer::Native(index)));
        }
        layers.sort_by(|a, b| a.0.total_cmp(&b.0));
        layers.into_iter().map(|(_, layer)| layer).collect()
    }

    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        index: usize,
        origin: Pos2,
        scale: f32,
        fit: Option<Rect>,
    ) {
        let Some(object) = self.workbook.as_ref().and_then(|w| w.objects.get(index)) else {
            return;
        };
        let source_rect = self
            .placement
            .get(&index)
            .copied()
            .unwrap_or_else(|| image_rect(object, None));
        let rect = if let Some(fit) = fit {
            let ratio = source_rect.width() / source_rect.height().max(1.0);
            let size = if fit.width() / fit.height() > ratio {
                Vec2::new(fit.height() * ratio, fit.height())
            } else {
                Vec2::new(fit.width(), fit.width() / ratio)
            };
            Rect::from_center_size(fit.center(), size)
        } else {
            Rect::from_min_size(
                origin + source_rect.min.to_vec2() * scale,
                source_rect.size() * scale,
            )
        };
        if !painter.clip_rect().intersects(rect) {
            return;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.textures.entry(index)
            && let Ok(image) = image::load_from_memory(&object.png)
        {
            let image = image.to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            entry.insert(painter.ctx().load_texture(
                format!("excel-{}-{index}", self.document_id),
                color,
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(texture) = self.textures.get(&index) {
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

/// Excel PNGs include effect margins. Never squeeze reflection/shadow pixels
/// into the original shape rectangle. For a flat, opaque solid-fill shape,
/// register the largest fill component to the original geometry's center.
/// This excludes its translucent reflection and asymmetric shadow padding.
fn image_rect(object: &excel_render::RenderedObject, source: Option<&DrawingObject>) -> Rect {
    let [x, y, w, h] = object.rect_points;
    let anchor = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
    let pixels = Vec2::new(object.png_pixels[0] as f32, object.png_pixels[1] as f32);
    let density = object.pixels_per_point.map(|p| Vec2::new(p[0], p[1]));
    let size = density.map(|p| pixels / p).unwrap_or_else(|| {
        let a = object.source_rotation_degrees.to_radians();
        let rotated_w = w * a.cos().abs() + h * a.sin().abs();
        Vec2::new(rotated_w, rotated_w * pixels.y / pixels.x.max(1.0))
    });
    if let (Some(density), Some(source)) = (density, source)
        && source.rotation.abs() < 0.001
        && let crate::drawings::DrawingContent::Shape(shape) = &source.content
        && shape.effects.scene.is_none()
        && shape.gradient.is_none()
        && let Some(fill) = shape.fill.as_ref().and_then(|c| solid_rgb(c))
        && let Ok(image) = image::load_from_memory(&object.png)
        && let Some(center) = fill_component_center(&image.to_rgba8(), fill)
    {
        return Rect::from_min_size(anchor.center() - center / density, size);
    }
    Rect::from_center_size(anchor.center(), size)
}

fn solid_rgb(color: &str) -> Option<[u8; 3]> {
    let c = color.strip_prefix('#').unwrap_or(color);
    if !c.is_ascii() {
        return None;
    }
    let rgb = match c.len() {
        6 => c,
        8 if c[..2].eq_ignore_ascii_case("ff") => &c[2..],
        _ => return None,
    };
    let value = u32::from_str_radix(rgb, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

fn fill_component_center(image: &image::RgbaImage, fill: [u8; 3]) -> Option<Vec2> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w.checked_mul(h)? > 2_000_000 {
        return None;
    }
    let mut mask: Vec<bool> = image
        .pixels()
        .map(|p| p[3] >= 254 && (0..3).all(|c| p[c].abs_diff(fill[c]) <= 2))
        .collect();
    let mut best = (0, Vec2::ZERO);
    let mut pending = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] {
            continue;
        }
        mask[start] = false;
        pending.push(start);
        let (mut left, mut right, mut top, mut bottom, mut count) = (w, 0, h, 0, 0usize);
        while let Some(i) = pending.pop() {
            let (x, y) = (i % w, i / w);
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
            count += 1;
            for next in [
                if x > 0 { Some(i - 1) } else { None },
                if x + 1 < w { Some(i + 1) } else { None },
                if y > 0 { Some(i - w) } else { None },
                if y + 1 < h { Some(i + w) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if mask[next] {
                    mask[next] = false;
                    pending.push(next);
                }
            }
        }
        if count > best.0 {
            best = (
                count,
                Vec2::new(
                    (left + right + 1) as f32 / 2.0,
                    (top + bottom + 1) as f32 / 2.0,
                ),
            );
        }
    }
    (best.0 >= 64).then_some(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reflection_and_asymmetric_shadow_do_not_shrink_or_shift_solid_body() {
        let mut png = image::RgbaImage::new(220, 190);
        for y in 6..86 {
            for x in 6..206 {
                png.put_pixel(x, y, image::Rgba([22, 160, 133, 255]));
            }
        }
        for y in 100..180 {
            for x in 6..206 {
                png.put_pixel(x, y, image::Rgba([22, 160, 133, 128]));
            }
        }
        let center = fill_component_center(&png, [22, 160, 133]).unwrap();
        assert_eq!(center, Vec2::new(106.0, 46.0));
        assert_eq!(Vec2::new(50.0, 20.0) - center / 2.0, Vec2::new(-3.0, -3.0));
    }
}
