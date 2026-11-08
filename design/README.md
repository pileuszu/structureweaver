# StructureWeaver design sources

The workbench board is an **editable DesignCraft document**. Its rectangles, paths,
connectors, text and controls were created and rendered by the installed Craft CLI.
It is a UI design reference; the running Rust application's UI is implemented in `src/ui.rs`.

## Reproduce the workbench

```sh
python3 scripts/design_workbench.py
```

Requires `/Applications/DesignCraft.app/Contents/MacOS/designcraft-cli`.
Verified CLI version: **designcraft-cli 0.17.2**. Python uses only its standard library.
The generator executes the equivalent of:

```sh
/Applications/DesignCraft.app/Contents/MacOS/designcraft-cli script \
  design/structureweaver-workbench.commands.json \
  --save design/structureweaver-workbench.designcraft \
  --export design/structureweaver-workbench.png
```

Outputs:

- `structureweaver-workbench.designcraft`: editable 1440 × 960 board.
- `structureweaver-workbench.png`: CLI-rendered 2× preview (2880 × 1920).
- `structureweaver-workbench.commands.json`: reproducible native Craft commands.
- `structureweaver-workbench.validation.json`: CLI version, document inspection,
  text overflow, font availability and preflight results.

The source generator is `scripts/design_workbench.py`. Update its palette and layout
helpers, then regenerate; do not hand-edit the generated command JSON.

## UI reference

The light editor chrome follows [FigCraft](https://github.com/storytold/figcraft) at
commit `8eb0b87646c5aef712326d6c431ccde5408630de`, checked against the installed app.
`figcraft-reference.png` and `figcraft-ui-reference.json` are local, gitignored inspection evidence when present.

- Canvas `#F5F5F5`, white panels, borders `#E6E6E6` / `#D9D9D9`.
- Text `#1E1E1E`, secondary text `#6E6E6E`, tertiary text `#8A8A8A`.
- Active green `#02DA96`, dark accent `#00875F`, selected rows `#E0FBF1`.
- Purple relationships `#9747FF`; 40 px tabbar `#E8E8E8`.
- Compact source/library panel and property inspector around the open canvas.
- Bottom floating toolbar: 48 px high, 13 px radius, green active controls with 5 px radius.

The StructureWeaver name, woven mark, workbook content, ERD semantics and evidence
inspector are its own product design. `brand.figcraft` and `brand.*` are separate
editable/exported brand assets authored with FigCraft.

## Visual verification

The regenerated PNG was visually inspected. Final native preflight reports zero
errors and warnings, no missing glyphs/fonts, and no overset text stories. Korean
uses installed Apple SD Gothic Neo; English identifiers use Arial. Moon and chevron
controls are native vector paths rather than unsupported Unicode glyphs.

The demo shows three entities, both foreign-key candidates, their source cell
ranges and all four LineItems fields. Relationship lines terminate at the actual
PK/FK attribute rows.
