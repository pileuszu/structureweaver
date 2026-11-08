//! Read-only DrawingML charts. Cached values and internal cell references are
//! rendered without evaluating workbook formulas or following external links.
use crate::{
    domain::{CellKind, Document},
    number_format,
    xlsx_styles::{Node, Theme, parse},
};
use anyhow::{Context, Result, ensure};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[path = "chart_3d.rs"]
mod three_d;

const MAX_POINTS: usize = 20_000;
const FALLBACK_COLORS: [&str; 6] = ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartData {
    pub formula: Option<String>,
    pub numbers: Vec<Option<f64>>,
    pub labels: Vec<Option<String>>,
    pub format: String,
    pub hidden: Vec<bool>,
}
impl ChartData {
    fn len(&self) -> usize {
        self.numbers.len().max(self.labels.len())
    }
    fn label(&self, index: usize) -> String {
        self.labels
            .get(index)
            .and_then(Clone::clone)
            .or_else(|| {
                self.numbers
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|v| format_value(v, &self.format))
            })
            .unwrap_or_default()
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DataLabels {
    pub value: bool,
    pub percent: bool,
    pub category: bool,
    pub series: bool,
    pub format: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartSeries {
    pub name: String,
    pub name_reference: Option<String>,
    pub values: ChartData,
    pub categories: ChartData,
    pub x_values: ChartData,
    pub bubble_sizes: ChartData,
    pub color: Option<String>,
    pub line_color: Option<String>,
    pub line_width: f32,
    pub no_line: bool,
    pub marker: String,
    pub marker_size: f32,
    #[serde(deserialize_with = "deserialize_point_colors")]
    pub point_colors: BTreeMap<usize, String>,
    pub labels: DataLabels,
    pub smooth: bool,
    pub shape: Option<String>,
    pub explosion: f32,
}
// Internally tagged DrawingContent buffers JSON keys as strings. Deserialize
// those keys explicitly so saved charts work both directly and inside groups.
fn deserialize_point_colors<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<usize, String>, D::Error> {
    BTreeMap::<String, String>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, color)| {
            key.parse::<usize>()
                .map(|index| (index, color))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartGroup {
    /// bar, line, area, pie, doughnut, scatter or bubble.
    pub kind: String,
    pub horizontal: bool,
    pub grouping: String,
    pub series: Vec<ChartSeries>,
    pub axis_ids: Vec<String>,
    pub gap_width: f32,
    pub overlap: f32,
    pub hole_size: f32,
    pub first_slice_angle: f32,
    pub vary_colors: bool,
    pub scatter_style: String,
    pub three_dimensional: bool,
    pub gap_depth: f32,
    pub shape: String,
    pub wireframe: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartAxis {
    pub id: String,
    pub kind: String,
    pub position: String,
    pub title: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub major_unit: Option<f64>,
    pub log_base: Option<f64>,
    pub reversed: bool,
    pub number_format: String,
    pub major_gridlines: bool,
    pub hidden: bool,
}
/// Office stores layout coordinates as fractions of the complete chart rectangle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartLayout {
    pub target: String,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub x_mode: String,
    pub y_mode: String,
    pub width_mode: String,
    pub height_mode: String,
}
impl Default for ChartLayout {
    fn default() -> Self {
        Self {
            target: "outer".into(),
            x: None,
            y: None,
            width: None,
            height: None,
            x_mode: "factor".into(),
            y_mode: "factor".into(),
            width_mode: "factor".into(),
            height_mode: "factor".into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartView3d {
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub height_percent: f32,
    pub depth_percent: f32,
    pub right_angle_axes: bool,
    pub perspective: f32,
    pub floor_color: Option<String>,
    pub side_wall_color: Option<String>,
    pub back_wall_color: Option<String>,
}
impl Default for ChartView3d {
    fn default() -> Self {
        Self {
            rotation_x: 15.0,
            rotation_y: 20.0,
            height_percent: 100.0,
            depth_percent: 100.0,
            right_angle_axes: true,
            perspective: 30.0,
            floor_color: None,
            side_wall_color: None,
            back_wall_color: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Chart {
    pub title: String,
    pub title_reference: Option<String>,
    pub groups: Vec<ChartGroup>,
    pub axes: Vec<ChartAxis>,
    /// None means the workbook disabled the legend.
    pub legend: Option<String>,
    pub background: Option<String>,
    pub plot_background: Option<String>,
    pub border: Option<String>,
    pub style_id: Option<u32>,
    pub rounded_corners: bool,
    pub title_size: f32,
    pub grid_color: String,
    pub palette: Vec<String>,
    pub blanks: String,
    pub visible_only: bool,
    pub warnings: Vec<String>,
    pub view_3d: ChartView3d,
    pub plot_layout: Option<ChartLayout>,
    pub title_layout: Option<ChartLayout>,
    pub legend_layout: Option<ChartLayout>,
    pub title_overlay: bool,
    pub legend_overlay: bool,
}
impl Default for Chart {
    fn default() -> Self {
        Self {
            title: String::new(),
            title_reference: None,
            groups: vec![],
            axes: vec![],
            legend: None,
            background: Some("FFFFFF".into()),
            plot_background: None,
            border: None,
            style_id: None,
            rounded_corners: false,
            title_size: 14.0,
            grid_color: "DCDCDC".into(),
            palette: FALLBACK_COLORS.iter().map(|s| (*s).into()).collect(),
            blanks: "gap".into(),
            visible_only: true,
            warnings: vec![],
            view_3d: ChartView3d::default(),
            plot_layout: None,
            title_layout: None,
            legend_layout: None,
            title_overlay: false,
            legend_overlay: false,
        }
    }
}
fn val<'a>(n: &'a Node, name: &str) -> Option<&'a str> {
    n.child(name).and_then(|n| n.attr("val"))
}
fn num(n: &Node, name: &str) -> Option<f64> {
    val(n, name)
        .and_then(|v| v.parse().ok())
        .filter(|v: &f64| v.is_finite())
}
fn enabled(n: &Node, name: &str) -> bool {
    n.child(name)
        .is_some_and(|n| !matches!(n.attr("val"), Some("0" | "false")))
}
fn descendants<'a>(node: &'a Node, name: &str, out: &mut Vec<&'a Node>) {
    if node.name == name {
        out.push(node);
    }
    for c in &node.children {
        descendants(c, name, out);
    }
}
fn rich_text(node: Option<&Node>) -> String {
    let Some(n) = node else { return String::new() };
    let mut runs = vec![];
    descendants(n, "t", &mut runs);
    runs.iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("")
}
fn scheme_color(name: &str, theme: &Theme) -> Option<String> {
    let index = match name {
        "lt1" | "bg1" => 0,
        "dk1" | "tx1" => 1,
        "lt2" | "bg2" => 2,
        "dk2" | "tx2" => 3,
        "accent1" => 4,
        "accent2" => 5,
        "accent3" => 6,
        "accent4" => 7,
        "accent5" => 8,
        "accent6" => 9,
        "hlink" => 10,
        "folHlink" => 11,
        _ => return None,
    };
    theme.colors.get(index).cloned()
}
fn color(n: &Node, theme: &Theme) -> Option<String> {
    let base = match n.name.as_str() {
        "srgbClr" => n.attr("val").map(str::to_owned),
        "sysClr" => n.attr("lastClr").map(str::to_owned),
        "schemeClr" => n.attr("val").and_then(|v| scheme_color(v, theme)),
        "prstClr" => match n.attr("val") {
            Some("red") => Some("FF0000".into()),
            Some("white") => Some("FFFFFF".into()),
            Some("black") => Some("000000".into()),
            Some("blue") => Some("0000FF".into()),
            _ => None,
        },
        _ => None,
    }?;
    let mut c = rgba(&base);
    for t in &n.children {
        let v = t
            .attr("val")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(100000.0)
            / 100000.0;
        let modify = |x: u8| match t.name.as_str() {
            // DrawingML tint is the retained input fraction, unlike the
            // SpreadsheetML cell-color tint attribute.
            "tint" => (x as f32 * v + 255.0 * (1.0 - v)).round().clamp(0.0, 255.0) as u8,
            "shade" | "lumMod" => (x as f32 * v).clamp(0.0, 255.0) as u8,
            "lumOff" => (x as f32 + 255.0 * v).clamp(0.0, 255.0) as u8,
            _ => x,
        };
        c = Color32::from_rgba_unmultiplied(
            modify(c.r()),
            modify(c.g()),
            modify(c.b()),
            if t.name == "alpha" {
                (255.0 * v).clamp(0.0, 255.0) as u8
            } else {
                c.a()
            },
        );
    }
    Some(format!(
        "{:02X}{:02X}{:02X}{:02X}",
        c.a(),
        c.r(),
        c.g(),
        c.b()
    ))
}
fn fill(n: Option<&Node>, theme: &Theme) -> Option<String> {
    n.and_then(|n| n.child("solidFill"))
        .and_then(|f| f.children.first())
        .and_then(|c| color(c, theme))
}
fn data(n: Option<&Node>) -> Result<ChartData> {
    let Some(n) = n else {
        return Ok(ChartData::default());
    };
    let mut d = ChartData::default();
    let source = n
        .children
        .iter()
        .find(|c| {
            matches!(
                c.name.as_str(),
                "numRef" | "strRef" | "numLit" | "strLit" | "multiLvlStrRef"
            )
        })
        .unwrap_or(n);
    d.formula = source.child("f").map(|f| f.text.clone());
    let cache = source
        .child("numCache")
        .or_else(|| source.child("strCache"))
        .or_else(|| source.child("multiLvlStrCache"))
        .unwrap_or(source);
    d.format = cache
        .child("formatCode")
        .map(|n| n.text.clone())
        .unwrap_or_default();
    let count = num(cache, "ptCount").unwrap_or(0.0).max(0.0) as usize;
    ensure!(
        count <= MAX_POINTS,
        "Chart has more than {MAX_POINTS} cached points"
    );
    let mut points = vec![];
    descendants(cache, "pt", &mut points);
    let length = points
        .iter()
        .filter_map(|p| p.attr("idx")?.parse::<usize>().ok())
        .max()
        .map_or(count, |m| count.max(m.saturating_add(1)));
    ensure!(
        length <= MAX_POINTS,
        "Chart cache index exceeds {MAX_POINTS}"
    );
    d.numbers.resize(length, None);
    d.labels.resize(length, None);
    let numeric = matches!(source.name.as_str(), "numRef" | "numLit") || cache.name == "numCache";
    for p in points {
        let Some(i) = p
            .attr("idx")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|i| *i < length)
        else {
            continue;
        };
        let Some(v) = p.child("v") else { continue };
        if numeric {
            d.numbers[i] = v.text.parse::<f64>().ok().filter(|v| v.is_finite());
        } else if let Some(previous) = &mut d.labels[i] {
            if !v.text.is_empty() {
                *previous = format!("{} / {}", v.text, previous);
            }
        } else {
            d.labels[i] = Some(v.text.clone());
        }
    }
    Ok(d)
}
fn labels(n: Option<&Node>) -> DataLabels {
    n.map(|n| DataLabels {
        value: enabled(n, "showVal"),
        percent: enabled(n, "showPercent"),
        category: enabled(n, "showCatName"),
        series: enabled(n, "showSerName"),
        format: n
            .child("numFmt")
            .and_then(|n| n.attr("formatCode"))
            .unwrap_or_default()
            .into(),
    })
    .unwrap_or_default()
}

fn manual_layout(node: Option<&Node>) -> Option<ChartLayout> {
    let n = node?.child("layout")?.child("manualLayout")?;
    Some(ChartLayout {
        target: val(n, "layoutTarget").unwrap_or("outer").into(),
        x: num(n, "x").map(|v| v as f32),
        y: num(n, "y").map(|v| v as f32),
        width: num(n, "w").map(|v| v as f32),
        height: num(n, "h").map(|v| v as f32),
        x_mode: val(n, "xMode").unwrap_or("factor").into(),
        y_mode: val(n, "yMode").unwrap_or("factor").into(),
        width_mode: val(n, "wMode").unwrap_or("factor").into(),
        height_mode: val(n, "hMode").unwrap_or("factor").into(),
    })
}

pub(crate) fn parse_chart(xml: &str, theme: &Theme) -> Result<Chart> {
    let root = parse(xml)?;
    let c = root
        .child("chart")
        .context("Chart part has no chart element")?;
    let plot = c.child("plotArea").context("Chart has no plot area")?;
    let mut chart = Chart {
        title: rich_text(c.child("title")),
        title_reference: c
            .child("title")
            .and_then(|n| n.child("tx"))
            .and_then(|n| n.child("strRef"))
            .and_then(|n| n.child("f"))
            .map(|n| n.text.clone()),
        legend: c
            .child("legend")
            .map(|n| val(n, "legendPos").unwrap_or("r").into()),
        background: fill(root.child("spPr"), theme).or(Some("FFFFFF".into())),
        plot_background: fill(plot.child("spPr"), theme),
        border: root.child("spPr").and_then(|n| fill(n.child("ln"), theme)),
        palette: (4..10)
            .filter_map(|i| theme.colors.get(i).cloned())
            .collect(),
        blanks: val(c, "dispBlanksAs").unwrap_or("gap").into(),
        visible_only: !matches!(val(c, "plotVisOnly"), Some("0" | "false")),
        plot_layout: manual_layout(Some(plot)),
        title_layout: manual_layout(c.child("title")),
        legend_layout: manual_layout(c.child("legend")),
        title_overlay: c.child("title").is_some_and(|n| enabled(n, "overlay")),
        legend_overlay: c.child("legend").is_some_and(|n| enabled(n, "overlay")),
        ..Default::default()
    };
    chart.style_id = num(&root, "style").map(|v| v as u32);
    chart.rounded_corners = enabled(&root, "roundedCorners");
    // This preset is verified against Excel for Mac. Explicit stored formatting
    // wins; other Office auto-style effects keep the neutral preview defaults.
    if chart.style_id == Some(10) {
        chart.border = chart.border.or(Some("A6A6A6".into()));
        chart.rounded_corners = !matches!(val(&root, "roundedCorners"), Some("0" | "false"));
        chart.title_size = 11.0;
        chart.grid_color = "333333".into();
    } else if chart.style_id.is_some() {
        chart
            .warnings
            .push("차트 자동 스타일의 일부 장식은 기본 모양으로 표시합니다".into());
    }
    if let Some(view) = c.child("view3D") {
        chart.view_3d.rotation_x = num(view, "rotX").unwrap_or(15.0).clamp(-90.0, 90.0) as f32;
        chart.view_3d.rotation_y = num(view, "rotY").unwrap_or(20.0).rem_euclid(360.0) as f32;
        chart.view_3d.height_percent =
            num(view, "hPercent").unwrap_or(100.0).clamp(5.0, 500.0) as f32;
        chart.view_3d.depth_percent = num(view, "depthPercent")
            .unwrap_or(100.0)
            .clamp(20.0, 2000.0) as f32;
        chart.view_3d.right_angle_axes = !matches!(val(view, "rAngAx"), Some("0" | "false"));
        chart.view_3d.perspective =
            num(view, "perspective").unwrap_or(30.0).clamp(0.0, 240.0) as f32;
    }
    chart.view_3d.floor_color = c.child("floor").and_then(|n| fill(n.child("spPr"), theme));
    chart.view_3d.side_wall_color = c
        .child("sideWall")
        .and_then(|n| fill(n.child("spPr"), theme));
    chart.view_3d.back_wall_color = c
        .child("backWall")
        .and_then(|n| fill(n.child("spPr"), theme));
    if let Some(title) = c.child("title") {
        let mut properties = vec![];
        descendants(title, "rPr", &mut properties);
        descendants(title, "defRPr", &mut properties);
        if let Some(size) = properties.iter().find_map(|n| {
            n.attr("sz")
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|v| v.is_finite())
        }) {
            chart.title_size = (size / 100.0).clamp(1.0, 200.0);
        }
    }
    if chart.title.is_empty() {
        chart.title = c
            .child("title")
            .and_then(|n| n.child("tx"))
            .map(|n| data(Some(n)))
            .transpose()?
            .map(|d| d.label(0))
            .unwrap_or_default();
    }
    for n in &plot.children {
        if matches!(n.name.as_str(), "catAx" | "dateAx" | "valAx" | "serAx") {
            let scale = n.child("scaling");
            chart.axes.push(ChartAxis {
                id: val(n, "axId").unwrap_or_default().into(),
                kind: n.name.clone(),
                position: val(n, "axPos").unwrap_or("l").into(),
                title: rich_text(n.child("title")),
                minimum: scale.and_then(|s| num(s, "min")),
                maximum: scale.and_then(|s| num(s, "max")),
                major_unit: num(n, "majorUnit"),
                log_base: scale
                    .and_then(|s| num(s, "logBase"))
                    .filter(|v| *v >= 2.0 && *v <= 1000.0),
                reversed: scale.and_then(|s| val(s, "orientation")) == Some("maxMin"),
                number_format: n
                    .child("numFmt")
                    .and_then(|n| n.attr("formatCode"))
                    .unwrap_or_default()
                    .into(),
                major_gridlines: n.child("majorGridlines").is_some(),
                hidden: enabled(n, "delete"),
            });
            continue;
        }
        let kind = match n.name.as_str() {
            "barChart" | "bar3DChart" => "bar",
            "lineChart" | "line3DChart" => "line",
            "areaChart" | "area3DChart" => "area",
            "pieChart" | "pie3DChart" => "pie",
            "surface3DChart" => "surface",
            "doughnutChart" => "doughnut",
            "scatterChart" => "scatter",
            "bubbleChart" => "bubble",
            name if name.ends_with("Chart") => {
                chart
                    .warnings
                    .push(format!("지원하지 않는 차트 유형: {name}"));
                continue;
            }
            _ => continue,
        };
        let mut group = ChartGroup {
            kind: kind.into(),
            three_dimensional: n.name.contains("3D"),
            gap_depth: num(n, "gapDepth").unwrap_or(150.0).clamp(0.0, 500.0) as f32,
            shape: val(n, "shape").unwrap_or("box").into(),
            wireframe: enabled(n, "wireframe"),
            horizontal: val(n, "barDir") == Some("bar"),
            grouping: val(n, "grouping").unwrap_or("standard").into(),
            axis_ids: n
                .children
                .iter()
                .filter(|n| n.name == "axId")
                .filter_map(|n| n.attr("val").map(str::to_owned))
                .collect(),
            gap_width: num(n, "gapWidth").unwrap_or(150.0).clamp(0.0, 500.0) as f32,
            overlap: num(n, "overlap").unwrap_or(0.0).clamp(-100.0, 100.0) as f32,
            hole_size: num(n, "holeSize").unwrap_or(50.0).clamp(10.0, 90.0) as f32,
            first_slice_angle: num(n, "firstSliceAng").unwrap_or(0.0) as f32,
            vary_colors: enabled(n, "varyColors"),
            // Office defaults to lineMarker, unlike the schema's marker default.
            // https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/ef2915e0-e766-43e4-a9f0-40a577c59934
            scatter_style: val(n, "scatterStyle").unwrap_or("lineMarker").into(),
            ..Default::default()
        };
        for (i, s) in n.children.iter().filter(|n| n.name == "ser").enumerate() {
            let tx = s.child("tx");
            let name = tx
                .and_then(|n| n.child("v"))
                .map(|n| n.text.clone())
                .unwrap_or_else(|| format!("Series {}", i + 1));
            let marker = s.child("marker");
            let line = s.child("spPr").and_then(|n| n.child("ln"));
            let mut series = ChartSeries {
                name,
                name_reference: tx
                    .and_then(|n| n.child("strRef"))
                    .and_then(|n| n.child("f"))
                    .map(|n| n.text.clone()),
                values: data(s.child("val").or_else(|| s.child("yVal")))?,
                categories: data(s.child("cat"))?,
                x_values: data(s.child("xVal"))?,
                bubble_sizes: data(s.child("bubbleSize"))?,
                color: fill(s.child("spPr"), theme),
                line_color: fill(line, theme),
                line_width: line
                    .and_then(|n| n.attr("w"))
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 12700.0)
                    .unwrap_or(2.0),
                no_line: line.is_some_and(|n| n.child("noFill").is_some()),
                marker: marker
                    .and_then(|n| val(n, "symbol"))
                    .unwrap_or(if kind == "scatter" { "circle" } else { "none" })
                    .into(),
                marker_size: marker.and_then(|n| num(n, "size")).unwrap_or(5.0) as f32,
                labels: labels(s.child("dLbls").or_else(|| n.child("dLbls"))),
                smooth: enabled(s, "smooth") || group.scatter_style.starts_with("smooth"),
                shape: val(s, "shape").map(str::to_owned),
                explosion: num(s, "explosion").unwrap_or(0.0).clamp(0.0, 400.0) as f32,
                ..Default::default()
            };
            if let Some(tx) = tx {
                let cached = data(Some(tx))?;
                if !cached.label(0).is_empty() {
                    series.name = cached.label(0);
                }
            }
            for point in s.children.iter().filter(|n| n.name == "dPt") {
                if let (Some(index), Some(color)) =
                    (num(point, "idx"), fill(point.child("spPr"), theme))
                {
                    series.point_colors.insert(index as usize, color);
                }
            }
            if s.child("trendline").is_some() || s.child("errBars").is_some() {
                chart
                    .warnings
                    .push("차트 추세선·오차 막대는 아직 표시하지 않습니다".into());
            }
            group.series.push(series);
        }
        chart.groups.push(group);
    }
    ensure!(
        chart.groups.iter().map(|g| g.series.len()).sum::<usize>() <= 256,
        "Chart exceeds the 256-series preview limit"
    );
    ensure!(
        chart
            .groups
            .iter()
            .flat_map(|g| &g.series)
            .map(|s| s.values.len().max(s.categories.len()).max(s.x_values.len()))
            .sum::<usize>()
            <= 50_000,
        "Chart exceeds the 50,000-point preview limit"
    );
    if chart.axes.iter().any(|a| a.kind == "dateAx") {
        chart
            .warnings
            .push("날짜 축의 간격은 저장된 항목 순서로 표시합니다".into());
    }
    if chart.groups.iter().any(|g| g.three_dimensional) {
        chart
            .warnings
            .push("3D 차트의 투영·조명은 네이티브 근사 렌더링입니다".into());
        let estimated_faces: usize = chart
            .groups
            .iter()
            .flat_map(|g| {
                g.series.iter().map(move |s| {
                    let faces_per_point = if g.kind == "pie" {
                        8
                    } else if matches!(
                        s.shape.as_deref().unwrap_or(&g.shape),
                        "cylinder" | "cone" | "coneToMax"
                    ) {
                        26
                    } else {
                        6
                    };
                    s.values.len() * faces_per_point
                })
            })
            .sum();
        if estimated_faces > 80_000 {
            chart.warnings.push(
                "3D 차트가 미리 보기의 80,000개 면 제한을 초과하여 일부 데이터 도형은 생략됩니다"
                    .into(),
            );
        }
    }
    chart.warnings.sort();
    chart.warnings.dedup();
    Ok(chart)
}

fn reference(formula: &str) -> Option<(String, u32, u32, u32, u32)> {
    let formula = formula.trim_start_matches('=');
    if formula.contains('[') || formula.contains(']') {
        return None;
    }
    let (sheet, range) = formula.rsplit_once('!')?;
    let sheet = if sheet.starts_with('\'') && sheet.ends_with('\'') {
        sheet[1..sheet.len() - 1].replace("''", "'")
    } else {
        sheet.to_owned()
    };
    fn cell(s: &str) -> Option<(u32, u32)> {
        let s = s.replace('$', "");
        let split = s.find(|c: char| c.is_ascii_digit())?;
        let (letters, digits) = s.split_at(split);
        if letters.is_empty()
            || !letters.bytes().all(|b| b.is_ascii_alphabetic())
            || !digits.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut col = 0u32;
        for c in letters.bytes() {
            col = col
                .checked_mul(26)?
                .checked_add((c.to_ascii_uppercase() - b'A' + 1) as u32)?;
        }
        let row = digits.parse().ok()?;
        if row == 0 || row > 1_048_576 || col > 16_384 {
            return None;
        }
        Some((row, col))
    }
    let (a, b) = range.split_once(':').unwrap_or((range, range));
    let (r0, c0) = cell(a)?;
    let (r1, c1) = cell(b)?;
    if r1 < r0
        || c1 < c0
        || (r0 != r1 && c0 != c1)
        || (u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1)) > MAX_POINTS as u64
    {
        return None;
    }
    Some((sheet, r0, c0, r1, c1))
}
/// Fill absent caches from the already imported workbook; cached charts remain
/// stable even if a producer saved values that differ from formula cells.
pub fn hydrate(chart: &mut Chart, document: &Document) {
    let estimated_points = chart
        .groups
        .iter()
        .flat_map(|g| &g.series)
        .map(|s| {
            [&s.values, &s.categories, &s.x_values]
                .iter()
                .map(|data| {
                    data.formula
                        .as_deref()
                        .and_then(reference)
                        .map(|(_, r0, c0, r1, c1)| ((r1 - r0 + 1) * (c1 - c0 + 1)) as usize)
                        .unwrap_or(data.len())
                })
                .max()
                .unwrap_or(0)
        })
        .sum::<usize>();
    if estimated_points > 50_000 {
        chart
            .warnings
            .push("차트 원본이 50,000개 점 제한을 넘어 저장된 캐시만 표시합니다".into());
        return;
    }
    let maps: HashMap<_, _> = document
        .pages
        .iter()
        .filter_map(|p| {
            p.grid.as_ref().map(|g| {
                (
                    p.name.as_str(),
                    (
                        g,
                        g.cells
                            .iter()
                            .map(|c| ((c.row, c.col), c))
                            .collect::<HashMap<_, _>>(),
                    ),
                )
            })
        })
        .collect();
    let mut warnings = vec![];
    let resolve = |d: &mut ChartData, warnings: &mut Vec<String>| {
        let Some(formula) = &d.formula else { return };
        let Some((sheet, r0, c0, r1, c1)) = reference(formula) else {
            warnings.push("차트의 외부 참조·복합 수식은 저장된 캐시만 사용합니다".into());
            return;
        };
        let Some((grid, cells)) = maps.get(sheet.as_str()) else {
            warnings.push("차트 원본 시트를 찾지 못해 저장된 캐시만 사용합니다".into());
            return;
        };
        let missing = d.numbers.is_empty() && d.labels.is_empty();
        let length = ((r1 - r0 + 1) * (c1 - c0 + 1)) as usize;
        d.hidden.resize(length, false);
        let mut index = 0;
        for row in r0..=r1 {
            for col in c0..=c1 {
                let cell = cells.get(&(row, col));
                if missing {
                    let number = cell.and_then(|c| {
                        c.raw_number.or_else(|| {
                            matches!(c.kind, CellKind::Number | CellKind::Date)
                                .then(|| c.value.parse().ok())
                                .flatten()
                        })
                    });
                    d.numbers.push(number);
                    d.labels.push(
                        cell.filter(|c| c.has_content())
                            .map(|c| number_format::display_value(c, grid.layout.date1904)),
                    );
                    if d.format.is_empty() {
                        d.format = cell
                            .and_then(|c| c.style.number_format.clone())
                            .unwrap_or_default();
                    }
                }
                // Visibility is separate from cache indices and point styles.
                d.hidden[index] = chart.visible_only
                    && (grid.hidden_rows.contains(&row) || grid.hidden_cols.contains(&col));
                index += 1;
            }
        }
    };
    if let Some(f) = &chart.title_reference {
        let mut d = ChartData {
            formula: Some(f.clone()),
            ..Default::default()
        };
        resolve(&mut d, &mut warnings);
        let title = d.label(0);
        if !title.is_empty() {
            chart.title = title;
        }
    }
    for g in &mut chart.groups {
        for s in &mut g.series {
            if let Some(f) = &s.name_reference {
                let mut d = ChartData {
                    formula: Some(f.clone()),
                    ..Default::default()
                };
                resolve(&mut d, &mut warnings);
                let name = d.label(0);
                if !name.is_empty() {
                    s.name = name;
                }
            }
            resolve(&mut s.values, &mut warnings);
            resolve(&mut s.categories, &mut warnings);
            resolve(&mut s.x_values, &mut warnings);
            resolve(&mut s.bubble_sizes, &mut warnings);
        }
    }
    chart.warnings.extend(warnings);
    chart.warnings.sort();
    chart.warnings.dedup();
}

fn rgba(s: &str) -> Color32 {
    let s = s.trim_start_matches('#');
    let hex = if s.len() == 8 { &s[2..] } else { s };
    if hex.len() != 6 {
        return Color32::from_rgb(68, 114, 196);
    };
    let n = u32::from_str_radix(hex, 16).unwrap_or(0x4472c4);
    let a = if s.len() == 8 {
        u8::from_str_radix(&s[..2], 16).unwrap_or(255)
    } else {
        255
    };
    Color32::from_rgba_unmultiplied((n >> 16) as u8, (n >> 8) as u8, n as u8, a)
}
fn format_value(v: f64, fmt: &str) -> String {
    if !fmt.is_empty()
        && fmt != "General"
        && let Some(s) = number_format::format_number(v, fmt, false)
    {
        return s;
    }
    if v.abs() >= 1e8 || (v != 0.0 && v.abs() < 0.001) {
        format!("{v:.1e}")
    } else {
        let text = format!("{v:.3}");
        text.trim_end_matches('0').trim_end_matches('.').into()
    }
}
fn color_at(chart: &Chart, index: usize) -> Color32 {
    rgba(
        chart
            .palette
            .get(index % chart.palette.len().max(1))
            .map(String::as_str)
            .unwrap_or(FALLBACK_COLORS[index % 6]),
    )
}
fn series_color(chart: &Chart, series: &ChartSeries, index: usize) -> Color32 {
    series
        .color
        .as_deref()
        .or(series.line_color.as_deref())
        .map(rgba)
        .unwrap_or_else(|| color_at(chart, index))
}
fn text(p: &egui::Painter, pos: Pos2, align: Align2, value: &str, size: f32, color: Color32) {
    p.text(pos, align, value, FontId::proportional(size), color);
}
#[derive(Clone, Copy, Debug)]
struct Scale {
    min: f64,
    max: f64,
    log: Option<f64>,
    reverse: bool,
}
impl Scale {
    fn fraction(self, v: f64) -> Option<f32> {
        let (v, min, max) = if let Some(base) = self.log {
            if v <= 0.0 {
                return None;
            }
            (v.log(base), self.min.log(base), self.max.log(base))
        } else {
            (v, self.min, self.max)
        };
        // Halving before subtraction avoids overflow for large finite values.
        let t = ((v / 2.0 - min / 2.0) / (max / 2.0 - min / 2.0)) as f32;
        t.is_finite()
            .then_some(if self.reverse { 1.0 - t } else { t })
    }
    fn baseline(self) -> f64 {
        if self.log.is_some() {
            self.min
        } else {
            0.0_f64.clamp(self.min, self.max)
        }
    }
}
fn limits(values: impl Iterator<Item = f64>, axis: Option<&ChartAxis>, zero: bool) -> Scale {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let log = axis.and_then(|a| a.log_base);
    for v in values.filter(|v| v.is_finite() && (log.is_none() || *v > 0.0)) {
        min = min.min(v);
        max = max.max(v)
    }
    if !min.is_finite() {
        min = if log.is_some() { 1.0 } else { 0.0 };
        max = if log.is_some() { 10.0 } else { 1.0 };
    }
    if zero && log.is_none() {
        min = min.min(0.0);
        max = max.max(0.0);
    }
    if min == max {
        if log.is_some() {
            min /= 10.0;
            max *= 10.0
        } else {
            let pad = min.abs().max(1.0) * 0.1;
            min -= pad;
            max += pad;
        }
    }
    if log.is_none() {
        let step = nice_step(max / 5.0 - min / 5.0);
        min = (min / step).floor() * step;
        max = (max / step).ceil() * step;
    } else if let Some(base) = log {
        min = base.powf(min.log(base).floor());
        max = base.powf(max.log(base).ceil());
    }
    if let Some(a) = axis {
        min = a
            .minimum
            .filter(|v| log.is_none() || *v > 0.0)
            .unwrap_or(min);
        max = a
            .maximum
            .filter(|v| log.is_none() || *v > 0.0)
            .unwrap_or(max);
    }
    if min >= max {
        max = min + min.abs().max(1.0);
    }
    Scale {
        min,
        max,
        log,
        reverse: axis.is_some_and(|a| a.reversed),
    }
}
fn nice_step(raw: f64) -> f64 {
    let power = 10_f64.powf(raw.max(f64::MIN_POSITIVE).log10().floor());
    let n = raw / power;
    power
        * if n <= 1.0 {
            1.0
        } else if n <= 2.0 {
            2.0
        } else if n <= 5.0 {
            5.0
        } else {
            10.0
        }
}
fn ticks(scale: Scale, axis: Option<&ChartAxis>) -> Vec<f64> {
    if let Some(base) = scale.log {
        return (scale.min.log(base).ceil() as i32
            ..=(scale.max.log(base).floor() as i32).min(scale.min.log(base).ceil() as i32 + 30))
            .map(|v| base.powi(v))
            .collect();
    }
    let step = axis
        .and_then(|a| a.major_unit)
        .filter(|v| *v > 0.0)
        .unwrap_or_else(|| nice_step((scale.max - scale.min) / 5.0));
    let start = (scale.min / step).ceil() * step;
    (0..40)
        .map(|i| start + i as f64 * step)
        .take_while(|v| *v <= scale.max + step * 1e-8)
        .collect()
}
fn hidden(series: &ChartSeries, i: usize) -> bool {
    [&series.values, &series.categories, &series.x_values]
        .iter()
        .any(|d| d.hidden.get(i).copied().unwrap_or(false))
}
fn stacked(group: &ChartGroup) -> bool {
    matches!(group.grouping.as_str(), "stacked" | "percentStacked")
}
fn stack_segment(group: &ChartGroup, series: usize, index: usize) -> Option<(f64, f64)> {
    if hidden(group.series.get(series)?, index) {
        return None;
    }
    let value = group
        .series
        .get(series)?
        .values
        .numbers
        .get(index)
        .copied()
        .flatten()?;
    if !stacked(group) {
        return Some((0.0, value));
    }
    let denom = if group.grouping == "percentStacked" {
        group
            .series
            .iter()
            .filter_map(|s| s.values.numbers.get(index).copied().flatten())
            .map(f64::abs)
            .sum::<f64>()
            .max(f64::MIN_POSITIVE)
    } else {
        1.0
    };
    let start = group
        .series
        .iter()
        .take(series)
        .filter_map(|s| s.values.numbers.get(index).copied().flatten())
        .filter(|v| (*v >= 0.0) == (value >= 0.0))
        .sum::<f64>()
        / denom;
    let end = start + value / denom;
    (start.is_finite() && end.is_finite()).then_some((start, end))
}
fn value_axis<'a>(chart: &'a Chart, g: &ChartGroup) -> Option<&'a ChartAxis> {
    if matches!(g.kind.as_str(), "scatter" | "bubble")
        && let Some(axis) = g
            .axis_ids
            .get(1)
            .and_then(|id| chart.axes.iter().find(|a| &a.id == id))
    {
        return Some(axis);
    }
    chart
        .axes
        .iter()
        .find(|a| {
            g.axis_ids.contains(&a.id)
                && a.kind == "valAx"
                && if matches!(g.kind.as_str(), "scatter" | "bubble") {
                    matches!(a.position.as_str(), "l" | "r")
                } else {
                    true
                }
        })
        .or_else(|| chart.axes.iter().find(|a| a.kind == "valAx"))
}
fn category_axis<'a>(chart: &'a Chart, g: &ChartGroup) -> Option<&'a ChartAxis> {
    if matches!(g.kind.as_str(), "scatter" | "bubble")
        && let Some(axis) = g
            .axis_ids
            .first()
            .and_then(|id| chart.axes.iter().find(|a| &a.id == id))
    {
        return Some(axis);
    }
    chart
        .axes
        .iter()
        .find(|a| g.axis_ids.contains(&a.id) && matches!(a.kind.as_str(), "catAx" | "dateAx"))
        .or_else(|| {
            chart.axes.iter().find(|a| {
                g.axis_ids.contains(&a.id)
                    && a.kind == "valAx"
                    && matches!(a.position.as_str(), "b" | "t")
            })
        })
}
fn marker(p: &egui::Painter, point: Pos2, size: f32, kind: &str, color: Color32) {
    let r = size / 2.0;
    match kind {
        "none" => {}
        "square" => {
            p.rect_filled(Rect::from_center_size(point, Vec2::splat(size)), 0.0, color);
        }
        "diamond" => {
            p.add(Shape::convex_polygon(
                vec![
                    point + Vec2::new(-r, 0.0),
                    point + Vec2::new(0.0, -r),
                    point + Vec2::new(r, 0.0),
                    point + Vec2::new(0.0, r),
                ],
                color,
                Stroke::NONE,
            ));
        }
        "triangle" => {
            p.add(Shape::convex_polygon(
                vec![
                    point + Vec2::new(0.0, -r),
                    point + Vec2::new(r, r),
                    point + Vec2::new(-r, r),
                ],
                color,
                Stroke::NONE,
            ));
        }
        "x" | "plus" => {
            let d = if kind == "x" {
                Vec2::splat(r)
            } else {
                Vec2::new(r, 0.0)
            };
            p.line_segment([point - d, point + d], Stroke::new(1.2_f32, color));
            let d = Vec2::new(-d.y, d.x);
            p.line_segment([point - d, point + d], Stroke::new(1.2_f32, color));
        }
        _ => {
            p.circle_filled(point, r, color);
        }
    }
}
fn point_label(s: &ChartSeries, i: usize, value: f64, total: f64) -> String {
    let mut parts = vec![];
    if s.labels.series {
        parts.push(s.name.clone())
    }
    if s.labels.category {
        parts.push(s.categories.label(i))
    }
    if s.labels.value {
        parts.push(format_value(
            value,
            if s.labels.format.is_empty() {
                &s.values.format
            } else {
                &s.labels.format
            },
        ))
    }
    if s.labels.percent {
        parts.push(format!(
            "{:.0}%",
            if total == 0.0 {
                0.0
            } else {
                value / total * 100.0
            }
        ))
    }
    parts.join(" · ")
}

