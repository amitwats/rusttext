//! Cut / copy / paste of text, formatted text, tables and table cells.
//!
//! The session keeps a *rich* internal clipboard ([`ClipContent`]). The GUI
//! additionally places the plain-text rendition on the system clipboard so
//! other applications can paste it. When pasting, the GUI compares the
//! system clipboard text with the internal clipboard's plain text: if they
//! match the rich content is pasted, otherwise the plain system text is.

use super::history::EditKind;
use super::model::{Block, Cell, Paragraph};
use super::position::{ParaPath, Selection, SelectionKind, TextPos};
use super::session::EditSession;
use super::table_ops::table_to_paragraphs;

/// Rich clipboard content.
#[derive(Clone, PartialEq, Debug)]
pub enum ClipContent {
    /// A document fragment: formatted paragraphs and/or whole tables.
    Blocks(Vec<Block>),
    /// A rectangular block of table cells.
    Cells(Vec<Vec<Cell>>),
}

impl ClipContent {
    /// Plain-text rendition (tables as tab separated values).
    pub fn plain_text(&self) -> String {
        match self {
            ClipContent::Blocks(blocks) => blocks
                .iter()
                .map(Block::plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            ClipContent::Cells(rows) => rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| c.plain_text().replace('\n', " "))
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// Normalizes line endings so clipboard text from different platforms compares equal.
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

impl EditSession {
    /// Returns the selected content, or `None` when nothing is selected.
    pub fn copy_selection(&self) -> Option<ClipContent> {
        match self.selection_kind() {
            SelectionKind::Caret(_) => None,
            SelectionKind::Cells { block, rows, cols } => {
                let t = self.doc.blocks.get(block)?.as_table()?;
                Some(ClipContent::Cells(
                    (rows.0..=rows.1)
                        .map(|r| {
                            (cols.0..=cols.1)
                                .filter_map(|c| t.cell(r, c).cloned())
                                .collect()
                        })
                        .collect(),
                ))
            }
            SelectionKind::Range { start, end } => {
                if start.path.cell.is_some() {
                    // Paragraphs inside one cell.
                    let mut out = Vec::new();
                    for pi in start.path.para..=end.path.para {
                        let path = ParaPath {
                            para: pi,
                            ..start.path
                        };
                        let p = self.doc.paragraph(path)?;
                        let s = if pi == start.path.para {
                            start.offset
                        } else {
                            0
                        };
                        let e = if pi == end.path.para {
                            end.offset
                        } else {
                            p.char_len()
                        };
                        out.push(Block::Paragraph(p.slice(s, e)));
                    }
                    return Some(ClipContent::Blocks(out));
                }
                let mut out = Vec::new();
                for bi in start.path.block..=end.path.block {
                    match &self.doc.blocks[bi] {
                        Block::Paragraph(p) => {
                            let s = if bi == start.path.block {
                                start.offset
                            } else {
                                0
                            };
                            let e = if bi == end.path.block {
                                end.offset
                            } else {
                                p.char_len()
                            };
                            out.push(Block::Paragraph(p.slice(s, e)));
                        }
                        Block::Table(t) => out.push(Block::Table(t.clone())),
                    }
                }
                Some(ClipContent::Blocks(out))
            }
        }
    }

    /// Copies the selection to the internal clipboard and returns its plain text.
    pub fn copy(&mut self) -> Option<String> {
        let c = self.copy_selection()?;
        let text = c.plain_text();
        self.clipboard = Some(c);
        Some(text)
    }

    /// Cuts the selection to the internal clipboard and returns its plain text.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.delete_selection();
        Some(text)
    }

    /// Pastes the internal clipboard if its text matches `system_text`
    /// (or if `system_text` is `None`), otherwise pastes `system_text` as
    /// plain text.
    pub fn paste_from(&mut self, system_text: Option<&str>) {
        let rich = match (&self.clipboard, system_text) {
            (Some(c), None) => Some(c.clone()),
            (Some(c), Some(t)) if normalize_newlines(&c.plain_text()) == normalize_newlines(t) => {
                Some(c.clone())
            }
            _ => None,
        };
        match (rich, system_text) {
            (Some(c), _) => self.paste(c),
            (None, Some(t)) => self.insert_text(t),
            (None, None) => {}
        }
    }

    /// Pastes rich content at the caret (replacing the selection).
    pub fn paste(&mut self, content: ClipContent) {
        self.begin_edit(EditKind::Other);
        // Pasting cells onto a cell selection writes to its top-left corner.
        let cell_anchor = match self.selection_kind() {
            SelectionKind::Cells { block, rows, cols } => Some((block, rows.0, cols.0)),
            _ => None,
        };
        match content {
            ClipContent::Cells(cells) => {
                if let Some((b, r, c)) = cell_anchor {
                    self.overwrite_cells_raw(b, r, c, cells);
                } else {
                    self.delete_selection_raw();
                    let path = self.caret().path;
                    if let Some((r, c)) = path.cell {
                        self.overwrite_cells_raw(path.block, r, c, cells);
                    } else {
                        let table = super::model::Table::from_cells(cells);
                        self.paste_blocks_raw(vec![Block::Table(table)]);
                    }
                }
            }
            ClipContent::Blocks(blocks) => {
                self.delete_selection_raw();
                self.paste_blocks_raw(blocks);
            }
        }
        self.end_edit();
    }

    fn paste_blocks_raw(&mut self, blocks: Vec<Block>) {
        if blocks.is_empty() {
            return;
        }
        let pos = self.caret();
        let in_cell = pos.path.cell.is_some();

        // Inside a cell only paragraphs are allowed: flatten tables.
        let mut items: Vec<Block> = Vec::new();
        for b in blocks {
            match b {
                Block::Table(t) if in_cell => {
                    items.extend(table_to_paragraphs(&t).into_iter().map(Block::Paragraph))
                }
                Block::Table(t) => {
                    // Keep a paragraph between adjacent tables.
                    if matches!(items.last(), Some(Block::Table(_))) {
                        items.push(Block::Paragraph(Paragraph::new()));
                    }
                    items.push(Block::Table(t));
                }
                p => items.push(p),
            }
        }

        // Single paragraph fragment: inline insertion.
        if items.len() == 1
            && let Block::Paragraph(frag) = &items[0]
        {
            let n = frag.char_len();
            let runs = frag.runs.clone();
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.insert_runs(pos.offset, runs);
            }
            self.selection = Selection::caret(TextPos::new(pos.path, pos.offset + n));
            return;
        }

        let Some(target) = self.doc.paragraph_mut(pos.path) else {
            return;
        };
        let tail = target.split_off(pos.offset);
        let mut iter = items.into_iter();
        let mut rest: Vec<Block> = Vec::new();
        match iter.next() {
            Some(Block::Paragraph(first)) => {
                if target.is_empty() {
                    target.style = first.style.clone();
                }
                target.append(first);
            }
            Some(table) => rest.push(table),
            None => {}
        }
        rest.extend(iter);

        // Re-attach the tail and compute the final caret.
        let caret_offset;
        match rest.last_mut() {
            Some(Block::Paragraph(last)) => {
                caret_offset = last.char_len();
                last.append(tail);
            }
            Some(Block::Table(_)) => {
                caret_offset = 0;
                rest.push(Block::Paragraph(tail));
            }
            None => {
                // Only the first paragraph: append the tail to the target.
                let target = self.doc.paragraph_mut(pos.path).expect("exists");
                let off = target.char_len();
                target.append(tail);
                self.selection = Selection::caret(TextPos::new(pos.path, off));
                return;
            }
        }
        let n = rest.len();
        let caret_path = if let Some((r, c)) = pos.path.cell {
            let cell = self.doc.blocks[pos.path.block]
                .as_table_mut()
                .and_then(|t| t.cell_mut(r, c))
                .expect("caret cell exists");
            for (k, b) in rest.into_iter().enumerate() {
                if let Block::Paragraph(p) = b {
                    cell.paragraphs.insert(pos.path.para + 1 + k, p);
                }
            }
            ParaPath::cell(pos.path.block, r, c, pos.path.para + n)
        } else {
            for (k, b) in rest.into_iter().enumerate() {
                self.doc.blocks.insert(pos.path.block + 1 + k, b);
            }
            ParaPath::top(pos.path.block + n)
        };
        self.selection = Selection::caret(TextPos::new(caret_path, caret_offset));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::{Document, Table};

    #[test]
    fn copy_paste_formatted_text() {
        let mut s = EditSession::new(Document::from_plain_text("hello world"));
        s.select(
            TextPos::new(ParaPath::top(0), 0),
            TextPos::new(ParaPath::top(0), 5),
        );
        s.toggle_attr(crate::document::ToggleAttr::Bold);
        let text = s.copy().unwrap();
        assert_eq!(text, "hello");
        s.move_to(s.doc.last_pos(), false);
        s.paste_from(Some("hello"));
        assert_eq!(s.doc.plain_text(), "hello worldhello");
        let p = s.doc.paragraph(ParaPath::top(0)).unwrap();
        assert!(p.runs.last().unwrap().style.bold);
    }

    #[test]
    fn foreign_text_is_pasted_plain() {
        let mut s = EditSession::new(Document::from_plain_text("ab"));
        s.select_all();
        s.copy();
        s.move_to(s.doc.last_pos(), false);
        s.paste_from(Some("x\ny"));
        assert_eq!(s.doc.plain_text(), "abx\ny");
    }

    #[test]
    fn cut_paste_multi_paragraph() {
        let mut s = EditSession::new(Document::from_plain_text("one\ntwo\nthree"));
        s.select(
            TextPos::new(ParaPath::top(0), 1),
            TextPos::new(ParaPath::top(1), 1),
        );
        assert_eq!(s.cut().unwrap(), "ne\nt");
        assert_eq!(s.doc.plain_text(), "owo\nthree");
        s.move_to(s.doc.last_pos(), false);
        s.paste_from(None);
        assert_eq!(s.doc.plain_text(), "owo\nthreene\nt");
    }

    #[test]
    fn copy_paste_table_and_cells() {
        let mut s = EditSession::new(Document::from_blocks(vec![
            Block::Paragraph(Paragraph::with_text("a")),
            Block::Table(Table::new(2, 2)),
            Block::Paragraph(Paragraph::with_text("b")),
        ]));
        // Put text in cells.
        for (r, c, t) in [(0, 0, "1"), (0, 1, "2"), (1, 0, "3"), (1, 1, "4")] {
            s.move_to(TextPos::start_of(ParaPath::cell(1, r, c, 0)), false);
            s.insert_text(t);
        }
        // Copy a 2x1 cell block.
        s.select(
            TextPos::start_of(ParaPath::cell(1, 0, 1, 0)),
            TextPos::start_of(ParaPath::cell(1, 1, 1, 0)),
        );
        assert_eq!(s.copy().unwrap(), "2\n4");
        // Paste into the first column: overwrites cells.
        s.move_to(TextPos::start_of(ParaPath::cell(1, 0, 0, 0)), false);
        s.paste_from(None);
        let t = s.doc.blocks[1].as_table().unwrap();
        assert_eq!(t.plain_text(), "2\t2\n4\t4");
        // Paste the cells into the body text: creates a new table.
        s.move_to(s.doc.last_pos(), false);
        s.paste_from(None);
        assert_eq!(
            s.doc
                .blocks
                .iter()
                .filter(|b| b.as_table().is_some())
                .count(),
            2
        );

        // Copy whole table with surrounding text and paste at the end.
        s.select(
            TextPos::new(ParaPath::top(0), 0),
            TextPos::new(ParaPath::top(2), 1),
        );
        s.copy();
        s.move_to(s.doc.last_pos(), false);
        s.paste_from(None);
        assert_eq!(
            s.doc
                .blocks
                .iter()
                .filter(|b| b.as_table().is_some())
                .count(),
            3
        );
    }
}
