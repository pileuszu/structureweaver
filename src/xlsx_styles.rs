//! Resolve SpreadsheetML styles and DrawingML theme colors into portable visual metadata.
use crate::domain::*;
use anyhow::{Context, Result, ensure};
use quick_xml::{Reader, events::Event};
use std::collections::HashMap;

#[derive(Default, Clone)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
}
impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|node| node.name == name)
    }
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    fn enabled(&self, key: &str) -> bool {
        matches!(self.attr(key), Some("1" | "true"))
    }
    fn index(&self, key: &str) -> Option<usize> {
        self.attr(key).and_then(|value| value.parse().ok())
    }
}
fn parse(xml: &str) -> Result<Node> {
    let mut reader = Reader::from_str(xml);
    let mut stack = vec![Node::default()];
    loop {
        let event = reader.read_event()?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name();
                let name = name
                    .as_ref()
                    .rsplit(|&b| b == b':')
                    .next()
                    .unwrap_or_default();
                let node = Node {
                    name: std::str::from_utf8(name)?.to_owned(),
                    attrs: element
                        .attributes()
                        .map(|a| {
                            let a = a?;
                            Ok((
                                std::str::from_utf8(a.key.as_ref())?.to_owned(),
                                quick_xml::escape::unescape(std::str::from_utf8(&a.value)?)?
                                    .into_owned(),
                            ))
                        })
                        .collect::<Result<_>>()?,
                    children: vec![],
                };
                if empty {
                    stack
                        .last_mut()
                        .context("Missing XML root")?
                        .children
                        .push(node);
                } else {
                    ensure!(stack.len() < 128, "Style XML nesting limit exceeded");
                    stack.push(node);
                }
            }
            Event::End(_) => {
                ensure!(stack.len() > 1, "Unbalanced style XML");
                let node = stack.pop().context("Missing XML node")?;
                stack
                    .last_mut()
                    .context("Missing XML parent")?
                    .children
                    .push(node);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.len() == 1, "Unclosed style XML element");
    stack
        .pop()
        .context("Missing style XML")?
        .children
        .into_iter()
        .next()
        .context("Empty style XML")
}

