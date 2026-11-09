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
    #[serde(default = "default_light_rig")]
    pub light_rig: String,
    #[serde(default)]
    pub light_rotation: [f32; 3],
}
fn default_light_rig() -> String {
    "threePt".into()
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
            light_rig: scene
                .and_then(|n| n.child("lightRig"))
                .and_then(|n| n.attr("rig"))
                .unwrap_or("threePt")
                .into(),
            light_rotation: {
                let rot = scene
                    .and_then(|n| n.child("lightRig"))
                    .and_then(|n| n.child("rot"));
                [
                    angle(rot, "lat", 0.0),
                    angle(rot, "lon", 0.0),
                    angle(rot, "rev", 0.0),
                ]
            },
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
        let source_rect = Rect::from_min_size(Pos2::ZERO, size);
        let source_bounds = safe.scene.as_ref().map_or(source_rect, |scene| {
            let mut bounds = source_rect;
            for q in [
                source_rect.left_top(),
                source_rect.right_top(),
                source_rect.right_bottom(),
                source_rect.left_bottom(),
            ] {
                for depth in [0.0, finite_range(scene.extrusion, 0.0, 512.0, 0.0) * z] {
                    bounds.extend_with(project(q, source_rect, depth, scene));
                }
            }
            bounds.expand(finite_range(scene.contour, 0.0, 512.0, 0.0) * z + 2.0)
        });
        let extrusion = (-source_bounds.left())
            .max(-source_bounds.top())
            .max(source_bounds.right() - size.x)
            .max(source_bounds.bottom() - size.y)
            .max(0.0);
        // Effects consume the projected silhouette, and their frame can remain
        // screen-aligned while the source rotates. Its diagonal bounds both axes.
        let effect_size = Vec2::splat(source_bounds.size().length().min(1e8));
        let reflection = safe.reflection.as_ref().map_or(0.0, |r| {
            effect_size.x * r.scale[0].abs().max(r.scale[1].abs())
                + (r.distance + r.blur * 3.0) * z
                + r.skew[0].to_radians().tan().abs() * effect_size.y
                + r.skew[1].to_radians().tan().abs() * effect_size.x
        });
        (safe.padding(effect_size, z) + reflection + extrusion).min(1e8)
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
            .min(1e8)
    }
}

