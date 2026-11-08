//! Read-only DrawingML objects, in worksheet coordinates. Never follows external media links.
use crate::{
    charts,
    ooxml::{self, Node, Relationship},
    xlsx_styles::Theme,
};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File};
use zip::ZipArchive;

const MAX_OBJECTS: usize = 2048;
const MAX_MEDIA: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_MEDIA: usize = 64 * 1024 * 1024;
const EMU_PER_POINT: f32 = 12700.0;
const POINT_SCALE: f32 = if cfg!(target_os = "macos") {
    1.0
} else {
    96.0 / 72.0
};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Marker {
    pub row: u32,
    pub col: u32,
    pub row_offset_emu: f64,
    pub col_offset_emu: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawingAnchor {
    TwoCell {
        from: Marker,
        to: Marker,
    },
    OneCell {
        from: Marker,
        width_emu: f64,
        height_emu: f64,
    },
    Absolute {
        x_emu: f64,
        y_emu: f64,
        width_emu: f64,
        height_emu: f64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawingObject {
    pub anchor: DrawingAnchor,
    pub name: String,
    pub rotation: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub content: DrawingContent,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawingContent {
    Shape(Box<ShapeData>),
    Picture(PictureData),
    Chart(Box<charts::Chart>),
    Group { children: Vec<GroupChild> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupChild {
    /// Child rectangle normalized to the group's coordinate system.
    pub rect: [f32; 4],
    pub object: DrawingObject,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PictureData {
    pub source: String,
    pub base64: String,
    /// Fraction of source image cropped from left/top/right/bottom.
    pub crop: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShapeData {
    pub geometry: String,
    pub fill: Option<String>,
    #[serde(default)]
    pub gradient: Option<GradientFill>,
    pub line: Option<String>,
    pub line_width: f32,
    pub dash: String,
    pub head: String,
    pub tail: String,
    pub paragraphs: Vec<TextParagraph>,
    pub vertical: String,
    pub text_rotation: f32,
    pub margins: [f32; 4],
    pub wrap: bool,
    pub paths: Vec<ShapePath>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GradientFill {
    pub stops: Vec<GradientStop>,
    pub angle: f32,
    pub scaled: bool,
    pub rotate_with_shape: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShapePath {
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
    pub fill: bool,
    pub stroke: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextParagraph {
    pub align: String,
    pub runs: Vec<TextRun>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: String,
    pub font_name: Option<String>,
}
fn num(node: Option<&Node>, attr: &str, default: f64) -> f64 {
    node.and_then(|n| n.attr(attr))
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}
fn enabled(node: Option<&Node>, attr: &str) -> bool {
    node.and_then(|n| n.attr(attr))
        .is_some_and(|v| v == "1" || v == "true")
}
fn marker(node: &Node) -> Result<Marker> {
    let component = |key: &str| -> Result<f64> {
        let v = node
            .child(key)
            .context("Incomplete drawing marker")?
            .text
            .parse::<f64>()?;
        ensure!(v.is_finite() && v >= 0.0, "Invalid drawing marker");
        Ok(v)
    };
    let row = component("row")?;
    let col = component("col")?;
    ensure!(
        row < 1_048_576.0 && col < 16_384.0,
        "Drawing marker outside worksheet"
    );
    Ok(Marker {
        row: row as u32 + 1,
        col: col as u32 + 1,
        row_offset_emu: component("rowOff")?,
        col_offset_emu: component("colOff")?,
    })
}
fn anchor(node: &Node) -> Result<DrawingAnchor> {
    let ext = node.child("ext");
    let width = num(ext, "cx", 0.0);
    let height = num(ext, "cy", 0.0);
    ensure!(
        width >= 0.0 && height >= 0.0 && width < 1e13 && height < 1e13,
        "Invalid drawing extent"
    );
    Ok(match node.name.as_str() {
        "twoCellAnchor" => DrawingAnchor::TwoCell {
            from: marker(node.child("from").context("Missing drawing start")?)?,
            to: marker(node.child("to").context("Missing drawing end")?)?,
        },
        "oneCellAnchor" => DrawingAnchor::OneCell {
            from: marker(node.child("from").context("Missing drawing start")?)?,
            width_emu: width,
            height_emu: height,
        },
        "absoluteAnchor" => DrawingAnchor::Absolute {
            x_emu: num(node.child("pos"), "x", 0.0),
            y_emu: num(node.child("pos"), "y", 0.0),
            width_emu: width,
            height_emu: height,
        },
        _ => anyhow::bail!("Unsupported drawing anchor"),
    })
}
fn color(node: &Node, theme: &Theme) -> Option<String> {
    let color = if ["srgbClr", "schemeClr", "sysClr", "prstClr"].contains(&node.name.as_str()) {
        node
    } else {
        node.children
            .iter()
            .find(|n| ["srgbClr", "schemeClr", "sysClr", "prstClr"].contains(&n.name.as_str()))?
    };
    let raw = match color.name.as_str() {
        "srgbClr" => color.attr("val")?.to_owned(),
        "sysClr" => color.attr("lastClr").unwrap_or("000000").to_owned(),
        "schemeClr" => {
            let idx = match color.attr("val")? {
                "lt1" | "bg1" => 0,
                "dk1" | "tx1" => 1,
                "lt2" | "bg2" => 2,
                "dk2" | "tx2" => 3,
                "accent1" | "phClr" => 4,
                "accent2" => 5,
                "accent3" => 6,
                "accent4" => 7,
                "accent5" => 8,
                "accent6" => 9,
                "hlink" => 10,
                "folHlink" => 11,
                _ => return None,
            };
            theme.colors.get(idx)?.clone()
        }
        "prstClr" => match color.attr("val")? {
            "white" => "FFFFFF",
            "black" => "000000",
            "red" => "FF0000",
            "blue" => "0000FF",
            "green" => "008000",
            "yellow" => "FFFF00",
            "gray" | "grey" => "808080",
            "orange" => "FFA500",
            "purple" => "800080",
            _ => return None,
        }
        .to_owned(),
        _ => return None,
    };
    let raw = if raw.len() == 8 {
        &raw[2..]
    } else {
        raw.as_str()
    };
    if raw.len() != 6 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut rgb = [0.0; 3];
    for (i, v) in rgb.iter_mut().enumerate() {
        *v = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).ok()? as f64 / 255.0;
    }
    let mut alpha = 1.0;
    for t in &color.children {
        let v = num(Some(t), "val", 100000.0) / 100000.0;
        match t.name.as_str() {
            "alpha" => alpha = v,
            "alphaMod" => alpha *= v,
            "alphaOff" => alpha += v,
            "tint" => {
                for c in &mut rgb {
                    *c = *c * v + (1.0 - v)
                }
            }
            "shade" => {
                for c in &mut rgb {
                    *c *= v
                }
            }
            "lumMod" | "lumOff" | "satMod" | "satOff" | "sat" => {
                adjust_hsl(&mut rgb, &t.name, v);
            }
            _ => {}
        }
    }
    Some(format!(
        "{:02X}{:02X}{:02X}{:02X}",
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8
    ))
}
fn adjust_hsl(rgb: &mut [f64; 3], operation: &str, value: f64) {
    let max = rgb.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = rgb.iter().copied().fold(f64::INFINITY, f64::min);
    let mut lum = (max + min) / 2.0;
    let delta = max - min;
    let mut sat = if delta.abs() < 1e-12 {
        0.0
    } else {
        delta / (1.0 - (2.0 * lum - 1.0).abs())
    };
    let hue = if delta.abs() < 1e-12 {
        0.0
    } else if max == rgb[0] {
        ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    match operation {
        "lumMod" => lum *= value,
        "lumOff" => lum += value,
        "satMod" => sat *= value,
        "satOff" => sat += value,
        "sat" => sat = value,
        _ => {}
    }
    lum = lum.clamp(0.0, 1.0);
    sat = sat.clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * lum - 1.0).abs()) * sat;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let base = match hue as u8 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    let offset = lum - chroma / 2.0;
    for i in 0..3 {
        rgb[i] = base[i] + offset
    }
}

fn text_runs(tx: &Node, theme: &Theme) -> Vec<TextParagraph> {
    tx.children_named("p")
        .map(|p| {
            let pp = p.child("pPr");
            let defaults = pp.and_then(|n| n.child("defRPr"));
            let mut runs = vec![];
            for n in &p.children {
                if !["r", "fld", "br"].contains(&n.name.as_str()) {
                    continue;
                }
                let props = n.child("rPr");
                let prop = |key: &str| {
                    props
                        .and_then(|n| n.attr(key))
                        .or_else(|| defaults.and_then(|n| n.attr(key)))
                };
                let run = TextRun {
                    text: if n.name == "br" {
                        "\n".into()
                    } else {
                        n.child("t").map(|t| t.text.clone()).unwrap_or_default()
                    },
                    size: prop("sz")
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(1100.0)
                        / 100.0,
                    bold: prop("b").is_some_and(|v| v == "1" || v == "true"),
                    italic: prop("i").is_some_and(|v| v == "1" || v == "true"),
                    underline: prop("u").is_some_and(|v| v != "none"),
                    color: props
                        .and_then(|n| n.child("solidFill"))
                        .or_else(|| defaults.and_then(|n| n.child("solidFill")))
                        .and_then(|n| color(n, theme))
                        .unwrap_or("FF000000".into()),
                    font_name: props
                        .and_then(|n| n.child("latin"))
                        .or_else(|| defaults.and_then(|n| n.child("latin")))
                        .and_then(|n| n.attr("typeface"))
                        .map(str::to_owned),
                };
                runs.push(run);
            }
            TextParagraph {
                align: pp.and_then(|n| n.attr("algn")).unwrap_or("l").into(),
                runs,
            }
        })
        .collect()
}
fn custom_paths(geometry: &Node) -> Vec<ShapePath> {
    let Some(list) = geometry.child("pathLst") else {
        return vec![];
    };
    list.children_named("path")
        .filter_map(|path| {
            let width = num(Some(path), "w", 1.0).max(1.0) as f32;
            let height = num(Some(path), "h", 1.0).max(1.0) as f32;
            let pt = |n: &Node| -> [f32; 2] {
                [
                    (num(Some(n), "x", 0.0) as f32) / width,
                    (num(Some(n), "y", 0.0) as f32) / height,
                ]
            };
            let mut points = vec![];
            let mut closed = false;
            for command in &path.children {
                match command.name.as_str() {
                    "moveTo" | "lnTo" => {
                        if let Some(p) = command.child("pt") {
                            points.push(pt(p))
                        }
                    }
                    "cubicBezTo" | "quadBezTo" => {
                        let Some(start) = points.last().copied() else {
                            continue;
                        };
                        let controls: Vec<_> = command.children_named("pt").map(pt).collect();
                        for i in 1..=16 {
                            let t = i as f32 / 16.0;
                            let u = 1.0 - t;
                            let p = match controls.as_slice() {
                                [a, b, c] => [
                                    u * u * u * start[0]
                                        + 3.0 * u * u * t * a[0]
                                        + 3.0 * u * t * t * b[0]
                                        + t * t * t * c[0],
                                    u * u * u * start[1]
                                        + 3.0 * u * u * t * a[1]
                                        + 3.0 * u * t * t * b[1]
                                        + t * t * t * c[1],
                                ],
                                [a, b] => [
                                    u * u * start[0] + 2.0 * u * t * a[0] + t * t * b[0],
                                    u * u * start[1] + 2.0 * u * t * a[1] + t * t * b[1],
                                ],
                                _ => continue,
                            };
                            points.push(p)
                        }
                    }
                    "close" => closed = true,
                    _ => return None,
                }
                if points.len() > 4096 {
                    return None;
                }
            }
            (!points.is_empty()).then_some(ShapePath {
                points,
                closed,
                fill: path.attr("fill") != Some("none"),
                stroke: path.attr("stroke") != Some("0"),
            })
        })
        .collect()
}
fn shape(node: &Node, theme: &Theme, warnings: &mut Vec<String>) -> ShapeData {
    let empty = Node::default();
    let sp = node.child("spPr").unwrap_or(&empty);
    let style = node.child("style");
    let geom = sp
        .child("prstGeom")
        .and_then(|n| n.attr("prst"))
        .unwrap_or(if node.name == "cxnSp" { "line" } else { "rect" });
    let paths = sp.child("custGeom").map(custom_paths).unwrap_or_default();
    let fill = if sp.child("noFill").is_some() {
        None
    } else {
        sp.child("solidFill")
            .and_then(|n| color(n, theme))
            .or_else(|| {
                style
                    .and_then(|n| n.child("fillRef"))
                    .filter(|n| n.attr("idx") != Some("0"))
                    .and_then(|n| color(n, theme))
            })
    };
    let ln = sp.child("ln");
    let line = if ln.is_some_and(|n| n.child("noFill").is_some()) {
        None
    } else {
        ln.and_then(|n| n.child("solidFill"))
            .and_then(|n| color(n, theme))
            .or_else(|| {
                style
                    .and_then(|n| n.child("lnRef"))
                    .filter(|n| n.attr("idx") != Some("0"))
                    .and_then(|n| color(n, theme))
            })
    };
    let gradient = sp.child("gradFill").and_then(|g| {
        if g.child("path").is_some() {
            warnings.push("도형 경로형 그라데이션".into());
            return None;
        }
        let mut stops: Vec<_> = g
            .child("gsLst")?
            .children_named("gs")
            .filter_map(|s| {
                Some(GradientStop {
                    position: (num(Some(s), "pos", 0.0) / 100000.0).clamp(0.0, 1.0) as f32,
                    color: color(s, theme)?,
                })
            })
            .collect();
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        if stops.is_empty() || stops.len() > 64 {
            warnings.push("그라데이션 색상 단계 제한".into());
            return None;
        }
        Some(GradientFill {
            stops,
            angle: (num(g.child("lin"), "ang", 0.0) / 60000.0) as f32,
            scaled: enabled(g.child("lin"), "scaled"),
            rotate_with_shape: g.attr("rotWithShape") != Some("0"),
        })
    });
    if sp
        .child("effectLst")
        .is_some_and(|n| !n.children.is_empty())
        || sp.child("scene3d").is_some()
    {
        warnings.push("도형 그림자·3D 효과".into())
    }
    if sp.child("custGeom").is_some() && paths.is_empty() {
        warnings.push("수식 또는 원호가 포함된 사용자 지정 도형".into())
    }
    if !supported_geometry(geom) && sp.child("custGeom").is_none() {
        warnings.push(format!("미지원 도형 모양: {geom}"))
    }
    let body = node.child("txBody").and_then(|n| n.child("bodyPr"));
    ShapeData {
        geometry: geom.into(),
        fill,
        gradient,
        line,
        line_width: (num(ln, "w", 12700.0) / 12700.0) as f32,
        dash: ln
            .and_then(|n| n.child("prstDash"))
            .and_then(|n| n.attr("val"))
            .unwrap_or("solid")
            .into(),
        head: ln
            .and_then(|n| n.child("headEnd"))
            .and_then(|n| n.attr("type"))
            .unwrap_or("none")
            .into(),
        tail: ln
            .and_then(|n| n.child("tailEnd"))
            .and_then(|n| n.attr("type"))
            .unwrap_or("none")
            .into(),
        paragraphs: node
            .child("txBody")
            .map(|n| text_runs(n, theme))
            .unwrap_or_default(),
        vertical: body.and_then(|n| n.attr("anchor")).unwrap_or("t").into(),
        text_rotation: (num(body, "rot", 0.0) / 60000.0) as f32,
        margins: [
            num(body, "lIns", 91440.0) as f32 / EMU_PER_POINT,
            num(body, "tIns", 45720.0) as f32 / EMU_PER_POINT,
            num(body, "rIns", 91440.0) as f32 / EMU_PER_POINT,
            num(body, "bIns", 45720.0) as f32 / EMU_PER_POINT,
        ],
        wrap: body.and_then(|n| n.attr("wrap")) != Some("none"),
        paths,
    }
}
// Office compatibility branches describe alternatives, not additional objects.
// Prefer the interoperable Fallback, otherwise inspect only the first Choice.
fn effective_children<'a>(node: &'a Node, warnings: &mut Vec<String>) -> Vec<&'a Node> {
    fn append<'a>(
        node: &'a Node,
        depth: usize,
        out: &mut Vec<&'a Node>,
        warnings: &mut Vec<String>,
    ) {
        if depth >= 24 || out.len() >= MAX_OBJECTS {
            warnings.push("그리기 호환성 분기의 복잡도 제한".into());
            return;
        }
        for child in &node.children {
            if out.len() >= MAX_OBJECTS {
                warnings.push("그리기 호환성 분기의 개체 수 제한".into());
                break;
            }
            if child.name == "AlternateContent" {
                if let Some(branch) = child.child("Fallback").or_else(|| child.child("Choice")) {
                    append(branch, depth + 1, out, warnings);
                } else {
                    warnings.push("선택할 수 있는 호환성 분기가 없는 그리기 개체".into());
                }
            } else {
                out.push(child)
            }
        }
    }
    let mut out = vec![];
    append(node, 0, &mut out, warnings);
    out
}
fn is_drawing_object(node: &Node) -> bool {
    matches!(
        node.name.as_str(),
        "sp" | "cxnSp" | "pic" | "graphicFrame" | "grpSp"
    )
}

struct ReadState<'a> {
    zip: &'a mut ZipArchive<File>,
    theme: &'a Theme,
    warnings: Vec<String>,
    objects: usize,
    media_bytes: usize,
}
impl ReadState<'_> {
    fn object(
        &mut self,
        node: &Node,
        anchor: DrawingAnchor,
        rels: &HashMap<String, Relationship>,
        depth: usize,
    ) -> Result<Option<DrawingObject>> {
        self.objects += 1;
        ensure!(
            self.objects <= MAX_OBJECTS && depth < 24,
            "Drawing object complexity limit exceeded"
        );
        let props = node.child(if node.name == "grpSp" {
            "grpSpPr"
        } else {
            "spPr"
        });
        let xf = props
            .and_then(|n| n.child("xfrm"))
            .or_else(|| node.child("xfrm"));
        let rotation = (num(xf, "rot", 0.0) / 60000.0) as f32;
        let flip_h = enabled(xf, "flipH");
        let flip_v = enabled(xf, "flipV");
        let name = node
            .descendant("cNvPr")
            .and_then(|n| n.attr("name"))
            .unwrap_or("")
            .to_owned();
        if node
            .descendant("cNvPr")
            .is_some_and(|n| enabled(Some(n), "hidden"))
        {
            return Ok(None);
        }
        let content = match node.name.as_str() {
            "sp" | "cxnSp" => {
                DrawingContent::Shape(Box::new(shape(node, self.theme, &mut self.warnings)))
            }
            "pic" => {
                let id = node
                    .child("blipFill")
                    .and_then(|n| n.child("blip"))
                    .and_then(|n| n.local_attr("embed"));
                let Some(rel) = id
                    .and_then(|id| rels.get(id))
                    .filter(|r| !r.external && r.kind.ends_with("/image"))
                else {
                    self.warnings.push("외부 연결 또는 누락된 그림".into());
                    return Ok(None);
                };
                let bytes = ooxml::part_bytes(self.zip, &rel.target, MAX_MEDIA)?;
                self.media_bytes += bytes.len();
                ensure!(
                    self.media_bytes <= MAX_TOTAL_MEDIA,
                    "Workbook drawing media exceeds 64 MiB limit"
                );
                let reader =
                    image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
                let (w, h) = match reader.into_dimensions() {
                    Ok(dim) => dim,
                    Err(_) => {
                        self.warnings.push(format!(
                            "미지원 그림 형식: {}",
                            rel.target.rsplit('.').next().unwrap_or("unknown")
                        ));
                        return Ok(None);
                    }
                };
                ensure!(
                    (w as u64) * (h as u64) <= 16_000_000,
                    "Drawing image exceeds 16 million pixels"
                );
                let crop = node.child("blipFill").and_then(|n| n.child("srcRect"));
                DrawingContent::Picture(PictureData {
                    source: rel.target.clone(),
                    base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    crop: ["l", "t", "r", "b"].map(|k| (num(crop, k, 0.0) / 100000.0) as f32),
                })
            }
            "graphicFrame" => {
                let Some(chart) = node
                    .child("graphic")
                    .and_then(|n| n.child("graphicData"))
                    .and_then(|n| n.child("chart"))
                else {
                    self.warnings.push("SmartArt 또는 기타 그래픽 개체".into());
                    return Ok(None);
                };
                let Some(rel) = chart
                    .local_attr("id")
                    .and_then(|id| rels.get(id))
                    .filter(|r| !r.external && r.kind.ends_with("/chart"))
                else {
                    self.warnings.push("외부 연결 또는 누락된 차트".into());
                    return Ok(None);
                };
                let chart =
                    charts::parse_chart(&ooxml::part_xml(self.zip, &rel.target)?, self.theme)?;
                self.warnings.extend(chart.warnings.iter().cloned());
                DrawingContent::Chart(Box::new(chart))
            }
            "grpSp" => {
                let off = xf.and_then(|n| n.child("chOff"));
                let ext = xf.and_then(|n| n.child("chExt"));
                let x = num(off, "x", 0.0);
                let y = num(off, "y", 0.0);
                let w = num(ext, "cx", 1.0).max(1.0);
                let h = num(ext, "cy", 1.0).max(1.0);
                let mut children = vec![];
                for child in effective_children(node, &mut self.warnings) {
                    if !is_drawing_object(child) {
                        if !matches!(child.name.as_str(), "nvGrpSpPr" | "grpSpPr" | "extLst") {
                            self.warnings
                                .push(format!("미지원 그룹 그리기 개체: {}", child.name));
                        }
                        continue;
                    }
                    let xf = child
                        .child(if child.name == "grpSp" {
                            "grpSpPr"
                        } else {
                            "spPr"
                        })
                        .and_then(|n| n.child("xfrm"))
                        .or_else(|| child.child("xfrm"));
                    let off = xf.and_then(|n| n.child("off"));
                    let ext = xf.and_then(|n| n.child("ext"));
                    let rect = [
                        ((num(off, "x", x) - x) / w) as f32,
                        ((num(off, "y", y) - y) / h) as f32,
                        (num(ext, "cx", w) / w) as f32,
                        (num(ext, "cy", h) / h) as f32,
                    ];
                    if let Some(object) = self.object(child, anchor.clone(), rels, depth + 1)? {
                        children.push(GroupChild { rect, object })
                    }
                }
                DrawingContent::Group { children }
            }
            _ => {
                self.warnings
                    .push(format!("미지원 그리기 개체: {}", node.name));
                return Ok(None);
            }
        };
        Ok(Some(DrawingObject {
            anchor,
            name,
            rotation,
            flip_h,
            flip_v,
            content,
        }))
    }
}
pub(crate) fn read_sheet_drawings(
    zip: &mut ZipArchive<File>,
    sheet_part: &str,
    sheet_xml: &str,
    theme: &Theme,
) -> Result<(Vec<DrawingObject>, Vec<String>)> {
    let sheet = ooxml::parse(sheet_xml)?;
    let sheet_rels = ooxml::read_relationships(zip, sheet_part)?;
    let mut state = ReadState {
        zip,
        theme,
        warnings: vec![],
        objects: 0,
        media_bytes: 0,
    };
    let mut objects = vec![];
    let sheet_children = effective_children(&sheet, &mut state.warnings);
    for reference in sheet_children
        .iter()
        .copied()
        .filter(|n| n.name == "drawing")
    {
        let Some(rel) = reference
            .local_attr("id")
            .and_then(|id| sheet_rels.get(id))
            .filter(|r| !r.external && r.kind.ends_with("/drawing"))
        else {
            state
                .warnings
                .push("누락되거나 외부에 연결된 그리기 레이어".into());
            continue;
        };
        let root = ooxml::parse(&ooxml::part_xml(state.zip, &rel.target)?)?;
        let rels = ooxml::read_relationships(state.zip, &rel.target)?;
        for node in effective_children(&root, &mut state.warnings) {
            if !matches!(
                node.name.as_str(),
                "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor"
            ) {
                if node.name != "extLst" {
                    state
                        .warnings
                        .push(format!("미지원 그리기 앵커: {}", node.name));
                }
                continue;
            }
            let anchor = anchor(node)?;
            for child in effective_children(node, &mut state.warnings) {
                if is_drawing_object(child) {
                    match state.object(child, anchor.clone(), &rels, 0) {
                        Ok(Some(object)) => objects.push(object),
                        Ok(None) => {}
                        Err(error) => state
                            .warnings
                            .push(format!("그리기 개체를 표시하지 못했습니다: {error}")),
                    }
                } else if !matches!(
                    child.name.as_str(),
                    "from" | "to" | "pos" | "ext" | "clientData" | "extLst"
                ) {
                    state
                        .warnings
                        .push(format!("미지원 그리기 개체: {}", child.name));
                }
            }
        }
    }
    for reference in sheet_children
        .iter()
        .copied()
        .filter(|n| n.name == "legacyDrawing")
    {
        let Some(rel) = reference
            .local_attr("id")
            .and_then(|id| sheet_rels.get(id))
            .filter(|r| !r.external && r.kind.ends_with("/vmlDrawing"))
        else {
            state
                .warnings
                .push("누락되거나 외부에 연결된 VML 개체".into());
            continue;
        };
        match ooxml::part_xml(state.zip, &rel.target)
            .and_then(|xml| read_vml(&xml, &mut state.warnings))
        {
            Ok(mut vml) => objects.append(&mut vml),
            Err(error) => state
                .warnings
                .push(format!("VML 개체를 표시하지 못했습니다: {error}")),
        }
    }
    state.warnings.sort();
    state.warnings.dedup();
    Ok((objects, state.warnings))
}

fn css_properties(style: &str) -> HashMap<&str, &str> {
    style
        .split(';')
        .filter_map(|item| item.split_once(':'))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}
fn css_points(value: Option<&&str>) -> Option<f64> {
    let raw = *value?;
    let (number, factor) = if let Some(v) = raw.strip_suffix("pt") {
        (v, 1.0)
    } else if let Some(v) = raw.strip_suffix("px") {
        (v, 0.75)
    } else if let Some(v) = raw.strip_suffix("in") {
        (v, 72.0)
    } else if let Some(v) = raw.strip_suffix("cm") {
        (v, 72.0 / 2.54)
    } else if let Some(v) = raw.strip_suffix("mm") {
        (v, 72.0 / 25.4)
    } else {
        (raw, 1.0)
    };
    number
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && v.abs() < 1e9)
        .map(|v| v * factor)
}
fn vml_color(value: Option<&str>, default: &str) -> String {
    let raw = value
        .unwrap_or(default)
        .split_whitespace()
        .next()
        .unwrap_or(default)
        .trim_start_matches('#');
    if raw.len() == 6 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return format!("FF{}", raw.to_ascii_uppercase());
    }
    if raw.len() == 3 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return format!(
            "FF{}",
            raw.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_ascii_uppercase()
        );
    }
    match raw.to_ascii_lowercase().as_str() {
        "white" | "window" => "FFFFFFFF",
        "black" | "windowtext" | "buttontext" => "FF000000",
        "buttonface" => "FFECECEC",
        "red" => "FFFF0000",
        "green" => "FF008000",
        "blue" => "FF0000FF",
        "yellow" => "FFFFFF00",
        "gray" | "grey" => "FF808080",
        _ => default,
    }
    .into()
}
fn vml_text(node: &Node, inherited: &TextRun, out: &mut Vec<TextRun>) {
    let style = css_properties(node.attr("style").unwrap_or(""));
    let mut run = inherited.clone();
    if let Some(size) = css_points(style.get("font-size")) {
        run.size = size as f32
    }
    if let Some(face) = node
        .attr("face")
        .or_else(|| style.get("font-family").copied())
    {
        run.font_name = Some(face.trim_matches('\'').to_owned())
    }
    if let Some(color) = node.attr("color").or_else(|| style.get("color").copied()) {
        run.color = vml_color(Some(color), "FF000000")
    }
    run.bold |= node.name == "b"
        || node.name == "strong"
        || style
            .get("font-weight")
            .is_some_and(|s| *s == "bold" || *s == "700");
    run.italic |=
        node.name == "i" || node.name == "em" || style.get("font-style") == Some(&"italic");
    run.underline |= node.name == "u";
    // VML textboxes contain indented HTML. XML indentation is not a visible
    // paragraph and must not push a button's label below its rectangle.
    if !node.text.is_empty() && (!node.text.trim().is_empty() || !node.text.contains(['\n', '\r']))
    {
        let mut text = run.clone();
        text.text = node.text.clone();
        out.push(text)
    }
    for child in &node.children {
        if child.name == "br" {
            let mut text = run.clone();
            text.text = "\n".into();
            out.push(text)
        } else {
            vml_text(child, &run, out)
        }
    }
}
fn read_vml(xml: &str, warnings: &mut Vec<String>) -> Result<Vec<DrawingObject>> {
    let root = ooxml::parse(xml)?;
    let mut out = vec![];
    for node in &root.children {
        if !["shape", "rect", "roundrect", "oval", "line"].contains(&node.name.as_str()) {
            if node.name == "group" {
                warnings.push("기존 VML 그룹 도형".into());
            }
            continue;
        }
        if node.child("imagedata").is_some() {
            warnings.push("기존 VML에 포함된 그림".into());
        }
        ensure!(out.len() < MAX_OBJECTS, "VML object limit exceeded");
        let css = css_properties(node.attr("style").unwrap_or(""));
        if css.get("visibility") == Some(&"hidden") || css.get("display") == Some(&"none") {
            continue;
        }
        let client = node.child("ClientData");
        let object_type = client.and_then(|n| n.attr("ObjectType")).unwrap_or("Shape");
        if object_type == "Note" {
            warnings.push("표시 상태인 기존 Excel 메모".into());
            continue;
        }
        let Some(width) = css_points(css.get("width")) else {
            warnings.push("위치 정보를 읽을 수 없는 VML 개체".into());
            continue;
        };
        let Some(height) = css_points(css.get("height")) else {
            warnings.push("크기 정보를 읽을 수 없는 VML 개체".into());
            continue;
        };
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        let geometry = match node.name.as_str() {
            "oval" => "ellipse",
            "roundrect" => "roundRect",
            "line" => "line",
            _ => "rect",
        };
        if node.name == "shape"
            && !node
                .attr("type")
                .is_some_and(|v| v.ends_with("_t201") || v.ends_with("_t202") || v.ends_with("_t1"))
        {
            warnings.push("기존 VML 사용자 지정 도형은 사각형 윤곽으로 표시됩니다".into());
        }
        let mut runs = vec![];
        if let Some(textbox) = node.child("textbox") {
            vml_text(
                textbox,
                &TextRun {
                    text: String::new(),
                    size: 11.0,
                    bold: false,
                    italic: false,
                    underline: false,
                    color: "FF000000".into(),
                    font_name: Some("Arial".into()),
                },
                &mut runs,
            )
        }
        if !["Button", "TextBox", "Shape"].contains(&object_type) {
            warnings.push(format!("기존 Excel {object_type} 컨트롤의 정적 외형"))
        }
        let align = client
            .and_then(|n| n.child("TextHAlign"))
            .map(|n| n.text.as_str())
            .unwrap_or("Left");
        let vertical = client
            .and_then(|n| n.child("TextVAlign"))
            .map(|n| n.text.as_str())
            .unwrap_or("Top");
        let line_width =
            css_points(Some(&node.attr("strokeweight").unwrap_or("1pt"))).unwrap_or(1.0) as f32;
        out.push(DrawingObject {
            anchor: DrawingAnchor::Absolute {
                x_emu: css_points(css.get("margin-left").or_else(|| css.get("left")))
                    .unwrap_or(0.0)
                    * 12700.0,
                y_emu: css_points(css.get("margin-top").or_else(|| css.get("top"))).unwrap_or(0.0)
                    * 12700.0,
                width_emu: width * 12700.0,
                height_emu: height * 12700.0,
            },
            name: node.attr("id").unwrap_or("VML object").into(),
            rotation: css
                .get("rotation")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            flip_h: false,
            flip_v: false,
            content: DrawingContent::Shape(Box::new(ShapeData {
                geometry: geometry.into(),
                fill: if node.attr("filled") == Some("f") {
                    None
                } else {
                    Some(vml_color(
                        node.attr("fillcolor"),
                        if object_type == "Button" {
                            "FFECECEC"
                        } else {
                            "FFFFFFFF"
                        },
                    ))
                },
                gradient: None,
                line: if node.attr("stroked") == Some("f") {
                    None
                } else {
                    Some(vml_color(node.attr("strokecolor"), "FF000000"))
                },
                line_width,
                dash: "solid".into(),
                head: "none".into(),
                tail: "none".into(),
                paragraphs: vec![TextParagraph {
                    align: match align {
                        "Center" => "ctr",
                        "Right" => "r",
                        _ => "l",
                    }
                    .into(),
                    runs,
                }],
                vertical: match vertical {
                    "Center" => "ctr",
                    "Bottom" => "b",
                    _ => "t",
                }
                .into(),
                text_rotation: 0.0,
                margins: [3.0; 4],
                wrap: true,
                paths: vec![],
            })),
        });
    }
    Ok(out)
}

fn supported_geometry(name: &str) -> bool {
    matches!(
        name,
        "rect"
            | "roundRect"
            | "ellipse"
            | "line"
            | "straightConnector1"
            | "bentConnector2"
            | "bentConnector3"
            | "triangle"
            | "rtTriangle"
            | "diamond"
            | "parallelogram"
            | "trapezoid"
            | "pentagon"
            | "hexagon"
            | "heptagon"
            | "octagon"
            | "decagon"
            | "dodecagon"
            | "star4"
            | "star5"
            | "star6"
            | "star8"
            | "chevron"
            | "rightArrow"
            | "leftArrow"
            | "upArrow"
            | "downArrow"
            | "leftRightArrow"
            | "upDownArrow"
            | "plus"
            | "flowChartProcess"
            | "flowChartDecision"
            | "flowChartTerminator"
            | "flowChartInputOutput"
            | "flowChartConnector"
    )
}
fn polygon(name: &str) -> (Vec<[f32; 2]>, bool) {
    let regular = |n: usize| -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                [0.5 + 0.5 * angle.cos(), 0.5 + 0.5 * angle.sin()]
            })
            .collect()
    };
    let star = |n: usize| -> Vec<[f32; 2]> {
        (0..n * 2)
            .map(|i| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / n as f32;
                let r = if i % 2 == 0 { 0.5 } else { 0.22 };
                [0.5 + r * angle.cos(), 0.5 + r * angle.sin()]
            })
            .collect()
    };
    let points = match name {
        "line" | "straightConnector1" => return (vec![[0.0, 0.0], [1.0, 1.0]], false),
        "bentConnector2" => return (vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], false),
        "bentConnector3" => return (vec![[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [1.0, 1.0]], false),
        "ellipse" | "flowChartConnector" => regular(64),
        "roundRect" | "flowChartTerminator" => {
            let mut p = vec![];
            for (cx, cy, angle) in [
                (0.85, 0.15, -90.0_f32),
                (0.85, 0.85, 0.0),
                (0.15, 0.85, 90.0),
                (0.15, 0.15, 180.0),
            ] {
                for i in 0..=8 {
                    let a = (angle + i as f32 * 90.0 / 8.0).to_radians();
                    p.push([cx + 0.15 * a.cos(), cy + 0.15 * a.sin()])
                }
            }
            p
        }
        "triangle" => vec![[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]],
        "rtTriangle" => vec![[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        "diamond" | "flowChartDecision" => vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]],
        "parallelogram" | "flowChartInputOutput" => {
            vec![[0.2, 0.0], [1.0, 0.0], [0.8, 1.0], [0.0, 1.0]]
        }
        "trapezoid" => vec![[0.2, 0.0], [0.8, 0.0], [1.0, 1.0], [0.0, 1.0]],
        "pentagon" => regular(5),
        "hexagon" => regular(6),
        "heptagon" => regular(7),
        "octagon" => regular(8),
        "decagon" => regular(10),
        "dodecagon" => regular(12),
        "star4" => star(4),
        "star5" => star(5),
        "star6" => star(6),
        "star8" => star(8),
        "chevron" => vec![
            [0.0, 0.0],
            [0.7, 0.0],
            [1.0, 0.5],
            [0.7, 1.0],
            [0.0, 1.0],
            [0.3, 0.5],
        ],
        "rightArrow" | "leftArrow" | "upArrow" | "downArrow" => {
            let p = vec![
                [0.0, 0.25],
                [0.65, 0.25],
                [0.65, 0.0],
                [1.0, 0.5],
                [0.65, 1.0],
                [0.65, 0.75],
                [0.0, 0.75],
            ];
            p.into_iter()
                .map(|[x, y]| match name {
                    "leftArrow" => [1.0 - x, y],
                    "upArrow" => [y, 1.0 - x],
                    "downArrow" => [y, x],
                    _ => [x, y],
                })
                .collect()
        }
        "leftRightArrow" | "upDownArrow" => {
            let p = vec![
                [0.0, 0.5],
                [0.3, 0.0],
                [0.3, 0.25],
                [0.7, 0.25],
                [0.7, 0.0],
                [1.0, 0.5],
                [0.7, 1.0],
                [0.7, 0.75],
                [0.3, 0.75],
                [0.3, 1.0],
            ];
            p.into_iter()
                .map(|[x, y]| {
                    if name == "upDownArrow" {
                        [y, x]
                    } else {
                        [x, y]
                    }
                })
                .collect()
        }
        "plus" => vec![
            [0.35, 0.0],
            [0.65, 0.0],
            [0.65, 0.35],
            [1.0, 0.35],
            [1.0, 0.65],
            [0.65, 0.65],
            [0.65, 1.0],
            [0.35, 1.0],
            [0.35, 0.65],
            [0.0, 0.65],
            [0.0, 0.35],
            [0.35, 0.35],
        ],
        _ => vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    };
    (points, true)
}
fn rgba(hex: Option<&str>, fallback: Color32) -> Color32 {
    let Some(hex) = hex else { return fallback };
    let hex = hex.trim_start_matches('#');
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return fallback;
    }
    let part = |i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    match hex.len() {
        8 => Color32::from_rgba_unmultiplied(part(2), part(4), part(6), part(0)),
        6 => Color32::from_rgb(part(0), part(2), part(4)),
        _ => fallback,
    }
}
fn transform(point: Pos2, center: Pos2, rotation: f32, flip_h: bool, flip_v: bool) -> Pos2 {
    let mut delta = point - center;
    if flip_h {
        delta.x = -delta.x
    }
    if flip_v {
        delta.y = -delta.y
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    center + Vec2::new(delta.x * cos - delta.y * sin, delta.x * sin + delta.y * cos)
}
fn triangles(points: &[Pos2]) -> Vec<[u32; 3]> {
    let mut ids: Vec<usize> = (0..points.len()).collect();
    let mut out = vec![];
    let cross = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let area: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum();
    if area < 0.0 {
        ids.reverse()
    }
    for _ in 0..points.len().saturating_mul(points.len()) {
        if ids.len() < 3 {
            break;
        }
        let mut clipped = false;
        for i in 0..ids.len() {
            let a = ids[(i + ids.len() - 1) % ids.len()];
            let b = ids[i];
            let c = ids[(i + 1) % ids.len()];
            if cross(points[a], points[b], points[c]) <= 0.00001 {
                continue;
            }
            let occupied = ids.iter().any(|&j| {
                j != a
                    && j != b
                    && j != c
                    && cross(points[a], points[b], points[j]) >= 0.0
                    && cross(points[b], points[c], points[j]) >= 0.0
                    && cross(points[c], points[a], points[j]) >= 0.0
            });
            if !occupied {
                out.push([a as u32, b as u32, c as u32]);
                ids.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    out
}
fn filled_path(p: &egui::Painter, points: &[Pos2], fill: Color32) {
    if fill == Color32::TRANSPARENT || points.len() < 3 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for &point in points {
        mesh.colored_vertex(point, fill)
    }
    for tri in triangles(points) {
        mesh.add_triangle(tri[0], tri[1], tri[2])
    }
    p.add(egui::Shape::mesh(mesh));
}
fn gradient_color(gradient: &GradientFill, t: f32) -> Color32 {
    let Some(first) = gradient.stops.first() else {
        return Color32::TRANSPARENT;
    };
    if t <= first.position {
        return rgba(Some(&first.color), Color32::TRANSPARENT);
    }
    for pair in gradient.stops.windows(2) {
        if t <= pair[1].position {
            let a = rgba(Some(&pair[0].color), Color32::TRANSPARENT);
            let b = rgba(Some(&pair[1].color), Color32::TRANSPARENT);
            let mix = ((t - pair[0].position)
                / (pair[1].position - pair[0].position).max(0.000001))
            .clamp(0.0, 1.0);
            let a = a.to_srgba_unmultiplied();
            let b = b.to_srgba_unmultiplied();
            let channel =
                |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * mix).round() as u8;
            return Color32::from_rgba_unmultiplied(channel(0), channel(1), channel(2), channel(3));
        }
    }
    rgba(
        gradient.stops.last().map(|s| s.color.as_str()),
        Color32::TRANSPARENT,
    )
}
fn gradient_clip(poly: &[(Pos2, f32)], limit: f32, above: bool) -> Vec<(Pos2, f32)> {
    let mut out = vec![];
    if poly.is_empty() {
        return out;
    }
    let mut prev = poly[poly.len() - 1];
    let inside = |t: f32| if above { t >= limit } else { t <= limit };
    for &point in poly {
        if inside(prev.1) != inside(point.1) {
            let mix = (limit - prev.1) / (point.1 - prev.1);
            out.push((prev.0 + (point.0 - prev.0) * mix, limit));
        }
        if inside(point.1) {
            out.push(point)
        }
        prev = point;
    }
    out
}
fn filled_gradient(
    p: &egui::Painter,
    points: &[Pos2],
    local: &[[f32; 2]],
    gradient: &GradientFill,
    rect: Rect,
    pose: Pose,
) {
    if gradient.stops.is_empty() || points.len() < 3 {
        return;
    }
    let angle = (gradient.angle
        - if gradient.rotate_with_shape {
            0.0
        } else {
            pose.rotation
        })
    .to_radians();
    let mut direction = Vec2::new(angle.cos(), angle.sin());
    if gradient.scaled {
        direction.x *= rect.width();
        direction.y *= rect.height();
    }
    direction = direction.normalized();
    let corner_values = [
        0.0,
        direction.x * rect.width(),
        direction.y * rect.height(),
        direction.x * rect.width() + direction.y * rect.height(),
    ];
    let min = corner_values.into_iter().fold(f32::INFINITY, f32::min);
    let max = corner_values.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let values: Vec<_> = local
        .iter()
        .map(|[x, y]| {
            ((x * rect.width() * direction.x + y * rect.height() * direction.y - min)
                / (max - min).max(0.0001))
            .clamp(0.0, 1.0)
        })
        .collect();
    let mut breaks = vec![0.0];
    breaks.extend(gradient.stops.iter().map(|s| s.position));
    breaks.push(1.0);
    breaks.sort_by(f32::total_cmp);
    breaks.dedup();
    let mut mesh = egui::Mesh::default();
    for triangle in triangles(points) {
        let triangle: Vec<_> = triangle
            .into_iter()
            .map(|i| (points[i as usize], values[i as usize]))
            .collect();
        for band in breaks.windows(2) {
            let poly = gradient_clip(&gradient_clip(&triangle, band[0], true), band[1], false);
            if poly.len() < 3 {
                continue;
            }
            let start = mesh.vertices.len() as u32;
            for &(pos, t) in &poly {
                mesh.colored_vertex(pos, gradient_color(gradient, t));
            }
            for i in 1..poly.len() - 1 {
                mesh.add_triangle(start, start + i as u32, start + i as u32 + 1);
            }
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

fn stroke_path(p: &egui::Painter, points: &[Pos2], closed: bool, stroke: Stroke, dash: &str) {
    if stroke.width <= 0.0 || stroke.color == Color32::TRANSPARENT || points.len() < 2 {
        return;
    }
    if dash == "solid" {
        p.add(egui::epaint::PathShape {
            points: points.to_vec(),
            closed,
            fill: Color32::TRANSPARENT,
            stroke: stroke.into(),
        });
        return;
    }
    let length = if dash.to_ascii_lowercase().contains("dot") {
        stroke.width.max(1.0)
    } else {
        stroke.width.max(1.0) * 4.0
    };
    let gap = stroke.width.max(1.0) * 2.5;
    let clip = p.clip_rect().expand(stroke.width * 2.0);
    for i in 0..points.len() - usize::from(!closed) {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let d = b - a;
        let len = d.length();
        if len < 0.01 {
            continue;
        }
        let direction = d / len;
        // Keep work bounded even for huge offscreen objects.
        let mut t0 = 0.0_f32;
        let mut t1 = 1.0_f32;
        for (coord, delta, min, max) in [
            (a.x, d.x, clip.left(), clip.right()),
            (a.y, d.y, clip.top(), clip.bottom()),
        ] {
            if delta.abs() < 0.00001 {
                if coord < min || coord > max {
                    t1 = -1.0;
                    break;
                }
            } else {
                let u = (min - coord) / delta;
                let v = (max - coord) / delta;
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v))
            }
        }
        if t1 < t0 {
            continue;
        }
        let mut cursor = ((t0 * len) / (length + gap)).floor() * (length + gap);
        let end = (t1 * len).min(len);
        let mut segments = 0;
        while cursor < end && segments < 10000 {
            let start = cursor.max(t0 * len);
            let stop = (cursor + length).min(end);
            if stop > start {
                p.line_segment([a + direction * start, a + direction * stop], stroke);
            }
            cursor += length + gap;
            segments += 1;
        }
    }
}
fn arrow(p: &egui::Painter, tip: Pos2, previous: Pos2, kind: &str, stroke: Stroke, z: f32) {
    if kind == "none" || stroke.color == Color32::TRANSPARENT {
        return;
    }
    let direction = (tip - previous).normalized();
    let normal = Vec2::new(-direction.y, direction.x);
    let length = (stroke.width * 5.0).max(7.0 * z);
    let width = length * 0.4;
    let points = match kind {
        "diamond" => vec![
            tip,
            tip - direction * length * 0.5 + normal * width,
            tip - direction * length,
            tip - direction * length * 0.5 - normal * width,
        ],
        "oval" => (0..24)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 24.0;
                tip - direction * length * 0.5
                    + direction * (a.cos() * length * 0.5)
                    + normal * (a.sin() * width)
            })
            .collect(),
        _ => vec![
            tip - direction * length + normal * width,
            tip,
            tip - direction * length - normal * width,
        ],
    };
    if kind == "arrow" {
        stroke_path(p, &points, false, stroke, "solid")
    } else {
        filled_path(p, &points, stroke.color)
    }
}
#[derive(Default)]
pub struct DrawingRenderer {
    textures: HashMap<String, Option<egui::TextureHandle>>,
}
#[derive(Clone, Copy)]
struct Pose {
    rotation: f32,
    flip_h: bool,
    flip_v: bool,
}
impl DrawingRenderer {
    pub fn clear(&mut self) {
        self.textures.clear()
    }
    pub fn paint(&mut self, p: &egui::Painter, rect: Rect, object: &DrawingObject, zoom: f32) {
        self.paint_with_fonts(p, rect, object, zoom, &|_| egui::FontFamily::Proportional)
    }
    pub fn paint_with_fonts(
        &mut self,
        p: &egui::Painter,
        rect: Rect,
        object: &DrawingObject,
        zoom: f32,
        resolve_font: &dyn Fn(&TextRun) -> egui::FontFamily,
    ) {
        self.paint_transformed(
            p,
            rect,
            object,
            zoom,
            resolve_font,
            Pose {
                rotation: object.rotation,
                flip_h: object.flip_h,
                flip_v: object.flip_v,
            },
        );
    }
    fn paint_transformed(
        &mut self,
        p: &egui::Painter,
        rect: Rect,
        object: &DrawingObject,
        zoom: f32,
        resolve_font: &dyn Fn(&TextRun) -> egui::FontFamily,
        pose: Pose,
    ) {
        if !rect.is_finite() || rect.width().abs() > 1e8 || rect.height().abs() > 1e8 {
            return;
        }
        // Rotated objects can extend beyond their unrotated rectangle.
        if !p
            .clip_rect()
            .intersects(rect.expand(rect.size().length() * 0.5))
        {
            return;
        }
        let z = zoom * POINT_SCALE;
        let point = |xy: [f32; 2]| {
            transform(
                rect.min + Vec2::new(xy[0] * rect.width(), xy[1] * rect.height()),
                rect.center(),
                pose.rotation,
                pose.flip_h,
                pose.flip_v,
            )
        };
        match &object.content {
            DrawingContent::Shape(shape) => {
                let fill = rgba(shape.fill.as_deref(), Color32::TRANSPARENT);
                let stroke = Stroke::new(
                    shape.line_width.clamp(0.0, 1000.0) * z,
                    rgba(shape.line.as_deref(), Color32::TRANSPARENT),
                );
                let paths = if shape.paths.is_empty() {
                    let (mut points, closed) = polygon(&shape.geometry);
                    if matches!(shape.geometry.as_str(), "roundRect" | "flowChartTerminator") {
                        let radius = rect.width().min(rect.height()) / 6.0;
                        for [x, y] in &mut points {
                            *x = if *x < 0.5 {
                                (*x / 0.15) * radius / rect.width().max(1.0)
                            } else {
                                1.0 - ((1.0 - *x) / 0.15) * radius / rect.width().max(1.0)
                            };
                            *y = if *y < 0.5 {
                                (*y / 0.15) * radius / rect.height().max(1.0)
                            } else {
                                1.0 - ((1.0 - *y) / 0.15) * radius / rect.height().max(1.0)
                            };
                        }
                    }
                    vec![ShapePath {
                        points,
                        closed,
                        fill: true,
                        stroke: true,
                    }]
                } else {
                    shape.paths.clone()
                };
                for path in paths {
                    let points: Vec<_> = path.points.iter().copied().map(point).collect();
                    if path.fill && path.closed {
                        if let Some(gradient) = &shape.gradient {
                            filled_gradient(p, &points, &path.points, gradient, rect, pose);
                        } else {
                            filled_path(p, &points, fill);
                        }
                    }
                    if path.stroke {
                        stroke_path(p, &points, path.closed, stroke, &shape.dash)
                    }
                    if !path.closed && points.len() > 1 {
                        arrow(p, points[0], points[1], &shape.head, stroke, z);
                        arrow(
                            p,
                            points[points.len() - 1],
                            points[points.len() - 2],
                            &shape.tail,
                            stroke,
                            z,
                        )
                    }
                }
                self.text(p, rect, pose.rotation, shape, z, resolve_font);
            }
            DrawingContent::Picture(pic) => {
                // The cache is reset on workbook/sheet changes; source part is unique inside a workbook.
                let texture = self.textures.entry(pic.source.clone()).or_insert_with(|| {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&pic.base64)
                        .ok()?;
                    if bytes.len() > MAX_MEDIA as usize {
                        return None;
                    }
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()
                        .ok()?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(16384);
                    limits.max_image_height = Some(16384);
                    limits.max_alloc = Some(96 * 1024 * 1024);
                    reader.limits(limits);
                    let image = reader.decode().ok()?.to_rgba8();
                    if u64::from(image.width()) * u64::from(image.height()) > 16_000_000 {
                        return None;
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    Some(
                        p.ctx()
                            .load_texture(&pic.source, image, egui::TextureOptions::LINEAR),
                    )
                });
                if let Some(texture) = texture {
                    let mut mesh = egui::Mesh::with_texture(texture.id());
                    let uv = [
                        [pic.crop[0], pic.crop[1]],
                        [1.0 - pic.crop[2], pic.crop[1]],
                        [1.0 - pic.crop[2], 1.0 - pic.crop[3]],
                        [pic.crop[0], 1.0 - pic.crop[3]],
                    ];
                    for (i, xy) in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
                        .into_iter()
                        .enumerate()
                    {
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: point(xy),
                            uv: Pos2::new(uv[i][0], uv[i][1]),
                            color: Color32::WHITE,
                        })
                    }
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    p.add(egui::Shape::mesh(mesh));
                }
            }
            DrawingContent::Chart(chart) => charts::paint_chart(p, rect, chart, zoom),
            DrawingContent::Group { children } => {
                for child in children {
                    let [x, y, w, h] = child.rect;
                    let center = point([x + w * 0.5, y + h * 0.5]);
                    let child_rect = Rect::from_center_size(
                        center,
                        Vec2::new(w * rect.width(), h * rect.height()),
                    );
                    let rotation = pose.rotation
                        + if pose.flip_h ^ pose.flip_v {
                            -child.object.rotation
                        } else {
                            child.object.rotation
                        };
                    self.paint_transformed(
                        p,
                        child_rect,
                        &child.object,
                        zoom,
                        resolve_font,
                        Pose {
                            rotation,
                            flip_h: pose.flip_h ^ child.object.flip_h,
                            flip_v: pose.flip_v ^ child.object.flip_v,
                        },
                    );
                }
            }
        }
    }
    fn text(
        &self,
        p: &egui::Painter,
        rect: Rect,
        rotation: f32,
        shape: &ShapeData,
        z: f32,
        resolve_font: &dyn Fn(&TextRun) -> egui::FontFamily,
    ) {
        if shape.paragraphs.is_empty() {
            return;
        }
        // Preset geometry supplies an inscribed text rectangle before body margins.
        let text_rect = match shape.geometry.as_str() {
            "diamond" => rect.shrink2(rect.size() * 0.25),
            "ellipse" => rect.shrink2(rect.size() * (1.0 - std::f32::consts::FRAC_1_SQRT_2) * 0.5),
            _ => rect,
        };
        let inner = Rect::from_min_max(
            text_rect.min + Vec2::new(shape.margins[0] * z, shape.margins[1] * z),
            text_rect.max - Vec2::new(shape.margins[2] * z, shape.margins[3] * z),
        );
        if !inner.is_positive() {
            return;
        }
        let mut paragraphs = vec![];
        for paragraph in &shape.paragraphs {
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = if shape.wrap {
                inner.width()
            } else {
                f32::INFINITY
            };
            job.halign = match paragraph.align.as_str() {
                "ctr" => egui::Align::Center,
                "r" => egui::Align::RIGHT,
                _ => egui::Align::LEFT,
            };
            for run in &paragraph.runs {
                let color = rgba(Some(&run.color), Color32::BLACK);
                job.append(
                    &run.text,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::new(run.size.clamp(1.0, 400.0) * z, resolve_font(run)),
                        color,
                        italics: run.italic,
                        underline: if run.underline {
                            Stroke::new(z.max(0.5), color)
                        } else {
                            Stroke::NONE
                        },
                        ..Default::default()
                    },
                )
            }
            if job.text.is_empty() {
                job.append(
                    " ",
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(11.0 * z),
                        ..Default::default()
                    },
                )
            }
            paragraphs.push((p.layout_job(job), paragraph.align.as_str()));
        }
        let total: f32 = paragraphs.iter().map(|(g, _)| g.size().y).sum();
        let mut y = match shape.vertical.as_str() {
            "ctr" => inner.center().y - total * 0.5,
            "b" => inner.bottom() - total,
            _ => inner.top(),
        };
        let angle = (rotation + shape.text_rotation).to_radians();
        // Shape text stays readable when the shape geometry is flipped, as Excel's default body behavior.
        let rotate = |pos: Pos2| {
            transform(
                pos,
                rect.center(),
                rotation + shape.text_rotation,
                false,
                false,
            )
        };
        for (galley, align) in paragraphs {
            let x = match align {
                "ctr" => inner.center().x,
                "r" => inner.right(),
                _ => inner.left(),
            };
            let origin = Pos2::new(x, y);
            p.add(
                egui::epaint::TextShape::new(rotate(origin), galley.clone(), Color32::BLACK)
                    .with_angle(angle),
            );
            y += galley.size().y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchors_are_one_based_and_keep_emu_offsets() {
        let n=ooxml::parse(r#"<twoCellAnchor><from><col>2</col><colOff>12700</colOff><row>4</row><rowOff>25400</rowOff></from><to><col>7</col><colOff>0</colOff><row>12</row><rowOff>0</rowOff></to></twoCellAnchor>"#).unwrap();
        match anchor(&n).unwrap() {
            DrawingAnchor::TwoCell { from, to } => {
                assert_eq!((from.row, from.col), (5, 3));
                assert_eq!(from.row_offset_emu, 25400.0);
                assert_eq!((to.row, to.col), (13, 8))
            }
            _ => panic!("wrong anchor"),
        }
    }
    #[test]
    fn rich_shape_text_and_theme_colors_survive() {
        let n=ooxml::parse(r#"<sp><spPr><prstGeom prst="roundRect"/><solidFill><schemeClr val="accent1"><alpha val="50000"/></schemeClr></solidFill><ln w="25400"><solidFill><srgbClr val="112233"/></solidFill></ln></spPr><txBody><bodyPr anchor="ctr"/><p><pPr algn="ctr"/><r><rPr sz="1600" b="1"><solidFill><srgbClr val="FFFFFF"/></solidFill></rPr><t>Hello &amp; Excel</t></r></p></txBody></sp>"#).unwrap();
        let s = shape(&n, &Theme::default(), &mut vec![]);
        assert_eq!(s.geometry, "roundRect");
        assert_eq!(s.fill.as_deref(), Some("804472C4"));
        assert_eq!(s.line_width, 2.0);
        assert_eq!(s.paragraphs[0].runs[0].text, "Hello & Excel");
        assert!(s.paragraphs[0].runs[0].bold);
    }
    #[test]
    fn arrow_geometry_is_triangulated_without_filling_notches() {
        let (points, _) = polygon("rightArrow");
        let points: Vec<_> = points.into_iter().map(|[x, y]| Pos2::new(x, y)).collect();
        let tris = triangles(&points);
        assert_eq!(tris.len(), 5);
        let area: f32 = tris
            .iter()
            .map(|[a, b, c]| {
                let a = points[*a as usize];
                let b = points[*b as usize];
                let c = points[*c as usize];
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() * 0.5
            })
            .sum();
        assert!((area - 0.5).abs() < 1e-5);
    }
    #[test]
    fn legacy_vml_buttons_have_static_text_and_point_anchors() {
        let xml = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:x="urn:schemas-microsoft-com:office:excel"><v:shape id="Button 1" type="#_x0000_t201" style="position:absolute;margin-left:72pt;margin-top:144pt;width:100pt;height:28pt" fillcolor="buttonface [67]" strokecolor="#223344"><v:textbox><div><b><font face="Arial" color="#112233">Refresh &amp; inspect</font></b></div></v:textbox><x:ClientData ObjectType="Button"><x:TextHAlign>Center</x:TextHAlign><x:TextVAlign>Center</x:TextVAlign></x:ClientData></v:shape><v:shape id="Hidden comment" type="#_x0000_t202" style="visibility:hidden;width:10pt;height:10pt"><x:ClientData ObjectType="Note"/></v:shape></xml>"##;
        let mut warnings = vec![];
        let xml = xml.replace("><", ">\n  <");
        let objects = read_vml(&xml, &mut warnings).unwrap();
        assert_eq!(objects.len(), 1);
        assert!(warnings.is_empty());
        let DrawingAnchor::Absolute { x_emu, y_emu, .. } = objects[0].anchor else {
            panic!("VML lost its point anchor")
        };
        assert_eq!(x_emu, 914400.0);
        assert_eq!(y_emu, 1828800.0);
        let DrawingContent::Shape(s) = &objects[0].content else {
            unreachable!()
        };
        assert_eq!(s.fill.as_deref(), Some("FFECECEC"));
        assert_eq!(s.paragraphs[0].align, "ctr");
        assert_eq!(s.paragraphs[0].runs[0].text, "Refresh & inspect");
        assert!(s.paragraphs[0].runs[0].bold);
    }
    #[test]
    fn external_picture_relationships_are_never_followed() {
        let file = tempfile::tempfile().unwrap();
        let file = zip::ZipWriter::new(file).finish().unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        let theme = Theme::default();
        let mut state = ReadState {
            zip: &mut zip,
            theme: &theme,
            warnings: vec![],
            objects: 0,
            media_bytes: 0,
        };
        let mut rels = HashMap::new();
        rels.insert(
            "rId1".into(),
            Relationship {
                target: "http://127.0.0.1/private.png".into(),
                kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"
                    .into(),
                external: true,
            },
        );
        let node=ooxml::parse(r#"<pic><blipFill><blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="rId1"/></blipFill></pic>"#).unwrap();
        assert!(
            state
                .object(
                    &node,
                    DrawingAnchor::Absolute {
                        x_emu: 0.0,
                        y_emu: 0.0,
                        width_emu: 12700.0,
                        height_emu: 12700.0
                    },
                    &rels,
                    0
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(state.media_bytes, 0);
        assert_eq!(state.warnings.len(), 1);
    }
    #[test]
    fn linear_gradient_keeps_intermediate_stops_and_alpha() {
        let n=ooxml::parse(r#"<sp><spPr><prstGeom prst="rect"/><gradFill rotWithShape="1"><gsLst><gs pos="0"><srgbClr val="FF0000"/></gs><gs pos="50000"><srgbClr val="00FF00"><alpha val="50000"/></srgbClr></gs><gs pos="100000"><srgbClr val="0000FF"/></gs></gsLst><lin ang="5400000" scaled="1"/></gradFill></spPr></sp>"#).unwrap();
        let mut warnings = vec![];
        let shape = shape(&n, &Theme::default(), &mut warnings);
        let gradient = shape.gradient.unwrap();
        assert_eq!(gradient.angle, 90.0);
        assert!(gradient.scaled);
        assert!(warnings.is_empty());
        assert_eq!(gradient.stops.len(), 3);
        assert_eq!(gradient_color(&gradient, 0.0), Color32::RED);
        let middle = gradient_color(&gradient, 0.5).to_srgba_unmultiplied();
        assert_eq!(middle[3], 128);
        assert_eq!(middle[1], 255);
        assert_eq!(gradient_color(&gradient, 1.0), Color32::BLUE);
    }
    #[test]
    fn grouped_shapes_keep_child_coordinates_and_rotation() {
        let file = tempfile::tempfile().unwrap();
        let file = zip::ZipWriter::new(file).finish().unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        let theme = Theme::default();
        let mut state = ReadState {
            zip: &mut zip,
            theme: &theme,
            warnings: vec![],
            objects: 0,
            media_bytes: 0,
        };
        let node=ooxml::parse(r#"<grpSp><grpSpPr><xfrm rot="5400000"><off x="0" y="0"/><ext cx="1000" cy="500"/><chOff x="100" y="200"/><chExt cx="2000" cy="1000"/></xfrm></grpSpPr><sp><spPr><xfrm><off x="600" y="450"/><ext cx="500" cy="500"/></xfrm><prstGeom prst="ellipse"/><solidFill><srgbClr val="00FF00"/></solidFill></spPr></sp></grpSp>"#).unwrap();
        let object = state
            .object(
                &node,
                DrawingAnchor::Absolute {
                    x_emu: 0.0,
                    y_emu: 0.0,
                    width_emu: 1000.0,
                    height_emu: 500.0,
                },
                &HashMap::new(),
                0,
            )
            .unwrap()
            .unwrap();
        assert_eq!(object.rotation, 90.0);
        let DrawingContent::Group { children } = object.content else {
            panic!("Group was flattened")
        };
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].rect, [0.25, 0.25, 0.25, 0.5]);
        let DrawingContent::Shape(shape) = &children[0].object.content else {
            unreachable!()
        };
        assert_eq!(shape.geometry, "ellipse");
        assert_eq!(shape.fill.as_deref(), Some("FF00FF00"));
    }
    #[test]
    fn drawing_tint_retains_the_stated_fraction_of_the_input_color() {
        let n = ooxml::parse(
            r#"<solidFill><srgbClr val="000000"><tint val="20000"/></srgbClr></solidFill>"#,
        )
        .unwrap();
        assert_eq!(color(&n, &Theme::default()).as_deref(), Some("FFCCCCCC"));
    }
    #[test]
    fn alternate_content_selects_one_branch_at_anchor_object_and_group_levels() {
        use std::io::Write;
        let drawing = r#"<wsDr xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <mc:AlternateContent><mc:Choice Requires="a14"><ignoredChoiceAnchor/></mc:Choice><mc:Fallback>
            <absoluteAnchor><pos x="0" y="0"/><ext cx="1270000" cy="1270000"/>
                <mc:AlternateContent><mc:Choice Requires="a14"><ignoredChoiceShape/></mc:Choice><mc:Fallback>
                <grpSp><grpSpPr><xfrm><chOff x="0" y="0"/><chExt cx="100" cy="100"/></xfrm></grpSpPr>
                    <mc:AlternateContent><mc:Choice Requires="a14"><ignoredChoiceGroupChild/></mc:Choice><mc:Fallback>
                    <sp><spPr><prstGeom prst="ellipse"/></spPr></sp>
                    </mc:Fallback></mc:AlternateContent><unsupportedGroupObject/>
                </grpSp>
                </mc:Fallback></mc:AlternateContent><contentPart/><clientData/>
            </absoluteAnchor></mc:Fallback></mc:AlternateContent><unsupportedAnchor/>
            <mc:AlternateContent><mc:Choice Requires="a"><absoluteAnchor><pos x="0" y="0"/><ext cx="12700" cy="12700"/><sp><spPr><prstGeom prst="rect"/></spPr></sp></absoluteAnchor></mc:Choice><mc:Choice Requires="b"><ignoredSecondChoice/></mc:Choice></mc:AlternateContent>
        </wsDr>"#;
        let file = tempfile::tempfile().unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, content) in [
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Target="../drawings/drawing1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing"/></Relationships>"#,
            ),
            ("xl/drawings/drawing1.xml", drawing),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        let mut zip = ZipArchive::new(writer.finish().unwrap()).unwrap();
        let (objects,warnings)=read_sheet_drawings(&mut zip,"xl/worksheets/sheet1.xml",r#"<worksheet xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><drawing r:id="rId1"/></worksheet>"#,&Theme::default()).unwrap();
        assert_eq!(objects.len(), 2);
        let DrawingContent::Group { children } = &objects[0].content else {
            panic!("Fallback group missing")
        };
        assert_eq!(children.len(), 1);
        let DrawingContent::Shape(shape) = &children[0].object.content else {
            panic!("Fallback child missing")
        };
        assert_eq!(shape.geometry, "ellipse");
        assert!(matches!(objects[1].content, DrawingContent::Shape(_)));
        assert_eq!(warnings.len(), 3);
        assert!(warnings.iter().any(|w| w.contains("contentPart")));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unsupportedGroupObject"))
        );
        assert!(warnings.iter().any(|w| w.contains("unsupportedAnchor")));
        assert!(warnings.iter().all(|w| !w.contains("ignored")));
    }
}
