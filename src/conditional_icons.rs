//! Native vector counterparts of SpreadsheetML's 17 legacy icon sets.
//! Set ordering follows MS-XLSX ST_IconSetType and Excel's XlIcon enumeration.
//! Concave arrows and pie sectors use explicit triangle meshes, never a convex fill.
use crate::conditional::Icon;
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

const RED: Color32 = Color32::from_rgb(222, 70, 62);
const YELLOW: Color32 = Color32::from_rgb(247, 194, 69);
const GREEN: Color32 = Color32::from_rgb(95, 166, 84);
const INK: Color32 = Color32::from_rgb(61, 64, 64);
const GREY: Color32 = Color32::from_rgb(153, 153, 153);

fn count(set: &str) -> usize {
    if set.starts_with('5') {
        5
    } else if set.starts_with('4') {
        4
    } else {
        3
    }
}
fn traffic_color(index: usize, count: usize) -> Color32 {
    if index == 0 {
        RED
    } else if index + 1 == count {
        GREEN
    } else {
        YELLOW
    }
}
fn darken(color: Color32) -> Color32 {
    Color32::from_rgb(
        (f32::from(color.r()) * 0.58) as u8,
        (f32::from(color.g()) * 0.58) as u8,
        (f32::from(color.b()) * 0.58) as u8,
    )
}
fn lighten(color: Color32, amount: f32) -> Color32 {
    let blend = |c| (f32::from(c) + (255.0 - f32::from(c)) * amount) as u8;
    Color32::from_rgb(blend(color.r()), blend(color.g()), blend(color.b()))
}
fn arrow_angle(index: usize, count: usize) -> f32 {
    match count {
        4 => [PI, 3.0 * FRAC_PI_4, FRAC_PI_4, 0.0][index.min(3)],
        5 => [PI, 3.0 * FRAC_PI_4, FRAC_PI_2, FRAC_PI_4, 0.0][index.min(4)],
        _ => [PI, FRAC_PI_2, 0.0][index.min(2)],
    }
}
fn transform(center: Pos2, size: f32, angle: f32, p: [f32; 2]) -> Pos2 {
    let (sin, cos) = angle.sin_cos();
    center + Vec2::new(p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos) * size
}
fn arrow_mesh(center: Pos2, size: f32, angle: f32, color: Color32) -> (Mesh, Vec<Pos2>) {
    let points = [
        [-0.16, 0.43],
        [0.16, 0.43],
        [0.16, -0.04],
        [0.43, -0.04],
        [0.0, -0.47],
        [-0.43, -0.04],
        [-0.16, -0.04],
    ];
    let mut mesh = Mesh::default();
    let path: Vec<_> = points
        .iter()
        .map(|p| transform(center, size, angle, *p))
        .collect();
    for (p, original) in path.iter().zip(points) {
        let shade = if original[1] < -0.03 {
            lighten(color, 0.18)
        } else {
            color
        };
        mesh.colored_vertex(*p, shade);
    }
    // A rectangular shaft and a triangular head share an edge; their interiors
    // do not overlap. A fan over this concave outline would fill its notches.
    for [a, b, c] in [[0, 1, 2], [0, 2, 6], [3, 4, 5]] {
        mesh.add_triangle(a, b, c);
    }
    (mesh, path)
}
fn sector_mesh(center: Pos2, radius: f32, fraction: f32, color: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction == 0.0 {
        return mesh;
    }
    let steps = (64.0 * fraction).ceil() as u32;
    mesh.colored_vertex(center, color);
    for i in 0..=steps {
        let angle = -FRAC_PI_2 + TAU * fraction * i as f32 / steps as f32;
        mesh.colored_vertex(center + Vec2::angled(angle) * radius, color);
    }
    for i in 1..=steps {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh
}
fn polygon(p: &egui::Painter, points: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    p.add(Shape::convex_polygon(points, fill, stroke));
}
fn circle(p: &egui::Painter, center: Pos2, radius: f32, color: Color32, width: f32) {
    p.circle_filled(center, radius, color);
    p.circle_stroke(center, radius, Stroke::new(width, darken(color)));
    // Excel's legacy circle icons have a small highlight in the upper left.
    p.circle_filled(
        center + Vec2::new(-radius * 0.27, -radius * 0.3),
        radius * 0.34,
        lighten(color, 0.28),
    );
}

pub(crate) fn paint(p: &egui::Painter, center: Pos2, size: f32, icon: &Icon) {
    if !size.is_finite() || size <= 0.0 {
        return;
    }
    let set = icon.set.as_str();
    let count = count(set);
    let index = icon.index.min(count - 1);
    let at = |x, y| center + Vec2::new(x, y) * size;
    let line = (size / 16.0).max(0.45);
    let color = if set.ends_with("Gray") {
        GREY
    } else {
        traffic_color(index, count)
    };
    let stroke = Stroke::new(line, darken(color));
    match set {
        "3Arrows" | "3ArrowsGray" | "4Arrows" | "4ArrowsGray" | "5Arrows" | "5ArrowsGray" => {
            let (mesh, path) = arrow_mesh(center, size, arrow_angle(index, count), color);
            p.add(Shape::mesh(mesh));
            p.add(Shape::closed_line(path, stroke));
        }
        "3Flags" => {
            p.line_segment(
                [at(-0.29, -0.43), at(-0.29, 0.45)],
                Stroke::new(line * 1.4, INK),
            );
            let path = vec![
                at(-0.24, -0.40),
                at(0.08, -0.34),
                at(0.39, -0.39),
                at(0.39, 0.03),
                at(0.08, 0.08),
                at(-0.24, 0.02),
            ];
            let mut mesh = Mesh::default();
            for point in &path {
                mesh.colored_vertex(*point, color);
            }
            for [a, b, c] in [[0, 1, 5], [1, 4, 5], [1, 2, 4], [2, 3, 4]] {
                mesh.add_triangle(a, b, c);
            }
            p.add(Shape::mesh(mesh));
            p.add(Shape::closed_line(path, stroke));
            p.line_segment(
                [at(-0.2, -0.32), at(0.07, -0.27)],
                Stroke::new(line, lighten(color, 0.5)),
            );
        }
        "3TrafficLights1" => circle(p, center, size * 0.4, color, line),
        "3TrafficLights2" => {
            let frame = Rect::from_center_size(center, Vec2::new(size * 0.87, size * 0.97));
            p.rect_filled(frame, size * 0.12, INK);
            p.circle_filled(center, size * 0.32, color);
            p.circle_stroke(center, size * 0.32, Stroke::new(line, Color32::BLACK));
            p.circle_filled(at(-0.09, -0.10), size * 0.10, lighten(color, 0.38));
        }
        "4TrafficLights" => {
            // Low-to-high is black, red, yellow, green, not two red levels.
            let color = [INK, RED, YELLOW, GREEN][index];
            circle(p, center, size * 0.4, color, line);
        }
        "4RedToBlack" => {
            // This set's highest level is red; it intentionally differs from
            // traffic lights. Excel calls the intermediate levels gray and pink.
            let color = [
                Color32::from_rgb(48, 48, 48),
                Color32::from_rgb(155, 155, 155),
                Color32::from_rgb(235, 170, 170),
                Color32::from_rgb(238, 82, 83),
            ][index];
            circle(p, center, size * 0.4, color, line);
        }
        "3Signs" => match index {
            0 => polygon(
                p,
                vec![at(0.0, -0.45), at(0.45, 0.0), at(0.0, 0.45), at(-0.45, 0.0)],
                RED,
                stroke,
            ),
            1 => polygon(
                p,
                vec![at(0.0, -0.47), at(0.47, 0.4), at(-0.47, 0.4)],
                YELLOW,
                stroke,
            ),
            _ => circle(p, center, size * 0.4, GREEN, line),
        },
        "3Symbols" | "3Symbols2" => {
            let circled = set == "3Symbols";
            let ink = if circled {
                if index == 1 {
                    Color32::from_rgb(110, 83, 10)
                } else {
                    Color32::WHITE
                }
            } else {
                color
            };
            if circled {
                p.circle_filled(center, size * 0.44, color);
                p.circle_stroke(center, size * 0.44, stroke);
            }
            let pen = Stroke::new(size * 0.13, ink);
            match index {
                0 => {
                    p.line_segment([at(-0.24, -0.24), at(0.24, 0.24)], pen);
                    p.line_segment([at(-0.24, 0.24), at(0.24, -0.24)], pen);
                }
                1 => {
                    p.line_segment([at(0.0, -0.27), at(0.0, 0.10)], pen);
                    p.circle_filled(at(0.0, 0.28), size * 0.065, ink);
                }
                _ => {
                    p.line_segment([at(-0.28, -0.01), at(-0.07, 0.22)], pen);
                    p.line_segment([at(-0.07, 0.22), at(0.30, -0.28)], pen);
                }
            }
        }
        "4Rating" | "5Rating" => {
            // Both sets use four bars. 4Rating has one through four filled;
            // 5Rating additionally has the all-empty level (XlIcon0Bars).
            let filled = if count == 4 { index + 1 } else { index };
            for i in 0..4 {
                let left = -0.45 + i as f32 * 0.24;
                let height = 0.22 + i as f32 * 0.22;
                let rect = Rect::from_min_max(at(left, 0.44 - height), at(left + 0.17, 0.44));
                let fill = if i < filled {
                    Color32::from_rgb(82, 105, 136)
                } else {
                    Color32::from_rgb(234, 237, 241)
                };
                p.rect_filled(rect, 0.0, fill);
                p.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(line * 0.65, Color32::from_rgb(97, 105, 115)),
                    StrokeKind::Inside,
                );
            }
        }
        "5Quarters" => {
            p.circle_filled(center, size * 0.43, Color32::WHITE);
            let mesh = sector_mesh(center, size * 0.43, index as f32 / 4.0, INK);
            if !mesh.indices.is_empty() {
                p.add(Shape::mesh(mesh));
            }
            p.circle_stroke(center, size * 0.43, Stroke::new(line, INK));
        }
        _ => {} // Parser explicitly reports unknown sets; never substitute one.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mesh_area(mesh: &Mesh) -> f32 {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let a = mesh.vertices[t[0] as usize].pos;
                let b = mesh.vertices[t[1] as usize].pos;
                let c = mesh.vertices[t[2] as usize].pos;
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
            })
            .sum()
    }
    #[test]
    fn three_quarter_sector_is_tessellated_without_convex_hull_overfill() {
        let mesh = sector_mesh(Pos2::ZERO, 10.0, 0.75, INK);
        assert!((mesh_area(&mesh) - PI * 100.0 * 0.75).abs() < 0.5);
        assert!(mesh.is_valid());
        let full = sector_mesh(Pos2::ZERO, 10.0, 1.0, INK);
        assert!((mesh_area(&full) - PI * 100.0).abs() < 0.6);
    }
    #[test]
    fn concave_arrow_mesh_keeps_notches_and_cardinal_directions() {
        let (mesh, _) = arrow_mesh(Pos2::ZERO, 1.0, 0.0, GREEN);
        // Shaft .32 × .47 + triangular head .86 × .43 / 2.
        assert!((mesh_area(&mesh) - (0.32 * 0.47 + 0.86 * 0.43 / 2.0)).abs() < 0.0001);
        assert!(mesh.is_valid());
        let tip = transform(Pos2::ZERO, 1.0, arrow_angle(1, 3), [0.0, -0.47]);
        assert!(tip.x > 0.46 && tip.y.abs() < 0.001);
        let tip = transform(Pos2::ZERO, 1.0, arrow_angle(1, 4), [0.0, -0.47]);
        assert!(tip.x > 0.3 && tip.y > 0.3);
    }
}