#[derive(Default)]
pub(super) struct EffectRenderer {
    cache: HashMap<u64, CachedEffect>,
    pixels: usize,
    rejected: std::collections::HashSet<u64>,
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
        self.rejected.clear();
    }
    pub(super) fn shape(
        &mut self,
        p: &egui::Painter,
        rect: Rect,
        shape: &ShapeData,
        paths: &[ShapePath],
        context: (Pose, f32),
        resolve_font: &dyn Fn(&TextRun) -> egui::FontFamily,
    ) -> bool {
        let (pose, z) = context;
        if !shape.effects.has_raster_effects() && shape.effects.scene.is_none() {
            return false;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        format!("{shape:?}").hash(&mut hash);
        for run in shape
            .paragraphs
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
        {
            resolve_font(run).hash(&mut hash);
        }
        let key = effect_key(
            hash.finish(),
            rect.size(),
            pose,
            z,
            p.ctx().pixels_per_point(),
        );
        if self.rejected.contains(&key) {
            return false;
        }
        if !self.cache.contains_key(&key) {
            let Some((source, bounds)) =
                shape_source(p, rect.size(), shape, paths, z, pose, resolve_font)
            else {
                if self.rejected.len() >= 64 {
                    self.rejected.clear();
                }
                self.rejected.insert(key);
                return false;
            };
            // Geometry flips are baked into the source; text retains its readable orientation.
            let display_pose = Pose {
                rotation: pose.rotation,
                flip_h: false,
                flip_v: false,
            };
            self.insert(p, key, source, bounds, &shape.effects, (display_pose, z));
        }
        self.paint(
            p,
            key,
            rect,
            Pose {
                rotation: pose.rotation,
                flip_h: false,
                flip_v: false,
            },
        )
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
            self.insert(
                p,
                key,
                source,
                Rect::from_min_size(Pos2::ZERO, rect.size()),
                &picture.effects,
                (pose, z),
            );
        }
        self.paint(p, key, rect, pose)
    }
    fn insert(
        &mut self,
        p: &egui::Painter,
        key: u64,
        source: image::RgbaImage,
        source_bounds: Rect,
        effects: &DrawingEffects,
        context: (Pose, f32),
    ) {
        let (image, bounds) =
            composite(source, source_bounds.size(), effects, context.0, context.1);
        let bounds = bounds.translate(source_bounds.min.to_vec2());
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
/// Capture the same vector geometry and glyph atlas used on screen. This makes
/// text, arrowheads and projected 3D faces part of the effect silhouette instead
/// of laying an unaffected text label over a blurred/reflected shape.
fn shape_source(
    p: &egui::Painter,
    size: Vec2,
    shape: &ShapeData,
    paths: &[ShapePath],
    z: f32,
    pose: Pose,
    resolve_font: &dyn Fn(&TextRun) -> egui::FontFamily,
) -> Option<(image::RgbaImage, Rect)> {
    let layer = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("drawing-effect-source"),
    );
    let capture = egui::Painter::new(p.ctx().clone(), layer, Rect::EVERYTHING);
    let rect = Rect::from_min_size(Pos2::ZERO, size);
    let mut source_shape = shape.clone();
    if let Some(gradient) = &mut source_shape.gradient
        && !gradient.rotate_with_shape
    {
        gradient.angle -= pose.rotation;
    }
    DrawingRenderer::shape_body(
        &capture,
        rect,
        &source_shape,
        paths,
        Pose {
            rotation: 0.0,
            flip_h: pose.flip_h,
            flip_v: pose.flip_v,
        },
        z,
        resolve_font,
    );
    // Take only this private layer; no captured primitive reaches the main canvas.
    let list = p
        .ctx()
        .graphics_mut(|graphics| std::mem::take(graphics.entry(layer)));
    let shapes: Vec<_> = list.all_entries().cloned().collect();
    let bounds = shapes.iter().fold(rect, |bounds, shape| {
        bounds.union(shape.shape.visual_bounding_rect())
    });
    if !bounds.is_finite() || !bounds.is_positive() {
        return None;
    }
    let primitives = p.ctx().tessellate(shapes, p.ctx().pixels_per_point());
    let atlas = p.ctx().fonts(|fonts| fonts.image());
    raster_primitives(&primitives, &atlas, bounds).map(|image| (image, bounds))
}

