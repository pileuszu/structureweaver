# MVP verification — 2026-11-06 (Asia/Seoul)

The archived evidence uses generated, non-personal fixtures only.

## Original worksheet styling — 2026-11-08

The Rust worksheet viewer was checked against Microsoft Excel for Mac using
`fixtures/styled.xlsx` at the workbook's saved 90% zoom. The fixture opens in Excel
without a repair warning. The native app displays the merged title and note,
fonts and decorations, colored headers, patterned and blank-cell fills, borders,
rotated and shrink-to-fit text, currency/percentage/date/time formats, row heights
and column widths. Scrolling preserves the five frozen rows and first column;
the hidden row and column keep their original source coordinates. macOS worksheet
point sizing is kept separate from application UI typography.

All **49 automated tests** pass, including style inheritance, theme/tint/indexed
colors, worksheet geometry, number-format sections and dates, old analysis JSON
compatibility, and DrawingML extension elements whose `row`/`col` names must not be
treated as worksheet coordinates. Strict Clippy and formatting checks pass. Rich
presentation styles are excluded from LLM context so the original analysis budget
is preserved, and styled blank cells do not schedule inference or inflate missing
data coverage.

This first pass covered stored cell formatting. The subsequent visual-object
verification below extends it with conditional formatting, charts and drawings.
User workbooks and session backups are not part of the archived evidence.

## Conditional formatting, charts and drawings — 2026-11-08

`fixtures/visuals.xlsx` is generated with `scripts/make_visual_fixture.py` and
opens in Excel for Mac without a repair warning. The fixture includes seven
conditional rules, five vector shapes, an embedded image, a linear gradient,
nine charts and a standalone chart sheet. The file's Normal style deliberately
differs from cellXfs[0], testing that object anchor geometry uses the Normal
font's digit width while default cells retain their own font.

Native UI checks cover conditional fill/font colors, duplicate and formula rules,
legacy data-bar lengths, color scales, arrow icons, native shapes/text/image,
gradient appearance, pie rendering without internal triangle seams, scrolling
through the chart gallery and a full-canvas standalone chart. Chart data with no
saved cache is resolved from the fixture's separate data sheet. Excel and the
native application were compared at 100% zoom; some chart auto-layout and
typography differ, so this is not a claim of pixel identity.

Automated coverage additionally checks rule priority and stopIfTrue, absolute and
relative formula references, blank ranges beyond used cells, x14 negative bars,
icon geometry, source-data immutability, chart cache indices/internal references,
negative stacks/log scales, finite native geometry, DrawingML colors, anchors,
groups, VML static controls, gradients and rejection of external media links.
All **83 tests**, strict Clippy, formatting and macOS release packaging pass.
Saved chart point colors are round-tripped through JSON both directly and inside
nested drawing groups, preserving numeric point indices and avoiding tagged-enum
deserialization regressions when reopening exported analyses.

## 3D charts, special effects and layout — 2026-11-08

`fixtures/fidelity.xlsx` contains six native 3D chart families, explicit camera
coordinates, automatic and saved title/legend/plot layouts, and eight DrawingML
effect examples. The synthetic file opens in Excel without a repair dialog.
Its generator explicitly writes view3D for pie/line/area because openpyxl otherwise
omits it. Source values and presentation metadata survive JSON round-tripping.

Native UI inspection confirms 3D columns/pies and effect rendering. Native shadows,
glow, soft edges, reflection and shape depth display in the source canvas. Saved
layout and measured automatic margins have automated coverage, as do finite
projection under extreme camera values, negative pie labels, effect bounds beyond
the anchor, nested groups and malformed saved effect parameters. **98 tests**,
strict Clippy and formatting checks pass. Pure native Office lighting, typography
and layout remain approximate; reflected/projected shape text and combined 3D/2D
effects are specifically disclosed in the source view.

As of 2026-11-09, source rendering uses Rust exclusively and requires no Microsoft
Excel or Office installation. The earlier Excel comparisons are development
checks against synthetic fixtures, not an application runtime dependency.

## Native effects and automatic layout refinements — 2026-11-09

The expanded `fixtures/fidelity.xlsx` includes 14 effect shapes, text-only and
rotated reflections, combined 3D/shadow/reflection, three material/lighting cases,
and dense category/legend examples. Explicit axis visibility, overlay settings
and string category caches make the generated chart intent unambiguous. The file
opens in Excel for Mac without a repair warning; source text, numeric values,
presentation metadata and JSON round-tripping have automated coverage.