pub(crate) struct Theme {
    colors: Vec<String>,
    major_font: String,
    minor_font: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: [
                "FFFFFF", "000000", "E7E6E6", "44546A", "4472C4", "ED7D31", "A5A5A5", "FFC000",
                "5B9BD5", "70AD47", "0563C1", "954F72",
            ]
            .into_iter()
            .map(|s| format!("FF{s}"))
            .collect(),
            major_font: "Calibri Light".into(),
            minor_font: "Calibri".into(),
        }
    }
}
impl Theme {
    pub(crate) fn from_xml(xml: &str) -> Result<Self> {
        let root = parse(xml)?;
        let mut theme = Self::default();
        if let Some(elements) = root.child("themeElements") {
            if let Some(scheme) = elements.child("clrScheme") {
                for (index, name) in [
                    "lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4",
                    "accent5", "accent6", "hlink", "folHlink",
                ]
                .into_iter()
                .enumerate()
                {
                    if let Some(color) = scheme.child(name).and_then(|n| n.children.first()) {
                        let value = color.attr("lastClr").or_else(|| color.attr("val"));
                        if let Some(value) = value.and_then(normalize_rgb) {
                            theme.colors[index] = value;
                        }
                    }
                }
            }
            if let Some(scheme) = elements.child("fontScheme") {
                if let Some(font) = scheme
                    .child("majorFont")
                    .and_then(|n| n.child("latin"))
                    .and_then(|n| n.attr("typeface"))
                {
                    theme.major_font = font.to_owned();
                }
                if let Some(font) = scheme
                    .child("minorFont")
                    .and_then(|n| n.child("latin"))
                    .and_then(|n| n.attr("typeface"))
                {
                    theme.minor_font = font.to_owned();
                }
            }
        }
        Ok(theme)
    }
}
fn normalize_rgb(value: &str) -> Option<String> {
    let value = value.trim_start_matches('#');
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match value.len() {
        6 => Some(format!("FF{}", value.to_ascii_uppercase())),
        8 => Some(value.to_ascii_uppercase()),
        _ => None,
    }
}
const INDEXED_COLORS: [&str; 64] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "000000",
    "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "800000", "008000",
    "000080", "808000", "800080", "008080", "C0C0C0", "808080", "9999FF", "993366", "FFFFCC",
    "CCFFFF", "660066", "FF8080", "0066CC", "CCCCFF", "000080", "FF00FF", "FFFF00", "00FFFF",
    "800080", "800000", "008080", "0000FF", "00CCFF", "CCFFFF", "CCFFCC", "FFFF99", "99CCFF",
    "FF99CC", "CC99FF", "FFCC99", "3366FF", "33CCCC", "99CC00", "FFCC00", "FF9900", "FF6600",
    "666699", "969696", "003366", "339966", "003300", "333300", "993300", "993366", "333399",
    "333333",
];
fn resolve_color(node: &Node, theme: &Theme, indexed: &[String]) -> Option<String> {
    let color = if let Some(rgb) = node.attr("rgb") {
        normalize_rgb(rgb)
    } else if let Some(index) = node.index("theme") {
        theme.colors.get(index).cloned()
    } else if let Some(index) = node.index("indexed") {
        indexed.get(index).cloned()
    } else if node.enabled("auto") {
        Some("FF000000".into())
    } else {
        None
    }?;
    let tint = node
        .attr("tint")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
        .clamp(-1.0, 1.0);
    if tint.abs() < f64::EPSILON {
        return Some(color);
    }
    // SpreadsheetML tint adjusts HLS luminance, not individual RGB channels.
    let rgb: Vec<f64> = (2..8)
        .step_by(2)
        .map(|i| u8::from_str_radix(&color[i..i + 2], 16).unwrap_or(0) as f64 / 255.0)
        .collect();
    let max = rgb.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = rgb.iter().copied().fold(f64::INFINITY, f64::min);
    let old_lum = (max + min) / 2.0;
    let delta = max - min;
    let sat = if delta.abs() < f64::EPSILON {
        0.0
    } else {
        delta / (1.0 - (2.0 * old_lum - 1.0).abs())
    };
    let hue = if delta.abs() < f64::EPSILON {
        0.0
    } else if max == rgb[0] {
        ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    let lum = if tint < 0.0 {
        old_lum * (1.0 + tint)
    } else {
        old_lum * (1.0 - tint) + tint
    };
    let chroma = (1.0 - (2.0 * lum - 1.0).abs()) * sat;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lum - chroma / 2.0;
    Some(format!(
        "{}{:02X}{:02X}{:02X}",
        &color[..2],
        ((r + m) * 255.0 + 1e-9).round() as u8,
        ((g + m) * 255.0 + 1e-9).round() as u8,
        ((b + m) * 255.0 + 1e-9).round() as u8
    ))
}

#[derive(Default)]
pub(crate) struct Styles {
    pub cells: Vec<CellStyle>,
    pub default_style: CellStyle,
}

fn font(node: &Node, theme: &Theme, indexed: &[String]) -> CellStyle {
    let mut style = CellStyle::default();
    let flag = |name| {
        node.child(name)
            .is_some_and(|n| !matches!(n.attr("val"), Some("0" | "false")))
    };
    style.bold = flag("b");
    style.italic = flag("i");
    style.strike = flag("strike");
    style.underline = node
        .child("u")
        .map(|n| n.attr("val").unwrap_or("single").to_owned())
        .unwrap_or_else(|| "none".into());
    style.font_size = node
        .child("sz")
        .and_then(|n| n.attr("val"))
        .and_then(|n| n.parse::<f32>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(11.0);
    style.font_name = node
        .child("name")
        .and_then(|n| n.attr("val"))
        .map(str::to_owned);
    if let Some(scheme) = node.child("scheme").and_then(|n| n.attr("val")) {
        match scheme {
            "major" => style.font_name = Some(theme.major_font.clone()),
            "minor" => style.font_name = Some(theme.minor_font.clone()),
            _ => {}
        }
    }
    style.font_color = node
        .child("color")
        .and_then(|n| resolve_color(n, theme, indexed));
    style
}
fn borders(node: &Node, theme: &Theme, indexed: &[String]) -> CellBorders {
    let side = |name, fallback| {
        let node = node.child(name).or_else(|| node.child(fallback));
        BorderSide {
            style: node
                .and_then(|n| n.attr("style"))
                .unwrap_or("none")
                .to_owned(),
            color: node
                .and_then(|n| n.child("color"))
                .and_then(|n| resolve_color(n, theme, indexed)),
        }
    };
    CellBorders {
        left: side("left", "start"),
        right: side("right", "end"),
        top: side("top", "top"),
        bottom: side("bottom", "bottom"),
        diagonal: side("diagonal", "diagonal"),
        diagonal_up: node.enabled("diagonalUp"),
        diagonal_down: node.enabled("diagonalDown"),
    }
}
fn alignment(node: &Node, style: &mut CellStyle) {
    if let Some(value) = node.attr("horizontal") {
        style.horizontal = value.into();
    }
    if let Some(value) = node.attr("vertical") {
        style.vertical = value.into();
    }
    if let Some(value) = node.attr("indent").and_then(|v| v.parse().ok()) {
        style.indent = value;
    }
    if node.attr("wrapText").is_some() {
        style.wrap_text = node.enabled("wrapText");
    }
    if node.attr("shrinkToFit").is_some() {
        style.shrink_to_fit = node.enabled("shrinkToFit");
    }
    if let Some(value) = node.attr("textRotation").and_then(|v| v.parse().ok()) {
        style.text_rotation = value;
    }
}

pub(crate) fn styles(xml: &str, theme: &Theme) -> Result<Styles> {
    let root = parse(xml)?;
    let mut indexed: Vec<String> = INDEXED_COLORS.iter().map(|c| format!("FF{c}")).collect();
    if let Some(colors) = root.child("colors").and_then(|n| n.child("indexedColors")) {
        for (i, color) in colors.children.iter().enumerate() {
            if let Some(color) = color.attr("rgb").and_then(normalize_rgb) {
                if i < indexed.len() {
                    indexed[i] = color;
                } else {
                    indexed.push(color);
                }
            }
        }
    }
    let fonts: Vec<CellStyle> = root
        .child("fonts")
        .map(|n| {
            n.children
                .iter()
                .map(|n| font(n, theme, &indexed))
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<(String, Option<String>, Option<String>)> = root
        .child("fills")
        .map(|n| {
            n.children
                .iter()
                .map(|n| {
                    if let Some(pattern) = n.child("patternFill") {
                        let kind = pattern.attr("patternType").unwrap_or("none").to_owned();
                        let fg = pattern
                            .child("fgColor")
                            .and_then(|n| resolve_color(n, theme, &indexed));
                        let bg = pattern
                            .child("bgColor")
                            .and_then(|n| resolve_color(n, theme, &indexed));
                        (kind, fg, bg)
                    } else if n.child("gradientFill").is_some() {
                        ("gradient".into(), None, None)
                    } else {
                        ("none".into(), None, None)
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let borders: Vec<CellBorders> = root
        .child("borders")
        .map(|n| {
            n.children
                .iter()
                .map(|n| borders(n, theme, &indexed))
                .collect()
        })
        .unwrap_or_default();
    let number_formats: HashMap<u32, String> = root
        .child("numFmts")
        .map(|n| {
            n.children
                .iter()
                .filter_map(|n| {
                    Some((
                        n.attr("numFmtId")?.parse().ok()?,
                        n.attr("formatCode")?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let resolve_xf = |node: &Node, parent: &CellStyle| {
        let mut style = parent.clone();
        let applies = |flag| !matches!(node.attr(flag), Some("0" | "false"));
        if applies("applyFont")
            && let Some(font) = node.index("fontId").and_then(|i| fonts.get(i))
        {
            style.font_name = font.font_name.clone();
            style.font_size = font.font_size;
            style.bold = font.bold;
            style.italic = font.italic;
            style.underline = font.underline.clone();
            style.strike = font.strike;
            style.font_color = font.font_color.clone();
        }
        if applies("applyFill")
            && let Some((kind, fg, bg)) = node.index("fillId").and_then(|i| fills.get(i))
        {
            style.fill_pattern = kind.clone();
            style.fill = fg.clone();
            style.fill_background = bg.clone();
        }
        if applies("applyBorder")
            && let Some(border) = node.index("borderId").and_then(|i| borders.get(i))
        {
            style.borders = border.clone();
        }
        if applies("applyNumberFormat")
            && let Some(id) = node.attr("numFmtId").and_then(|v| v.parse::<u32>().ok())
        {
            style.number_format = number_formats
                .get(&id)
                .cloned()
                .or_else(|| crate::number_format::builtin_num_format(id));
        }
        if applies("applyAlignment")
            && let Some(node) = node.child("alignment")
        {
            alignment(node, &mut style);
        }
        style
    };
    let plain = fonts.first().cloned().unwrap_or_default();
    let bases: Vec<CellStyle> = root
        .child("cellStyleXfs")
        .map(|n| n.children.iter().map(|n| resolve_xf(n, &plain)).collect())
        .unwrap_or_default();
    let normal_index = root
        .child("cellStyles")
        .and_then(|n| {
            n.children
                .iter()
                .find(|n| n.attr("builtinId") == Some("0") || n.attr("name") == Some("Normal"))
        })
        .and_then(|n| n.index("xfId"))
        .unwrap_or(0);
    let default_style = bases.get(normal_index).cloned().unwrap_or(plain);
    let cells: Vec<CellStyle> = root
        .child("cellXfs")
        .map(|n| {
            n.children
                .iter()
                .map(|n| {
                    let base = n
                        .index("xfId")
                        .and_then(|i| bases.get(i))
                        .unwrap_or(&default_style);
                    resolve_xf(n, base)
                })
                .collect()
        })
        .unwrap_or_else(|| vec![default_style.clone()]);
    // A cell without an explicit s attribute uses cellXfs index 0 (unless a
    // row/column style applies), which may add formatting to the Normal style.
    let default_style = cells.first().cloned().unwrap_or(default_style);
    Ok(Styles {
        cells,
        default_style,
    })
}
