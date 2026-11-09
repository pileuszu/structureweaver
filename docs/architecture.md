# StructureWeaver MVP architecture

## Boundaries

| Module | Responsibility |
| --- | --- |
| `domain.rs` | Serializable document, grid/page source coordinates, table regions, header trees, relational draft and page status |
| `ingest.rs` | `DocumentAdapter`, Excel OOXML metadata + calamine typed values, input/resource validation |
| `xlsx_styles.rs` | Workbook style inheritance, font/alignment/border/pattern metadata, resolved theme/indexed colors |
| `number_format.rs` | Presentation-only Excel number/date/time/currency formats; preserves analysis values |
| `conditional.rs` / `conditional_icons.rs` | Stored conditional rules, bounded formula evaluation and priority-aware style overlays; native icon geometry |
| `charts.rs` | Chart metadata, cached/internal range data and native chart geometry, including standalone chart sheets |
| `drawings.rs` / `ooxml.rs` | Bounded OPC relationships, anchored shapes/groups/images/VML appearance, text and native drawing renderer |
| `chart_3d.rs` | Depth-sorted 3D chart meshes, camera projection and bounded scene geometry |
| `drawing_effects.rs` | Bounded alpha-mask compositing for shadows, glow, soft edges and reflection; projected shape depth/bevel |
| `sheet_view.rs` | Read-only native worksheet canvas with source fonts, geometry, merges, frozen panes and viewport culling |
| `render.rs` | Unicode text and cell-type overlay PNGs with global row/column labels |
| `llama.rs` | Local-only blocking HTTP transport, model discovery, vision + JSON schema requests, bounded errors/timeouts |
| `analysis.rs` | Tile scheduling, structured decisions, validation/retry, whole-sheet reconciliation, coverage, hierarchy |
| `schema.rs` | Full-source value profiles, records/matrix conversion, candidate keys and relationships |
| `export.rs` | Portable analysis JSON, escaped Mermaid and SQLite DDL drafts |
| `ui.rs` | Native egui UI, background worker, source inspection and ERD rendering |
| `main.rs` | GUI startup and reproducible CLI |

## Pipeline invariants

Coordinates are one-based and inclusive. Source page IDs preserve workbook order; the document SHA-256 identifies original bytes. Hidden cells are included by default. Formula text and cached value remain separate.

Every populated cell belongs to exactly one scheduled tile. Empty physical tiles and style-only blank cells do not consume model calls. Blank formatted cells are still preserved for display. The VLM sees exact cell data and a visual overlay; long visual labels are clipped for readability but the JSON facts are not shortened. Merged anchor values are supplied even if the anchor is outside the current tile. The prompt uses compact bold/indent/fill hints, so full viewer styles do not inflate the model context.

Raw rectangles must first be within the source tile/table. Luna-style deterministic normalization separates overlapping title/header/data bands; explicit merged text spans close missing parent-header levels and adjacent category columns under a blank header corner. These corrections are identified in table evidence and never synthesize source values. Final table and component rectangles must be valid and disjoint; headers must align with the data axis. Record tables cannot classify a leading text field as a row header. The model can return multiple independent tables. A failed tile does not erase other successful sheets. Invalid tile inference responses receive one bounded retry. Input/context-budget errors fail explicitly before inference; whole-sheet reconciliation makes one request and retains fragments on failure.

For multi-tile sheets a second structured inference sees table fragments, header/sample source values and merge geometry, and reconstructs full global table ranges. Validation checks sheet bounds, mutually exclusive tables, region geometry and that previously identified data cells remain data. A failed reconciliation preserves fragments and marks the page partial; no geometric-only merge silently asserts semantic equivalence.

Coverage counts the union of title, column-header, row-header and data regions, not the bounding box of a table. Notes/unresolved cells remain in the document and produce partial status. This is structural coverage, not an accuracy measurement.

Header hierarchy uses merged span containment, position and explicit row-label indentation. Blank unmerged cells are not blindly forward-filled. Every attribute retains its source coordinates. A header parent is a document context relation, not automatically an ERD foreign key or class inheritance.

PK candidates need non-null, unique text/integer IDs in at least two observations and plausible ID names; equally plausible competing keys remain unresolved. FK candidates need a unique target PK, compatible types, semantic ID names, at least two distinct values and complete inclusion of observed non-null values. A numeric overlap alone cannot create a relation. All FK outputs require review; SQL keeps them as comments.

## Adding PDF and PowerPoint

Implement `DocumentAdapter` for the new input. Use `PageKind::PdfPage` or `Slide`, `TextBlock` and `SourceRegion::PageRect` for native page positions. Add a format-specific visual layout analyzer producing a neutral table/value representation; the current grid-only analyzer must not pretend that page rectangles are spreadsheet cells. Introduce a normalized observation boundary for the relational extractor, retain source references, then reuse schema/ERD rendering and exports. OCR/rendering belongs in that adapter and should expose missing text/unsupported content explicitly.

