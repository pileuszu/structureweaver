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
    let mut manual_layouts = vec![];
    descendants(c, "manualLayout", &mut manual_layouts);
    if !manual_layouts.is_empty() {
        chart
            .warnings
            .push("차트 내부 수동 배치는 자동 배치로 표시합니다".into());
    }
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
            "barChart" => "bar",
            "lineChart" => "line",
            "areaChart" => "area",
            "pieChart" => "pie",
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
            if enabled(s, "smooth") || group.scatter_style.starts_with("smooth") {
                chart
                    .warnings
                    .push("차트의 부드러운 곡선은 직선 구간으로 표시합니다".into());
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
                value.abs() / total * 100.0
            }
        ))
    }
    parts.join(" · ")
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
    let mut content = rect.shrink(12.0 * z);
    if !chart.title.is_empty() {
        text(
            &p,
            Pos2::new(content.center().x, content.top()),
            Align2::CENTER_TOP,
            &chart.title,
            chart.title_size * z,
            ink,
        );
        content.min.y += 27.0 * z;
    }
    let pie = chart
        .groups
        .iter()
        .all(|g| matches!(g.kind.as_str(), "pie" | "doughnut"));
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
                            .map(|s| rgba(s))
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
    if let Some(legend) = &chart.legend {
        if matches!(legend.as_str(), "b" | "t") {
            let h = 20.0 * z;
            let y = if legend == "t" {
                let y = content.top();
                content.min.y += h;
                y
            } else {
                content.max.y -= h;
                content.bottom() + 4.0 * z
            };
            let mut x = content.left();
            for (name, color) in entries.iter().take(24) {
                let width = (name.chars().count() as f32 * 5.4 + 25.0) * z;
                if x + width > content.right() {
                    break;
                }
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, y + 4.0 * z), Vec2::new(9.0 * z, 6.0 * z)),
                    0.0,
                    *color,
                );
                text(
                    &p,
                    Pos2::new(x + 13.0 * z, y),
                    Align2::LEFT_TOP,
                    name,
                    9.0 * z,
                    ink,
                );
                x += width;
            }
        } else {
            let w = (content.width() * 0.24).clamp(40.0 * z, 120.0 * z);
            let x = if legend == "l" {
                let x = content.left();
                content.min.x += w;
                x
            } else {
                content.max.x -= w;
                content.right() + 8.0 * z
            };
            let start = content.center().y - entries.len().min(20) as f32 * 7.5 * z;
            for (i, (name, color)) in entries.iter().take(20).enumerate() {
                let y = start + i as f32 * 15.0 * z;
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, y + 4.0 * z), Vec2::new(9.0 * z, 6.0 * z)),
                    0.0,
                    *color,
                );
                text(
                    &p,
                    Pos2::new(x + 13.0 * z, y),
                    Align2::LEFT_TOP,
                    name,
                    9.0 * z,
                    ink,
                );
            }
        }
    }
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
    if pie {
        paint_pies(&p, content, chart, z);
        return;
    }
    let plot = Rect::from_min_max(
        content.min + Vec2::new(49.0 * z, 5.0 * z),
        content.max - Vec2::new(12.0 * z, 35.0 * z),
    );
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
    let scales: Vec<_> = chart
        .groups
        .iter()
        .map(|g| {
            let a = value_axis(chart, g);
            // Every group sharing a value-axis ID uses one scale (combo charts).
            let vals = chart
                .groups
                .iter()
                .filter(|other| value_axis(chart, other).map(|a| &a.id) == a.map(|a| &a.id))
                .flat_map(|other| {
                    other.series.iter().enumerate().flat_map(move |(s, ser)| {
                        (0..ser.values.len())
                            .filter_map(move |i| stack_segment(other, s, i))
                            .flat_map(|(a, b)| [a, b])
                    })
                });
            limits(vals, a, true)
        })
        .collect();
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
                (plot.width() / (45.0 * z)) as usize
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
    if let Some(a) = axis.filter(|a| !a.title.is_empty()) {
        let pos = if horizontal {
            Pos2::new(plot.center().x, content.bottom())
        } else {
            Pos2::new(content.left(), plot.top() - 2.0 * z)
        };
        text(
            &p,
            pos,
            if horizontal {
                Align2::CENTER_BOTTOM
            } else {
                Align2::LEFT_BOTTOM
            },
            &a.title,
            10.0 * z,
            ink,
        );
    }
    if let Some(a) = xaxis.filter(|a| !a.title.is_empty()) {
        text(
            &p,
            Pos2::new(plot.center().x, content.bottom()),
            Align2::CENTER_BOTTOM,
            &a.title,
            10.0 * z,
            ink,
        );
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
                            points.clone(),
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
    fn unsupported_types_are_disclosed_and_large_caches_rejected() {
        let xml = "<chartSpace><chart><plotArea><surface3DChart/></plotArea></chart></chartSpace>";
        assert_eq!(
            parse_chart(xml, &Theme::default()).unwrap().warnings.len(),
            1
        );
        let xml = "<chartSpace><chart><plotArea><lineChart><ser><val><numLit><ptCount val=\"999999\"/></numLit></val></ser></lineChart></plotArea></chart></chartSpace>";
        assert!(parse_chart(xml, &Theme::default()).is_err());
    }
}
