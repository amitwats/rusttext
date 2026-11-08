# RustText – Developer Guide

This document describes the high-level design of RustText and explains how
to extend it. For end-user documentation see [README.md](README.md).

---

## 1. Goals

* A small, dependable **rich text editor** in pure Rust.
* Read, edit and save **TXT, Markdown, RTF and DOCX**.
* Character formatting (bold, italic, underline, strikethrough, font family
  from the installed system fonts, size, color), paragraph formatting
  (alignment, bullets, numbering, indentation, headings) and **tables**
  (insert/delete rows and columns, cell colors, cell alignment, formatted
  cell text).
* Cut / copy / paste of text, formatted text, whole tables and selected
  table cells.
* Modular code with a GUI-independent core that can be unit-tested.

## 2. Technology

| Concern            | Crate                                   |
|--------------------|-----------------------------------------|
| GUI / windowing    | `eframe` + `egui` 0.37 (OpenGL via glow)  |
| File dialogs       | `rfd` (native dialogs / XDG portal)      |
| System clipboard   | `arboard` (menu paste), egui events (keyboard) |
| System fonts       | `fontdb`                                |
| Markdown           | `pulldown-cmark` (reading); writer is hand-written |
| DOCX               | `zip` + `quick-xml` (hand-written OOXML reader/writer) |
| RTF                | hand-written tokenizer, parser and writer |
| Errors / logging   | `thiserror`, `anyhow`, `log`, `env_logger` |

Rust edition 2024 (Rust ≥ 1.85).

## 3. Project layout

```text
rusttext/
├── Cargo.toml
├── README.md               user documentation
├── develop.md              this file
├── scripts/                build & packaging scripts
│   ├── build-linux.sh
│   ├── build-macos.sh      (creates RustText.app, optional universal binary)
│   ├── build-windows.ps1
│   └── build-windows.bat   (wrapper for the PowerShell script)
├── src/
│   ├── main.rs             binary entry point (CLI args, window setup)
│   ├── lib.rs              crate root, module overview
│   ├── document/           GUI-independent model + editing engine
│   │   ├── model.rs        Document, Block, Paragraph, Run, Table, Row, Cell
│   │   ├── style.rs        CharStyle, ParagraphStyle, CellStyle, Color, …
│   │   ├── position.rs     ParaPath, TextPos, Selection, SelectionKind
│   │   ├── session.rs      EditSession: navigation & text editing
│   │   ├── formatting.rs   character / paragraph formatting commands
│   │   ├── table_ops.rs    table structure commands
│   │   ├── clipboard.rs    cut / copy / paste
│   │   └── history.rs      undo / redo
│   ├── formats/            file format codecs
│   │   ├── mod.rs          Codec trait, FileFormat, load()/save()
│   │   ├── txt.rs
│   │   ├── markdown.rs
│   │   ├── rtf/{mod,reader,writer}.rs
│   │   └── docx/{mod,reader,writer}.rs
│   ├── fonts/mod.rs        FontManager: system font discovery & egui registration
│   ├── editor/             the egui editing widget
│   │   ├── layout.rs       document → positioned galleys, hit testing
│   │   ├── render.rs       painting (page, selection, text, grid, caret)
│   │   └── widget.rs       EditorView: input handling
│   └── app/                the desktop application
│       ├── mod.rs          RustTextApp: panels, command dispatch, file handling
│       ├── commands.rs     Command enum (every user action)
│       ├── menu.rs         menu bar
│       ├── toolbar.rs      formatting toolbar
│       ├── icons.rs        vector-drawn toolbar icons
│       ├── dialogs.rs      modal dialogs
│       └── file_ops.rs     open/save dialogs, clipboard access
├── tests/
│   ├── format_files.rs     save/load every format through the filesystem
│   └── gui_headless.rs     drives the full UI with synthetic input events
└── examples/
    └── screenshot.rs       renders the UI to a PNG without a window or GPU
```

## 4. Architecture

The crate is layered; dependencies only point downwards:

```text
        ┌──────────────────────────────────────────────┐
        │ app  (menus, toolbar, dialogs, file handling) │
        └──────┬───────────────┬──────────────┬────────┘
               │               │              │
        ┌──────▼──────┐  ┌─────▼─────┐  ┌─────▼─────┐
        │   editor    │  │  formats  │  │   fonts   │
        │ (egui view) │  │  (codecs) │  │ (fontdb)  │
        └──────┬──────┘  └─────┬─────┘  └───────────┘
               │               │
        ┌──────▼───────────────▼──────┐
        │          document           │   no GUI dependencies
        │ (model + editing commands)  │
        └─────────────────────────────┘
```