The native renderer now captures vector shapes and actual glyphs together,
projects text with the 3D surface, composes effects over the resulting silhouette,
and honors screen-space reflection for `rotWithShape="0"`. Regression tests cover
transparent triangle seams, font-atlas stability, rejected-work caching, rotated
reflection direction and compound 3D/effect bounds. Material, light direction and
light rotation affect diffuse/specular face shading and curved bevels.

Category labels use saved rotation, size, interval and offset; automatic labels
wrap or rotate with measured margins. Legends use measured wrapped heights and
reflow within manual rectangles. Tests cover endpoint anchoring, overlap, hidden
tick labels with preserved gridlines, diagonal glyph clearance and old analysis
defaults. All **110 tests**,
strict Clippy and formatting checks pass for these rendering changes.

Office-specific light intensities, automatic numeric axis ranges/tick intervals,
some camera/bevel presets, picture 3D projection and exact theme typography remain
compatibility limits. These checks demonstrate improved native rendering, not
pixel identity across arbitrary workbooks.

The application settings window contains the server address, model ID/list and
connection action, and opens with the Settings button or Command-comma. Native
UI checks confirmed model discovery and selection; navigation and sheet controls
are ordinary buttons, with no switch, checkbox or selectable-toggle controls.
Worksheet visibility actions live in an explicit View menu. The final macOS
package includes a versioned arm64 DMG, an Applications symlink, installation
notes and SHA256. The image was mounted read-only, its application signature and
version checked, and a copy launched independently of the user's active analysis.
This is an ad hoc signed local preview, without Apple notarization.

## Local model inference — 2026-11-06

| Case | Actual local inference outcome |
| --- | --- |
| `fixtures/demo.xlsx` | Gemma through Llama.app: 3/3 sheets complete, 3 entities, 2 many-to-one relationship candidates (Orders.customer_id → Customers.customer_id; LineItems.order_id → Orders.order_id). |
| `fixtures/wide.xlsx` | Gemma: two 10-column tiles reconciled into 1 entity, 12 attributes, 3 records. |
| `fixtures/matrix.xlsx` | Gemma with final merged rendering/normalization: complete; header C1:F3, row hierarchy A4:B7, data C4:F7; 16 observations, 2 row dimensions, 3 column levels and value. |

Each subfolder contains the actual exported `analysis.json`, `schema.mmd`, `schema.sql`. Every SQL file was executed successfully against in-memory SQLite. These are small smoke examples, not an accuracy benchmark.

Model: `unsloth/gemma-4-E4B-it-GGUF:Q4_K_XL`; endpoint `http://127.0.0.1:9931/v1/chat/completions`. No remote inference provider was used. Before merged-header repair, both Gemma and Qwen failed the complex matrix fixture; the application reported failed/partial pages rather than a complete ERD. The final pipeline additionally preserves true visual merge spans and derives missing header levels only from actual merged text anchors. Model behavior can still vary on other documents.

Automated tests cover all sheets including hidden/empty, content at row450/column66, merged extents and hierarchy, formulas and cached values, resource limits, invalid geometry, unclassified source gaps, matrix labels, column/row parents, false PK/FK relationships, SQL escaping, HTTP image+schema contracts, error responses and local endpoint restrictions.

Commands:

```sh
./scripts/cargo.sh test --locked
./scripts/cargo.sh clippy --all-targets --locked -- -D warnings
./scripts/cargo.sh fmt -- --check
./scripts/package-macos.sh
```

Native UI was manually inspected after packaging: the connection button listed three installed models; selecting the Orders entity navigated to source cell A2 (value 1001); the native export folder dialog produced all three output files in `/private/tmp/structureweaver-ui-export`. The exported JSON contained three entities and two relationship candidates, and its SQL executed successfully in SQLite. Opening `fixtures/demo.xlsx` through the native file picker and pressing the analysis button completed all three sheets with Gemma, yielding three entities and two relationship candidates. The coverage tab showed each sheet complete (1/1 tile) and the execution log. The actual application screenshot was checked against the final light FigCraft-style layout. See `design/` for editable Craft sources and exported visual references.

Design provenance: `scripts/design_workbench.py` invokes the installed `designcraft-cli script … --save … --export …`; its CLI/version and preflight results are in `design/structureweaver-workbench.validation.json`. `scripts/design_brand.py` sends commands to FigCraft’s local TCP control endpoint. The exported Craft board is a design reference, not a screenshot of the running Rust UI.