/// MS-OE376 §2.1.1634: factor XY offsets the automatic position; edge XY
/// starts at the chart origin. Width/height edge mode specifies the far edge.
fn resolve_layout(layout: Option<&ChartLayout>, chart: Rect, automatic: Rect) -> Rect {
    let Some(l) = layout else { return automatic };
    if [(l.x, &l.x_mode), (l.y, &l.y_mode)]
        .iter()
        .any(|(v, mode)| {
            v.is_some_and(|v| {
                !v.is_finite() || v > 1.0 || v < if *mode == "edge" { 0.0 } else { -1.0 }
            })
        })
        || [l.width, l.height]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v <= 0.0 || *v > 1.0)
    {
        return automatic;
    }
    let x = l.x.map(|v| if l.x_mode == "edge" { chart.left() } else { automatic.left() } + v * chart.width()).unwrap_or(automatic.left());
    let y = l.y.map(|v| if l.y_mode == "edge" { chart.top() } else { automatic.top() } + v * chart.height()).unwrap_or(automatic.top());
    let w = l
        .width
        .map(|v| {
            if l.width_mode == "edge" {
                chart.left() + v * chart.width() - x
            } else {
                v * chart.width()
            }
        })
        .unwrap_or(automatic.width());
    let h = l
        .height
        .map(|v| {
            if l.height_mode == "edge" {
                chart.top() + v * chart.height() - y
            } else {
                v * chart.height()
            }
        })
        .unwrap_or(automatic.height());
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return automatic;
    }
    let size = Vec2::new(w.min(chart.width()), h.min(chart.height()));
    Rect::from_min_size(
        Pos2::new(
            x.clamp(chart.left(), chart.right() - size.x),
            y.clamp(chart.top(), chart.bottom() - size.y),
        ),
        size,
    )
}
fn measured(p: &egui::Painter, text: &str, size: f32) -> Vec2 {
    p.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::BLACK)
        .size()
}
fn group_scales(chart: &Chart) -> Vec<Scale> {
    chart
        .groups
        .iter()
        .map(|g| {
            let axis = value_axis(chart, g);
            let values = chart
                .groups
                .iter()
                .filter(|other| value_axis(chart, other).map(|a| &a.id) == axis.map(|a| &a.id))
                .flat_map(|other| {
                    other
                        .series
                        .iter()
                        .enumerate()
                        .flat_map(move |(s, series)| {
                            (0..series.values.len())
                                .filter_map(move |i| stack_segment(other, s, i))
                                .flat_map(|(a, b)| [a, b])
                        })
                });
            limits(values, axis, true)
        })
        .collect()
}
fn auto_plot_rect(p: &egui::Painter, content: Rect, chart: &Chart, z: f32) -> Rect {
    let Some(g) = chart.groups.first() else {
        return content;
    };
    let axis = value_axis(chart, g);
    let scales = group_scales(chart);
    let tick_width = ticks(scales[0], axis)
        .iter()
        .map(|v| {
            measured(
                p,
                &format_value(
                    *v,
                    axis.map(|a| a.number_format.as_str()).unwrap_or_default(),
                ),
                9.0 * z,
            )
            .x
        })
        .fold(0.0, f32::max);
    let cat_width = g
        .series
        .iter()
        .flat_map(|s| (0..s.categories.len()).map(move |i| s.categories.label(i)))
        .map(|v| measured(p, &v, 9.0 * z).x)
        .fold(0.0, f32::max);
    let left = if g.horizontal { cat_width } else { tick_width }
        + 9.0 * z
        + if (if g.horizontal {
            category_axis(chart, g)
        } else {
            axis
        })
        .is_some_and(|a| !a.title.is_empty())
        {
            17.0 * z
        } else {
            0.0
        };
    let bottom = 20.0 * z
        + if category_axis(chart, g).is_some_and(|a| !a.title.is_empty())
            || g.horizontal && axis.is_some_and(|a| !a.title.is_empty())
        {
            18.0 * z
        } else {
            0.0
        };
    Rect::from_min_max(
        content.min + Vec2::new(left.max(20.0 * z).min(content.width() * 0.38), 8.0 * z),
        content.max - Vec2::new(12.0 * z, bottom.min(content.height() * 0.35)),
    )
}
fn chart_content(p: &egui::Painter, rect: Rect, chart: &Chart, z: f32, ink: Color32) -> Rect {
    let mut content = rect.shrink(12.0 * z);
    if !content.is_positive() {
        return content;
    }
    if !chart.title.is_empty() {
        let galley = p.layout(
            chart.title.clone(),
            FontId::proportional(chart.title_size * z),
            ink,
            content.width(),
        );
        let automatic = Rect::from_min_size(
            Pos2::new(content.center().x - galley.size().x / 2.0, content.top()),
            galley.size(),
        );
        let mut title_rect = resolve_layout(chart.title_layout.as_ref(), rect, automatic);
        let galley = p.layout(
            chart.title.clone(),
            FontId::proportional(chart.title_size * z),
            ink,
            title_rect.width(),
        );
        if chart
            .title_layout
            .as_ref()
            .is_none_or(|l| l.height.is_none())
        {
            title_rect.max.y = (title_rect.min.y + galley.size().y).min(rect.bottom());
        }
        p.with_clip_rect(title_rect.intersect(p.clip_rect()))
            .galley(title_rect.min, galley, ink);
        if !chart.title_overlay {
            content.min.y = content.min.y.max(title_rect.bottom() + 8.0 * z);
        }
    }
    let mut entries = vec![];
    let mut color_index = 0;
    for g in &chart.groups {
        if matches!(g.kind.as_str(), "pie" | "doughnut") {
            if let Some(s) = g.series.first() {
                for i in (0..s.values.len()).filter(|i| !hidden(s, *i)) {
                    entries.push((
                        s.categories.label(i),
                        s.point_colors
                            .get(&i)
                            .map(|c| rgba(c))
                            .unwrap_or_else(|| color_at(chart, i)),
                    ));
                }
            }
        } else {
            for s in &g.series {
                entries.push((s.name.clone(), series_color(chart, s, color_index)));
                color_index += 1;
            }
        }
    }
    let Some(position) = &chart.legend else {
        return content;
    };
    if entries.is_empty() || !content.is_positive() {
        return content;
    }
    let horizontal = matches!(position.as_str(), "b" | "t");
    let row_height = measured(p, "Ag", 9.0 * z).y + 6.0 * z;
    let widths: Vec<_> = entries
        .iter()
        .map(|(name, _)| measured(p, name, 9.0 * z).x + 24.0 * z)
        .collect();
    let max_width = widths
        .iter()
        .copied()
        .fold(0.0, f32::max)
        .min(content.width() * 0.42);
    let mut slots = vec![];
    let size = if horizontal {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut row_width: f32 = 0.0;
        for width in &widths {
            let width = width.min(content.width());
            if x > 0.0 && x + width > content.width() {
                x = 0.0;
                y += row_height;
            }
            slots.push(Rect::from_min_size(
                Pos2::new(x, y),
                Vec2::new(width, row_height),
            ));
            x += width;
            row_width = row_width.max(x);
        }
        Vec2::new(row_width, (y + row_height).min(content.height() * 0.4))
    } else {
        let max_rows = (content.height() / row_height).floor().max(1.0) as usize;
        for i in 0..entries.len() {
            slots.push(Rect::from_min_size(
                Pos2::new(
                    (i / max_rows) as f32 * max_width,
                    (i % max_rows) as f32 * row_height,
                ),
                Vec2::new(max_width, row_height),
            ));
        }
        Vec2::new(
            (entries.len().div_ceil(max_rows) as f32 * max_width).min(content.width() * 0.45),
            entries.len().min(max_rows) as f32 * row_height,
        )
    };
    let automatic = if horizontal {
        Rect::from_min_size(
            Pos2::new(
                content.center().x - size.x / 2.0,
                if position == "t" {
                    content.top()
                } else {
                    content.bottom() - size.y
                },
            ),
            size,
        )
    } else {
        Rect::from_min_size(
            Pos2::new(
                if position == "l" {
                    content.left()
                } else {
                    content.right() - size.x
                },
                content.center().y - size.y / 2.0,
            ),
            size,
        )
    };
    let legend_rect = resolve_layout(chart.legend_layout.as_ref(), rect, automatic);
    let legend_p = p.with_clip_rect(legend_rect.intersect(p.clip_rect()));
    for ((name, color), slot) in entries.iter().zip(&slots) {
        let origin = legend_rect.min + slot.min.to_vec2();
        legend_p.rect_filled(
            Rect::from_min_size(
                origin + Vec2::new(0.0, 4.0 * z),
                Vec2::new(9.0 * z, 6.0 * z),
            ),
            0.0,
            *color,
        );
        let galley = legend_p.layout(
            name.clone(),
            FontId::proportional(9.0 * z),
            ink,
            (slot.width() - 15.0 * z).max(1.0),
        );
        legend_p.galley(origin + Vec2::new(13.0 * z, 0.0), galley, ink);
    }
    if !chart.legend_overlay {
        match position.as_str() {
            "t" => content.min.y = content.min.y.max(legend_rect.bottom() + 6.0 * z),
            "b" => content.max.y = content.max.y.min(legend_rect.top() - 6.0 * z),
            "l" => content.min.x = content.min.x.max(legend_rect.right() + 6.0 * z),
            _ => content.max.x = content.max.x.min(legend_rect.left() - 6.0 * z),
        }
    }
    content
}