/// Bounded CPU tessellation rasterizer. Font atlas texels and vertex colors are
/// premultiplied, just like egui's GPU path, so translucent glyph edges do not
/// acquire a dark fringe when subsequently blurred.
fn raster_primitives(
    primitives: &[egui::epaint::ClippedPrimitive],
    atlas: &egui::epaint::FontImage,
    bounds: Rect,
) -> Option<image::RgbaImage> {
    if atlas.width() == 0 || atlas.height() == 0 {
        return None;
    }
    let [w, h] = raster_size(bounds.size());
    let factor = Vec2::new(w as f32 / bounds.width(), h as f32 / bounds.height());
    let atlas_pixels: Vec<_> = atlas
        .srgba_pixels(None)
        .map(|c| c.a() as f32 / 255.0)
        .collect();
    let coverage = |uv: Pos2| {
        let x =
            (uv.x * atlas.width() as f32 - 0.5).clamp(0.0, atlas.width().saturating_sub(1) as f32);
        let y = (uv.y * atlas.height() as f32 - 0.5)
            .clamp(0.0, atlas.height().saturating_sub(1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = (
            (x0 + 1).min(atlas.width() - 1),
            (y0 + 1).min(atlas.height() - 1),
        );
        let (fx, fy) = (x.fract(), y.fract());
        let sample = |x, y| atlas_pixels[y * atlas.width() + x];
        (sample(x0, y0) * (1.0 - fx) + sample(x1, y0) * fx) * (1.0 - fy)
            + (sample(x0, y1) * (1.0 - fx) + sample(x1, y1) * fx) * fy
    };
    let mut pixels = vec![[0.0_f32; 4]; (w * h) as usize];
    let mut work = 0_u64;
    for primitive in primitives {
        let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
            continue;
        };
        if mesh.texture_id != egui::TextureId::default() || !mesh.is_valid() {
            return None;
        }
        for indices in mesh.indices.as_chunks::<3>().0 {
            let mut v =
                [indices[0], indices[1], indices[2]].map(|index| mesh.vertices[index as usize]);
            let mut q = v.map(|vertex| (vertex.pos - bounds.min) * factor);
            let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
            let mut area = cross(q[1] - q[0], q[2] - q[0]);
            if !area.is_finite() || area.abs() < 1e-6 {
                continue;
            }
            if area < 0.0 {
                q.swap(1, 2);
                v.swap(1, 2);
                area = -area;
            }
            let min = q
                .into_iter()
                .fold(Vec2::splat(f32::INFINITY), Vec2::min)
                .floor()
                .max(Vec2::ZERO);
            let max = q
                .into_iter()
                .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max)
                .ceil()
                .min(Vec2::new(w as f32, h as f32));
            work = work
                .saturating_add((max.x - min.x).max(0.0) as u64 * (max.y - min.y).max(0.0) as u64);
            if work > 96_000_000 {
                return None;
            }
            let edge = |a: Vec2, b: Vec2, point: Vec2| cross(b - a, point - a);
            let included = |value: f32, a: Vec2, b: Vec2| {
                value > 0.00001
                    || (value.abs() <= 0.00001 && (b.y < a.y || (b.y == a.y && b.x > a.x)))
            };
            for y in min.y as u32..max.y as u32 {
                for x in min.x as u32..max.x as u32 {
                    let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let weights = [
                        edge(q[1], q[2], point),
                        edge(q[2], q[0], point),
                        edge(q[0], q[1], point),
                    ];
                    if !(included(weights[0], q[1], q[2])
                        && included(weights[1], q[2], q[0])
                        && included(weights[2], q[0], q[1]))
                    {
                        continue;
                    }
                    let local = bounds.min + point / factor;
                    if !primitive.clip_rect.contains(local) {
                        continue;
                    }
                    let weights = weights.map(|weight| weight / area);
                    let uv = Pos2::new(
                        (0..3).map(|i| v[i].uv.x * weights[i]).sum(),
                        (0..3).map(|i| v[i].uv.y * weights[i]).sum(),
                    );
                    let alpha = coverage(uv);
                    let color: [f32; 4] = std::array::from_fn(|channel| {
                        (0..3)
                            .map(|i| v[i].color[channel] as f32 / 255.0 * weights[i])
                            .sum::<f32>()
                            * alpha
                    });
                    let dst = &mut pixels[(y * w + x) as usize];
                    for channel in 0..4 {
                        dst[channel] = color[channel] + dst[channel] * (1.0 - color[3]);
                    }
                }
            }
        }
    }
    Some(image::RgbaImage::from_fn(w, h, |x, y| {
        let p = pixels[(y * w + x) as usize];
        let rgba = std::array::from_fn(|channel| {
            (if channel == 3 {
                p[3]
            } else if p[3] > 0.0 {
                p[channel] / p[3]
            } else {
                0.0
            })
            .clamp(0.0, 1.0)
            .mul_add(255.0, 0.5) as u8
        });
        image::Rgba(rgba)
    }))
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
fn affine(q: Vec2, origin: Vec2, scale: [f32; 2], skew: [f32; 2], offset: Vec2) -> Vec2 {
    let xy = q - origin;
    origin
        + offset
        + Vec2::new(
            xy.x * scale[0] + xy.y * skew[0].clamp(-80.0, 80.0).to_radians().tan(),
            xy.y * scale[1] + xy.x * skew[1].clamp(-80.0, 80.0).to_radians().tan(),
        )
}
struct EffectTransform {
    size: Vec2,
    origin: Vec2,
    scale: [f32; 2],
    skew: [f32; 2],
    delta: Vec2,
    frame_pose: Pose,
}
impl EffectTransform {
    fn new(
        size: Vec2,
        align: &str,
        scale: [f32; 2],
        skew: [f32; 2],
        transform: (f32, f32, bool),
        context: (Pose, f32),
    ) -> Self {
        let (distance, direction, rotates) = transform;
        let (pose, z) = context;
        let frame_pose = if rotates {
            Pose {
                rotation: 0.0,
                flip_h: false,
                flip_v: false,
            }
        } else {
            pose
        };
        let mut out = Self {
            size,
            origin: Vec2::ZERO,
            scale,
            skew,
            delta: offset(distance, direction, true, frame_pose, z),
            frame_pose,
        };
        let bounds =
            out.corners()
                .into_iter()
                .map(|q| out.frame(q))
                .fold(Rect::NOTHING, |mut b, p| {
                    b.extend_with(p.to_pos2());
                    b
                });
        out.origin = bounds.min.to_vec2() + alignment(align, bounds.size());
        out
    }
    fn corners(&self) -> [Vec2; 4] {
        [
            Vec2::ZERO,
            Vec2::new(self.size.x, 0.0),
            self.size,
            Vec2::new(0.0, self.size.y),
        ]
    }
    fn frame(&self, q: Vec2) -> Vec2 {
        transform(
            q.to_pos2(),
            (self.size * 0.5).to_pos2(),
            self.frame_pose.rotation,
            self.frame_pose.flip_h,
            self.frame_pose.flip_v,
        )
        .to_vec2()
    }
    fn unframe(&self, q: Vec2) -> Vec2 {
        let center = (self.size * 0.5).to_pos2();
        let q = transform(q.to_pos2(), center, -self.frame_pose.rotation, false, false);
        transform(
            q,
            center,
            0.0,
            self.frame_pose.flip_h,
            self.frame_pose.flip_v,
        )
        .to_vec2()
    }
    fn forward(&self, q: Vec2) -> Vec2 {
        self.unframe(affine(
            self.frame(q),
            self.origin,
            self.scale,
            self.skew,
            self.delta,
        ))
    }
    fn inverse(&self, q: Vec2) -> Vec2 {
        self.unframe(inverse_affine(
            self.frame(q),
            self.origin,
            self.scale,
            self.skew,
            self.delta,
        ))
    }
    fn bounds(&self) -> Rect {
        self.corners()
            .into_iter()
            .map(|q| self.forward(q))
            .fold(Rect::NOTHING, |mut b, p| {
                b.extend_with(p.to_pos2());
                b
            })
    }
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
    let source_bounds = Rect::from_min_size(Pos2::ZERO, size);
    let mut bounds = source_bounds.expand(2.0);
    if let Some(s) = &effects.outer_shadow {
        bounds = bounds.union(
            EffectTransform::new(
                size,
                &s.alignment,
                s.scale,
                s.skew,
                (s.distance, s.direction, s.rotate_with_shape),
                (pose, z),
            )
            .bounds()
            .expand(s.blur * z * 3.0 + 2.0),
        );
    }
    if let Some(g) = &effects.glow {
        bounds = bounds.union(source_bounds.expand(g.radius * z * 2.0 + 2.0));
    }
    if let Some(r) = &effects.reflection {
        bounds = bounds.union(
            EffectTransform::new(
                size,
                &r.alignment,
                r.scale,
                r.skew,
                (r.distance, r.direction, r.rotate_with_shape),
                (pose, z),
            )
            .bounds()
            .expand(r.blur * z * 3.0 + 2.0),
        );
    }
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
        let transform = EffectTransform::new(
            size,
            &s.alignment,
            s.scale,
            s.skew,
            (s.distance, s.direction, s.rotate_with_shape),
            (pose, z),
        );
        let shifted = image::GrayImage::from_fn(w, h, |x, y| {
            image::Luma([sample(transform.inverse(local(x, y)))[3]])
        });
        let blurred = image::imageops::fast_blur(&shifted, (s.blur * z * factor.x * 0.5).max(0.01));
        let color = rgba(Some(&s.color), Color32::TRANSPARENT).to_srgba_unmultiplied();
        for (i, value) in blurred.pixels().enumerate() {
            over(&mut output[i], color, value[0] as f32 / 255.0);
        }
    }
    if let Some(r) = &effects.reflection {
        let transform = EffectTransform::new(
            size,
            &r.alignment,
            r.scale,
            r.skew,
            (r.distance, r.direction, r.rotate_with_shape),
            (pose, z),
        );
        let a = r.fade_direction.to_radians();
        let direction = Vec2::new(a.cos(), a.sin());
        let corners = transform
            .corners()
            .map(|q| transform.frame(transform.forward(q)).dot(direction));
        let start = corners.into_iter().fold(f32::INFINITY, f32::min);
        let end = corners.into_iter().fold(f32::NEG_INFINITY, f32::max);
        let reflected = image::RgbaImage::from_fn(w, h, |x, y| {
            let q = local(x, y);
            let src = transform.inverse(q);
            let mut color = sample(src);
            let t = ((transform.frame(q).dot(direction) - start) / (end - start).max(0.001))
                .clamp(0.0, 1.0);
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
fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt().max(0.0001);
    v.map(|v| v / length)
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn rotate3(v: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    let [lat, lon, rev] = angles.map(finite_angle).map(f32::to_radians);
    let x = v[0] * lon.cos() + v[2] * lon.sin();
    let depth = -v[0] * lon.sin() + v[2] * lon.cos();
    let y = v[1] * lat.cos() - depth * lat.sin();
    let depth = v[1] * lat.sin() + depth * lat.cos();
    [
        x * rev.cos() - y * rev.sin(),
        x * rev.sin() + y * rev.cos(),
        depth,
    ]
}
/// A deterministic native lighting model for DrawingML materials. Office's
/// proprietary rig intensities are not specified by OOXML; preserve the saved
/// direction/rotation and material response instead of giving every face the
/// same fixed two-tone shading.
pub(super) fn surface_color(scene: &Scene3D, color: Color32, normal: [f32; 3]) -> Color32 {
    if scene.material == "flat" {
        return color;
    }
    let dir = scene.light_direction.as_str();
    let light = normalized(rotate3(
        [
            if dir.contains('l') {
                -0.8
            } else if dir.contains('r') {
                0.8
            } else {
                0.0
            },
            if dir.contains('t') {
                -0.8
            } else if dir.contains('b') {
                0.8
            } else {
                0.0
            },
            -1.0,
        ],
        scene.light_rotation,
    ));
    let normal = normalized(rotate3(normal, scene.rotation));
    let diffuse = dot(normal, light).max(0.0);
    let half = normalized([light[0], light[1], light[2] - 1.0]);
    let (ambient, diffuse_gain, specular, shininess, metallic): (f32, f32, f32, f32, bool) =
        match scene.material.as_str() {
            "plastic" | "legacyPlastic" => (0.50, 0.58, 0.45, 30.0, false),
            "metal" | "legacyMetal" => (0.30, 0.80, 1.10, 42.0, true),
            "softmetal" => (0.50, 0.62, 0.70, 12.0, true),
            "powder" | "translucentPowder" => (0.72, 0.38, 0.08, 5.0, false),
            "dkEdge" => (0.34, 0.70, 0.30, 18.0, false),
            "softEdge" => (0.72, 0.34, 0.10, 10.0, false),
            "clear" => (0.38, 0.48, 0.85, 55.0, false),
            _ => (0.65, 0.48, 0.06, 8.0, false),
        };
    let rig_gain = match scene.light_rig.as_str() {
        "harsh" | "twoPt" => 1.20,
        "soft" | "balanced" | "threePt" => 1.0,
        "flat" | "flood" => 0.55,
        "contrasting" => 1.35,
        _ => 1.0,
    };
    let highlight = dot(normal, half).max(0.0).powf(shininess) * specular * rig_gain;
    let lighting = ambient + diffuse * diffuse_gain * rig_gain;
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let channel = |value: u8| {
        let specular_color = if metallic {
            value as f32 * 0.65 + 255.0 * 0.35
        } else {
            255.0
        };
        (value as f32 * lighting + specular_color * highlight)
            .clamp(0.0, 255.0)
            .round() as u8
    };
    Color32::from_rgba_unmultiplied(channel(r), channel(g), channel(b), a)
}

fn inset_polygon(points: &[Pos2], width: f32) -> Vec<Pos2> {
    let orientation: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum();
    let sign = orientation.signum();
    let center = points.iter().fold(Vec2::ZERO, |sum, q| sum + q.to_vec2()) / points.len() as f32;
    points
        .iter()
        .enumerate()
        .map(|(i, &q)| {
            let before = (q - points[(i + points.len() - 1) % points.len()]).normalized();
            let after = (points[(i + 1) % points.len()] - q).normalized();
            let first = Vec2::new(-before.y, before.x) * sign;
            let second = Vec2::new(-after.y, after.x) * sign;
            let bisector = (first + second).normalized();
            let length = (width / bisector.dot(second).max(0.25))
                .min((center - q.to_vec2()).length() * 0.45);
            q + bisector * length
        })
        .collect()
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
        let edge = (points[j] - points[i]).normalized();
        let normal = [
            edge.y * orientation.signum(),
            -edge.x * orientation.signum(),
            0.0,
        ];
        filled_path(
            p,
            &[front[i], rear[i], rear[j], front[j]],
            surface_color(scene, base, normal),
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
        let height = finite_range(scene.bevel_height, 0.0, 512.0, 0.0) * z;
        let steps = if matches!(
            scene.bevel.as_str(),
            "circle" | "softRound" | "convex" | "coolSlant"
        ) {
            5
        } else {
            1
        };
        let orientation: f32 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum();
        for band in 0..steps {
            let start = band as f32 / steps as f32;
            let end = (band + 1) as f32 / steps as f32;
            let outer = inset_polygon(points, width * start);
            let inner = inset_polygon(points, width * end);
            // Curved bevels progressively turn from the side normal toward the face normal.
            let phase = (start + end) * 0.5 * std::f32::consts::FRAC_PI_2;
            let slope = if steps > 1 {
                phase.cos() * height / width.max(0.001)
            } else {
                height / width.max(0.001)
            };
            for i in 0..points.len() {
                let j = (i + 1) % points.len();
                let edge = (points[j] - points[i]).normalized();
                let normal = [
                    edge.y * orientation.signum() * slope,
                    -edge.x * orientation.signum() * slope,
                    -1.0,
                ];
                filled_path(
                    p,
                    &[
                        transform3(outer[i]),
                        transform3(outer[j]),
                        transform3(inner[j]),
                        transform3(inner[i]),
                    ],
                    surface_color(scene, fill, normal),
                );
            }
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
    fn glyph_shape(effect_xml: &str, fill: bool) -> ShapeData {
        let xml = format!(
            r#"<sp><spPr><prstGeom prst="rect"/>{}<ln><noFill/></ln>{effect_xml}</spPr><txBody><bodyPr anchor="ctr"/><p><pPr algn="ctr"/><r><rPr sz="1800"><solidFill><srgbClr val="FF0000"/></solidFill></rPr><t>RUST </t></r><r><rPr sz="1800"><solidFill><srgbClr val="0000FF"/></solidFill></rPr><t>TEXT</t></r></p></txBody></sp>"#,
            if fill {
                "<solidFill><srgbClr val=\"16A085\"/></solidFill>"
            } else {
                "<noFill/>"
            }
        );
        super::super::shape(&ooxml::parse(&xml).unwrap(), &Theme::default(), &mut vec![])
    }
    fn rectangle_paths() -> Vec<ShapePath> {
        vec![ShapePath {
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            closed: true,
            fill: true,
            stroke: true,
        }]
    }
    fn no_pose() -> Pose {
        Pose {
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
        }
    }

    #[test]
    fn text_only_effects_capture_native_glyphs_colors_and_reflection() {
        let shape = glyph_shape(
            r#"<effectLst><reflection sy="-100000" stA="70000" endA="0" dist="38100" dir="5400000"/></effectLst>"#,
            false,
        );
        let original = serde_json::to_string(&shape).unwrap();
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.debug_painter();
            let (source, source_bounds) = shape_source(
                &painter,
                Vec2::new(180.0, 60.0),
                &shape,
                &rectangle_paths(),
                1.0,
                no_pose(),
                &|_| egui::FontFamily::Proportional,
            )
            .unwrap();
            assert!(
                source
                    .pixels()
                    .filter(|p| p[0] > 200 && p[2] < 30 && p[3] > 200)
                    .count()
                    > 40
            );
            assert!(
                source
                    .pixels()
                    .filter(|p| p[2] > 200 && p[0] < 30 && p[3] > 200)
                    .count()
                    > 40
            );
            assert!(source.pixels().filter(|p| p[3] == 0).count() > source.len() / 8);
            let (image, bounds) =
                composite(source, source_bounds.size(), &shape.effects, no_pose(), 1.0);
            let reflected = image
                .pixels
                .iter()
                .enumerate()
                .filter(|(i, p)| {
                    let y = bounds.top()
                        + (*i / image.size[0]) as f32 / image.size[1] as f32 * bounds.height();
                    y > source_bounds.height() && p.a() > 10
                })
                .count();
            assert!(
                reflected > 80,
                "glyphs must participate in reflection even without a shape fill"
            );
        });
        assert_eq!(serde_json::to_string(&shape).unwrap(), original);
    }

    #[test]
    fn projected_text_and_extrusion_join_the_effect_silhouette() {
        let shape = glyph_shape(
            r#"<effectLst><outerShdw blurRad="25400" dist="50800" dir="5400000"><srgbClr val="000000"/></outerShdw><reflection sy="-100000" stA="60000"/></effectLst><scene3d><camera prst="isometricRightUp"/><lightRig rig="threePt" dir="tl"/></scene3d><sp3d extrusionH="254000" prstMaterial="plastic"><bevelT w="38100" h="38100"/></sp3d>"#,
            true,
        );
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            let (source, bounds) = shape_source(
                &ctx.debug_painter(),
                Vec2::new(180.0, 60.0),
                &shape,
                &rectangle_paths(),
                1.0,
                no_pose(),
                &|_| egui::FontFamily::Proportional,
            )
            .unwrap();
            assert!(
                bounds.top() < 0.0,
                "projected extrusion extends beyond original rectangle"
            );
            let color_center = |red: bool| {
                let points: Vec<_> = source
                    .enumerate_pixels()
                    .filter(|(_, _, p)| {
                        p[3] > 200
                            && if red {
                                p[0] > 200 && p[2] < 30
                            } else {
                                p[2] > 200 && p[0] < 30
                            }
                    })
                    .map(|(x, y, _)| Vec2::new(x as f32, y as f32))
                    .collect();
                assert!(points.len() > 20);
                points.iter().fold(Vec2::ZERO, |sum, q| sum + *q) / points.len() as f32
            };
            let red = color_center(true);
            let blue = color_center(false);
            assert!(
                blue.x > red.x && blue.y < red.y,
                "glyphs must slope with the projected front face"
            );
            let (_, effects_bounds) =
                composite(source, bounds.size(), &shape.effects, no_pose(), 1.0);
            assert!(effects_bounds.bottom() > bounds.height() * 1.5);
        });
    }

    #[test]
    fn fixed_reflection_mirrors_rotation_and_keeps_sideways_effect_bounds() {
        let pose = Pose {
            rotation: 20.0,
            flip_h: false,
            flip_v: false,
        };
        let transform = EffectTransform::new(
            Vec2::new(100.0, 40.0),
            "bl",
            [1.0, -1.0],
            [0.0, 0.0],
            (3.0, 90.0, false),
            (pose, 1.0),
        );
        let a = Vec2::new(10.0, 20.0);
        let b = Vec2::new(90.0, 20.0);
        let source = transform.frame(b) - transform.frame(a);
        let reflected =
            transform.frame(transform.forward(b)) - transform.frame(transform.forward(a));
        assert!(source.y > 0.0 && reflected.y < 0.0);
        assert!((source.x - reflected.x).abs() < 0.001);
        assert!((transform.inverse(transform.forward(a)) - a).length() < 0.001);
        let node = ooxml::parse(r#"<spPr><effectLst><reflection sx="-100000" sy="100000" algn="l" dist="127000" dir="10800000" fadeDir="10800000"/></effectLst></spPr>"#).unwrap();
        let effects = parse(&node, None, &Theme::default(), &mut vec![]);
        let (image, bounds) = composite(
            image::RgbaImage::from_pixel(20, 20, image::Rgba([200, 40, 10, 255])),
            Vec2::splat(20.0),
            &effects,
            no_pose(),
            1.0,
        );
        assert!(bounds.left() <= -30.0);
        assert!(image.pixels.iter().enumerate().any(|(i, p)| {
            bounds.left() + (i % image.size[0]) as f32 / image.size[0] as f32 * bounds.width()
                < -10.0
                && p.a() > 20
        }));
    }

    #[test]
    fn lighting_material_rotation_and_saved_scene_defaults_are_preserved() {
        let node = ooxml::parse(r#"<spPr><scene3d><camera prst="orthographicFront"/><lightRig rig="harsh" dir="tl"><rot rev="10800000"/></lightRig></scene3d><sp3d prstMaterial="metal"/></spPr>"#).unwrap();
        let mut scene = parse(&node, None, &Theme::default(), &mut vec![])
            .scene
            .unwrap();
        assert_eq!(scene.light_rig, "harsh");
        assert_eq!(scene.light_rotation, [0.0, 0.0, 180.0]);
        let fill = Color32::from_rgb(22, 160, 133);
        let normal = [-0.6, -0.6, -1.0];
        let rotated = surface_color(&scene, fill, normal);
        scene.light_rotation = [0.0; 3];
        let facing = surface_color(&scene, fill, normal);
        assert_ne!(rotated, facing);
        scene.material = "matte".into();
        assert_ne!(surface_color(&scene, fill, normal), facing);
        let mut json = serde_json::to_value(&scene).unwrap();
        json.as_object_mut().unwrap().remove("light_rig");
        json.as_object_mut().unwrap().remove("light_rotation");
        let restored: Scene3D = serde_json::from_value(json).unwrap();
        assert_eq!(restored.light_rig, "threePt");
        assert_eq!(restored.light_rotation, [0.0; 3]);
    }

    #[test]
    fn software_mesh_edges_do_not_double_blend_translucency() {
        let mut mesh = egui::Mesh::default();
        for pos in [
            Pos2::ZERO,
            Pos2::new(4.0, 0.0),
            Pos2::new(4.0, 4.0),
            Pos2::new(0.0, 4.0),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: Pos2::new(0.5, 0.5),
                color: Color32::from_rgba_unmultiplied(255, 0, 0, 128),
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::splat(4.0));
        let primitive = egui::epaint::ClippedPrimitive {
            clip_rect: bounds,
            primitive: egui::epaint::Primitive::Mesh(mesh),
        };
        let atlas = egui::epaint::FontImage {
            size: [1, 1],
            pixels: vec![1.0],
        };
        let image = raster_primitives(&[primitive], &atlas, bounds).unwrap();
        assert!(
            image
                .pixels()
                .all(|p| p[3] == 128 && p[0] == 255 && p[1] == 0 && p[2] == 0)
        );
    }
    #[test]
    fn composite_outset_includes_projected_and_rotated_reflections() {
        let mut shape = glyph_shape(
            r#"<effectLst><reflection sy="-100000" rotWithShape="0"/></effectLst><scene3d><camera prst="orthographicFront" zoom="400000"/></scene3d>"#,
            true,
        );
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            for (size, rotation, camera) in [
                (Vec2::splat(100.0), 0.0, "orthographicFront"),
                (Vec2::new(400.0, 40.0), 90.0, "perspectiveFront"),
            ] {
                shape.effects.scene.as_mut().unwrap().camera = camera.into();
                let pose = Pose {
                    rotation,
                    ..no_pose()
                };
                let (source, bounds) = shape_source(
                    &ctx.debug_painter(),
                    size,
                    &shape,
                    &rectangle_paths(),
                    1.0,
                    pose,
                    &|_| egui::FontFamily::Proportional,
                )
                .unwrap();
                let (_, composed) = composite(source, bounds.size(), &shape.effects, pose, 1.0);
                let composed = composed.translate(bounds.min.to_vec2());
                let source_rect = Rect::from_min_size(Pos2::ZERO, size);
                let radius = size.length() * 0.5 + shape.effects.outset(size, 1.0);
                let conservative =
                    Rect::from_center_size(source_rect.center(), Vec2::splat(radius * 2.0));
                for q in [
                    composed.left_top(),
                    composed.right_top(),
                    composed.right_bottom(),
                    composed.left_bottom(),
                ] {
                    let displayed = transform(q, source_rect.center(), rotation, false, false);
                    assert!(
                        conservative.contains(displayed),
                        "{displayed:?} must remain visible within {conservative:?}"
                    );
                }
            }
        });
    }

    #[test]
    fn rejected_capture_is_not_retried_every_repaint() {
        let mut shape = glyph_shape(
            r#"<effectLst><glow rad="12700"><srgbClr val="00FF00"/></glow></effectLst>"#,
            false,
        );
        shape.paragraphs.clear();
        let context = egui::Context::default();
        let mut renderer = EffectRenderer::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            let p = ctx.debug_painter();
            let bad_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(f32::INFINITY, 10.0));
            assert!(
                !renderer.shape(&p, bad_rect, &shape, &[], (no_pose(), 1.0), &|_| {
                    egui::FontFamily::Proportional
                })
            );
            assert_eq!(renderer.rejected.len(), 1);
            let layer = egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("drawing-effect-source"),
            );
            ctx.graphics_mut(|graphics| {
                graphics
                    .entry(layer)
                    .add(Rect::EVERYTHING, egui::Shape::Noop);
            });
            assert!(
                !renderer.shape(&p, bad_rect, &shape, &[], (no_pose(), 1.0), &|_| {
                    egui::FontFamily::Proportional
                })
            );
            assert_eq!(
                ctx.graphics(|graphics| graphics.get(layer).unwrap().all_entries().len()),
                1
            );
        });
        renderer.clear();
        assert!(renderer.rejected.is_empty());
    }
}
