//! Read-only evaluation of stored SpreadsheetML conditional formatting.
//! It never changes cell values or the base formatting used by structure analysis.
use crate::{
    domain::*,
    xlsx_styles::{self, Node, Styles, Theme},
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleOverlay {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<String>,
    pub strike: Option<bool>,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
    pub font_color: Option<String>,
    pub fill: Option<String>,
    pub fill_background: Option<String>,
    pub fill_pattern: Option<String>,
    pub number_format: Option<String>,
    pub left: Option<BorderSide>,
    pub right: Option<BorderSide>,
    pub top: Option<BorderSide>,
    pub bottom: Option<BorderSide>,
}
impl StyleOverlay {
    pub(crate) fn from_node(node: &Node, theme: &Theme, indexed: &[String]) -> Self {
        let mut s = Self::default();
        if let Some(font) = node.child("font") {
            let flag = |n| {
                font.child(n)
                    .map(|n| !matches!(n.attr("val"), Some("0" | "false")))
            };
            s.bold = flag("b");
            s.italic = flag("i");
            s.strike = flag("strike");
            s.underline = font
                .child("u")
                .map(|n| n.attr("val").unwrap_or("single").into());
            s.font_name = font
                .child("name")
                .and_then(|n| n.attr("val"))
                .map(str::to_owned);
            s.font_size = font
                .child("sz")
                .and_then(|n| n.attr("val"))
                .and_then(|v| v.parse().ok())
                .filter(|v: &f32| v.is_finite() && *v > 0.0);
            s.font_color = font
                .child("color")
                .and_then(|n| xlsx_styles::resolve_color(n, theme, indexed));
        }
        if let Some(fill) = node.child("fill").and_then(|n| n.child("patternFill")) {
            s.fill_pattern = fill.attr("patternType").map(str::to_owned);
            s.fill = fill
                .child("fgColor")
                .and_then(|n| xlsx_styles::resolve_color(n, theme, indexed));
            s.fill_background = fill
                .child("bgColor")
                .and_then(|n| xlsx_styles::resolve_color(n, theme, indexed));
            // Differential fills commonly omit patternType and supply bgColor only.
            if s.fill.is_none() {
                s.fill = s.fill_background.clone();
            }
            if s.fill.is_some() && s.fill_pattern.as_deref().is_none_or(|v| v == "none") {
                s.fill_pattern = Some("solid".into());
            }
        }
        s.number_format = node.child("numFmt").and_then(|n| {
            n.attr("formatCode").map(str::to_owned).or_else(|| {
                n.attr("numFmtId")
                    .and_then(|v| v.parse().ok())
                    .and_then(crate::number_format::builtin_num_format)
            })
        });
        if let Some(border) = node.child("border") {
            let side = |name, alt| {
                border
                    .child(name)
                    .or_else(|| border.child(alt))
                    .map(|n| BorderSide {
                        style: n.attr("style").unwrap_or("none").into(),
                        color: n
                            .child("color")
                            .and_then(|n| xlsx_styles::resolve_color(n, theme, indexed)),
                    })
            };
            s.left = side("left", "start");
            s.right = side("right", "end");
            s.top = side("top", "top");
            s.bottom = side("bottom", "bottom");
        }
        s
    }
    pub fn apply(&self, s: &mut CellStyle) {
        macro_rules! scalar { ($($f:ident),*) => { $(if let Some(v) = &self.$f { s.$f = v.clone(); })* }; }
        scalar!(bold, italic, underline, strike, font_size, fill_pattern);
        macro_rules! option { ($($f:ident),*) => { $(if let Some(v) = &self.$f { s.$f = Some(v.clone()); })* }; }
        option!(font_name, font_color, fill, fill_background, number_format);
        if let Some(v) = &self.left {
            s.borders.left = v.clone();
        }
        if let Some(v) = &self.right {
            s.borders.right = v.clone();
        }
        if let Some(v) = &self.top {
            s.borders.top = v.clone();
        }
        if let Some(v) = &self.bottom {
            s.borders.bottom = v.clone();
        }
    }
    fn take_unset(&mut self, other: &Self) {
        macro_rules! fields { ($($f:ident),*) => { $(if self.$f.is_none() { self.$f = other.$f.clone(); })* }; }
        fields!(
            bold,
            italic,
            underline,
            strike,
            font_name,
            font_size,
            font_color,
            fill,
            fill_background,
            fill_pattern,
            number_format,
            left,
            right,
            top,
            bottom
        );
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Threshold {
    pub kind: String,
    pub value: String,
    pub inclusive: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConditionalRule {
    pub ranges: Vec<Bounds>,
    pub priority: u32,
    pub stop_if_true: bool,
    pub kind: String,
    pub operator: String,
    pub formulas: Vec<String>,
    pub text: String,
    pub style: StyleOverlay,
    pub thresholds: Vec<Threshold>,
    pub colors: Vec<String>,
    pub rank: u32,
    pub percent: bool,
    pub bottom: bool,
    pub above_average: bool,
    pub equal_average: bool,
    pub std_dev: u32,
    pub show_value: bool,
    pub reverse: bool,
    pub icon_set: String,
    pub min_length: f32,
    pub max_length: f32,
    pub gradient: bool,
    pub extended_bar: bool,
    pub negative_color: Option<String>,
    pub axis_color: Option<String>,
    pub direction: String,
}
#[derive(Clone, Debug)]
pub struct DataBar {
    /// Fractions of the padded cell width, including the zero axis.
    pub start: f32,
    pub end: f32,
    pub zero: Option<f32>,
    pub color: String,
    pub gradient: bool,
    pub axis_color: String,
}
#[derive(Clone, Debug)]
pub struct Icon {
    pub set: String,
    pub index: usize,
    pub count: usize,
}
#[derive(Clone, Debug, Default)]
pub struct ConditionalCell {
    pub style: StyleOverlay,
    pub data_bar: Option<DataBar>,
    pub icon: Option<Icon>,
    pub hide_value: bool,
}
#[derive(Default)]
pub struct PreparedConditional {
    pub cells: HashMap<(u32, u32), ConditionalCell>,
    pub warnings: Vec<String>,
}

fn coordinate(s: &str) -> Option<(u32, u32)> {
    let s = s.trim_matches('$');
    let mut col = 0u32;
    let mut row = String::new();
    for b in s.bytes().filter(|b| *b != b'$') {
        if b.is_ascii_alphabetic() && row.is_empty() {
            col = col
                .checked_mul(26)?
                .checked_add(u32::from(b.to_ascii_uppercase() - b'A' + 1))?;
        } else if b.is_ascii_digit() {
            row.push(char::from(b));
        } else {
            return None;
        }
    }
    let row: u32 = row.parse().ok()?;
    (row > 0 && row <= 1_048_576 && col > 0 && col <= 16384).then_some((row, col))
}
fn bounds(s: &str) -> Option<Bounds> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (top, left) = coordinate(a)?;
    let (bottom, right) = coordinate(b)?;
    let b = Bounds {
        top,
        left,
        bottom,
        right,
    };
    b.valid().then_some(b)
}
fn flag(n: &Node, key: &str, default: bool) -> bool {
    n.attr(key)
        .map(|v| v == "1" || v == "true")
        .unwrap_or(default)
}
fn num(n: &Node, key: &str, default: u32) -> u32 {
    n.attr(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn thresholds(n: &Node) -> Vec<Threshold> {
    n.children
        .iter()
        .filter(|n| n.name == "cfvo")
        .map(|n| Threshold {
            kind: n.attr("type").unwrap_or("num").into(),
            value: n
                .attr("val")
                .map(str::to_owned)
                .or_else(|| n.child("f").map(|n| n.text.clone()))
                .unwrap_or_default(),
            inclusive: flag(n, "gte", true),
        })
        .collect()
}

pub(crate) fn parse(
    xml: &str,
    styles: &Styles,
    theme: &Theme,
) -> Result<(Vec<ConditionalRule>, Vec<String>)> {
    let root = xlsx_styles::parse(xml)?;
    let mut rules = vec![];
    let mut warnings = BTreeSet::new();
    for group in root
        .children
        .iter()
        .filter(|n| n.name == "conditionalFormatting")
    {
        let range_text = group.attr("sqref").unwrap_or("");
        let ranges: Vec<_> = range_text.split_whitespace().filter_map(bounds).collect();
        if ranges.len() != range_text.split_whitespace().count() || ranges.is_empty() {
            warnings.insert("잘못된 조건부 서식 범위".into());
            continue;
        }
        for node in group.children.iter().filter(|n| n.name == "cfRule") {
            let kind = node.attr("type").unwrap_or("");
            if !matches!(
                kind,
                "cellIs"
                    | "expression"
                    | "containsText"
                    | "notContainsText"
                    | "beginsWith"
                    | "endsWith"
                    | "duplicateValues"
                    | "uniqueValues"
                    | "top10"
                    | "aboveAverage"
                    | "containsBlanks"
                    | "notContainsBlanks"
                    | "containsErrors"
                    | "notContainsErrors"
                    | "colorScale"
                    | "dataBar"
                    | "iconSet"
            ) {
                warnings.insert(format!("조건부 서식 규칙: {kind}"));
                continue;
            }
            let style = if let Some(id) = node.index("dxfId") {
                if let Some(style) = styles.differential.get(id) {
                    style.clone()
                } else {
                    warnings.insert("조건부 서식의 차등 스타일 참조가 없습니다".into());
                    continue;
                }
            } else {
                StyleOverlay::default()
            };
            let mut r = ConditionalRule {
                ranges: ranges.clone(),
                priority: num(node, "priority", u32::MAX),
                stop_if_true: flag(node, "stopIfTrue", false),
                kind: kind.into(),
                operator: node.attr("operator").unwrap_or("").into(),
                formulas: node
                    .children
                    .iter()
                    .filter(|n| n.name == "formula")
                    .map(|n| n.text.clone())
                    .collect(),
                text: node.attr("text").unwrap_or("").into(),
                style,
                rank: num(node, "rank", 10),
                percent: flag(node, "percent", false),
                bottom: flag(node, "bottom", false),
                above_average: flag(node, "aboveAverage", true),
                equal_average: flag(node, "equalAverage", false),
                std_dev: num(node, "stdDev", 0),
                show_value: true,
                min_length: 10.0,
                max_length: 90.0,
                gradient: true,
                ..Default::default()
            };
            if let Some(visual) = node.child(kind) {
                r.thresholds = thresholds(visual);
                r.colors = visual
                    .children
                    .iter()
                    .filter(|n| n.name == "color")
                    .filter_map(|n| xlsx_styles::resolve_color(n, theme, &styles.indexed))
                    .collect();
                r.show_value = flag(visual, "showValue", true);
                r.reverse = flag(visual, "reverse", false);
                r.icon_set = visual.attr("iconSet").unwrap_or("3TrafficLights1").into();
                r.min_length = num(visual, "minLength", 10).min(100) as f32;
                r.max_length = num(visual, "maxLength", 90).min(100) as f32;
                if r.min_length > r.max_length {
                    warnings.insert("잘못된 데이터 막대 길이 설정".into());
                    continue;
                }
            }
            rules.push(r);
        }
    }
    // Extensions can refine a legacy data-bar rule by matching the same GUID.
    // They must not be applied a second time at a different priority.
    if let Some(ext_list) = root.child("extLst") {
        let mut extended = vec![];
        descendants(ext_list, "conditionalFormatting", &mut extended);
        for group in extended {
            let sqref = group.child("sqref").map(|n| n.text.as_str()).unwrap_or("");
            let ranges: Vec<_> = sqref.split_whitespace().filter_map(bounds).collect();
            for ext in group.children.iter().filter(|n| n.name == "cfRule") {
                let ext_id = ext.attr("id").unwrap_or("");
                let legacy = if !ext_id.is_empty() {
                    root.children
                        .iter()
                        .filter(|n| n.name == "conditionalFormatting")
                        .flat_map(|n| &n.children)
                        .find(|n| {
                            let mut ids = vec![];
                            descendants(n, "id", &mut ids);
                            ids.iter().any(|id| id.text.eq_ignore_ascii_case(ext_id))
                        })
                        .and_then(|n| n.attr("priority"))
                        .and_then(|v| v.parse::<u32>().ok())
                        .and_then(|p| {
                            rules
                                .iter_mut()
                                .find(|r| r.priority == p && r.kind == "dataBar")
                        })
                } else {
                    None
                };
                if let (Some(r), Some(bar)) = (legacy, ext.child("dataBar")) {
                    let t = thresholds(bar);
                    if t.len() == 2 {
                        r.thresholds = t;
                    }
                    r.extended_bar = true;
                    r.gradient = flag(bar, "gradient", true);
                    r.direction = bar.attr("direction").unwrap_or("context").into();
                    if let Some(color) = bar
                        .child("fillColor")
                        .and_then(|n| xlsx_styles::resolve_color(n, theme, &styles.indexed))
                    {
                        r.colors = vec![color];
                    }
                    r.negative_color = bar
                        .child("negativeFillColor")
                        .and_then(|n| xlsx_styles::resolve_color(n, theme, &styles.indexed));
                    r.axis_color = bar
                        .child("axisColor")
                        .and_then(|n| xlsx_styles::resolve_color(n, theme, &styles.indexed));
                    if flag(bar, "border", false)
                        || bar.attr("axisPosition").is_some_and(|v| v != "automatic")
                    {
                        warnings.insert("데이터 막대의 사용자 지정 외곽선·축 위치".into());
                    }
                } else if !ranges.is_empty() || !ext_id.is_empty() {
                    warnings.insert("확장 조건부 서식 중 지원되지 않는 규칙".into());
                }
            }
        }
    }
    rules.sort_by_key(|r| r.priority);
    Ok((rules, warnings.into_iter().collect()))
}
fn descendants<'a>(n: &'a Node, name: &str, out: &mut Vec<&'a Node>) {
    for c in &n.children {
        if c.name == name {
            out.push(c);
        }
        descendants(c, name, out);
    }
}

#[derive(Clone, Debug)]
enum Value {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error,
    Ref(u32, u32),
    Range(Bounds),
}
struct Context<'a> {
    cells: &'a HashMap<(u32, u32), &'a Cell>,
    row: u32,
    col: u32,
    base: (u32, u32),
}
impl Context<'_> {
    fn deref(&self, v: &Value) -> Value {
        match v {
            Value::Ref(r, c) => self
                .cells
                .get(&(*r, *c))
                .map(|c| value(c))
                .unwrap_or(Value::Blank),
            _ => v.clone(),
        }
    }
    fn number(&self, v: &Value) -> Option<f64> {
        match self.deref(v) {
            Value::Number(n) => Some(n),
            Value::Blank => Some(0.0),
            Value::Bool(b) => Some(u8::from(b) as f64),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
    fn boolean(&self, v: &Value) -> Option<bool> {
        match self.deref(v) {
            Value::Bool(b) => Some(b),
            Value::Number(n) => Some(n != 0.0),
            Value::Blank => Some(false),
            _ => None,
        }
    }
    fn text(&self, v: &Value) -> String {
        match self.deref(v) {
            Value::Text(s) => s,
            Value::Number(n) => n.to_string(),
            Value::Bool(v) => if v { "TRUE" } else { "FALSE" }.into(),
            Value::Blank => String::new(),
            _ => "#VALUE!".into(),
        }
    }
    fn flatten(&self, v: Value) -> Vec<Value> {
        if let Value::Range(b) = v {
            self.cells
                .iter()
                .filter(|((r, c), _)| b.contains(*r, *c))
                .map(|(_, c)| value(c))
                .collect()
        } else {
            vec![self.deref(&v)]
        }
    }
}
fn value(c: &Cell) -> Value {
    match c.kind {
        CellKind::Empty => Value::Blank,
        CellKind::Number | CellKind::Date => c
            .raw_number
            .or_else(|| c.value.parse().ok())
            .map(Value::Number)
            .unwrap_or(Value::Error),
        CellKind::Boolean => Value::Bool(c.value.eq_ignore_ascii_case("true")),
        CellKind::Error => Value::Error,
        CellKind::Text => Value::Text(c.value.clone()),
    }
}
fn numeric(c: &Cell) -> Option<f64> {
    if matches!(c.kind, CellKind::Number | CellKind::Date) {
        c.raw_number
            .or_else(|| c.value.parse().ok())
            .filter(|n| n.is_finite())
    } else {
        None
    }
}
fn equality_key(c: &Cell) -> String {
    match value(c) {
        Value::Number(n) => format!("n:{n}"),
        Value::Blank => "blank".into(),
        Value::Text(s) => format!("t:{}", s.to_lowercase()),
        Value::Bool(b) => format!("b:{b}"),
        _ => format!("e:{}", c.value),
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Value(Value),
    Reference(String),
    Range(String, String),
    Unary(char, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Text(String),
    Id(String),
    Op(String),
    Open,
    Close,
    Comma,
    Colon,
    End,
}
fn tokenize(s: &str) -> Result<Vec<Token>> {
    let mut out = vec![];
    let chars: Vec<_> = s.trim().trim_start_matches('=').chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '"' {
                    if chars.get(i + 1) == Some(&'"') {
                        s.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                s.push(chars[i]);
                i += 1;
            }
            ensure!(closed, "조건부 수식 문자열이 닫히지 않았습니다");
            out.push(Token::Text(s));
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if chars.get(i).is_some_and(|c| *c == 'e' || *c == 'E') {
                i += 1;
                if chars.get(i).is_some_and(|c| *c == '+' || *c == '-') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().collect();
            let n: f64 = s.parse()?;
            ensure!(n.is_finite(), "수식 숫자가 유효하지 않습니다");
            out.push(Token::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '$'))
            {
                i += 1;
            }
            out.push(Token::Id(chars[start..i].iter().collect()));
            continue;
        }
        i += 1;
        out.push(match c {
            '(' => Token::Open,
            ')' => Token::Close,
            ',' | ';' => Token::Comma,
            ':' => Token::Colon,
            '+' | '-' | '*' | '/' | '^' | '&' | '%' | '=' => Token::Op(c.to_string()),
            '<' | '>' => {
                let mut op = c.to_string();
                if chars.get(i) == Some(&'=') || (c == '<' && chars.get(i) == Some(&'>')) {
                    op.push(chars[i]);
                    i += 1;
                }
                Token::Op(op)
            }
            _ => bail!("지원하지 않는 조건부 수식 문자: {c}"),
        });
        ensure!(out.len() < 4096, "조건부 수식 길이 한도");
    }
    out.push(Token::End);
    Ok(out)
}
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}
impl Parser {
    fn parse(s: &str) -> Result<Expr> {
        let mut p = Self {
            tokens: tokenize(s)?,
            pos: 0,
            depth: 0,
        };
        let e = p.expr(0)?;
        ensure!(
            p.tokens[p.pos] == Token::End,
            "수식 뒤에 해석하지 못한 내용이 있습니다"
        );
        Ok(e)
    }
    fn expr(&mut self, min: u8) -> Result<Expr> {
        self.depth += 1;
        ensure!(self.depth <= 64, "조건부 수식 중첩 한도");
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        let mut left = match t {
            Token::Num(n) => Expr::Value(Value::Number(n)),
            Token::Text(s) => Expr::Value(Value::Text(s)),
            Token::Op(op) if op == "+" || op == "-" => {
                Expr::Unary(op.chars().next().unwrap_or('+'), Box::new(self.expr(6)?))
            }
            Token::Open => {
                let e = self.expr(0)?;
                ensure!(self.tokens[self.pos] == Token::Close, "수식 괄호 오류");
                self.pos += 1;
                e
            }
            Token::Id(id) => {
                if self.tokens[self.pos] == Token::Open {
                    self.pos += 1;
                    let name = id
                        .to_ascii_uppercase()
                        .trim_start_matches("_XLFN.")
                        .to_owned();
                    ensure!(
                        matches!(
                            name.as_str(),
                            "AND"
                                | "OR"
                                | "NOT"
                                | "IF"
                                | "IFERROR"
                                | "ISBLANK"
                                | "ISNUMBER"
                                | "ISTEXT"
                                | "ISERROR"
                                | "MOD"
                                | "ROW"
                                | "COLUMN"
                                | "LEN"
                                | "LOWER"
                                | "UPPER"
                                | "TRIM"
                                | "ABS"
                                | "INT"
                                | "ROUND"
                                | "SUM"
                                | "AVERAGE"
                                | "MIN"
                                | "MAX"
                                | "COUNT"
                                | "COUNTA"
                                | "COUNTIF"
                                | "SEARCH"
                                | "FIND"
                                | "LEFT"
                                | "RIGHT"
                                | "TRUE"
                                | "FALSE"
                        ),
                        "지원하지 않는 조건부 수식 함수: {name}"
                    );
                    let mut args = vec![];
                    if self.tokens[self.pos] != Token::Close {
                        loop {
                            args.push(self.expr(0)?);
                            if self.tokens[self.pos] != Token::Comma {
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                    ensure!(self.tokens[self.pos] == Token::Close, "함수 괄호 오류");
                    self.pos += 1;
                    Expr::Call(name, args)
                } else if id.eq_ignore_ascii_case("true") || id.eq_ignore_ascii_case("false") {
                    Expr::Value(Value::Bool(id.eq_ignore_ascii_case("true")))
                } else {
                    ensure!(coordinate(&id).is_some(), "지원하지 않는 수식 이름: {id}");
                    if self.tokens[self.pos] == Token::Colon {
                        self.pos += 1;
                        let Token::Id(end) = self.tokens[self.pos].clone() else {
                            bail!("범위 참조 오류")
                        };
                        ensure!(coordinate(&end).is_some(), "범위 참조 오류");
                        self.pos += 1;
                        Expr::Range(id, end)
                    } else {
                        Expr::Reference(id)
                    }
                }
            }
            _ => bail!("조건부 수식 구문 오류"),
        };
        while let Token::Op(op) = &self.tokens[self.pos] {
            let prec = match op.as_str() {
                "=" | "<>" | "<" | ">" | "<=" | ">=" => 1,
                "&" => 2,
                "+" | "-" => 3,
                "*" | "/" => 4,
                "^" => 5,
                "%" => 7,
                _ => 0,
            };
            if prec < min || prec == 0 {
                break;
            }
            let op = op.clone();
            self.pos += 1;
            if op == "%" {
                left = Expr::Binary(
                    "/".into(),
                    Box::new(left),
                    Box::new(Expr::Value(Value::Number(100.0))),
                );
                continue;
            }
            let right = self.expr(prec + u8::from(op != "^"))?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
}
fn reference(s: &str, ctx: &Context<'_>) -> Option<(u32, u32)> {
    let (r, c) = coordinate(s)?;
    let letters_end = s.find(|c: char| c.is_ascii_digit())?;
    let row_abs = s[..letters_end].ends_with('$');
    let col_abs = s.starts_with('$');
    let row = r as i64
        + if row_abs {
            0
        } else {
            ctx.row as i64 - ctx.base.0 as i64
        };
    let col = c as i64
        + if col_abs {
            0
        } else {
            ctx.col as i64 - ctx.base.1 as i64
        };
    (row > 0 && row <= 1_048_576 && col > 0 && col <= 16384).then_some((row as u32, col as u32))
}
fn compare(a: &Value, b: &Value, ctx: &Context<'_>) -> Option<std::cmp::Ordering> {
    let a = ctx.deref(a);
    let b = ctx.deref(b);
    match (&a, &b) {
        (Value::Error, _) | (_, Value::Error) => None,
        (Value::Text(x), Value::Text(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
        (Value::Text(s), Value::Blank) | (Value::Blank, Value::Text(s)) if s.is_empty() => {
            Some(std::cmp::Ordering::Equal)
        }
        _ => ctx
            .number(&a)
            .zip(ctx.number(&b))
            .and_then(|(a, b)| a.partial_cmp(&b))
            .or_else(|| {
                let order = |v: &Value| match v {
                    Value::Text(_) => 1,
                    Value::Bool(_) => 2,
                    _ => 0,
                };
                Some(order(&a).cmp(&order(&b)))
            }),
    }
}
fn eval(e: &Expr, ctx: &Context<'_>) -> Value {
    match e {
        Expr::Value(v) => v.clone(),
        Expr::Reference(s) => reference(s, ctx)
            .map(|(r, c)| Value::Ref(r, c))
            .unwrap_or(Value::Error),
        Expr::Range(a, b) => reference(a, ctx)
            .zip(reference(b, ctx))
            .map(|((r, c), (r2, c2))| {
                Value::Range(Bounds {
                    top: r.min(r2),
                    left: c.min(c2),
                    bottom: r.max(r2),
                    right: c.max(c2),
                })
            })
            .unwrap_or(Value::Error),
        Expr::Unary(op, e) => ctx
            .number(&eval(e, ctx))
            .map(|n| Value::Number(if *op == '-' { -n } else { n }))
            .unwrap_or(Value::Error),
        Expr::Binary(op, a, b) => {
            let a = eval(a, ctx);
            let b = eval(b, ctx);
            if op == "&" {
                return Value::Text(ctx.text(&a) + &ctx.text(&b));
            }
            if matches!(op.as_str(), "=" | "<>" | "<" | ">" | "<=" | ">=") {
                return compare(&a, &b, ctx)
                    .map(|c| {
                        Value::Bool(match op.as_str() {
                            "=" => c.is_eq(),
                            "<>" => !c.is_eq(),
                            "<" => c.is_lt(),
                            ">" => c.is_gt(),
                            "<=" => !c.is_gt(),
                            _ => !c.is_lt(),
                        })
                    })
                    .unwrap_or(Value::Error);
            }
            ctx.number(&a)
                .zip(ctx.number(&b))
                .map(|(a, b)| match op.as_str() {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => a / b,
                    "^" => a.powf(b),
                    _ => f64::NAN,
                })
                .filter(|n| n.is_finite())
                .map(Value::Number)
                .unwrap_or(Value::Error)
        }
        Expr::Call(name, args) => {
            let arg = |i: usize| args.get(i).map(|e| eval(e, ctx)).unwrap_or(Value::Blank);
            let n = |i: usize| ctx.number(&arg(i));
            if name == "IF" {
                return if ctx.boolean(&arg(0)).unwrap_or(false) {
                    arg(1)
                } else {
                    arg(2)
                };
            }
            if name == "IFERROR" {
                let v = arg(0);
                return if matches!(ctx.deref(&v), Value::Error) {
                    arg(1)
                } else {
                    v
                };
            }
            match name.as_str() {
                "TRUE" => Value::Bool(true),
                "FALSE" => Value::Bool(false),
                "NOT" => ctx
                    .boolean(&arg(0))
                    .map(|b| Value::Bool(!b))
                    .unwrap_or(Value::Error),
                "AND" | "OR" => {
                    let vals: Option<Vec<bool>> = args
                        .iter()
                        .flat_map(|a| ctx.flatten(eval(a, ctx)))
                        .map(|v| ctx.boolean(&v))
                        .collect();
                    vals.map(|v| {
                        Value::Bool(if name == "AND" {
                            v.iter().all(|v| *v)
                        } else {
                            v.iter().any(|v| *v)
                        })
                    })
                    .unwrap_or(Value::Error)
                }
                "ISBLANK" => Value::Bool(matches!(ctx.deref(&arg(0)), Value::Blank)),
                "ISNUMBER" => Value::Bool(matches!(ctx.deref(&arg(0)), Value::Number(_))),
                "ISTEXT" => Value::Bool(matches!(ctx.deref(&arg(0)), Value::Text(_))),
                "ISERROR" => Value::Bool(matches!(ctx.deref(&arg(0)), Value::Error)),
                "ROW" | "COLUMN" => {
                    if args.is_empty() {
                        Value::Number(if name == "ROW" { ctx.row } else { ctx.col } as f64)
                    } else {
                        match arg(0) {
                            Value::Ref(r, c) => {
                                Value::Number(if name == "ROW" { r } else { c } as f64)
                            }
                            Value::Range(b) => {
                                Value::Number(if name == "ROW" { b.top } else { b.left } as f64)
                            }
                            _ => Value::Error,
                        }
                    }
                }
                "MOD" => n(0)
                    .zip(n(1))
                    .map(|(a, b)| a - b * (a / b).floor())
                    .filter(|n| n.is_finite())
                    .map(Value::Number)
                    .unwrap_or(Value::Error),
                "ABS" | "INT" | "ROUND" => n(0)
                    .map(|v| match name.as_str() {
                        "ABS" => v.abs(),
                        "INT" => v.floor(),
                        _ => {
                            let f = 10f64.powf(n(1).unwrap_or(0.0).clamp(-308.0, 308.0));
                            (v * f).round() / f
                        }
                    })
                    .filter(|n| n.is_finite())
                    .map(Value::Number)
                    .unwrap_or(Value::Error),
                "LEN" => Value::Number(ctx.text(&arg(0)).chars().count() as f64),
                "LOWER" => Value::Text(ctx.text(&arg(0)).to_lowercase()),
                "UPPER" => Value::Text(ctx.text(&arg(0)).to_uppercase()),
                "TRIM" => Value::Text(
                    ctx.text(&arg(0))
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                "LEFT" | "RIGHT" => {
                    let text = ctx.text(&arg(0));
                    let len = n(1).unwrap_or(1.0);
                    if len < 0.0 {
                        return Value::Error;
                    }
                    let chars: Vec<_> = text.chars().collect();
                    let len = (len as usize).min(chars.len());
                    Value::Text(if name == "LEFT" {
                        chars[..len].iter().collect()
                    } else {
                        chars[chars.len() - len..].iter().collect()
                    })
                }
                "SEARCH" | "FIND" => {
                    let needle = ctx.text(&arg(0));
                    let haystack = ctx.text(&arg(1));
                    let start = n(2).unwrap_or(1.0).max(1.0) as usize - 1;
                    let haystack: String = haystack.chars().skip(start).collect();
                    let (needle, haystack) = if name == "SEARCH" {
                        (needle.to_lowercase(), haystack.to_lowercase())
                    } else {
                        (needle, haystack)
                    };
                    haystack
                        .find(&needle)
                        .map(|index| {
                            Value::Number((start + haystack[..index].chars().count() + 1) as f64)
                        })
                        .unwrap_or(Value::Error)
                }
                "SUM" | "MIN" | "MAX" | "AVERAGE" | "COUNT" | "COUNTA" => {
                    let vals: Vec<_> = args
                        .iter()
                        .flat_map(|a| ctx.flatten(eval(a, ctx)))
                        .collect();
                    if name == "COUNTA" {
                        return Value::Number(
                            vals.iter().filter(|v| !matches!(v, Value::Blank)).count() as f64,
                        );
                    }
                    if name != "COUNT" && vals.iter().any(|v| matches!(v, Value::Error)) {
                        return Value::Error;
                    }
                    let nums: Vec<_> = vals
                        .iter()
                        .filter_map(|v| {
                            if let Value::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect();
                    Value::Number(match name.as_str() {
                        "COUNT" => nums.len() as f64,
                        "SUM" => nums.iter().sum(),
                        "MIN" => nums.iter().copied().reduce(f64::min).unwrap_or(0.0),
                        "MAX" => nums.iter().copied().reduce(f64::max).unwrap_or(0.0),
                        _ => {
                            if nums.is_empty() {
                                return Value::Error;
                            }
                            nums.iter().sum::<f64>() / nums.len() as f64
                        }
                    })
                }
                "COUNTIF" => {
                    let criterion = ctx.deref(&arg(1));
                    let vals = ctx.flatten(arg(0));
                    Value::Number(
                        vals.iter()
                            .filter(|v| criterion_match(v, &criterion, ctx))
                            .count() as f64,
                    )
                }
                _ => Value::Error,
            }
        }
    }
}
fn criterion_match(v: &Value, criterion: &Value, ctx: &Context<'_>) -> bool {
    if let Value::Text(s) = criterion {
        for op in ["<=", ">=", "<>", "<", ">", "="] {
            if let Some(t) = s.strip_prefix(op) {
                let target = t
                    .parse()
                    .map(Value::Number)
                    .unwrap_or_else(|_| Value::Text(t.into()));
                return compare(v, &target, ctx).is_some_and(|o| match op {
                    "<=" => !o.is_gt(),
                    ">=" => !o.is_lt(),
                    "<>" => !o.is_eq(),
                    "<" => o.is_lt(),
                    ">" => o.is_gt(),
                    _ => o.is_eq(),
                });
            }
        }
        if s.contains('*') || s.contains('?') {
            return wildcard(&ctx.text(v).to_lowercase(), &s.to_lowercase());
        }
    }
    compare(v, criterion, ctx).is_some_and(|o| o.is_eq())
}
fn wildcard(text: &str, pattern: &str) -> bool {
    let t: Vec<_> = text.chars().collect();
    let p: Vec<_> = pattern.chars().collect();
    let mut prev = vec![false; t.len() + 1];
    prev[0] = true;
    let mut i = 0;
    while i < p.len() {
        let mut next = vec![false; t.len() + 1];
        let escaped = p[i] == '~' && i + 1 < p.len();
        let c = if escaped {
            i += 1;
            p[i]
        } else {
            p[i]
        };
        if c == '*' && !escaped {
            next[0] = prev[0];
            for j in 1..=t.len() {
                next[j] = prev[j] || next[j - 1];
            }
        } else {
            for j in 1..=t.len() {
                next[j] = prev[j - 1] && ((c == '?' && !escaped) || c == t[j - 1]);
            }
        }
        prev = next;
        i += 1;
    }
    prev[t.len()]
}

struct Statistics {
    numbers: Vec<f64>,
    counts: HashMap<String, usize>,
    average: f64,
    deviation: f64,
}
impl Statistics {
    fn new(grid: &Grid, ranges: &[Bounds], target_count: usize) -> Self {
        let mut numbers = vec![];
        let mut counts = HashMap::new();
        let mut nonblank = 0;
        for c in grid
            .cells
            .iter()
            .filter(|c| ranges.iter().any(|b| b.contains(c.row, c.col)))
        {
            if !matches!(value(c), Value::Blank) {
                nonblank += 1;
                *counts.entry(equality_key(c)).or_insert(0) += 1;
            }
            if let Some(n) = numeric(c) {
                numbers.push(n);
            }
        }
        counts.insert("blank".into(), target_count.saturating_sub(nonblank));
        numbers.sort_by(f64::total_cmp);
        let average = if numbers.is_empty() {
            0.0
        } else {
            numbers.iter().sum::<f64>() / numbers.len() as f64
        };
        let deviation = if numbers.is_empty() {
            0.0
        } else {
            (numbers.iter().map(|n| (n - average).powi(2)).sum::<f64>() / numbers.len() as f64)
                .sqrt()
        };
        Self {
            numbers,
            counts,
            average,
            deviation,
        }
    }
    fn percentile(&self, p: f64) -> Option<f64> {
        if self.numbers.is_empty() {
            return None;
        }
        let x = p.clamp(0.0, 100.0) / 100.0 * (self.numbers.len() - 1) as f64;
        let lo = x.floor() as usize;
        let hi = x.ceil() as usize;
        Some(self.numbers[lo] + (self.numbers[hi] - self.numbers[lo]) * (x - lo as f64))
    }
}
fn threshold(t: &Threshold, s: &Statistics, ctx: &Context<'_>, expr: Option<&Expr>) -> Option<f64> {
    let min = *s.numbers.first()?;
    let max = *s.numbers.last()?;
    match t.kind.as_str() {
        "min" => Some(min),
        "max" => Some(max),
        "autoMin" => Some(min.min(0.0)),
        "autoMax" => Some(max.max(0.0)),
        "num" => t.value.parse().ok(),
        "percent" => t
            .value
            .parse::<f64>()
            .ok()
            .map(|p| min + (max - min) * p.clamp(0.0, 100.0) / 100.0),
        "percentile" => t.value.parse().ok().and_then(|p| s.percentile(p)),
        "formula" => expr.and_then(|e| ctx.number(&eval(e, ctx))),
        _ => None,
    }
    .filter(|n| n.is_finite())
}
fn rgb(c: &str) -> Option<[u8; 3]> {
    let c = c.trim_start_matches('#');
    let c = if c.len() == 8 { &c[2..] } else { c };
    if c.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&c[0..2], 16).ok()?,
        u8::from_str_radix(&c[2..4], 16).ok()?,
        u8::from_str_radix(&c[4..6], 16).ok()?,
    ])
}
fn mix_color(a: &str, b: &str, f: f64) -> Option<String> {
    let a = rgb(a)?;
    let b = rgb(b)?;
    let f = f.clamp(0.0, 1.0);
    let c: Vec<_> = a
        .iter()
        .zip(b)
        .map(|(a, b)| (*a as f64 + (b as f64 - *a as f64) * f).round() as u8)
        .collect();
    Some(format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2]))
}
fn rule_matches(
    r: &ConditionalRule,
    c: Option<&Cell>,
    ctx: &Context<'_>,
    s: &Statistics,
    exprs: &[Expr],
) -> bool {
    let v = c.map(value).unwrap_or(Value::Blank);
    let n = c.and_then(numeric);
    let text = ctx.text(&v).to_lowercase();
    let cmp = |i: usize| exprs.get(i).and_then(|e| compare(&v, &eval(e, ctx), ctx));
    match r.kind.as_str() {
        "expression" => exprs
            .first()
            .and_then(|e| ctx.boolean(&eval(e, ctx)))
            .unwrap_or(false),
        "cellIs" => {
            let Some(a) = cmp(0) else { return false };
            match r.operator.as_str() {
                "equal" => a.is_eq(),
                "notEqual" => !a.is_eq(),
                "greaterThan" => a.is_gt(),
                "lessThan" => a.is_lt(),
                "greaterThanOrEqual" => !a.is_lt(),
                "lessThanOrEqual" => !a.is_gt(),
                "between" => !a.is_lt() && cmp(1).is_some_and(|b| !b.is_gt()),
                "notBetween" => a.is_lt() || cmp(1).is_some_and(|b| b.is_gt()),
                _ => false,
            }
        }
        "containsText" => text.contains(&r.text.to_lowercase()),
        "notContainsText" => !text.contains(&r.text.to_lowercase()),
        "beginsWith" => text.starts_with(&r.text.to_lowercase()),
        "endsWith" => text.ends_with(&r.text.to_lowercase()),
        "containsBlanks" => text.trim().is_empty(),
        "notContainsBlanks" => !text.trim().is_empty(),
        "containsErrors" => matches!(v, Value::Error),
        "notContainsErrors" => !matches!(v, Value::Error),
        "duplicateValues" | "uniqueValues" => {
            let key = c.map(equality_key).unwrap_or_else(|| "blank".into());
            let count = s.counts.get(&key).copied().unwrap_or(0);
            if r.kind == "duplicateValues" {
                count > 1
            } else {
                count == 1
            }
        }
        "top10" => {
            let Some(n) = n else { return false };
            let count = if r.percent {
                (s.numbers.len() as f64 * r.rank.min(100) as f64 / 100.0).ceil() as usize
            } else {
                r.rank as usize
            };
            if count == 0 {
                return false;
            }
            let count = count.min(s.numbers.len());
            if r.bottom {
                s.numbers.get(count - 1).is_some_and(|v| n <= *v)
            } else {
                s.numbers
                    .get(s.numbers.len() - count)
                    .is_some_and(|v| n >= *v)
            }
        }
        "aboveAverage" => n.is_some_and(|n| {
            let cut = s.average
                + if r.above_average { 1.0 } else { -1.0 } * s.deviation * r.std_dev as f64;
            if r.above_average {
                n > cut || (r.equal_average && n == cut)
            } else {
                n < cut || (r.equal_average && n == cut)
            }
        }),
        "colorScale" | "dataBar" | "iconSet" => n.is_some(),
        _ => false,
    }
}

/// Evaluate once when a worksheet is opened; drawing the viewport only looks up results.
/// All ranges retain original coordinates, including hidden rows and columns.
pub fn prepare(grid: &Grid, rules: &[ConditionalRule]) -> PreparedConditional {
    let mut out = PreparedConditional::default();
    let mut warnings = BTreeSet::new();
    let cells: HashMap<_, _> = grid.cells.iter().map(|c| ((c.row, c.col), c)).collect();
    let mut stopped = HashSet::new();
    let mut rules: Vec<_> = rules.iter().collect();
    rules.sort_by_key(|r| r.priority);
    let mut evaluations = 0usize;
    for r in rules {
        if r.ranges.is_empty() {
            continue;
        }
        let parsed: Result<Vec<_>> = r.formulas.iter().map(|f| Parser::parse(f)).collect();
        let exprs = match parsed {
            Ok(v) => v,
            Err(e) => {
                warnings.insert(e.to_string());
                continue;
            }
        };
        let thresholds_parsed: Result<Vec<_>> = r
            .thresholds
            .iter()
            .map(|t| {
                if t.kind == "formula" {
                    Parser::parse(&t.value).map(Some)
                } else {
                    Ok(None)
                }
            })
            .collect();
        let threshold_exprs = match thresholds_parsed {
            Ok(v) => v,
            Err(e) => {
                warnings.insert(e.to_string());
                continue;
            }
        };
        if (r.kind == "expression" && exprs.is_empty())
            || (r.kind == "cellIs"
                && (exprs.is_empty()
                    || (matches!(r.operator.as_str(), "between" | "notBetween")
                        && exprs.len() < 2)))
        {
            warnings.insert("수식이 없는 조건부 서식 규칙".into());
            continue;
        }
        if r.kind == "colorScale"
            && (r.colors.len() != r.thresholds.len() || !(2..=3).contains(&r.colors.len()))
        {
            warnings.insert("잘못된 색조 조건부 서식".into());
            continue;
        }
        if r.kind == "dataBar" && (r.colors.is_empty() || r.thresholds.len() != 2) {
            warnings.insert("잘못된 데이터 막대 조건부 서식".into());
            continue;
        }
        if r.kind == "iconSet"
            && (!(3..=5).contains(&r.thresholds.len())
                || !matches!(
                    r.icon_set.as_str(),
                    "3Arrows"
                        | "3ArrowsGray"
                        | "3Flags"
                        | "3TrafficLights1"
                        | "3TrafficLights2"
                        | "3Signs"
                        | "3Symbols"
                        | "3Symbols2"
                        | "4Arrows"
                        | "4ArrowsGray"
                        | "4RedToBlack"
                        | "4Rating"
                        | "4TrafficLights"
                        | "5Arrows"
                        | "5ArrowsGray"
                        | "5Rating"
                        | "5Quarters"
                ))
        {
            warnings.insert(format!("지원하지 않는 아이콘 집합: {}", r.icon_set));
            continue;
        }
        let mut targets = HashSet::new();
        for b in &r.ranges {
            // Keep finite blank-only ranges (for example A500:A510) even when
            // they lie beyond stored cells. Whole-column rules can address over
            // a million cells: evaluate the used extent and disclose that limit.
            let area = u64::from(b.bottom.saturating_sub(b.top) + 1)
                * u64::from(b.right.saturating_sub(b.left) + 1);
            let (bottom, right) = if area > 500_000 {
                warnings.insert(
                    "조건부 서식의 큰 범위는 사용 영역까지만 표시합니다 (규칙당 500,000셀 한도)"
                        .into(),
                );
                (
                    b.bottom.min(grid.rows.max(40)),
                    b.right.min(grid.cols.max(20)),
                )
            } else {
                (b.bottom, b.right)
            };
            for row in b.top..=bottom {
                for col in b.left..=right {
                    if targets.len() >= 500_000 {
                        break;
                    }
                    targets.insert((row, col));
                }
                if targets.len() >= 500_000 {
                    break;
                }
            }
        }
        if targets.len() >= 500_000 {
            warnings.insert("조건부 서식 표시 한도: 규칙당 500,000셀".into());
        }
        if evaluations.saturating_add(targets.len()) > 2_000_000 {
            warnings.insert("조건부 서식 평가 한도: 2,000,000셀".into());
            break;
        }
        evaluations += targets.len();
        let stats = Statistics::new(grid, &r.ranges, targets.len());
        for (row, col) in targets {
            if stopped.contains(&(row, col)) {
                continue;
            }
            let ctx = Context {
                cells: &cells,
                row,
                col,
                base: (r.ranges[0].top, r.ranges[0].left),
            };
            let cell = cells.get(&(row, col)).copied();
            if !rule_matches(r, cell, &ctx, &stats, &exprs) {
                continue;
            }
            let thresholds: Option<Vec<_>> = r
                .thresholds
                .iter()
                .zip(&threshold_exprs)
                .map(|(t, e)| threshold(t, &stats, &ctx, e.as_ref()))
                .collect();
            if matches!(r.kind.as_str(), "colorScale" | "dataBar" | "iconSet")
                && thresholds.is_none()
            {
                warnings.insert("계산할 수 없는 조건부 서식 임계값".into());
                continue;
            }
            let entry = out.cells.entry((row, col)).or_default();
            entry.style.take_unset(&r.style);
            if let Some(n) = cell.and_then(numeric) {
                let t = thresholds.unwrap_or_default();
                match r.kind.as_str() {
                    "colorScale" if entry.style.fill.is_none() => {
                        if !t.windows(2).all(|w| w[0] <= w[1]) {
                            warnings.insert("색조 임계값 순서가 올바르지 않습니다".into());
                            continue;
                        }
                        let i = if t.len() == 3 && n > t[1] { 1 } else { 0 };
                        let f = if t[i + 1] == t[i] {
                            if n >= t[i + 1] { 1.0 } else { 0.0 }
                        } else {
                            (n - t[i]) / (t[i + 1] - t[i])
                        };
                        entry.style.fill = mix_color(&r.colors[i], &r.colors[i + 1], f);
                        entry.style.fill_pattern = Some("solid".into());
                    }
                    "dataBar" if entry.data_bar.is_none() => {
                        let min = t[0].min(0.0);
                        let max = t[1].max(0.0);
                        let span = max - min;
                        let zero = if span.abs() < f64::EPSILON {
                            0.0
                        } else {
                            -min / span
                        };
                        let min_len = r.min_length as f64 / 100.0;
                        let max_len = r.max_length as f64 / 100.0;
                        let (start, end) = if !r.extended_bar {
                            let f = if t[1] == t[0] {
                                1.0
                            } else {
                                ((n - t[0]) / (t[1] - t[0])).clamp(0.0, 1.0)
                            };
                            (0.0, min_len + (max_len - min_len) * f)
                        } else if min < 0.0 && max > 0.0 {
                            (zero, (n.clamp(min, max) - min) / span)
                        } else if max <= 0.0 {
                            let f = if min == 0.0 {
                                0.0
                            } else {
                                (n / min).clamp(0.0, 1.0)
                            };
                            let length = if n == 0.0 {
                                0.0
                            } else {
                                min_len + (max_len - min_len) * f
                            };
                            (1.0, 1.0 - length)
                        } else {
                            let f = if t[1] == t[0] {
                                1.0
                            } else {
                                ((n - t[0]) / (t[1] - t[0])).clamp(0.0, 1.0)
                            };
                            let length = if n == 0.0 {
                                0.0
                            } else {
                                min_len + (max_len - min_len) * f
                            };
                            (0.0, length)
                        };
                        let rtl = r.direction == "rightToLeft";
                        entry.data_bar = Some(DataBar {
                            start: if rtl { 1.0 - start } else { start } as f32,
                            end: if rtl { 1.0 - end } else { end } as f32,
                            zero: (r.extended_bar && min < 0.0 && max > 0.0).then_some(if rtl {
                                1.0 - zero
                            } else {
                                zero
                            }
                                as f32),
                            color: if r.extended_bar && n < 0.0 {
                                r.negative_color
                                    .clone()
                                    .unwrap_or_else(|| "FFFF0000".into())
                            } else {
                                r.colors[0].clone()
                            },
                            gradient: r.gradient,
                            axis_color: r.axis_color.clone().unwrap_or_else(|| "FF000000".into()),
                        });
                        entry.hide_value |= !r.show_value;
                    }
                    "iconSet" if entry.icon.is_none() => {
                        let mut index = 0;
                        for (i, cut) in t.iter().enumerate().skip(1) {
                            if n > *cut || (r.thresholds[i].inclusive && n == *cut) {
                                index = i;
                            }
                        }
                        if r.reverse {
                            index = t.len() - 1 - index;
                        }
                        entry.icon = Some(Icon {
                            set: r.icon_set.clone(),
                            index,
                            count: t.len(),
                        });
                        entry.hide_value |= !r.show_value;
                    }
                    _ => {}
                }
            }
            if r.stop_if_true {
                stopped.insert((row, col));
            }
        }
    }
    out.warnings = warnings.into_iter().collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid(values: &[f64]) -> Grid {
        Grid {
            rows: values.len() as u32,
            cols: 1,
            cells: values
                .iter()
                .enumerate()
                .map(|(i, n)| Cell {
                    row: i as u32 + 1,
                    col: 1,
                    value: n.to_string(),
                    raw_number: Some(*n),
                    kind: CellKind::Number,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn rule(kind: &str, last: u32) -> ConditionalRule {
        ConditionalRule {
            kind: kind.into(),
            ranges: vec![Bounds {
                top: 1,
                left: 1,
                bottom: last,
                right: 1,
            }],
            show_value: true,
            above_average: true,
            ..Default::default()
        }
    }
    fn condition(xml: &str) -> Vec<ConditionalRule> {
        let styles=xlsx_styles::styles(r#"<styleSheet><dxfs count="2"><dxf><font><b/><color rgb="FFFF0000"/></font><fill><patternFill><bgColor rgb="FFFFFF00"/></patternFill></fill></dxf><dxf><font><i/></font><fill><patternFill patternType="solid"><fgColor rgb="FF00FF00"/></patternFill></fill></dxf></dxfs></styleSheet>"#,&Theme::default()).unwrap();
        parse(xml, &styles, &Theme::default()).unwrap().0
    }
    #[test]
    fn differential_styles_priority_and_stop_do_not_mutate_source() {
        let g = grid(&[5.0, -2.0]);
        let rules = condition(
            r#"<worksheet><conditionalFormatting sqref="A1:A2"><cfRule type="cellIs" priority="1" operator="greaterThan" dxfId="0"><formula>0</formula></cfRule><cfRule type="cellIs" priority="2" operator="greaterThan" dxfId="1"><formula>0</formula></cfRule></conditionalFormatting></worksheet>"#,
        );
        let p = prepare(&g, &rules);
        let s = &p.cells[&(1, 1)].style;
        assert_eq!(s.fill.as_deref(), Some("FFFFFF00"));
        assert_eq!(s.bold, Some(true));
        assert_eq!(s.italic, Some(true));
        assert!(!p.cells.contains_key(&(2, 1)));
        assert!(!g.cells[0].style.bold);
        assert_eq!(g.cells[0].value, "5");
        let mut rules = rules;
        rules[0].stop_if_true = true;
        assert_eq!(prepare(&g, &rules).cells[&(1, 1)].style.italic, None);
    }
    #[test]
    fn blank_only_ranges_past_used_cells_are_rendered_and_full_columns_disclosed() {
        let g = grid(&[1.0]);
        let mut r = rule("containsBlanks", 1);
        r.ranges = vec![Bounds {
            top: 500,
            left: 1,
            bottom: 502,
            right: 1,
        }];
        r.style.fill = Some("FF112233".into());
        let p = prepare(&g, &[r.clone()]);
        assert_eq!(p.cells.len(), 3);
        assert!(p.cells.contains_key(&(500, 1)));
        assert!(p.warnings.is_empty());
        r.ranges[0] = Bounds {
            top: 1,
            left: 1,
            bottom: 1_048_576,
            right: 1,
        };
        let p = prepare(&g, &[r]);
        assert!(!p.warnings.is_empty());
        assert!(p.cells.len() < 500_000);
    }
    #[test]
    fn escaped_formula_operators_survive_xml_parsing() {
        let rules = condition(
            r#"<worksheet><conditionalFormatting sqref="A1:A2"><cfRule type="expression" priority="1" dxfId="0"><formula>A1&lt;0</formula></cfRule></conditionalFormatting></worksheet>"#,
        );
        assert_eq!(rules[0].formulas[0], "A1<0");
        let p = prepare(&grid(&[-1.0, 2.0]), &rules);
        assert!(p.cells.contains_key(&(1, 1)));
        assert!(!p.cells.contains_key(&(2, 1)));
    }
    #[test]
    fn relative_and_absolute_references_and_lazy_formulas() {
        let g = grid(&[2.0, 4.0, -1.0, 8.0]);
        let mut r = rule("expression", 4);
        r.formulas = vec!["AND($A1>0,MOD(ROW(),2)=0,IF(ISBLANK(B1),TRUE,1/0))".into()];
        r.style.bold = Some(true);
        let p = prepare(&g, &[r]);
        assert_eq!(p.cells.len(), 2);
        assert!(p.cells.contains_key(&(2, 1)));
        assert!(p.cells.contains_key(&(4, 1)));
        let mut r = rule("expression", 4);
        r.formulas = vec!["A1>$A$1".into()];
        assert_eq!(prepare(&g, &[r]).cells.len(), 2);
    }
    #[test]
    fn blanks_top_percent_ties_average_and_duplicate_text() {
        let g = grid(&[1.0, 9.0, 9.0, 2.0]);
        let mut r = rule("top10", 4);
        r.rank = 1;
        assert_eq!(prepare(&g, &[r]).cells.len(), 2);
        let r = rule("aboveAverage", 4);
        assert_eq!(prepare(&g, &[r]).cells.len(), 2);
        let mut r = rule("containsBlanks", 5);
        r.style.fill = Some("FF00FF00".into());
        assert!(prepare(&g, &[r]).cells.contains_key(&(5, 1)));
        let mut r = rule("expression", 4);
        r.formulas = vec!["COUNTIF($A$1:$A$4,A1)>1".into()];
        assert_eq!(prepare(&g, &[r]).cells.len(), 2);
    }
    #[test]
    fn color_scale_uses_percentile_and_interpolates() {
        let g = grid(&[0.0, 25.0, 50.0, 100.0]);
        let mut r = rule("colorScale", 4);
        r.thresholds = vec![
            Threshold {
                kind: "min".into(),
                ..Default::default()
            },
            Threshold {
                kind: "percentile".into(),
                value: "50".into(),
                ..Default::default()
            },
            Threshold {
                kind: "max".into(),
                ..Default::default()
            },
        ];
        r.colors = vec!["FFFF0000".into(), "FFFFFF00".into(), "FF00FF00".into()];
        let p = prepare(&g, &[r]);
        assert_eq!(p.cells[&(1, 1)].style.fill.as_deref(), Some("FFFF0000"));
        assert_eq!(p.cells[&(4, 1)].style.fill.as_deref(), Some("FF00FF00"));
        assert_eq!(p.cells[&(2, 1)].style.fill.as_deref(), Some("FFFFAA00"));
    }
    #[test]
    fn signed_bars_and_reversed_icons_keep_values() {
        let g = grid(&[-20.0, 0.0, 80.0]);
        let mut r = rule("dataBar", 3);
        r.thresholds = vec![
            Threshold {
                kind: "min".into(),
                ..Default::default()
            },
            Threshold {
                kind: "max".into(),
                ..Default::default()
            },
        ];
        r.colors = vec!["FF00AAFF".into()];
        r.show_value = false;
        r.extended_bar = true;
        let p = prepare(&g, &[r]);
        let a = p.cells[&(1, 1)].data_bar.as_ref().unwrap();
        assert!((a.start - 0.2).abs() < 0.0001);
        assert_eq!(a.end, 0.0);
        assert!(p.cells[&(1, 1)].hide_value);
        assert_eq!(g.cells[0].value, "-20");
        let mut r = rule("iconSet", 3);
        r.icon_set = "3Arrows".into();
        r.reverse = true;
        r.thresholds = [0, 33, 67]
            .iter()
            .map(|n| Threshold {
                kind: "percent".into(),
                value: n.to_string(),
                inclusive: true,
            })
            .collect();
        let p = prepare(&g, &[r]);
        assert_eq!(p.cells[&(1, 1)].icon.as_ref().unwrap().index, 2);
        assert_eq!(p.cells[&(3, 1)].icon.as_ref().unwrap().index, 0);
    }
    #[test]
    fn legacy_data_bars_scale_negative_values_without_negative_axis() {
        let g = grid(&[-20.0, 0.0, 80.0]);
        let mut r = rule("dataBar", 3);
        r.min_length = 10.0;
        r.max_length = 90.0;
        r.thresholds = vec![
            Threshold {
                kind: "min".into(),
                ..Default::default()
            },
            Threshold {
                kind: "max".into(),
                ..Default::default()
            },
        ];
        r.colors = vec!["FF638EC6".into()];
        let p = prepare(&g, &[r]);
        let low = p.cells[&(1, 1)].data_bar.as_ref().unwrap();
        let high = p.cells[&(3, 1)].data_bar.as_ref().unwrap();
        assert_eq!(low.start, 0.0);
        assert_eq!(low.end, 0.1);
        assert_eq!(low.zero, None);
        assert_eq!(low.color, "FF638EC6");
        assert_eq!(high.end, 0.9);
    }
    #[test]
    fn unsupported_formulas_are_reported_instead_of_guessing() {
        let g = grid(&[1.0]);
        let mut r = rule("expression", 1);
        r.formulas = vec!["UNKNOWN(A1)>0".into()];
        let p = prepare(&g, &[r]);
        assert!(p.cells.is_empty());
        assert!(p.warnings[0].contains("UNKNOWN"));
        let mut r = rule("expression", 1);
        r.formulas = vec!["'Another Sheet'!A1>0".into()];
        assert!(!prepare(&g, &[r]).warnings.is_empty());
    }
    #[test]
    fn x14_databar_refines_matching_legacy_guid_once() {
        let rules = condition(
            r#"<worksheet xmlns:x14="urn:x14" xmlns:xm="urn:xm"><conditionalFormatting sqref="A1:A3"><cfRule type="dataBar" priority="1"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar><extLst><ext><x14:id>{GUID}</x14:id></ext></extLst></cfRule></conditionalFormatting><extLst><ext><x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="dataBar" id="{GUID}"><x14:dataBar gradient="0"><x14:cfvo type="autoMin"/><x14:cfvo type="autoMax"/><x14:negativeFillColor rgb="FFABCDEF"/></x14:dataBar></x14:cfRule><xm:sqref>A1:A3</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst></worksheet>"#,
        );
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].gradient);
        assert_eq!(rules[0].thresholds[0].kind, "autoMin");
        assert_eq!(rules[0].negative_color.as_deref(), Some("FFABCDEF"));
    }
}