## Reference mapping

Reference commit: `bist-mini-final/bist-mini-final@0e0d7638b96c9f9db816c3959cec6064a40b5045`.

- `modules/structure/luna_vlm_structure_detector.py`: image+cell-context classification, rectangular table/region contracts and validation.
- `backend/domains/data_sources/infrastructure/spreadsheets/prompt_guidance.py`: record vs matrix semantics and missing-value data handling.
- `grid_structure.py`: merged-span header parent construction. The BFS helpers in that file are not the current Luna detector's table-detection algorithm.
- `structured_cell_text.py`: preserve row/column context and merged anchor identity. StructureWeaver exports relational drafts instead of RAG text chunks.

New behavior: include hidden content, visit all populated tiles, reconcile whole-sheet fragments, infer/inspect ERD candidates, local Llama transport and native desktop UI. No Python runtime is used by the application. Basic fixtures use a development-only Python standard library generator; the visual styling fixture additionally uses openpyxl to normalize its OOXML package.

## Source appearance

Cells, charts, drawings and effects are rendered entirely in Rust. Microsoft Excel and Office are not runtime dependencies, and the application does not delegate rendering to an installed office application.

`Cell.value` remains the typed source text for analysis. `raw_number` retains numeric OOXML values (including date serials); `display_value` and the stored number format are for presentation only. `Grid.layout` records dimensions, row/column defaults, hidden axes, gridlines, zoom and frozen panes. New style/layout fields use serde defaults so previously exported analyses remain readable; their original workbook must be reopened to recover styling that old exports did not store.

The source canvas uses the document font family/weight from installed TTF/OTF/TTC faces, with an explicit fallback notice when unavailable. It draws only visible rows/columns and merged intersections. Analysis outlines are opt-in and do not recolor source cells. Unsupported visual constructs are detected and reported; the viewer is not a complete Excel rendering engine. See README for current limits.

Conditional rules are stored separately in `Grid.layout.conditional_formats`. The viewer prepares per-cell appearance once per sheet, merges only explicitly set DXF properties in priority order, and stops lower-priority rules where required. These display cells never replace the analysis grid. Number-format caches are invalidated only for display overrides; original values/formulas/styles remain intact.

`Grid.layout.drawings` stores anchored, ordered objects and embedded picture bytes. EMU coordinates are mapped through the same row/column geometry used by the cells; frozen panes clip each drawing in the corresponding viewport. Drawing extents extend the visible canvas without creating analysis records. After all sheets are ingested, chart ranges without caches resolve against local cells. Standalone chart sheets use a chart-only canvas and have no fabricated cell values. VML controls are inert drawings: macros, external media and linked workbooks are never executed or fetched.

Shape effects capture the native vector geometry and glyph atlas into a bounded offscreen texture before compositing shadows, reflections, glow and soft edges. This includes projected text and extruded faces. Texture caching avoids rasterizing unchanged shapes on every repaint; rejected work is bounded as well. Source rotation and effect rotation are separate, preserving screen-space reflection when `rotWithShape` is false. DrawingML light direction/rotation and material parameters affect face and bevel shading; proprietary Office lighting presets remain approximations.

Chart layout measures the same glyph bounds used for painting. Saved axis rotation, label interval, offset and font size override automatic choices; automatic labels wrap or rotate before increasing their interval. Legends reflow after saved manual dimensions are resolved. New presentation fields default safely when reopening older exported analyses. Automatic numeric axis ranges and exact Office theme typography remain separate compatibility limits.

Visual parsing bounds XML depth/size, decoded images, objects and chart series/points. Conditional evaluation caps rule/total cells and reports any skipped range. Unsupported effects and formulas remain explicit diagnostics rather than guessed output. The portable analysis JSON includes these visuals with backward-compatible defaults for old exports.

## UI style reference

The user selected [FigCraft](https://github.com/storytold/figcraft) as the visual reference. We inspected the running application and its public `crates/ui-egui/src/theme.rs` and `toolbar.rs` at commit `8eb0b87646c5aef712326d6c431ccde5408630de`. The resulting interface follows its light editor style: white side panels, #F5F5F5 canvas, #E6E6E6 dividers, #1E1E1E text, green selection/actions, compact document bar and floating canvas tools. StructureWeaver retains its own original woven icon, data workflow and original Rust UI implementation; FigCraft branding and proprietary third-party product assets are not embedded.

The editable workbench composition is authored with DesignCraft's installed CLI. The original vector icon is authored and exported using FigCraft's documented local control protocol. Reproduction command sources live in `scripts/design_workbench.py` and `scripts/design_brand.py`; editable documents and rendered assets live in `design/`.

Server and model controls are confined to the Settings window (button or Command-comma). Workspace navigation uses ordinary buttons with text/line selection indicators. Worksheet visibility uses explicit menu commands; the interface avoids switch, checkbox and selectable-toggle styling.