* **document** knows nothing about egui or files. Everything the user can do
  to a document is a method on `EditSession`, which makes the behaviour easy
  to unit-test.
* **formats** convert between bytes and `Document`.
* **editor** turns a `Document` into pixels and turns input events into
  `EditSession` calls.
* **app** wires everything together. UI elements never modify the document
  directly: they emit `Command`s, which `RustTextApp::execute` carries out.

### 4.1 Document model

```text
Document
 └─ blocks: Vec<Block>
     ├─ Block::Paragraph(Paragraph { runs: Vec<Run>, style: ParagraphStyle })
     └─ Block::Table(Table { rows: Vec<Row { cells: Vec<Cell> }> })
                                         Cell { paragraphs: Vec<Paragraph>, style: CellStyle }
Run { text: String, style: CharStyle }
```

* A **run** is text with uniform `CharStyle`. `Paragraph::normalize` merges
  equal neighbours and drops empty runs, so the run list stays minimal.
* Paragraph text never contains `\n`; a line break is a new paragraph.
  Tabs are stored as `\t`.
* **Offsets are character offsets**, never byte offsets.
* `Document::normalize` enforces invariants the editor relies on: at least
  one block, a paragraph before and after every table (so the caret can
  always be placed around it), rectangular tables, non-empty cells.
  It is called after every edit.

### 4.2 Positions and selections

* `ParaPath { block, cell: Option<(row, col)>, para }` addresses a paragraph.
  Its derived ordering is the reading order.
* `TextPos { path, offset }` is a caret position.
* `Selection { anchor, head }` is resolved by `Selection::kind()` into:
  * `Caret` – nothing selected;
  * `Range` – contiguous text in one container (top-level flow or one cell);
    tables between top-level endpoints are selected entirely;
  * `Cells` – a rectangle of table cells (anchor and head in different cells
    of the same table).
  A selection from body text into a table is widened to cover the whole table.

### 4.3 Editing commands, undo and the clipboard

Every mutating command follows the same pattern:

```rust
self.begin_edit(EditKind::Other); // snapshot for undo, mark modified
/* mutate self.doc / self.selection */
self.end_edit();                  // normalize document, clamp selection
```

* **Undo/redo** (`history.rs`) stores whole-document snapshots (documents are
  small; snapshots are simple and robust). Consecutive typing or deleting is
  merged into one step; moving the caret ends the merge.
* **Typing style**: toggling bold etc. with no selection sets
  `EditSession::typing_style`, used for the next inserted text.
* **Clipboard** (`clipboard.rs`): `ClipContent::Blocks` (formatted paragraphs
  and whole tables) or `ClipContent::Cells` (a cell rectangle). The rich
  content is kept inside the session; its plain-text rendition (tables as
  TSV) goes to the system clipboard. On paste, if the system clipboard text
  equals the rich content's text, the rich content is pasted; otherwise the
  foreign plain text is inserted. Pasting cells into a table overwrites
  cells starting at the caret (growing the table); pasting cells into body
  text creates a new table; tables pasted into a cell become tab-separated
  paragraphs (tables are not nested).

### 4.4 Layout and rendering (`editor/`)

* `layout.rs` builds one egui `LayoutJob` per paragraph (one section per run)
  and lets egui shape, wrap and align it. List markers, indentation and table
  grids are added around the galleys. Tables use equal column widths; row
  heights fit the tallest cell.
* Missing bold/italic font faces are synthesized: italics via egui's skew,
  bold by painting a second, offset copy of the bold glyphs.
* Hit testing (`DocLayout::hit_test`) and vertical caret movement
  (`DocLayout::vertical_neighbor`) work on the laid-out rows, so they behave
  correctly across wrapped lines, paragraphs and table cells.
* The layout is rebuilt every frame; egui caches galleys internally, which is
  fast enough for typical documents (see *Performance* below).
* `widget.rs` translates egui events into `EditSession` calls, keeps the caret
  visible, blinks it and owns transient UI state (zoom, preferred column).

### 4.5 Fonts (`fonts/`)

