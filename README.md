# RustText

A lightweight, cross-platform rich text editor written in Rust.

## Features

* **File formats** – open, edit and save **Word (.docx)**, **Rich Text
  (.rtf)**, **Markdown (.md)** and **plain text (.txt)**.
* **Text formatting** – bold, italic, underline, strikethrough, any installed
  system font, font size, text color, headings.
* **Paragraphs** – left / center / right / justified alignment, bulleted and
  numbered lists with nesting.
* **Tables** – insert tables, insert rows above/below and columns left/right,
  delete rows/columns/tables, format text inside cells, cell background and
  text colors, cell alignment.
* **Clipboard** – cut, copy and paste text, formatted text, whole tables and
  rectangular selections of table cells.
* Undo / redo, zoom, word count, unsaved-changes protection.

## Building

Requires [Rust](https://rustup.rs) 1.85 or newer.

```bash
cargo run --release                 # build and start
cargo run --release -- notes.docx   # open a file
```

Packaged builds:

| Platform | Command |
|----------|---------|
| Linux    | `scripts/build-linux.sh` |
| macOS    | `scripts/build-macos.sh` (add `--universal` for Intel + Apple Silicon) |
| Windows  | `scripts\build-windows.bat` (or `scripts\build-windows.ps1`) |

Results are written to `dist/`.

## Using the editor

* Click to place the cursor, drag to select. Double-click selects a word,
  triple-click a paragraph.
* Dragging across table cells selects a block of cells; copy it and paste
  into another table (overwrites cells) or into normal text (creates a new
  table). **Table → Select table** selects a whole table.
* In tables, **Tab** / **Shift+Tab** move between cells; Tab in the last
  cell adds a row. In lists, Tab / Shift+Tab change the nesting level.

### Keyboard shortcuts

(`Ctrl` is `Cmd` on macOS)

| Action | Shortcut |
|--------|----------|
| New / Open / Save / Save as | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S |
| Undo / Redo | Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) |
| Cut / Copy / Paste | Ctrl+X / Ctrl+C / Ctrl+V |
| Select all | Ctrl+A |
| Bold / Italic / Underline | Ctrl+B / Ctrl+I / Ctrl+U |
| Word left / right | Ctrl+← / Ctrl+→ (Alt on macOS) |
| Start / end of document | Ctrl+Home / Ctrl+End |
| Zoom in / out / reset | Ctrl++ / Ctrl+- / Ctrl+0 |

## Format notes

Markdown cannot store fonts, colors or paragraph alignment (except per table
column), and plain text stores no formatting. Use DOCX or RTF to keep all
formatting. See [develop.md](develop.md) for the full feature matrix.

## Development

See [develop.md](develop.md) for the architecture and a guide to extending
the editor.
