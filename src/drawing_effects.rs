//! DrawingML effects, kept separate from source cells and rasterized only at display time.
//! Distances are points; OOXML angles are clockwise in 1/60000 degree units.
use super::*;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrawingEffects {
    pub outer_shadow: Option<Shadow>,
    pub inner_shadow: Option<Shadow>,
    pub glow: Option<Glow>,
    pub soft_edge: f32,
    pub reflection: Option<Reflection>,
    pub scene: Option<Scene3D>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shadow {
    pub color: String,
    pub blur: f32,
    pub distance: f32,
    pub direction: f32,
    pub scale: [f32; 2],
    pub skew: [f32; 2],
    pub alignment: String,
    pub rotate_with_shape: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Glow {
    pub color: String,
    pub radius: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reflection {
    pub blur: f32,
    pub distance: f32,
    pub direction: f32,
    pub scale: [f32; 2],
    pub skew: [f32; 2],
    pub alignment: String,
    pub start_alpha: f32,
    pub end_alpha: f32,
    pub start_position: f32,
    pub end_position: f32,
    pub fade_direction: f32,
    pub rotate_with_shape: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene3D {
    pub camera: String,
    pub rotation: [f32; 3],
    pub zoom: f32,
    pub field_of_view: f32,
    pub extrusion: f32,
    pub extrusion_color: Option<String>,
    pub contour: f32,
    pub contour_color: Option<String>,
    pub bevel_width: f32,
    pub bevel_height: f32,
    pub bevel: String,
    pub material: String,
    pub light_direction: String,
}

fn distance(n: Option<&Node>, key: &str, default: f64) -> f32 {
    (num(n, key, default) / 12700.0).clamp(0.0, 512.0) as f32
}
fn angle(n: Option<&Node>, key: &str, default: f64) -> f32 {
    ((num(n, key, default) / 60000.0) % 360.0) as f32
}
fn fraction(n: Option<&Node>, key: &str, default: f64) -> f32 {
    (num(n, key, default) / 100000.0).clamp(-4.0, 4.0) as f32
}
fn shadow(n: &Node, theme: &Theme) -> Shadow {
    Shadow {
        color: color(n, theme).unwrap_or_else(|| "FF000000".into()),
        blur: distance(Some(n), "blurRad", 0.0),
        distance: distance(Some(n), "dist", 0.0),
        direction: angle(Some(n), "dir", 0.0),
        scale: [
            fraction(Some(n), "sx", 100000.0),
            fraction(Some(n), "sy", 100000.0),
        ],
        skew: [angle(Some(n), "kx", 0.0), angle(Some(n), "ky", 0.0)],
        alignment: n.attr("algn").unwrap_or("b").into(),
        rotate_with_shape: n.attr("rotWithShape") != Some("0"),
    }
}

/// Theme effect matrices are 1-based; explicit empty effectLst clears inheritance.
pub(super) fn parse(
    properties: &Node,
    inherited: Option<&Node>,
    theme: &Theme,
    warnings: &mut Vec<String>,
) -> DrawingEffects {
    let mut out = DrawingEffects::default();
    let effects = properties
        .child("effectLst")
        .or_else(|| inherited.and_then(|n| n.child("effectLst")));
    if let Some(list) = effects {
        for n in &list.children {
            match n.name.as_str() {
                "outerShdw" => out.outer_shadow = Some(shadow(n, theme)),
                "innerShdw" => out.inner_shadow = Some(shadow(n, theme)),
                "glow" => {
                    out.glow = Some(Glow {
                        color: color(n, theme).unwrap_or_else(|| "FF000000".into()),
                        radius: distance(Some(n), "rad", 0.0),
                    })
                }
                "softEdge" => out.soft_edge = distance(Some(n), "rad", 0.0),
                "reflection" => {
                    out.reflection = Some(Reflection {
                        blur: distance(Some(n), "blurRad", 0.0),
                        distance: distance(Some(n), "dist", 0.0),
                        direction: angle(Some(n), "dir", 0.0),
                        scale: [
                            fraction(Some(n), "sx", 100000.0),
                            fraction(Some(n), "sy", 100000.0),
                        ],
                        skew: [angle(Some(n), "kx", 0.0), angle(Some(n), "ky", 0.0)],
                        alignment: n.attr("algn").unwrap_or("b").into(),
                        start_alpha: fraction(Some(n), "stA", 100000.0).clamp(0.0, 1.0),
                        end_alpha: fraction(Some(n), "endA", 0.0).clamp(0.0, 1.0),
                        start_position: fraction(Some(n), "stPos", 0.0).clamp(0.0, 1.0),
                        end_position: fraction(Some(n), "endPos", 100000.0).clamp(0.0, 1.0),
                        fade_direction: angle(Some(n), "fadeDir", 5400000.0),
                        rotate_with_shape: n.attr("rotWithShape") != Some("0"),
                    })
                }
                other => warnings.push(format!("미지원 도형 효과: {other}")),
            }
        }
    }
    if properties.child("effectDag").is_some() {
        warnings.push("사용자 지정 도형 효과 그래프".into());
    }
    let scene = properties
        .child("scene3d")
        .or_else(|| inherited.and_then(|n| n.child("scene3d")));
    let three = properties
        .child("sp3d")
        .or_else(|| inherited.and_then(|n| n.child("sp3d")));
    if scene.is_some() || three.is_some() {
        let camera = scene.and_then(|n| n.child("camera"));
        let preset = camera
            .and_then(|n| n.attr("prst"))
            .unwrap_or("orthographicFront");
        let (mut lat, mut lon, mut rev): (f32, f32, f32) = match preset {
            "isometricRightUp" | "isometricTopUp" => (35.264, -45.0, 0.0),
            "isometricLeftUp" => (35.264, 45.0, 0.0),
            "isometricRightDown" => (-35.264, -45.0, 0.0),
            "isometricLeftDown" => (-35.264, 45.0, 0.0),
            "isometricBottomDown" => (-35.264, 45.0, 0.0),
            "perspectiveFront" | "orthographicFront" => (0.0, 0.0, 0.0),
            "perspectiveLeft" | "obliqueLeft" => (0.0, 35.0, 0.0),
            "perspectiveRight" | "obliqueRight" => (0.0, -35.0, 0.0),
            "perspectiveAbove" | "obliqueTop" => (35.0, 0.0, 0.0),
            "perspectiveBelow" | "obliqueBottom" => (-35.0, 0.0, 0.0),
            _ => {
                warnings.push(format!("근사 표시되는 3D 카메라 프리셋: {preset}"));
                (0.0, 0.0, 0.0)
            }
        };
        if let Some(rot) = camera.and_then(|n| n.child("rot")) {
            lat = angle(Some(rot), "lat", lat as f64 * 60000.0);
            lon = angle(Some(rot), "lon", lon as f64 * 60000.0);
            rev = angle(Some(rot), "rev", rev as f64 * 60000.0);
        }
        let bevel = three.and_then(|n| n.child("bevelT"));
        out.scene = Some(Scene3D {
            camera: preset.into(),
            rotation: [lat, lon, rev],
            zoom: fraction(camera, "zoom", 100000.0).clamp(0.01, 4.0),
            field_of_view: angle(camera, "fov", 2700000.0).clamp(1.0, 120.0),
            extrusion: distance(three, "extrusionH", 0.0),
            extrusion_color: three
                .and_then(|n| n.child("extrusionClr"))
                .and_then(|n| color(n, theme)),
            contour: distance(three, "contourW", 0.0),
            contour_color: three
                .and_then(|n| n.child("contourClr"))
                .and_then(|n| color(n, theme)),
            bevel_width: if bevel.is_some() {
                distance(bevel, "w", 76200.0)
            } else {
                0.0
            },
            bevel_height: if bevel.is_some() {
                distance(bevel, "h", 76200.0)
            } else {
                0.0
            },
            bevel: bevel
                .and_then(|n| n.attr("prst"))
                .unwrap_or("circle")
                .into(),
            material: three
                .and_then(|n| n.attr("prstMaterial"))
                .unwrap_or("warmMatte")
                .into(),
            light_direction: scene
                .and_then(|n| n.child("lightRig"))
                .and_then(|n| n.attr("dir"))
                .unwrap_or("tl")
                .into(),
        });
        warnings.push("3D 도형의 재질·조명·베벨은 네이티브 근사 표시입니다".into());
        if three.is_some_and(|n| n.child("bevelB").is_some()) {
            warnings.push("3D 도형의 아래쪽 베벨".into());
        }
    }
    out
}

fn finite_range(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}
fn finite_angle(value: f32) -> f32 {
    if value.is_finite() {
        value.rem_euclid(360.0)
    } else {
        0.0
    }
}
impl DrawingEffects {
    pub(super) fn has_raster_effects(&self) -> bool {
        self.outer_shadow.is_some()
            || self.inner_shadow.is_some()
            || self.glow.is_some()
            || self.soft_edge > 0.0
            || self.reflection.is_some()
    }
    // Restored JSON does not pass through the OOXML parser's numeric bounds.
    // Keep extent arithmetic and blur allocations safe for those inputs as well.
    fn bounded(&self) -> Self {
        let mut safe = self.clone();
        let shadow = |s: &mut Shadow| {
            s.blur = finite_range(s.blur, 0.0, 512.0, 0.0);
            s.distance = finite_range(s.distance, 0.0, 512.0, 0.0);
            s.direction = finite_angle(s.direction);
            s.scale = s.scale.map(|v| finite_range(v, -4.0, 4.0, 1.0));
            s.skew = s.skew.map(|v| finite_range(v, -80.0, 80.0, 0.0));
        };
        if let Some(s) = &mut safe.outer_shadow {
            shadow(s);
        }
        if let Some(s) = &mut safe.inner_shadow {
            shadow(s);
        }
        if let Some(g) = &mut safe.glow {
            g.radius = finite_range(g.radius, 0.0, 512.0, 0.0);
        }
        safe.soft_edge = finite_range(safe.soft_edge, 0.0, 512.0, 0.0);
        if let Some(r) = &mut safe.reflection {
            r.blur = finite_range(r.blur, 0.0, 512.0, 0.0);
            r.distance = finite_range(r.distance, 0.0, 512.0, 0.0);
            r.direction = finite_angle(r.direction);
            r.fade_direction = finite_angle(r.fade_direction);
            r.scale = r.scale.map(|v| finite_range(v, -4.0, 4.0, 1.0));
            r.skew = r.skew.map(|v| finite_range(v, -80.0, 80.0, 0.0));
            r.start_alpha = finite_range(r.start_alpha, 0.0, 1.0, 1.0);
            r.end_alpha = finite_range(r.end_alpha, 0.0, 1.0, 0.0);
            r.start_position = finite_range(r.start_position, 0.0, 1.0, 0.0);
            r.end_position = finite_range(r.end_position, 0.0, 1.0, 1.0);
        }
        safe
    }
    pub(super) fn outset(&self, size: Vec2, z: f32) -> f32 {
        if !z.is_finite() || z <= 0.0 {
            return 0.0;
        }
        let z = z.min(256.0);
        let size = Vec2::new(
            finite_range(size.x.abs(), 0.0, 1e8, 0.0),
            finite_range(size.y.abs(), 0.0, 1e8, 0.0),
        );
        let safe = self.bounded();
        let reflection = safe.reflection.as_ref().map_or(0.0, |r| {
            size.y * r.scale[1].abs()
                + size.x * (r.scale[0].abs() - 1.0).max(0.0)
                + (r.distance + r.blur * 3.0) * z
                + r.skew[0].to_radians().tan().abs() * size.y
                + r.skew[1].to_radians().tan().abs() * size.x
        });
        let extrusion = safe.scene.as_ref().map_or(0.0, |s| {
            let projection_gain = finite_range(s.zoom, 0.01, 4.0, 1.0)
                * if s.camera.starts_with("perspective") {
                    5.0
                } else {
                    1.0
                };
            (finite_range(s.extrusion, 0.0, 512.0, 0.0) + finite_range(s.contour, 0.0, 512.0, 0.0))
                * z
                * projection_gain
                + size.max_elem() * (projection_gain - 1.0).max(0.0)
        });
        (safe.padding(size, z) + reflection + extrusion).min(1e8)
    }
    fn padding(&self, size: Vec2, z: f32) -> f32 {
        if !self.has_raster_effects() {
            return 0.0;
        }
        let shadow = self.outer_shadow.as_ref().map_or(0.0, |s| {
            (s.blur * 3.0 + s.distance) * z
                + (s.scale[0].abs() - 1.0).max(0.0) * size.x
                + (s.scale[1].abs() - 1.0).max(0.0) * size.y
                + s.skew[0].to_radians().tan().abs() * size.y
                + s.skew[1].to_radians().tan().abs() * size.x
        });
        (shadow
            .max(self.glow.as_ref().map_or(0.0, |g| g.radius * 2.0 * z))
            .max(self.soft_edge * z)
            + 2.0)
            .min(2048.0)
    }
}

#[derive(Default)]
pub(super) struct EffectRenderer {
    cache: HashMap<u64, CachedEffect>,
    pixels: usize,
}
struct CachedEffect {
    texture: egui::TextureHandle,
    /// Local unrotated pixels relative to the object's top-left.
    bounds: Rect,
}
impl EffectRenderer {
    pub(super) fn clear(&mut self) {
        self.cache.clear();
        self.pixels = 0;
    }
    pub(super) fn shape(
        &mut self,
        p: &egui::Painter,
        rect: Rect,
        shape: &ShapeData,
        paths: &[ShapePath],
        pose: Pose,
        z: f32,
    ) -> bool {
        if !shape.effects.has_raster_effects() {
            return false;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        format!("{shape:?}").hash(&mut hash);
        let key = effect_key(
            hash.finish(),
            rect.size(),
            pose,
            z,
            p.ctx().pixels_per_point(),
        );
        if !self.cache.contains_key(&key) {
            let source = shape_source(rect.size(), shape, paths, z, pose);
            self.insert(p, key, source, rect.size(), &shape.effects, (pose, z));
        }
        self.paint(p, key, rect, pose)
    }
    pub(super) fn picture(
        &mut self,
        p: &egui::Painter,
        rect: Rect,
        picture: &PictureData,
        pose: Pose,
        z: f32,
    ) -> bool {
        if !picture.effects.has_raster_effects() {
            return false;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        picture.source.hash(&mut hash);
        format!("{:?}{:?}", picture.effects, picture.crop).hash(&mut hash);
        let key = effect_key(
            hash.finish(),
            rect.size(),
            pose,
            z,
            p.ctx().pixels_per_point(),
        );
        if !self.cache.contains_key(&key) {
            let Some(source) = picture_source(rect.size(), picture) else {
                return false;
            };
            self.insert(p, key, source, rect.size(), &picture.effects, (pose, z));
        }
        self.paint(p, key, rect, pose)
    }
    fn insert(
        &mut self,
        p: &egui::Painter,
        key: u64,
        source: image::RgbaImage,
        size: Vec2,
        effects: &DrawingEffects,
        context: (Pose, f32),
    ) {
        let (image, bounds) = composite(source, size, effects, context.0, context.1);
        if self.pixels + image.pixels.len() > 8_000_000 || self.cache.len() >= 64 {
            self.clear();
        }
        self.pixels += image.pixels.len();
        let texture = p.ctx().load_texture(
            format!("drawing-effect-{key}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.cache.insert(key, CachedEffect { texture, bounds });
    }
    fn paint(&self, p: &egui::Painter, key: u64, rect: Rect, pose: Pose) -> bool {
        let Some(cached) = self.cache.get(&key) else {
            return false;
        };
        let mut mesh = egui::Mesh::with_texture(cached.texture.id());
        for (pos, uv) in [
            (cached.bounds.left_top(), Pos2::new(0.0, 0.0)),
            (cached.bounds.right_top(), Pos2::new(1.0, 0.0)),
            (cached.bounds.right_bottom(), Pos2::new(1.0, 1.0)),
            (cached.bounds.left_bottom(), Pos2::new(0.0, 1.0)),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: transform(
                    rect.min + pos.to_vec2(),
                    rect.center(),
                    pose.rotation,
                    pose.flip_h,
                    pose.flip_v,
                ),
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        p.add(egui::Shape::mesh(mesh));
        true
    }
}
fn effect_key(seed: u64, size: Vec2, pose: Pose, z: f32, ppp: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    for n in [size.x, size.y, pose.rotation, z, ppp] {
        n.to_bits().hash(&mut h);
    }
    pose.flip_h.hash(&mut h);
    pose.flip_v.hash(&mut h);
    h.finish()
}
fn raster_size(size: Vec2) -> [u32; 2] {
    let factor = 2.0_f32
        .min(1536.0 / size.x.max(size.y).max(1.0))
        .min((1_000_000.0 / (size.x * size.y).max(1.0)).sqrt());
    [
        (size.x * factor).ceil().max(1.0) as u32,
        (size.y * factor).ceil().max(1.0) as u32,
    ]
}
fn inside(point: Pos2, points: &[[f32; 2]]) -> bool {
    let mut result = false;
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        if (a[1] > point.y) != (b[1] > point.y)
            && point.x < (b[0] - a[0]) * (point.y - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}
fn shape_source(
    size: Vec2,
    shape: &ShapeData,
    paths: &[ShapePath],
    z: f32,
    pose: Pose,
) -> image::RgbaImage {
    let [mut w, mut h] = raster_size(size);
    // Bound work for hostile custom paths without allocating a huge offscreen canvas.
    let edges = paths.iter().map(|p| p.points.len()).sum::<usize>().max(1);
    let budget = 64_000_000.0 / edges as f32;
    let reduction = (budget / (w as f32 * h as f32)).sqrt().min(1.0);
    w = ((w as f32 * reduction).floor() as u32).max(1);
    h = ((h as f32 * reduction).floor() as u32).max(1);
    let fill = rgba(shape.fill.as_deref(), Color32::TRANSPARENT);
    let line = rgba(shape.line.as_deref(), Color32::TRANSPARENT);
    let angle = shape
        .gradient
        .as_ref()
        .map_or(0.0, |g| {
            g.angle
                - if g.rotate_with_shape {
                    0.0
                } else {
                    pose.rotation
                }
        })
        .to_radians();
    let direction = Vec2::new(angle.cos(), angle.sin());
    let mut image = image::RgbaImage::new(w, h);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let q = Pos2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
        let mut c = Color32::TRANSPARENT;
        for path in paths {
            if path.closed && path.fill && inside(q, &path.points) {
                c = if let Some(g) = &shape.gradient {
                    let d = if g.scaled {
                        direction * size
                    } else {
                        direction
                    };
                    let d = Vec2::new(d.x * size.x, d.y * size.y);
                    let min = d.x.min(0.0) + d.y.min(0.0);
                    gradient_color(
                        g,
                        ((q.x * d.x + q.y * d.y - min) / (d.x.abs() + d.y.abs()).max(0.001))
                            .clamp(0.0, 1.0),
                    )
                } else {
                    fill
                };
            }
            if path.stroke && line.a() > 0 && shape.line_width > 0.0 {
                let point = Pos2::new(q.x * size.x, q.y * size.y);
                for pair in path
                    .points
                    .iter()
                    .zip(path.points.iter().cycle().skip(1))
                    .take(path.points.len() - usize::from(!path.closed))
                {
                    let (a, b) = (
                        Pos2::new(pair.0[0] * size.x, pair.0[1] * size.y),
                        Pos2::new(pair.1[0] * size.x, pair.1[1] * size.y),
                    );
                    let v = b - a;
                    let t = ((point - a).dot(v) / v.length_sq().max(0.001)).clamp(0.0, 1.0);
                    let width = (shape.line_width * z).max(1.0);
                    let length = if shape.dash.to_ascii_lowercase().contains("dot") {
                        width
                    } else {
                        width * 4.0
                    };
                    let visible = shape.dash == "solid"
                        || (t * v.length()) % (length + width * 2.5) <= length;
                    if visible && (point - (a + v * t)).length() <= shape.line_width * z * 0.5 {
                        c = line;
                    }
                }
            }
        }
        *pixel = image::Rgba(c.to_srgba_unmultiplied());
    }
    image
}
fn picture_source(size: Vec2, picture: &PictureData) -> Option<image::RgbaImage> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&picture.base64)
        .ok()?;
    if bytes.len() > MAX_MEDIA as usize {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    let original = reader.decode().ok()?.to_rgba8();
    let [w, h] = raster_size(size);
    Some(image::RgbaImage::from_fn(w, h, |x, y| {
        let u = picture.crop[0]
            + (1.0 - picture.crop[0] - picture.crop[2]) * (x as f32 + 0.5) / w as f32;
        let v = picture.crop[1]
            + (1.0 - picture.crop[1] - picture.crop[3]) * (y as f32 + 0.5) / h as f32;
        *original.get_pixel(
            (u * original.width() as f32).clamp(0.0, original.width() as f32 - 1.0) as u32,
            (v * original.height() as f32).clamp(0.0, original.height() as f32 - 1.0) as u32,
        )
    }))
}

fn alignment(name: &str, size: Vec2) -> Vec2 {
    Vec2::new(
        if name.contains('l') {
            0.0
        } else if name.contains('r') {
            size.x
        } else {
            size.x * 0.5
        },
        if name.contains('t') {
            0.0
        } else if name.contains('b') {
            size.y
        } else {
            size.y * 0.5
        },
    )
}
fn inverse_affine(q: Vec2, origin: Vec2, scale: [f32; 2], skew: [f32; 2], offset: Vec2) -> Vec2 {
    let xy = q - origin - offset;
    let kx = skew[0].clamp(-80.0, 80.0).to_radians().tan();
    let ky = skew[1].clamp(-80.0, 80.0).to_radians().tan();
    let determinant = scale[0] * scale[1] - kx * ky;
    if determinant.abs() < 0.0001 {
        return Vec2::splat(-1e8);
    }
    origin
        + Vec2::new(
            (xy.x * scale[1] - xy.y * kx) / determinant,
            (xy.y * scale[0] - xy.x * ky) / determinant,
        )
}
fn offset(distance: f32, direction: f32, rotates: bool, pose: Pose, z: f32) -> Vec2 {
    let a = (direction - if rotates { 0.0 } else { pose.rotation }).to_radians();
    let mut v = Vec2::new(a.cos(), a.sin()) * (distance * z);
    if !rotates {
        if pose.flip_h {
            v.x = -v.x;
        }
        if pose.flip_v {
            v.y = -v.y;
        }
    }
    v
}
fn over(dst: &mut [f32; 4], rgb: [u8; 4], alpha: f32) {
    let alpha = (alpha * rgb[3] as f32 / 255.0).clamp(0.0, 1.0);
    for i in 0..3 {
        dst[i] = rgb[i] as f32 / 255.0 * alpha + dst[i] * (1.0 - alpha);
    }
    dst[3] = alpha + dst[3] * (1.0 - alpha);
}
fn composite(
    source: image::RgbaImage,
    size: Vec2,
    effects: &DrawingEffects,
    pose: Pose,
    z: f32,
) -> (egui::ColorImage, Rect) {
    let effects = effects.bounded();
    let size = Vec2::new(
        finite_range(size.x, 0.001, 1e8, 1.0),
        finite_range(size.y, 0.001, 1e8, 1.0),
    );
    let z = finite_range(z, 0.001, 256.0, 1.0);
    let pad = effects.padding(size, z);
    let extra = effects.reflection.as_ref().map_or(0.0, |r| {
        size.y * r.scale[1].abs() + (r.distance + r.blur * 3.0) * z
    });
    let bounds = Rect::from_min_max(
        Pos2::new(-pad, -pad),
        Pos2::new(size.x + pad, size.y + pad + extra),
    );
    let [w, h] = raster_size(bounds.size());
    let factor = Vec2::new(w as f32 / bounds.width(), h as f32 / bounds.height());
    let local = |x: u32, y: u32| {
        bounds.min.to_vec2() + Vec2::new((x as f32 + 0.5) / factor.x, (y as f32 + 0.5) / factor.y)
    };
    let sample = |q: Vec2| -> [u8; 4] {
        if q.x < 0.0 || q.y < 0.0 || q.x >= size.x || q.y >= size.y {
            return [0; 4];
        }
        source
            .get_pixel(
                ((q.x / size.x) * source.width() as f32).min(source.width() as f32 - 1.0) as u32,
                ((q.y / size.y) * source.height() as f32).min(source.height() as f32 - 1.0) as u32,
            )
            .0
    };
    let mask = image::GrayImage::from_fn(w, h, |x, y| image::Luma([sample(local(x, y))[3]]));
    let mut output = vec![[0.0; 4]; (w * h) as usize];
    if let Some(g) = &effects.glow {
        let blurred = image::imageops::fast_blur(&mask, (g.radius * z * factor.x * 0.5).max(0.01));
        let color = rgba(Some(&g.color), Color32::TRANSPARENT).to_srgba_unmultiplied();
        for (i, value) in blurred.pixels().enumerate() {
            over(
                &mut output[i],
                color,
                (value[0] as f32 / 255.0 * 2.0).min(1.0),
            );
        }
    }
    if let Some(s) = &effects.outer_shadow {
        let delta = offset(s.distance, s.direction, s.rotate_with_shape, pose, z);
        let anchor = alignment(&s.alignment, size);
        let shifted = image::GrayImage::from_fn(w, h, |x, y| {
            image::Luma([sample(inverse_affine(local(x, y), anchor, s.scale, s.skew, delta))[3]])
        });
        let blurred = image::imageops::fast_blur(&shifted, (s.blur * z * factor.x * 0.5).max(0.01));
        let color = rgba(Some(&s.color), Color32::TRANSPARENT).to_srgba_unmultiplied();
        for (i, value) in blurred.pixels().enumerate() {
            over(&mut output[i], color, value[0] as f32 / 255.0);
        }
    }
    if let Some(r) = &effects.reflection {
        let delta = offset(r.distance, r.direction, r.rotate_with_shape, pose, z);
        let anchor = alignment(&r.alignment, size);
        let reflected = image::RgbaImage::from_fn(w, h, |x, y| {
            let q = local(x, y);
            let src = inverse_affine(q, anchor, r.scale, r.skew, delta);
            let mut color = sample(src);
            let a = r.fade_direction.to_radians();
            let direction = Vec2::new(a.cos(), a.sin());
            let extent = size.x * direction.x.abs() + size.y * direction.y.abs();
            let t = ((q - anchor - delta).dot(direction) / extent.max(1.0)).clamp(0.0, 1.0);
            let mix = ((t - r.start_position) / (r.end_position - r.start_position).max(0.001))
                .clamp(0.0, 1.0);
            color[3] = (color[3] as f32 * (r.start_alpha + (r.end_alpha - r.start_alpha) * mix))
                .round() as u8;
            // Premultiply before blur to avoid color halos around transparent pixels.
            for i in 0..3 {
                color[i] = (color[i] as u16 * color[3] as u16 / 255) as u8;
            }
            image::Rgba(color)
        });
        let reflected =
            image::imageops::fast_blur(&reflected, (r.blur * z * factor.x * 0.5).max(0.01));
        for (i, c) in reflected.pixels().enumerate() {
            let a = c[3] as f32 / 255.0;
            for j in 0..4 {
                output[i][j] = c[j] as f32 / 255.0 + output[i][j] * (1.0 - a);
            }
        }
    }
    let soft = (effects.soft_edge > 0.0).then(|| {
        image::imageops::fast_blur(&mask, (effects.soft_edge * z * factor.x * 0.5).max(0.01))
    });
    for (i, pixel) in output.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let coverage = soft.as_ref().map_or(1.0, |m| {
            ((m.get_pixel(x, y)[0] as f32 / 255.0 - 0.5).max(0.0) * 2.0).clamp(0.0, 1.0)
        });
        over(pixel, sample(local(x, y)), coverage);
    }
    if let Some(s) = &effects.inner_shadow {
        let delta = offset(s.distance, s.direction, true, pose, z);
        let shadow = image::GrayImage::from_fn(w, h, |x, y| {
            image::Luma([255 - sample(local(x, y) - delta)[3]])
        });
        let blurred = image::imageops::fast_blur(&shadow, (s.blur * z * factor.x * 0.5).max(0.01));
        let color = rgba(Some(&s.color), Color32::TRANSPARENT).to_srgba_unmultiplied();
        for (i, value) in blurred.pixels().enumerate() {
            over(
                &mut output[i],
                color,
                value[0] as f32 / 255.0 * mask.as_raw()[i] as f32 / 255.0,
            );
        }
    }
    let pixels = output
        .into_iter()
        .map(|p| {
            Color32::from_rgba_premultiplied(
                (p[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (p[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (p[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                (p[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            )
        })
        .collect();
    (
        egui::ColorImage {
            size: [w as usize, h as usize],
            pixels,
        },
        bounds,
    )
}

pub(super) fn project(q: Pos2, rect: Rect, depth: f32, scene: &Scene3D) -> Pos2 {
    let [lat, lon, rev] = scene.rotation.map(finite_angle).map(f32::to_radians);
    let depth = finite_range(depth, -131072.0, 131072.0, 0.0);
    let zoom = finite_range(scene.zoom, 0.01, 4.0, 1.0);
    let v = q - rect.center();
    let x = v.x * lon.cos() + depth * lon.sin();
    let z = -v.x * lon.sin() + depth * lon.cos();
    let y = v.y * lat.cos() - z * lat.sin();
    let z = v.y * lat.sin() + z * lat.cos();
    let perspective = if scene.camera.starts_with("perspective") {
        let focal = rect.size().max_elem()
            / (finite_range(scene.field_of_view, 1.0, 120.0, 45.0).to_radians() * 0.5)
                .tan()
                .max(0.1);
        (focal / (focal + z).max(focal * 0.2)).clamp(0.2, 5.0)
    } else {
        1.0
    };
    rect.center()
        + Vec2::new(x * rev.cos() - y * rev.sin(), x * rev.sin() + y * rev.cos())
            * (perspective * zoom)
}
fn shade(color: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let f = |v: u8| (v as f32 * factor).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(r), f(g), f(b), a)
}
pub(super) fn paint_sides(
    p: &egui::Painter,
    points: &[Pos2],
    rect: Rect,
    scene: &Scene3D,
    fill: Color32,
    pose: Pose,
    z: f32,
) {
    if points.len() < 3 || scene.extrusion <= 0.0 {
        return;
    }
    let transform3 = |q, d| {
        transform(
            project(q, rect, d, scene),
            rect.center(),
            pose.rotation,
            pose.flip_h,
            pose.flip_v,
        )
    };
    let front: Vec<_> = points.iter().map(|&q| transform3(q, 0.0)).collect();
    let rear: Vec<_> = points
        .iter()
        .map(|&q| transform3(q, finite_range(scene.extrusion, 0.0, 512.0, 0.0) * z))
        .collect();
    let base = rgba(scene.extrusion_color.as_deref(), fill);
    let orientation: f32 = front
        .iter()
        .zip(front.iter().cycle().skip(1))
        .take(front.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum();
    let mut faces = vec![];
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let a = front[j] - front[i];
        let b = rear[i] - front[i];
        if (a.x * b.y - a.y * b.x) * orientation < 0.0 {
            faces.push((i, (rear[i].y + rear[j].y) * 0.5));
        }
    }
    faces.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (i, _) in faces {
        let j = (i + 1) % points.len();
        let edge = (front[j] - front[i]).normalized();
        let light = if scene.light_direction.contains('l') {
            -0.5
        } else {
            0.5
        };
        let brightness = (0.65 + edge.x * 0.2 + edge.y * light * 0.25).clamp(0.35, 0.95);
        filled_path(
            p,
            &[front[i], rear[i], rear[j], front[j]],
            shade(base, brightness),
        );
    }
}
pub(super) fn paint_bevel(
    p: &egui::Painter,
    points: &[Pos2],
    rect: Rect,
    scene: &Scene3D,
    fill: Color32,
    pose: Pose,
    z: f32,
) {
    if points.len() < 3 {
        return;
    }
    let transform3 = |q| {
        transform(
            project(q, rect, 0.0, scene),
            rect.center(),
            pose.rotation,
            pose.flip_h,
            pose.flip_v,
        )
    };
    let width = finite_range(scene.bevel_width, 0.0, 512.0, 0.0) * z;
    if width > 0.0 && scene.bevel_height > 0.0 {
        let center =
            points.iter().fold(Vec2::ZERO, |sum, p| sum + p.to_vec2()) / points.len() as f32;
        let inner: Vec<_> = points
            .iter()
            .map(|&q| {
                let delta = center - q.to_vec2();
                q + delta.normalized() * width.min(delta.length() * 0.4)
            })
            .collect();
        for i in 0..points.len() {
            let j = (i + 1) % points.len();
            let edge = (points[j] - points[i]).normalized();
            let factor = (1.0 - edge.x * 0.24 + edge.y * 0.22).clamp(0.5, 1.5);
            filled_path(
                p,
                &[
                    transform3(points[i]),
                    transform3(points[j]),
                    transform3(inner[j]),
                    transform3(inner[i]),
                ],
                shade(fill, factor),
            );
        }
    }
    if scene.contour > 0.0 {
        let projected: Vec<_> = points.iter().map(|&q| transform3(q)).collect();
        stroke_path(
            p,
            &projected,
            true,
            Stroke::new(
                finite_range(scene.contour, 0.0, 512.0, 0.0) * z,
                rgba(scene.contour_color.as_deref(), Color32::BLACK),
            ),
            "solid",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_keep_ooxml_units_colors_and_override_empty_theme_list() {
        let xml = r#"<spPr><effectLst><outerShdw blurRad="50800" dist="38100" dir="2700000" rotWithShape="0"><srgbClr val="112233"><alpha val="40000"/></srgbClr></outerShdw><glow rad="76200"><schemeClr val="accent2"/></glow><softEdge rad="25400"/><reflection sy="-100000" stA="50000" endA="0" endPos="50000" dir="5400000" dist="12700"/></effectLst></spPr>"#;
        let n = ooxml::parse(xml).unwrap();
        let mut warnings = vec![];
        let e = parse(&n, None, &Theme::default(), &mut warnings);
        let s = e.outer_shadow.unwrap();
        assert_eq!(s.blur, 4.0);
        assert_eq!(s.distance, 3.0);
        assert_eq!(s.direction, 45.0);
        assert_eq!(s.color, "66112233");
        assert!(!s.rotate_with_shape);
        assert_eq!(e.glow.unwrap().radius, 6.0);
        assert_eq!(e.soft_edge, 2.0);
        assert_eq!(e.reflection.unwrap().scale[1], -1.0);
        assert!(warnings.is_empty());
        let explicit = ooxml::parse("<spPr><effectLst/></spPr>").unwrap();
        assert!(!parse(&explicit, Some(&n), &Theme::default(), &mut warnings).has_raster_effects());
    }
    #[test]
    fn shadow_is_outside_body_and_inner_shadow_stays_inside() {
        let source = image::RgbaImage::from_pixel(40, 40, image::Rgba([255, 0, 0, 255]));
        let node = ooxml::parse(r#"<spPr><effectLst><outerShdw dist="101600" dir="0"><srgbClr val="000000"/></outerShdw></effectLst></spPr>"#).unwrap();
        let e = parse(&node, None, &Theme::default(), &mut vec![]);
        let pose = Pose {
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
        };
        let (im, bounds) = composite(source.clone(), Vec2::splat(20.0), &e, pose, 1.0);
        let sample = |im: &egui::ColorImage, b: Rect, x: f32, y: f32| {
            let px = ((x - b.min.x) / b.width() * im.size[0] as f32) as usize;
            let py = ((y - b.min.y) / b.height() * im.size[1] as f32) as usize;
            im.pixels[py * im.size[0] + px]
        };
        assert_eq!(sample(&im, bounds, 10.0, 10.0), Color32::RED);
        assert_eq!(sample(&im, bounds, 24.0, 10.0), Color32::BLACK);
        assert_eq!(sample(&im, bounds, -2.0, 10.0), Color32::TRANSPARENT);
        let mut inner = e;
        inner.inner_shadow = inner.outer_shadow.take();
        let (im, bounds) = composite(source, Vec2::splat(20.0), &inner, pose, 1.0);
        assert_eq!(sample(&im, bounds, 2.0, 10.0), Color32::BLACK);
        assert_eq!(sample(&im, bounds, 12.0, 10.0), Color32::RED);
    }
    #[test]
    fn reflections_are_flipped_faded_and_cached_behind_source() {
        let source = image::RgbaImage::from_fn(20, 20, |_, y| {
            if y < 10 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let node = ooxml::parse(r#"<spPr><effectLst><reflection sy="-100000" stA="50000" endA="0" endPos="100000" dir="5400000" dist="25400"/></effectLst></spPr>"#).unwrap();
        let effects = parse(&node, None, &Theme::default(), &mut vec![]);
        let (im, b) = composite(
            source,
            Vec2::splat(20.0),
            &effects,
            Pose {
                rotation: 0.0,
                flip_h: false,
                flip_v: false,
            },
            1.0,
        );
        let at = |y: f32| {
            let x = ((10.0 - b.min.x) / b.width() * im.size[0] as f32) as usize;
            let y = ((y - b.min.y) / b.height() * im.size[1] as f32) as usize;
            im.pixels[y * im.size[0] + x].to_srgba_unmultiplied()
        };
        assert_eq!(at(5.0), [255, 0, 0, 255]);
        let near = at(25.0);
        let far = at(37.0);
        assert!(near[2] > near[0]);
        assert!(far[0] > far[2]);
        assert!(near[3] > far[3]);
        assert!(near[3] < 128);
    }
    #[test]
    fn scene_rotation_projects_depth_and_bevel_units() {
        let n = ooxml::parse(r#"<spPr><scene3d><camera prst="isometricRightUp"/><lightRig rig="threePt" dir="tl"/></scene3d><sp3d extrusionH="254000" prstMaterial="matte"><bevelT w="50800" h="38100"/></sp3d></spPr>"#).unwrap();
        let e = parse(&n, None, &Theme::default(), &mut vec![]);
        let scene = e.scene.unwrap();
        assert_eq!(scene.extrusion, 20.0);
        assert_eq!(scene.bevel_width, 4.0);
        assert_eq!(scene.bevel_height, 3.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 60.0));
        let front = project(rect.center(), rect, 0.0, &scene);
        let back = project(rect.center(), rect, 20.0, &scene);
        assert!(back.x < front.x && back.y < front.y);
        let mut malformed = scene;
        malformed.rotation = [f32::INFINITY, f32::NAN, f32::MAX];
        malformed.zoom = f32::MAX;
        malformed.camera = "perspectiveFront".into();
        malformed.field_of_view = f32::NAN;
        assert!(project(rect.right_bottom(), rect, f32::INFINITY, &malformed).is_finite());
    }
}