`FontManager` scans installed fonts with `fontdb` at start-up. A family is
loaded into egui only when it is used (chosen in the toolbar or present in a
document): its regular, bold, italic and bold-italic faces are registered as
the egui families `"<name>#r"`, `"#b"`, `"#i"`, `"#bi"`, each followed by
egui's default fonts as glyph fallbacks. egui applies new fonts at the start
of the next frame, so `FontManager` only reports a family as usable after
that (until then text falls back to the default font for a single frame).
Documents keep the requested font name even if it is not installed, so it is
preserved when saving.

### 4.6 File formats (`formats/`)

All formats implement:

```rust
pub trait Codec {
    fn read(&self, bytes: &[u8]) -> Result<Document>;
    fn write(&self, doc: &Document) -> Result<Vec<u8>>;
}
```

`formats::save` writes to a temporary file and renames it, so a failed save
never destroys the original.

| Feature              | DOCX | RTF | Markdown            | TXT              |
|----------------------|:----:|:---:|---------------------|------------------|
| Bold / italic        |  ✓   |  ✓  | ✓                   | –                |
| Underline            |  ✓   |  ✓  | ✓ (`<u>…</u>`)      | –                |
| Strikethrough        |  ✓   |  ✓  | ✓ (`~~…~~`)         | –                |
| Font family / size   |  ✓   |  ✓  | –                   | –                |
| Text color           |  ✓   |  ✓  | –                   | –                |
| Paragraph alignment  |  ✓   |  ✓  | tables only (per column) | –           |
| Headings             |  ✓   |  ✓ (`\outlinelevel`) | ✓     | –                |
| Bullets / numbering  |  ✓   |  ✓  | ✓                   | textual markers  |
| Tables               |  ✓   |  ✓  | ✓ (GFM)             | tab separated    |
| Cell background      |  ✓   |  ✓  | –                   | –                |
| Cell text color      | as run color | as run color | – | –               |

Implementation notes:

* **DOCX** – the writer emits the minimal package Word needs
  (`[Content_Types].xml`, relationships, `document.xml`, `styles.xml` with
  Heading 1–6 and Table Grid, `numbering.xml`). Each numbered list gets its
  own `w:num` so numbering restarts per list. The reader streams
  `document.xml`, ignores drawings, text boxes, deleted text and field
  instructions, maps `numbering.xml` formats to bullet/numbered and detects
  headings from `styles.xml` (falling back to the built-in style ids).
  Nested tables are flattened into the outer cell.
* **RTF** – the tokenizer handles control words, symbols, `\'hh` (CP-1252)
  and `\uN` (with `\ucN` fallback skipping and surrogate pairs). Lists are
  written with the legacy `\pn` syntax understood by WordPad, Word,
  LibreOffice and TextEdit; Word's `\listtext` is also understood on read.
* **Markdown** – CommonMark + GFM tables/strikethrough. The writer escapes
  special characters and moves whitespace outside emphasis markers.
* **TXT** – UTF-8 (with/without BOM), UTF-16 (BOM) and a Windows-1252
  fallback when reading; UTF-8 when writing.

## 5. Building and testing

```bash
cargo run                         # debug build
cargo run --release -- file.docx  # open a file
cargo test                        # unit + integration tests
cargo clippy --all-targets        # lints (kept warning-free)
cargo run --example screenshot -- out.png [document]   # headless screenshot
```

Packaging scripts (each runs the tests first unless told otherwise):

| Platform | Command                                              | Output |
|----------|------------------------------------------------------|--------|
| Linux    | `scripts/build-linux.sh [--debug] [--skip-tests]`    | `dist/linux/rusttext`, `dist/rusttext-<ver>-linux-<arch>.tar.gz` |
| macOS    | `scripts/build-macos.sh [--universal] [--skip-tests]`| `dist/macos/RustText.app`, `dist/rusttext-<ver>-macos.zip` |
| Windows  | `scripts\build-windows.bat [-SkipTests] [-Profile debug]` | `dist\windows\rusttext.exe`, `dist\rusttext-<ver>-windows-<arch>.zip` |

On Linux, the binary needs an X11 or Wayland session plus OpenGL and
`libxkbcommon` at run time (present on every desktop distribution).
Building needs no extra system packages.

### Test strategy

* **Unit tests** next to the code (`#[cfg(test)]`) cover the model, every
  editing command, the clipboard, undo and each codec (including round trips
  and parsing of files produced by other programs).
* **`tests/format_files.rs`** saves a feature-rich document in every format
  and loads it back. Set `RUSTTEXT_SAMPLE_DIR=/some/dir` to keep the files
  for manual inspection in Word / LibreOffice.
