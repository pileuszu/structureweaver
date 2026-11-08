//! Bounded software projection for DrawingML 3-D chart scenes. Source data stays
//! unchanged; faces are depth-sorted and tessellated by the native egui painter.
use super::*;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug)]
struct V3 {
    x: f32,
    y: f32,
    z: f32,
}
impl V3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
    fn cross(self, b: Self) -> Self {
        Self::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }
    fn dot(self, b: Self) -> f32 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }
}
struct Camera {
    view: ChartView3d,
    height: f32,
    depth: f32,
    origin: Pos2,
    scale: f32,
    offset: Vec2,
}
impl Camera {
    fn new(rect: Rect, view: &ChartView3d, pie: bool) -> Self {
        let mut camera = Self {
            view: view.clone(),
            height: if pie {
                0.13 * view.depth_percent / 100.0
            } else {
                0.8 * view.height_percent / 100.0
            },
            depth: if pie {
                1.0
            } else {
                0.5 * view.depth_percent / 100.0
            },
            origin: rect.center(),
            scale: 1.0,
            offset: Vec2::ZERO,
        };
        if pie {
            camera.view.right_angle_axes = false;
        }
        let mut bounds = Rect::NOTHING;
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    let v = camera.project_raw(V3::new(x, y, z));
                    bounds.extend_with(Pos2::new(v.x, v.y));
                }
            }
        }
        camera.offset = bounds.center().to_vec2();
        camera.scale = (rect.width() / bounds.width().max(0.001))
            .min(rect.height() / bounds.height().max(0.001))
            * 0.90;
        camera
    }
    fn rotate(&self, v: V3) -> V3 {
        let x = v.x - 0.5;
        let y = (v.y - 0.5) * self.height;
        let z = (v.z - 0.5) * self.depth;
        let (sx, cx) = self.view.rotation_x.to_radians().sin_cos();
        let (sy, cy) = self.view.rotation_y.to_radians().sin_cos();
        let xx = x * cy + z * sy;
        let zz = -x * sy + z * cy;
        V3::new(xx, y * cx - zz * sx, y * sx + zz * cx)
    }
    fn project_raw(&self, v: V3) -> V3 {
        let rotated = self.rotate(v);
        if self.view.right_angle_axes {
            let sx = self.view.rotation_x.to_radians().sin();
            let sy = self.view.rotation_y.to_radians().sin();
            V3::new(
                v.x - 0.5 + (v.z - 0.5) * self.depth * sy,
                -(v.y - 0.5) * self.height + (v.z - 0.5) * self.depth * sx,
                rotated.z,
            )
        } else {
            // OOXML perspective uses twice the angular field of view. Bound the
            // camera distance to keep every legal view finite at steep rotation.
            let angle = (self.view.perspective / 2.0).clamp(0.0, 120.0).to_radians();
            let strength = (angle / 2.0).tan() * 0.40;
            let perspective = 1.0 / (1.0 - rotated.z * strength).max(0.15);
            V3::new(rotated.x * perspective, -rotated.y * perspective, rotated.z)
        }
    }
    fn point(&self, v: V3) -> Pos2 {
        let v = self.project_raw(v);
        self.origin + (Vec2::new(v.x, v.y) - self.offset) * self.scale
    }
}
struct Face {
    points: Vec<V3>,
    color: Color32,
    line: bool,
}
fn shade(color: Color32, intensity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * intensity).clamp(0.0, 255.0) as u8,
        (color.g() as f32 * intensity).clamp(0.0, 255.0) as u8,
        (color.b() as f32 * intensity).clamp(0.0, 255.0) as u8,
        color.a(),
    )
}
fn face(faces: &mut Vec<Face>, points: Vec<V3>, color: Color32, line: bool) {
    if faces.len() < 80_000
        && points.len() >= 3
        && points
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
    {
        faces.push(Face {
            points,
            color,
            line,
        });
    }
}
fn cuboid(faces: &mut Vec<Face>, low: V3, high: V3, color: Color32) {
    let a = V3::new(low.x, low.y, low.z);
    let b = V3::new(high.x, low.y, low.z);
    let c = V3::new(high.x, high.y, low.z);
    let d = V3::new(low.x, high.y, low.z);
    let e = V3::new(low.x, low.y, high.z);
    let f = V3::new(high.x, low.y, high.z);
    let g = V3::new(high.x, high.y, high.z);
    let h = V3::new(low.x, high.y, high.z);
    for points in [
        vec![a, d, c, b],
        vec![e, f, g, h],
        vec![a, e, h, d],
        vec![b, c, g, f],
        vec![d, h, g, c],
        vec![a, b, f, e],
    ] {
        face(faces, points, color, false);
    }
}
fn round_bar(
    faces: &mut Vec<Face>,
    low: V3,
    high: V3,
    color: Color32,
    shape: &str,
    horizontal: bool,
) {
    let segments = if shape.starts_with("pyramid") { 4 } else { 24 };
    let cone = shape.starts_with("cone") || shape.starts_with("pyramid");
    let mut bottom = Vec::new();
    let mut top = Vec::new();
    for j in 0..segments {
        let angle = TAU * j as f32 / segments as f32 + if segments == 4 { TAU / 8.0 } else { 0.0 };
        let (sin, cos) = angle.sin_cos();
        let r = if segments == 4 {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };
        if horizontal {
            bottom.push(V3::new(
                low.x,
                (low.y + high.y) / 2.0 + (high.y - low.y) / 2.0 * cos * r,
                (low.z + high.z) / 2.0 + (high.z - low.z) / 2.0 * sin * r,
            ));
            top.push(if cone {
                V3::new(high.x, (low.y + high.y) / 2.0, (low.z + high.z) / 2.0)
            } else {
                V3::new(high.x, bottom[j].y, bottom[j].z)
            });
        } else {
            bottom.push(V3::new(
                (low.x + high.x) / 2.0 + (high.x - low.x) / 2.0 * cos * r,
                low.y,
                (low.z + high.z) / 2.0 + (high.z - low.z) / 2.0 * sin * r,
            ));
            top.push(if cone {
                V3::new((low.x + high.x) / 2.0, high.y, (low.z + high.z) / 2.0)
            } else {
                V3::new(bottom[j].x, high.y, bottom[j].z)
            });
        }
    }
    for j in 0..segments {
        let k = (j + 1) % segments;
        face(
            faces,
            vec![bottom[j], bottom[k], top[k], top[j]],
            color,
            false,
        );
    }
    face(faces, bottom, color, false);
    if !cone {
        face(faces, top, color, false);
    }
}
fn draw_faces(p: &egui::Painter, camera: &Camera, faces: &mut [Face], z: f32) {
    faces.sort_by(|a, b| {
        let depth = |f: &Face| {
            f.points.iter().map(|p| camera.rotate(*p).z).sum::<f32>() / f.points.len() as f32
        };
        depth(a).total_cmp(&depth(b))
    });
    for face in faces {
        let normal = camera
            .rotate(face.points[1])
            .sub(camera.rotate(face.points[0]))
            .cross(
                camera
                    .rotate(face.points[2])
                    .sub(camera.rotate(face.points[0])),
            );
        let norm = normal.dot(normal).sqrt();
        let illumination = if norm > 0.00001 {
            0.68 + 0.32 * (normal.dot(V3::new(-0.4, 0.7, 0.6)) / norm).abs()
        } else {
            0.8
        };
        let points: Vec<_> = face.points.iter().map(|v| camera.point(*v)).collect();
        if face.line {
            let mut outline = points;
            if let Some(first) = outline.first().copied() {
                outline.push(first);
            }
            p.add(Shape::line(outline, Stroke::new(0.7 * z, face.color)));
        } else {
            // Faces share exact edges; per-polygon antialiasing would introduce
            // pale seams across the pie fan and between adjacent surface cells.
            let mut mesh = egui::epaint::Mesh::default();
            for point in points {
                mesh.colored_vertex(point, shade(face.color, illumination));
            }
            for i in 1..face.points.len().saturating_sub(1) as u32 {
                mesh.add_triangle(0, i, i + 1);
            }
            p.add(Shape::mesh(mesh));
        }
    }
}
fn category_indices(chart: &Chart) -> Vec<usize> {
    let count = chart
        .groups
        .iter()
        .flat_map(|g| &g.series)
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0);
    (0..count)
        .filter(|i| {
            chart
                .groups
                .iter()
                .flat_map(|g| &g.series)
                .any(|s| *i < s.values.len() && !hidden(s, *i))
        })
        .collect()
}
pub(super) fn paint(p: &egui::Painter, rect: Rect, chart: &Chart, z: f32) {
    if !rect.is_positive() || chart.groups.is_empty() {
        return;
    }
    let pie = chart.groups[0].kind == "pie";
    let camera = Camera::new(rect, &chart.view_3d, pie);
    let mut faces = vec![];
    if pie {
        paint_pie(p, &camera, chart, z, &mut faces);
        return;
    }
    let categories = category_indices(chart);
    let count = categories.len().max(1);
    let scales = group_scales(chart);
    let primary = &chart.groups[0];
    let axis = value_axis(chart, primary);
    let floor = chart
        .view_3d
        .floor_color
        .as_deref()
        .map(rgba)
        .unwrap_or(Color32::from_gray(242));
    let wall = chart
        .view_3d
        .back_wall_color
        .as_deref()
        .or(chart.plot_background.as_deref())
        .map(rgba)
        .unwrap_or(Color32::from_gray(250));
    let side = chart
        .view_3d
        .side_wall_color
        .as_deref()
        .map(rgba)
        .unwrap_or(Color32::from_gray(245));
    let back = if camera.rotate(V3::new(0.5, 0.5, 0.0)).z < camera.rotate(V3::new(0.5, 0.5, 1.0)).z
    {
        0.0
    } else {
        1.0
    };
    let side_x =
        if camera.rotate(V3::new(0.0, 0.5, 0.5)).z < camera.rotate(V3::new(1.0, 0.5, 0.5)).z {
            0.0
        } else {
            1.0
        };
    let mut walls = vec![
        Face {
            points: vec![
                V3::new(0., 0., 0.),
                V3::new(1., 0., 0.),
                V3::new(1., 0., 1.),
                V3::new(0., 0., 1.),
            ],
            color: floor,
            line: false,
        },
        Face {
            points: vec![
                V3::new(0., 0., back),
                V3::new(1., 0., back),
                V3::new(1., 1., back),
                V3::new(0., 1., back),
            ],
            color: wall,
            line: false,
        },
        Face {
            points: vec![
                V3::new(side_x, 0., 0.),
                V3::new(side_x, 1., 0.),
                V3::new(side_x, 1., 1.),
                V3::new(side_x, 0., 1.),
            ],
            color: side,
            line: false,
        },
    ];
    draw_faces(p, &camera, &mut walls, z);
    if !axis.is_some_and(|a| a.hidden) {
        for value in ticks(scales[0], axis) {
            let t = scales[0].fraction(value).unwrap_or(0.0);
            let (a, b, c) = if primary.horizontal {
                (
                    V3::new(t, 0., 1. - back),
                    V3::new(t, 0., back),
                    V3::new(t, 1., back),
                )
            } else {
                (
                    V3::new(side_x, t, 1. - back),
                    V3::new(side_x, t, back),
                    V3::new(1. - side_x, t, back),
                )
            };
            if axis.is_none_or(|a| a.major_gridlines) {
                p.add(Shape::line(
                    vec![camera.point(a), camera.point(b), camera.point(c)],
                    Stroke::new(0.65 * z, rgba(&chart.grid_color)),
                ));
            }
            let pos = camera.point(a)
                + if primary.horizontal {
                    Vec2::new(0., 5. * z)
                } else {
                    Vec2::new(-5. * z, 0.)
                };
            text(
                p,
                pos,
                if primary.horizontal {
                    Align2::CENTER_TOP
                } else {
                    Align2::RIGHT_CENTER
                },
                &format_value(
                    value,
                    axis.map(|a| a.number_format.as_str()).unwrap_or_default(),
                ),
                9. * z,
                Color32::from_gray(64),
            );
        }
    }
    let mut labels = vec![];
    let mut palette = 0;
    for (gi, g) in chart.groups.iter().enumerate() {
        let scale = scales[gi];
        let nseries = g.series.len().max(1);
        if g.kind == "surface" {
            surface(&mut faces, g, scale, chart);
            palette += nseries;
            continue;
        }
        for (si, s) in g.series.iter().enumerate() {
            let base = series_color(chart, s, palette);
            palette += 1;
            let depth_series = g.grouping == "standard" && !stacked(g);
            let depth_band = 1.0 / if depth_series { nseries as f32 } else { 1.0 };
            let thickness = depth_band / (1.0 + g.gap_depth / 100.0);
            let depth_center = if depth_series {
                (si as f32 + 0.5) * depth_band
            } else {
                0.5
            };
            let z0 = depth_center - thickness / 2.;
            let z1 = depth_center + thickness / 2.;
            let mut previous = None;
            for (position, &index) in categories.iter().enumerate() {
                let Some((start, end)) = stack_segment(g, si, index) else {
                    previous = None;
                    continue;
                };
                let Some(value) = scale.fraction(end) else {
                    previous = None;
                    continue;
                };
                let baseline = scale
                    .fraction(if stacked(g) { start } else { scale.baseline() })
                    .unwrap_or(0.);
                let mut t = (position as f32 + 0.5) / count as f32;
                if category_axis(chart, g).is_some_and(|a| a.reversed) {
                    t = 1. - t;
                }
                let color =
                    s.point_colors
                        .get(&index)
                        .map(|c| rgba(c))
                        .unwrap_or(if g.vary_colors {
                            color_at(chart, index)
                        } else {
                            base
                        });
                let value = value.clamp(0., 1.);
                let baseline = baseline.clamp(0., 1.);
                let point = if g.horizontal {
                    V3::new(value, t, depth_center)
                } else {
                    V3::new(t, value, depth_center)
                };
                match g.kind.as_str() {
                    "bar" => {
                        let cluster = if stacked(g) || depth_series {
                            1
                        } else {
                            nseries
                        };
                        let width = 1.0 / count as f32 / (cluster as f32 + g.gap_width / 100.0);
                        let spacing = width * (1.0 - g.overlap / 100.0);
                        let total = width + spacing * (cluster - 1) as f32;
                        let offset = if cluster == 1 {
                            -width / 2.
                        } else {
                            -total / 2. + si as f32 * spacing
                        };
                        let (low, high) = if g.horizontal {
                            (
                                V3::new(baseline, t + offset, z0),
                                V3::new(value, t + offset + width, z1),
                            )
                        } else {
                            (
                                V3::new(t + offset, baseline, z0),
                                V3::new(t + offset + width, value, z1),
                            )
                        };
                        let shape = s.shape.as_deref().unwrap_or(&g.shape);
                        if matches!(
                            shape,
                            "cylinder" | "cone" | "coneToMax" | "pyramid" | "pyramidToMax"
                        ) {
                            round_bar(&mut faces, low, high, color, shape, g.horizontal)
                        } else {
                            cuboid(&mut faces, low, high, color)
                        }
                    }
                    "area" | "line" => {
                        if let Some((last, last_baseline)) = previous {
                            let a: V3 = last;
                            if g.kind == "area" {
                                let low_a = V3::new(a.x, last_baseline, z0);
                                let low_b = V3::new(point.x, baseline, z0);
                                face(
                                    &mut faces,
                                    vec![
                                        low_a,
                                        low_b,
                                        V3::new(point.x, point.y, z0),
                                        V3::new(a.x, a.y, z0),
                                    ],
                                    color,
                                    false,
                                );
                                face(
                                    &mut faces,
                                    vec![
                                        V3::new(a.x, a.y, z0),
                                        V3::new(point.x, point.y, z0),
                                        V3::new(point.x, point.y, z1),
                                        V3::new(a.x, a.y, z1),
                                    ],
                                    color,
                                    false,
                                );
                                face(
                                    &mut faces,
                                    vec![
                                        V3::new(a.x, last_baseline, z1),
                                        V3::new(point.x, baseline, z1),
                                        V3::new(point.x, point.y, z1),
                                        V3::new(a.x, a.y, z1),
                                    ],
                                    color,
                                    false,
                                );
                            } else {
                                let w = (s.line_width * z / camera.scale).max(0.004);
                                face(
                                    &mut faces,
                                    vec![
                                        V3::new(a.x, a.y - w, z0),
                                        V3::new(point.x, point.y - w, z0),
                                        V3::new(point.x, point.y + w, z0),
                                        V3::new(a.x, a.y + w, z0),
                                    ],
                                    color,
                                    false,
                                );
                                face(
                                    &mut faces,
                                    vec![
                                        V3::new(a.x, a.y + w, z0),
                                        V3::new(point.x, point.y + w, z0),
                                        V3::new(point.x, point.y + w, z1),
                                        V3::new(a.x, a.y + w, z1),
                                    ],
                                    color,
                                    false,
                                );
                            }
                        }
                        previous = Some((point, baseline));
                    }
                    _ => {}
                }
                let label = point_label(
                    s,
                    index,
                    s.values.numbers.get(index).copied().flatten().unwrap_or(0.),
                    0.,
                );
                if !label.is_empty() {
                    labels.push((camera.point(point), label));
                }
            }
        }
    }
    draw_faces(p, &camera, &mut faces, z);
    let xaxis = category_axis(chart, primary);
    if !xaxis.is_some_and(|a| a.hidden) {
        let longest = primary
            .series
            .iter()
            .flat_map(|s| categories.iter().map(move |i| s.categories.label(*i)))
            .map(|s| measured(p, &s, 9. * z).x + 8. * z)
            .fold(30. * z, f32::max);
        let stride = count.div_ceil((rect.width() / longest).max(1.) as usize);
        for (position, &i) in categories.iter().enumerate().step_by(stride) {
            let mut t = (position as f32 + 0.5) / count as f32;
            if xaxis.is_some_and(|a| a.reversed) {
                t = 1. - t;
            }
            let label = primary
                .series
                .iter()
                .map(|s| s.categories.label(i))
                .find(|s| !s.is_empty())
                .unwrap_or_else(|| (i + 1).to_string());
            let point = if primary.horizontal {
                V3::new(0., t, 1. - back)
            } else {
                V3::new(t, 0., 1. - back)
            };
            text(
                p,
                camera.point(point) + Vec2::new(0., 5. * z),
                Align2::CENTER_TOP,
                &label,
                9. * z,
                Color32::from_gray(64),
            );
        }
    }
    for (point, label) in labels {
        text(
            p,
            point - Vec2::new(0., 4. * z),
            Align2::CENTER_BOTTOM,
            &label,
            8. * z,
            Color32::from_gray(45),
        );
    }
}
fn surface(faces: &mut Vec<Face>, g: &ChartGroup, scale: Scale, chart: &Chart) {
    let rows = g.series.len();
    if rows < 2 {
        return;
    }
    let cols = g.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    if cols < 2 {
        return;
    }
    for r in 0..rows - 1 {
        for c in 0..cols - 1 {
            let points = [(r, c), (r, c + 1), (r + 1, c + 1), (r + 1, c)]
                .iter()
                .map(|(row, col)| {
                    let value = g.series[*row].values.numbers.get(*col).copied().flatten()?;
                    let y = scale.fraction(value)?.clamp(0., 1.);
                    Some(V3::new(
                        *col as f32 / (cols - 1) as f32,
                        y,
                        *row as f32 / (rows - 1) as f32,
                    ))
                })
                .collect::<Option<Vec<_>>>();
            if let Some(points) = points {
                let level = points.iter().map(|p| p.y).sum::<f32>() / 4.;
                let color = color_at(
                    chart,
                    (level * (chart.palette.len().max(1) - 1) as f32).round() as usize,
                );
                face(
                    faces,
                    vec![points[0], points[1], points[2]],
                    color,
                    g.wireframe,
                );
                face(
                    faces,
                    vec![points[0], points[2], points[3]],
                    color,
                    g.wireframe,
                );
            }
        }
    }
}
fn paint_pie(p: &egui::Painter, camera: &Camera, chart: &Chart, z: f32, faces: &mut Vec<Face>) {
    let g = &chart.groups[0];
    let Some(s) = g.series.first() else {
        return;
    };
    let total = s
        .values
        .numbers
        .iter()
        .enumerate()
        .filter(|(i, _)| !hidden(s, *i))
        .filter_map(|(_, v)| *v)
        .map(f64::abs)
        .sum::<f64>();
    if total <= 0. {
        return;
    }
    let mut angle = (g.first_slice_angle - 90.).to_radians();
    let mut labels = vec![];
    for (i, value) in s.values.numbers.iter().enumerate() {
        if hidden(s, i) {
            continue;
        }
        let Some(value) = value else {
            continue;
        };
        let sweep = (value.abs() / total) as f32 * TAU;
        let color = s
            .point_colors
            .get(&i)
            .map(|s| rgba(s))
            .unwrap_or_else(|| color_at(chart, i));
        let middle = angle + sweep / 2.;
        let explosion = s.explosion / 100. * 0.25;
        let center = V3::new(
            0.5 + middle.cos() * explosion,
            1.,
            0.5 + middle.sin() * explosion,
        );
        let n = (sweep * 90.).ceil().clamp(2., 360.) as usize;
        for j in 0..n {
            let a = angle + sweep * j as f32 / n as f32;
            let b = angle + sweep * (j + 1) as f32 / n as f32;
            let pa = V3::new(center.x + a.cos() * 0.48, 1., center.z + a.sin() * 0.48);
            let pb = V3::new(center.x + b.cos() * 0.48, 1., center.z + b.sin() * 0.48);
            face(faces, vec![center, pa, pb], color, false);
            face(
                faces,
                vec![V3::new(pa.x, 0., pa.z), V3::new(pb.x, 0., pb.z), pb, pa],
                color,
                false,
            );
            if j == 0 {
                face(
                    faces,
                    vec![
                        V3::new(center.x, 0., center.z),
                        V3::new(pa.x, 0., pa.z),
                        pa,
                        center,
                    ],
                    color,
                    false,
                );
            }
            if j == n - 1 {
                face(
                    faces,
                    vec![
                        V3::new(center.x, 0., center.z),
                        V3::new(pb.x, 0., pb.z),
                        pb,
                        center,
                    ],
                    color,
                    false,
                );
            }
        }
        let label = point_label(s, i, *value, total);
        if !label.is_empty() {
            labels.push((
                camera.point(V3::new(
                    center.x + middle.cos() * 0.31,
                    1.,
                    center.z + middle.sin() * 0.31,
                )),
                label,
            ));
        }
        angle += sweep;
    }
    draw_faces(p, camera, faces, z);
    for (pos, label) in labels {
        text(
            p,
            pos,
            Align2::CENTER_CENTER,
            &label,
            9. * z,
            Color32::from_gray(45),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_honors_rotation_perspective_and_depth_without_nonfinite_geometry() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 300.));
        for rot_x in [-90., 0., 30., 90.] {
            for rot_y in [0., 90., 180., 270.] {
                for perspective in [0., 30., 240.] {
                    let view = ChartView3d {
                        rotation_x: rot_x,
                        rotation_y: rot_y,
                        perspective,
                        right_angle_axes: false,
                        depth_percent: 2000.,
                        ..Default::default()
                    };
                    let camera = Camera::new(rect, &view, false);
                    let point = camera.point(V3::new(0.2, 0.7, 0.9));
                    assert!(point.x.is_finite() && point.y.is_finite());
                }
            }
        }
        let a = Camera::new(rect, &ChartView3d::default(), false).point(V3::new(1., 1., 1.));
        let b = Camera::new(
            rect,
            &ChartView3d {
                rotation_y: 70.,
                ..Default::default()
            },
            false,
        )
        .point(V3::new(1., 1., 1.));
        assert!(a.distance(b) > 10.);
    }
}
