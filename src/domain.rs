use serde::{Deserialize, Serialize};

/// Workbook cell coordinates are 1-based and inclusive everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}
impl Bounds {
    pub fn valid(self) -> bool {
        self.top > 0 && self.left > 0 && self.top <= self.bottom && self.left <= self.right
    }
    pub fn contains(self, row: u32, col: u32) -> bool {
        row >= self.top && row <= self.bottom && col >= self.left && col <= self.right
    }
    pub fn encloses(self, other: Self) -> bool {
        self.contains(other.top, other.left) && self.contains(other.bottom, other.right)
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.top <= other.bottom
            && other.top <= self.bottom
            && self.left <= other.right
            && other.left <= self.right
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    Text,
    Number,
    Boolean,
    Date,
    Error,
    Empty,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellStyle {
    pub bold: bool,
    pub indent: u32,
    pub fill: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
    pub value: String,
    pub kind: CellKind,
    pub formula: Option<String>,
    pub style: CellStyle,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grid {
    pub rows: u32,
    pub cols: u32,
    pub cells: Vec<Cell>,
    pub merges: Vec<Bounds>,
    pub hidden_rows: Vec<u32>,
    pub hidden_cols: Vec<u32>,
}
impl Grid {
    pub fn cell(&self, row: u32, col: u32) -> Option<&Cell> {
        self.cells.iter().find(|c| c.row == row && c.col == col)
    }
    pub fn resolved_cell(&self, row: u32, col: u32) -> Option<&Cell> {
        if let Some(m) = self.merges.iter().find(|m| m.contains(row, col)) {
            self.cell(m.top, m.left)
        } else {
            self.cell(row, col)
        }
    }
    pub fn bounds(&self) -> Bounds {
        Bounds {
            top: 1,
            left: 1,
            bottom: self.rows.max(1),
            right: self.cols.max(1),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    Sheet,
    Slide,
    PdfPage,
}
/// Common source locator; future PDF/PPT adapters use PageRect instead of grid cells.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRegion {
    Cells {
        bounds: Bounds,
    },
    PageRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub page_id: String,
    pub region: SourceRegion,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub region: SourceRegion,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub kind: PageKind,
    pub hidden: bool,
    pub grid: Option<Grid>,
    pub blocks: Vec<TextBlock>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub format: String,
    pub pages: Vec<Page>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderNode {
    pub id: String,
    pub label: String,
    pub axis: String,
    pub bounds: Bounds,
    pub parent_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub id: String,
    pub name: String,
    pub page_id: String,
    pub bounds: Bounds,
    pub title: Option<Bounds>,
    pub column_header: Option<Bounds>,
    pub row_header: Option<Bounds>,
    pub data: Bounds,
    pub orientation: String,
    pub headers: Vec<HeaderNode>,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub source: SourceRef,
    pub sample_values: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub table_id: String,
    pub attributes: Vec<Attribute>,
    pub row_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from_entity: String,
    pub from_attribute: String,
    pub to_entity: String,
    pub to_attribute: String,
    pub cardinality: String,
    pub confidence: f32,
    pub evidence: String,
    pub status: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageStatus {
    pub page_id: String,
    pub page_name: String,
    pub state: String,
    pub tiles_total: usize,
    pub tiles_completed: usize,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub version: String,
    pub document: Document,
    pub tables: Vec<Table>,
    pub schema: Schema,
    pub pages: Vec<PageStatus>,
    pub warnings: Vec<String>,
    pub model: String,
}