* **`tests/gui_headless.rs`** runs the whole application UI in an
  `egui::Context` without a window and feeds it keyboard, mouse and clipboard
  events – it catches layout and input regressions on CI machines.
* **`examples/screenshot.rs`** renders the UI to PNG with a tiny CPU
  rasterizer for visual checks without a display.

## 6. How to enhance RustText

### 6.1 Add a new user action (menu item / toolbar button / shortcut)

1. Implement the behaviour as a method on `EditSession` (in the topical file
   under `src/document/`), wrapped in `begin_edit` / `end_edit`. Add a unit
   test next to it.
2. Add a variant to `Command` in `src/app/commands.rs`.
3. Handle it in `RustTextApp::execute` (`src/app/mod.rs`).
4. Emit it from `menu.rs`, `toolbar.rs` and/or `RustTextApp::shortcuts`.

### 6.2 Add a character attribute (e.g. superscript, highlight)

1. Add a field to `CharStyle` (`style.rs`). Runs with different values are
   automatically kept separate.
2. If it is on/off, add a `ToggleAttr` variant (`formatting.rs`).
3. Render it in `LayoutCtx::text_format` (`editor/layout.rs`) – e.g.
   `TextFormat::background` for highlight or `valign` + smaller size for
   superscript.
4. Read/write it in each codec (`w:highlight`/`w:vertAlign` in DOCX,
   `\highlight`/`\super` in RTF, …) and extend the round-trip tests.
5. Expose it through a `Command` (6.1).

### 6.3 Add a paragraph attribute (e.g. line spacing)

Add the field to `ParagraphStyle`, apply it in `LayoutCtx::paragraph`
(spacing/height) and in the codecs, then expose a command using
`EditSession::apply_paragraph_style`.

### 6.4 Add a file format (e.g. HTML or ODT)

1. Create `src/formats/<name>.rs` (or a folder) implementing `Codec`.
2. Add a `FileFormat` variant with its label and extensions, and return the
   codec from `FileFormat::codec`. The open/save dialogs pick it up
   automatically.
3. Add round-trip tests and a case to `tests/format_files.rs`.

### 6.5 Table enhancements

* **Column widths** – add `column_widths: Vec<f32>` (fractions) to `Table`,
  use it in `LayoutCtx::table`, read/write `w:gridCol` / `\cellx`, and add
  drag handles on column edges in `widget.rs`.
* **Merged cells** – add `col_span`/`row_span` to `Cell`; layout must skip
  covered grid positions; selection rectangles should expand to whole merged
  cells (`Selection::kind`).
* **Borders** – add a border style to `CellStyle` and paint it in
  `render.rs` instead of the uniform grid.

### 6.6 Other ideas

* Find & replace – search runs' text, select matches with `EditSession::select`.
* Images – a new `Block::Image` (or inline run kind), DOCX `w:drawing`
  parsing, an egui texture per image.
* Spell checking – underline misspelled words with a wavy stroke in
  `render.rs`.
* Printing / PDF export – a new codec that paginates the `DocLayout`.
* Hyperlinks – a `link: Option<String>` in `CharStyle`.
* Auto-save / recent files – persist with `eframe::Storage` (`App::save`).

### 6.7 Performance

Layout is rebuilt every frame. For very large documents, cache
`ParaLayout`s per paragraph keyed by (paragraph content hash, width, zoom)
and only lay out paragraphs that intersect the visible rectangle
(`ui.clip_rect()`), keeping their heights from the previous frame.
`EditSession::revision` increments on every change and can be used to
invalidate caches cheaply. Undo snapshots can be replaced by an operation
log if memory becomes a concern.

## 7. Conventions

* Keep `document/` free of GUI types; use `Color`, not `Color32`.
* All text offsets are character offsets; convert with `model::byte_offset`.
* New mutating commands must use `begin_edit` / `end_edit`.
* UI code emits `Command`s instead of editing the document directly.
* `cargo fmt`, `cargo clippy --all-targets` and `cargo test` must pass.

## 8. Known limitations

* Tables use equal column widths and do not support merged cells or nesting.
* Paragraph spacing and line spacing are fixed.
* Images, headers/footers, footnotes and comments in imported DOCX/RTF files
  are ignored (their text is not imported).
* Markdown cannot store fonts, colors or alignment outside tables; plain
  text stores no formatting at all. The status bar mentions this when saving.
* Right-to-left scripts and complex shaping follow egui's text engine.