/// Paint in a worksheet object rectangle. Geometry comes from the workbook anchor.
pub fn paint_chart(painter: &egui::Painter, rect: Rect, chart: &Chart, zoom: f32) {
    if rect.width() < 10.0 || rect.height() < 10.0 || !rect.intersects(painter.clip_rect()) {
        return;
    }
    let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let z = zoom.clamp(0.1, 5.0);
    let ink = Color32::from_rgb(64, 64, 64);
    if let Some(fill) = &chart.background {
        p.rect_filled(
            rect,
            if chart.rounded_corners { 7.0 * z } else { 0.0 },
            rgba(fill),
        );
    }
    if let Some(border) = &chart.border {
        p.rect_stroke(rect, 0.0, Stroke::new(z, rgba(border)), StrokeKind::Inside);
    }
    let content = chart_content(&p, rect, chart, z, ink);
    if !content.is_positive() {
        return;
    }
    let pie = chart
        .groups
        .iter()
        .all(|g| matches!(g.kind.as_str(), "pie" | "doughnut"));
    if chart.groups.is_empty() {
        text(
            &p,
            content.center(),
            Align2::CENTER_CENTER,
            "지원하지 않는 차트",
            11.0 * z,
            ink,
        );
        return;
    }
    let automatic = if pie {
        content
    } else {
        auto_plot_rect(&p, content, chart, z)
    };
    let plot = if chart
        .plot_layout
        .as_ref()
        .is_some_and(|l| l.target == "outer")
    {
        let outer = resolve_layout(chart.plot_layout.as_ref(), rect, content);
        Rect::from_min_max(
            outer.min + (automatic.min - content.min),
            outer.max - (content.max - automatic.max),
        )
    } else {
        resolve_layout(chart.plot_layout.as_ref(), rect, automatic)
    };
    if chart
        .groups
        .iter()
        .any(|g| g.three_dimensional || g.kind == "surface")
    {
        three_d::paint(&p, plot, chart, z);
        return;
    }
    if pie {
        paint_pies(&p, plot, chart, z);
        return;
    }
    if !plot.is_positive() {
        return;
    }
    if let Some(c) = &chart.plot_background {
        p.rect_filled(plot, 0.0, rgba(c));
    }
    let primary = &chart.groups[0];
    let axis = value_axis(chart, primary);
    let horizontal = primary.horizontal;
    let numeric = matches!(primary.kind.as_str(), "scatter" | "bubble");
    let scales = group_scales(chart);
    let primary_scale = scales[0];
    let xaxis = category_axis(chart, primary);
    let xscale = limits(
        primary
            .series
            .iter()
            .flat_map(|s| s.x_values.numbers.iter().copied().flatten()),
        xaxis,
        false,
    );
    let gridline = Stroke::new(0.7 * z, rgba(&chart.grid_color));
    let axis_line = Stroke::new(0.7 * z, Color32::from_gray(165));
    if !axis.is_some_and(|a| a.hidden) {
        for v in ticks(primary_scale, axis) {
            let t = primary_scale.fraction(v).unwrap_or(0.0);
            if horizontal {
                let x = plot.left() + t * plot.width();
                if axis.is_none_or(|a| a.major_gridlines) {
                    p.line_segment(
                        [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                        gridline,
                    );
                }
                text(
                    &p,
                    Pos2::new(x, plot.bottom() + 5.0 * z),
                    Align2::CENTER_TOP,
                    &format_value(
                        v,
                        axis.map(|a| a.number_format.as_str()).unwrap_or_default(),
                    ),
                    9.0 * z,
                    ink,
                );
            } else {
                let y = plot.bottom() - t * plot.height();
                if axis.is_none_or(|a| a.major_gridlines) {
                    p.line_segment(
                        [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                        gridline,
                    );
                }
                text(
                    &p,
                    Pos2::new(plot.left() - 6.0 * z, y),
                    Align2::RIGHT_CENTER,
                    &format_value(
                        v,
                        axis.map(|a| a.number_format.as_str()).unwrap_or_default(),
                    ),
                    9.0 * z,
                    ink,
                );
            }
        }
    }
    p.line_segment([plot.left_bottom(), plot.right_bottom()], axis_line);
    p.line_segment([plot.left_bottom(), plot.left_top()], axis_line);
    let full_count = chart
        .groups
        .iter()
        .flat_map(|g| &g.series)
        .map(|s| s.values.len().max(s.categories.len()))
        .max()
        .unwrap_or(1);
    let categories: Vec<usize> = (0..full_count)
        .filter(|i| {
            chart
                .groups
                .iter()
                .flat_map(|g| &g.series)
                .any(|s| *i < s.values.len() && !hidden(s, *i))
        })
        .collect();
    let positions: HashMap<usize, usize> = categories
        .iter()
        .enumerate()
        .map(|(position, index)| (*index, position))
        .collect();
    let count = categories.len().max(1);
    if !xaxis.is_some_and(|a| a.hidden) {
        if numeric {
            for v in ticks(xscale, xaxis) {
                if let Some(t) = xscale.fraction(v) {
                    text(
                        &p,
                        Pos2::new(plot.left() + t * plot.width(), plot.bottom() + 5.0 * z),
                        Align2::CENTER_TOP,
                        &format_value(
                            v,
                            xaxis.map(|a| a.number_format.as_str()).unwrap_or_default(),
                        ),
                        9.0 * z,
                        ink,
                    );
                }
            }
        } else {
            let max_labels = if horizontal {
                (plot.height() / (16.0 * z)) as usize
            } else {
                (plot.width()
                    / primary
                        .series
                        .iter()
                        .flat_map(|s| (0..s.categories.len()).map(move |i| s.categories.label(i)))
                        .map(|label| measured(&p, &label, 9.0 * z).x + 10.0 * z)
                        .fold(35.0 * z, f32::max)) as usize
            }
            .max(1);
            let stride = count.div_ceil(max_labels);
            for (position, &i) in categories.iter().enumerate().step_by(stride) {
                let mut t = (position as f32 + 0.5) / count as f32;
                if xaxis.is_some_and(|a| a.reversed) {
                    t = 1.0 - t;
                }
                let label = primary
                    .series
                    .iter()
                    .map(|s| s.categories.label(i))
                    .find(|v| !v.is_empty())
                    .unwrap_or_else(|| (i + 1).to_string());
                if horizontal {
                    text(
                        &p,
                        Pos2::new(plot.left() - 6.0 * z, plot.top() + t * plot.height()),
                        Align2::RIGHT_CENTER,
                        &label,
                        9.0 * z,
                        ink,
                    );
                } else {
                    text(
                        &p,
                        Pos2::new(plot.left() + t * plot.width(), plot.bottom() + 6.0 * z),
                        Align2::CENTER_TOP,
                        &label,
                        9.0 * z,
                        ink,
                    );
                }
            }
        }
    }
    for (a, vertical) in [(axis, !horizontal), (xaxis, horizontal)] {
        if let Some(a) = a.filter(|a| !a.title.is_empty()) {
            if vertical {
                let galley = p.layout_no_wrap(a.title.clone(), FontId::proportional(10.0 * z), ink);
                let center = Pos2::new(content.left() + galley.size().y / 2.0, plot.center().y);
                let origin = center + Vec2::new(-galley.size().y / 2.0, galley.size().x / 2.0);
                p.add(
                    egui::epaint::TextShape::new(origin, galley, ink)
                        .with_angle(-std::f32::consts::FRAC_PI_2),
                );
            } else {
                text(
                    &p,
                    Pos2::new(plot.center().x, content.bottom()),
                    Align2::CENTER_BOTTOM,
                    &a.title,
                    10.0 * z,
                    ink,
                );
            }
        }
    }
    let data_p = p.with_clip_rect(plot.expand(1.0 * z).intersect(p.clip_rect()));
    let mut series_index = 0;
    for (gi, g) in chart.groups.iter().enumerate() {
        let scale = scales[gi];
        let cat_axis = category_axis(chart, g);
        let numeric = matches!(g.kind.as_str(), "scatter" | "bubble");
        let x_scale = if numeric {
            limits(
                g.series
                    .iter()
                    .flat_map(|s| s.x_values.numbers.iter().copied().flatten()),
                cat_axis,
                false,
            )
        } else {
            xscale
        };
        for (si, s) in g.series.iter().enumerate() {
            let base = series_color(chart, s, series_index);
            series_index += 1;
            let mut segment = vec![];
            let mut lower = vec![];
            let flush = |points: &mut Vec<Pos2>, lower: &mut Vec<Pos2>| {
                if points.len() > 1 {
                    if g.kind == "area" {
                        let mut mesh = egui::epaint::Mesh::default();
                        for (upper, bottom) in points.iter().zip(lower.iter()) {
                            mesh.colored_vertex(*upper, base);
                            mesh.colored_vertex(*bottom, base);
                        }
                        for index in 0..points.len().saturating_sub(1) as u32 {
                            let i = index * 2;
                            mesh.add_triangle(i, i + 2, i + 3);
                            mesh.add_triangle(i, i + 3, i + 1);
                        }
                        data_p.add(Shape::mesh(mesh));
                    }
                    if !s.no_line && g.kind != "area" {
                        data_p.add(Shape::line(
                            if s.smooth {
                                smooth_points(points)
                            } else {
                                points.clone()
                            },
                            Stroke::new(
                                s.line_width.max(0.5) * z,
                                s.line_color.as_deref().map(rgba).unwrap_or(base),
                            ),
                        ));
                    }
                }
                points.clear();
                lower.clear();
            };
            for i in 0..s.values.len() {
                if hidden(s, i) {
                    continue;
                }
                let value = s
                    .values
                    .numbers
                    .get(i)
                    .copied()
                    .flatten()
                    .or_else(|| (chart.blanks == "zero").then_some(0.0));
                let Some(value) = value else {
                    if chart.blanks != "span" {
                        flush(&mut segment, &mut lower)
                    }
                    continue;
                };
                let (start, end) = stack_segment(g, si, i).unwrap_or((0.0, value));
                let Some(y) = scale.fraction(end) else {
                    flush(&mut segment, &mut lower);
                    continue;
                };
                let baseline = scale
                    .fraction(if stacked(g) { start } else { scale.baseline() })
                    .unwrap_or(0.0);
                let mut t = (positions.get(&i).copied().unwrap_or(i) as f32 + 0.5) / count as f32;
                if cat_axis.is_some_and(|a| a.reversed) {
                    t = 1.0 - t;
                }
                let point_color =
                    s.point_colors
                        .get(&i)
                        .map(|v| rgba(v))
                        .unwrap_or(if g.vary_colors {
                            color_at(chart, i)
                        } else {
                            base
                        });
                if g.kind == "bar" {
                    let group_count = if stacked(g) { 1 } else { g.series.len().max(1) };
                    let band = if g.horizontal {
                        plot.height()
                    } else {
                        plot.width()
                    } / count as f32;
                    let width = band / (group_count as f32 + g.gap_width / 100.0);
                    let spacing = width * (1.0 - g.overlap / 100.0);
                    let total = width + spacing * (group_count - 1) as f32;
                    let offset = if stacked(g) {
                        -width / 2.0
                    } else {
                        -total / 2.0 + si as f32 * spacing
                    };
                    let bar = if g.horizontal {
                        let center = plot.top() + t * plot.height();
                        Rect::from_two_pos(
                            Pos2::new(plot.left() + baseline * plot.width(), center + offset),
                            Pos2::new(plot.left() + y * plot.width(), center + offset + width),
                        )
                    } else {
                        let center = plot.left() + t * plot.width();
                        Rect::from_two_pos(
                            Pos2::new(center + offset, plot.bottom() - baseline * plot.height()),
                            Pos2::new(center + offset + width, plot.bottom() - y * plot.height()),
                        )
                    };
                    data_p.rect_filled(bar, 0.0, point_color);
                    let label = point_label(s, i, value, 0.0);
                    if !label.is_empty() {
                        let pos = if g.horizontal {
                            Pos2::new(bar.right() + 3.0 * z, bar.center().y)
                        } else {
                            Pos2::new(bar.center().x, bar.top() - 3.0 * z)
                        };
                        text(
                            &p,
                            pos,
                            if g.horizontal {
                                Align2::LEFT_CENTER
                            } else {
                                Align2::CENTER_BOTTOM
                            },
                            &label,
                            8.0 * z,
                            ink,
                        );
                    }
                    continue;
                }
                let x = if numeric {
                    let xv = s
                        .x_values
                        .numbers
                        .get(i)
                        .copied()
                        .flatten()
                        .unwrap_or((i + 1) as f64);
                    let Some(x) = x_scale.fraction(xv) else {
                        continue;
                    };
                    x
                } else {
                    t
                };
                let point = Pos2::new(
                    plot.left() + x * plot.width(),
                    plot.bottom() - y * plot.height(),
                );
                if g.kind == "bubble" {
                    let max = s
                        .bubble_sizes
                        .numbers
                        .iter()
                        .copied()
                        .flatten()
                        .fold(0.0_f64, f64::max)
                        .max(1.0);
                    let radius = (s
                        .bubble_sizes
                        .numbers
                        .get(i)
                        .copied()
                        .flatten()
                        .unwrap_or(1.0)
                        .abs()
                        / max)
                        .sqrt() as f32
                        * (plot.width().min(plot.height()) * 0.1);
                    data_p.circle_filled(
                        point,
                        radius.max(2.0 * z),
                        point_color.gamma_multiply(0.7),
                    );
                } else {
                    let connect = g.kind != "scatter"
                        || matches!(
                            g.scatter_style.as_str(),
                            "line" | "lineMarker" | "smooth" | "smoothMarker"
                        );
                    if connect {
                        segment.push(point);
                        lower.push(Pos2::new(point.x, plot.bottom() - baseline * plot.height()));
                    }
                    if g.kind != "scatter"
                        || matches!(
                            g.scatter_style.as_str(),
                            "marker" | "lineMarker" | "smoothMarker"
                        )
                    {
                        marker(&data_p, point, s.marker_size * z, &s.marker, point_color);
                    }
                }
                let label = point_label(s, i, value, 0.0);
                if !label.is_empty() {
                    text(
                        &p,
                        point - Vec2::new(0.0, 5.0 * z),
                        Align2::CENTER_BOTTOM,
                        &label,
                        8.0 * z,
                        ink,
                    );
                }
            }
            flush(&mut segment, &mut lower);
        }
    }
}
fn smooth_points(points: &[Pos2]) -> Vec<Pos2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 8);
    for i in 0..points.len() - 1 {
        let p0 = points[i.saturating_sub(1)].to_vec2();
        let p1 = points[i].to_vec2();
        let p2 = points[i + 1].to_vec2();
        let p3 = points[(i + 2).min(points.len() - 1)].to_vec2();
        for j in 0..8 {
            let t = j as f32 / 8.0;
            let v = (2.0 * p1
                + (p2 - p0) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
                * 0.5;
            out.push(v.to_pos2());
        }
    }
    out.push(*points.last().unwrap());
    out
}
fn paint_pies(p: &egui::Painter, rect: Rect, chart: &Chart, z: f32) {
    let Some(g) = chart.groups.first() else {
        return;
    };
    let rings = if g.kind == "doughnut" {
        g.series.len()
    } else {
        1
    };
    let radius = rect.width().min(rect.height()) * 0.44;
    let hole = if g.kind == "doughnut" {
        g.hole_size / 100.0
    } else {
        0.0
    };
    for (ring, s) in g.series.iter().take(rings).enumerate() {
        let total = s
            .values
            .numbers
            .iter()
            .enumerate()
            .filter(|(i, _)| !hidden(s, *i))
            .filter_map(|(_, v)| *v)
            .map(f64::abs)
            .sum::<f64>();
        if total <= 0.0 {
            continue;
        }
        let outer = radius * (1.0 - ring as f32 * (1.0 - hole) / rings.max(1) as f32);
        let inner = if g.kind == "doughnut" {
            outer - radius * (1.0 - hole) / rings.max(1) as f32
        } else {
            0.0
        };
        let mut angle = (g.first_slice_angle - 90.0).to_radians();
        for (i, value) in s.values.numbers.iter().enumerate() {
            if hidden(s, i) {
                continue;
            }
            let Some(value) = value else { continue };
            let sweep = (value.abs() / total) as f32 * std::f32::consts::TAU;
            let color = s
                .point_colors
                .get(&i)
                .map(|s| rgba(s))
                .unwrap_or_else(|| color_at(chart, i));
            let segments = (sweep * radius / 4.0).ceil().clamp(2.0, 180.0) as usize;
            // One mesh per slice: antialiasing individual triangle borders creates
            // radial white seams. Shared vertices leave only the outer silhouette.
            let mut mesh = egui::epaint::Mesh::default();
            for j in 0..=segments {
                let a = angle + sweep * j as f32 / segments as f32;
                mesh.colored_vertex(rect.center() + Vec2::angled(a) * inner, color);
                mesh.colored_vertex(rect.center() + Vec2::angled(a) * outer, color);
            }
            for j in 0..segments as u32 {
                let i = j * 2;
                mesh.add_triangle(i, i + 1, i + 3);
                mesh.add_triangle(i, i + 3, i + 2);
            }
            p.add(Shape::mesh(mesh));
            let label = point_label(s, i, *value, total);
            if !label.is_empty() {
                let middle = angle + sweep / 2.0;
                let pos = rect.center() + Vec2::angled(middle) * (inner + (outer - inner) * 0.65);
                text(
                    p,
                    pos,
                    Align2::CENTER_CENTER,
                    &label,
                    9.0 * z,
                    Color32::from_gray(45),
                );
            }
            angle += sweep;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncached_charts_resolve_internal_ranges_and_keep_saved_caches() {
        let document = crate::ingest::read_document(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/visuals.xlsx"
        )))
        .unwrap();
        let xml = r#"<chartSpace><chart><title><tx><strRef><f>'Chart data'!$B$1</f></strRef></tx></title><plotArea><lineChart><ser><tx><strRef><f>'Chart data'!$C$1</f></strRef></tx><cat><strRef><f>'Chart data'!$A$2:$A$5</f></strRef></cat><val><numRef><f>'Chart data'!$B$2:$B$5</f></numRef></val></ser></lineChart></plotArea></chart></chartSpace>"#;
        let mut chart = parse_chart(xml, &Theme::default()).unwrap();
        hydrate(&mut chart, &document);
        assert_eq!(chart.title, "North");
        let series = &chart.groups[0].series[0];
        assert_eq!(series.name, "South");
        assert_eq!(
            series.values.numbers,
            vec![Some(15.0), Some(24.0), Some(18.0), Some(35.0)]
        );
        assert_eq!(series.categories.label(2), "Q3");
        chart.groups[0].series[0].values.numbers[0] = Some(999.0);
        hydrate(&mut chart, &document);
        assert_eq!(chart.groups[0].series[0].values.numbers[0], Some(999.0));
        assert_eq!(
            document.pages[1]
                .grid
                .as_ref()
                .unwrap()
                .cell(2, 2)
                .unwrap()
                .value,
            "15"
        );
    }
    #[test]
    fn all_fixture_chart_families_generate_finite_native_geometry() {
        use crate::drawings::DrawingContent;
        let document = crate::ingest::read_document(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/visuals.xlsx"
        )))
        .unwrap();
        let charts: Vec<_> = document
            .pages
            .iter()
            .filter_map(|p| p.grid.as_ref())
            .flat_map(|g| &g.layout.drawings)
            .filter_map(|d| match &d.content {
                DrawingContent::Chart(c) => Some(c.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(charts.len(), 9);
        let chart_sheet = document
            .pages
            .iter()
            .find(|p| p.name == "Chart sheet")
            .unwrap();
        assert!(chart_sheet.grid.as_ref().unwrap().layout.chart_sheet);
        let context = egui::Context::default();
        let output = context.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            for chart in &charts {
                assert!(
                    chart.groups.iter().flat_map(|g| &g.series).any(|s| s
                        .values
                        .numbers
                        .iter()
                        .any(Option::is_some))
                );
                paint_chart(
                    &painter,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0)),
                    chart,
                    1.0,
                );
            }
        });
        let meshes = context.tessellate(output.shapes, 1.0);
        assert!(!meshes.is_empty());
        for primitive in meshes {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                assert!(
                    mesh.vertices
                        .iter()
                        .all(|v| v.pos.x.is_finite() && v.pos.y.is_finite())
                );
            }
        }
    }
    #[test]
    fn default_excel_scatter_draws_all_points_and_doughnut_keeps_its_hole() {
        use crate::drawings::DrawingContent;
        let document = crate::ingest::read_document(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/visuals.xlsx"
        )))
        .unwrap();
        let charts: Vec<_> = document
            .pages
            .iter()
            .filter_map(|p| p.grid.as_ref())
            .flat_map(|g| &g.layout.drawings)
            .filter_map(|d| match &d.content {
                DrawingContent::Chart(c) => Some(c.as_ref()),
                _ => None,
            })
            .collect();
        let scatter = charts.iter().find(|c| c.title == "Scatter").unwrap();
        assert_eq!(scatter.groups[0].scatter_style, "lineMarker");
        assert_eq!(category_axis(scatter, &scatter.groups[0]).unwrap().id, "10");
        assert_eq!(value_axis(scatter, &scatter.groups[0]).unwrap().id, "20");
        let context = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let output = context.run(egui::RawInput::default(), |ctx| {
            paint_chart(
                &ctx.layer_painter(egui::LayerId::background()),
                rect,
                scatter,
                1.0,
            )
        });
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,Shape::Path(path) if path.points.len()==4 && path.stroke.width>=1.9)),"Scatter must paint its four-point data line, beyond axes/legend");
        let doughnut = charts.iter().find(|c| c.title == "Doughnut").unwrap();
        assert_eq!(doughnut.groups[0].hole_size, 10.0);
        let output = context.run(egui::RawInput::default(), |ctx| {
            paint_pies(
                &ctx.layer_painter(egui::LayerId::background()),
                rect,
                doughnut,
                1.0,
            )
        });
        let cross =
            |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        let mut meshes = 0;
        for shape in output.shapes {
            if let Shape::Mesh(mesh) = shape.shape {
                meshes += 1;
                for triangle in mesh.indices.as_chunks::<3>().0 {
                    let p = triangle
                        .iter()
                        .map(|i| mesh.vertices[*i as usize].pos)
                        .collect::<Vec<_>>();
                    let signs = [
                        cross(p[0], p[1], rect.center()),
                        cross(p[1], p[2], rect.center()),
                        cross(p[2], p[0], rect.center()),
                    ];
                    assert!(
                        !(signs.iter().all(|v| *v >= 0.0) || signs.iter().all(|v| *v <= 0.0)),
                        "No slice triangle may cover the doughnut center"
                    );
                }
            }
        }
        assert_eq!(meshes, 4);
    }
    #[test]
    fn sparse_caches_keep_indices_and_parse_formats() {
        let xml = r#"<chartSpace><chart><plotArea><barChart><barDir val="col"/><grouping val="stacked"/><ser><tx><v>Revenue</v></tx><val><numRef><f>'Sales'!$B$2:$B$5</f><numCache><formatCode>0.0%</formatCode><ptCount val="4"/><pt idx="0"><v>0.1</v></pt><pt idx="3"><v>0.4</v></pt></numCache></numRef></val></ser></barChart></plotArea></chart></chartSpace>"#;
        let c = parse_chart(xml, &Theme::default()).unwrap();
        let s = &c.groups[0].series[0];
        assert_eq!(s.values.numbers, vec![Some(0.1), None, None, Some(0.4)]);
        assert_eq!(s.values.format, "0.0%");
        assert_eq!(c.groups[0].grouping, "stacked");
    }
    #[test]
    fn drawing_tint_retains_the_specified_input_fraction() {
        let node = parse(r#"<srgbClr val="000000"><tint val="10000"/></srgbClr>"#).unwrap();
        assert_eq!(color(&node, &Theme::default()).as_deref(), Some("FFE6E6E6"));
        let node = parse(r#"<srgbClr val="123456"><tint val="100000"/></srgbClr>"#).unwrap();
        assert_eq!(color(&node, &Theme::default()).as_deref(), Some("FF123456"));
    }
    #[test]
    fn references_accept_quoted_names_but_not_external_or_rectangles() {
        assert_eq!(
            reference("'O''Brien Sales'!$B$2:$B$4"),
            Some(("O'Brien Sales".into(), 2, 2, 4, 2))
        );
        assert!(reference("'[Book.xlsx]Sheet'!A1:A3").is_none());
        assert!(reference("S!A1:B3").is_none());
        assert!(reference("S!A0").is_none());
    }
    #[test]
    fn pie_and_doughnut_percent_labels_keep_negative_source_sign() {
        // Excel uses |value| for the sector area but preserves the source sign
        // in its percent label, divided by the sum of absolute slice values.
        let series = ChartSeries {
            values: ChartData {
                numbers: vec![Some(18.0), Some(32.0), Some(-6.0), Some(28.0)],
                ..Default::default()
            },
            labels: DataLabels {
                percent: true,
                ..Default::default()
            },
            ..Default::default()
        };
        for (kind, three_dimensional) in [("pie", false), ("doughnut", false), ("pie", true)] {
            let chart = Chart {
                groups: vec![ChartGroup {
                    kind: kind.into(),
                    three_dimensional,
                    series: vec![series.clone()],
                    hole_size: 50.0,
                    ..Default::default()
                }],
                ..Default::default()
            };
            let context = egui::Context::default();
            let output = context.run(egui::RawInput::default(), |ctx| {
                paint_chart(
                    &ctx.layer_painter(egui::LayerId::background()),
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 360.0)),
                    &chart,
                    1.0,
                )
            });
            let rendered_labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                rendered_labels.contains(&"-7%"),
                "{kind} 3D={three_dimensional} must retain the negative sign: {rendered_labels:?}"
            );
            assert!(rendered_labels.contains(&"21%"));
            assert_eq!(chart.groups[0].series[0].values.numbers[2], Some(-6.0));
        }
    }
    #[test]
    fn stacks_separate_negative_and_positive_values() {
        let g = ChartGroup {
            grouping: "stacked".into(),
            series: [3.0, -2.0, 4.0, -5.0]
                .iter()
                .map(|v| ChartSeries {
                    values: ChartData {
                        numbers: vec![Some(*v)],
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        assert_eq!(stack_segment(&g, 2, 0), Some((3.0, 7.0)));
        assert_eq!(stack_segment(&g, 3, 0), Some((-2.0, -7.0)));
        let scale = limits([-7.0, 7.0].into_iter(), None, true);
        assert!(scale.min <= -7.0 && scale.max >= 7.0);
        assert_eq!(scale.baseline(), 0.0);
    }
    #[test]
    fn log_scales_do_not_plot_nonpositive_values() {
        let axis = ChartAxis {
            log_base: Some(10.0),
            ..Default::default()
        };
        let scale = limits([-10.0, 0.0, 1.0, 100.0].into_iter(), Some(&axis), true);
        assert_eq!(scale.fraction(10.0), Some(0.5));
        assert_eq!(scale.fraction(0.0), None);
    }
    #[test]
    fn saved_manual_layout_uses_office_edge_and_factor_semantics() {
        let xml = r#"<chartSpace><chart><view3D><rotX val="35"/><rotY val="60"/><depthPercent val="180"/><hPercent val="75"/><rAngAx val="0"/><perspective val="50"/></view3D><title><layout><manualLayout><xMode val="edge"/><yMode val="edge"/><x val="0.1"/><y val="0.05"/></manualLayout></layout><overlay val="1"/></title><plotArea><layout><manualLayout><layoutTarget val="inner"/><xMode val="edge"/><yMode val="edge"/><x val="0.2"/><y val="0.25"/><w val="0.6"/><h val="0.5"/></manualLayout></layout><bar3DChart><shape val="cylinder"/><gapDepth val="75"/></bar3DChart></plotArea><legend><layout><manualLayout><xMode val="factor"/><x val="0.1"/></manualLayout></layout><overlay val="1"/></legend></chart></chartSpace>"#;
        let chart = parse_chart(xml, &Theme::default()).unwrap();
        assert!(chart.groups[0].three_dimensional);
        assert_eq!(chart.groups[0].shape, "cylinder");
        assert_eq!(chart.groups[0].gap_depth, 75.0);
        assert_eq!(chart.view_3d.rotation_x, 35.0);
        assert_eq!(chart.view_3d.rotation_y, 60.0);
        assert_eq!(chart.view_3d.depth_percent, 180.0);
        assert_eq!(chart.view_3d.height_percent, 75.0);
        assert_eq!(chart.view_3d.perspective, 50.0);
        assert!(!chart.view_3d.right_angle_axes);
        assert!(chart.title_overlay && chart.legend_overlay);
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(400.0, 300.0));
        let automatic = Rect::from_min_size(Pos2::new(50.0, 60.0), Vec2::new(200.0, 100.0));
        assert_eq!(
            resolve_layout(chart.plot_layout.as_ref(), rect, automatic),
            Rect::from_min_size(Pos2::new(90.0, 95.0), Vec2::new(240.0, 150.0))
        );
        assert_eq!(
            resolve_layout(chart.legend_layout.as_ref(), rect, automatic).left(),
            90.0
        );
        let layout = ChartLayout {
            x: Some(0.25),
            width: Some(0.75),
            x_mode: "edge".into(),
            width_mode: "edge".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_layout(Some(&layout), rect, automatic).width(),
            200.0
        );
        let invalid = ChartLayout {
            x: Some(-0.1),
            x_mode: "edge".into(),
            ..Default::default()
        };
        assert_eq!(resolve_layout(Some(&invalid), rect, automatic), automatic);
        assert!(!chart.warnings.iter().any(|w| w.contains("수동 배치")));
    }
    #[test]
    fn three_d_families_paint_source_series_and_survive_saved_analysis() {
        let series = r#"<ser><tx><v>Revenue</v></tx><cat><strLit><pt idx="0"><v>Jan</v></pt><pt idx="1"><v>Feb</v></pt><pt idx="2"><v>Mar</v></pt></strLit></cat><val><numLit><pt idx="0"><v>8</v></pt><pt idx="1"><v>-3</v></pt><pt idx="2"><v>12</v></pt></numLit></val></ser>"#;
        for kind in [
            "bar3DChart",
            "line3DChart",
            "area3DChart",
            "pie3DChart",
            "surface3DChart",
        ] {
            let xml = format!(
                "<chartSpace><chart><view3D><rotX val=\"30\"/><rotY val=\"25\"/></view3D><plotArea><{kind}>{series}{series}</{kind}></plotArea></chart></chartSpace>"
            );
            let chart = parse_chart(&xml, &Theme::default()).unwrap();
            let saved = serde_json::to_string(&chart).unwrap();
            let restored: Chart = serde_json::from_str(&saved).unwrap();
            assert!(restored.groups[0].three_dimensional);
            assert_eq!(
                restored.groups[0].series[0].values.numbers,
                vec![Some(8.0), Some(-3.0), Some(12.0)]
            );
            let context = egui::Context::default();
            let output = context.run(egui::RawInput::default(), |ctx| {
                paint_chart(
                    &ctx.layer_painter(egui::LayerId::background()),
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 360.0)),
                    &restored,
                    1.0,
                )
            });
            let meshes = context.tessellate(output.shapes, 1.0);
            let count: usize = meshes
                .iter()
                .filter_map(|p| {
                    if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                        assert!(
                            m.vertices
                                .iter()
                                .all(|v| v.pos.x.is_finite() && v.pos.y.is_finite())
                        );
                        Some(m.indices.len())
                    } else {
                        None
                    }
                })
                .sum();
            assert!(
                count > 100,
                "{kind} must render data faces, axes and labels"
            );
        }
    }
    #[test]
    fn automatic_layout_measures_long_titles_legends_and_value_labels() {
        let context = egui::Context::default();
        let chart = Chart {
            title: "A long chart title that must wrap rather than collide with the legend and plot"
                .into(),
            legend: Some("b".into()),
            groups: vec![ChartGroup {
                kind: "bar".into(),
                series: (0..6)
                    .map(|i| ChartSeries {
                        name: format!("Department {i} and regional revenue"),
                        values: ChartData {
                            numbers: vec![Some(123456.0), Some(234567.0)],
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(450.0, 330.0));
        let _ = context.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::background());
            let content = chart_content(&p, rect, &chart, 1.0, Color32::BLACK);
            assert!(
                content.top() > 35.0,
                "wrapped title reserves measured height"
            );
            assert!(
                content.bottom() < 290.0,
                "multirow legend reserves measured height"
            );
            let plot = auto_plot_rect(&p, content, &chart, 1.0);
            let mut small = chart.clone();
            for series in &mut small.groups[0].series {
                series.values.numbers = vec![Some(1.0), Some(2.0)];
            }
            let small_plot = auto_plot_rect(&p, content, &small, 1.0);
            assert!(
                plot.left() > small_plot.left() + 5.0,
                "six-digit labels need more room than single-digit labels"
            );
            assert!(plot.is_positive());
        });
        let points = [Pos2::new(0., 10.), Pos2::new(10., 0.), Pos2::new(20., 10.)];
        let smooth = smooth_points(&points);
        assert_eq!(smooth.first(), points.first());
        assert_eq!(smooth.last(), points.last());
        assert_eq!(smooth.len(), 17);
    }
    #[test]
    fn crowded_or_tiny_chart_rectangles_never_panic() {
        let chart = Chart {
            title: "Long title".into(),
            legend: Some("r".into()),
            groups: vec![ChartGroup {
                kind: "bar".into(),
                series: vec![ChartSeries {
                    name: "A long legend label".into(),
                    values: ChartData {
                        numbers: vec![Some(10.)],
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            for size in [10., 25., 50., 100., 200.] {
                for zoom in [0.1, 1., 5.] {
                    paint_chart(
                        &painter,
                        Rect::from_min_size(Pos2::ZERO, Vec2::splat(size)),
                        &chart,
                        zoom,
                    );
                }
            }
        });
    }
    #[test]
    fn unsupported_types_are_disclosed_and_large_caches_rejected() {
        let xml = "<chartSpace><chart><plotArea><stockChart/></plotArea></chart></chartSpace>";
        assert_eq!(
            parse_chart(xml, &Theme::default()).unwrap().warnings.len(),
            1
        );
        let xml = "<chartSpace><chart><plotArea><lineChart><ser><val><numLit><ptCount val=\"999999\"/></numLit></val></ser></lineChart></plotArea></chart></chartSpace>";
        assert!(parse_chart(xml, &Theme::default()).is_err());
    }
}
