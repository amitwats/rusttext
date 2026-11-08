//! [`EditSession`]: a document together with its selection, undo history
//! and internal clipboard. All editing commands are methods on this type;
//! they are spread over several files by topic:
//!
//! * `session.rs`     – core state, navigation and text editing
//! * `formatting.rs`  – character & paragraph formatting
//! * `table_ops.rs`   – table creation and structure editing
//! * `clipboard.rs`   – cut / copy / paste
//!
//! The session knows nothing about the GUI; it can be driven from tests.

use super::history::{EditKind, History};
use super::model::{Block, Document, Paragraph};
use super::position::{ParaPath, Selection, SelectionKind, TextPos};
use super::style::CharStyle;

/// A selected character range inside a single paragraph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParaRange {
    pub path: ParaPath,
    pub start: usize,
    pub end: usize,
}

/// Editing state of one open document.
#[derive(Debug, Default)]
pub struct EditSession {
    pub doc: Document,
    pub selection: Selection,
    /// Formatting to use for the next typed text when the caret is collapsed
    /// (set by toggling e.g. bold without a selection).
    pub typing_style: Option<CharStyle>,
    pub history: History,
    /// Internal rich clipboard (see `clipboard.rs`).
    pub clipboard: Option<super::clipboard::ClipContent>,
    /// True when there are unsaved changes.
    pub modified: bool,
    /// Incremented on every document change; useful for caches.
    pub revision: u64,
}

impl EditSession {
    pub fn new(doc: Document) -> Self {
        let mut doc = doc;
        doc.normalize();
        let first = doc.first_pos();
        Self {
            doc,
            selection: Selection::caret(first),
            ..Default::default()
        }
    }

    /// Replaces the document (e.g. after opening a file), keeping the clipboard.
    pub fn load(&mut self, doc: Document) {
        let clipboard = self.clipboard.take();
        *self = Self::new(doc);
        self.clipboard = clipboard;
    }

    // ------------------------------------------------------------------
    // Change tracking
    // ------------------------------------------------------------------

    /// Must be called before every modification of `self.doc`.
    pub(crate) fn begin_edit(&mut self, kind: EditKind) {
        self.history.record(kind, &self.doc, self.selection);
        self.modified = true;
        self.revision += 1;
    }

    /// Marks the end of an edit: restores invariants and validates the selection.
    pub(crate) fn end_edit(&mut self) {
        self.doc.normalize();
        self.selection.anchor = self.doc.clamp_pos(self.selection.anchor);
        self.selection.head = self.doc.clamp_pos(self.selection.head);
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.history.undo(&mut self.doc, &mut self.selection);
        if ok {
            self.after_history_jump();
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.history.redo(&mut self.doc, &mut self.selection);
        if ok {
            self.after_history_jump();
        }
        ok
    }

    fn after_history_jump(&mut self) {
        self.typing_style = None;
        self.modified = true;
        self.end_edit();
    }

    // ------------------------------------------------------------------
    // Selection & navigation
    // ------------------------------------------------------------------

    pub fn caret(&self) -> TextPos {
        self.selection.head
    }

    pub fn selection_kind(&self) -> SelectionKind {
        self.selection.kind(&self.doc)
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_collapsed()
    }

    /// Moves the caret to `pos`; with `extend` the anchor is kept.
    pub fn move_to(&mut self, pos: TextPos, extend: bool) {
        let pos = self.doc.clamp_pos(pos);
        self.selection.head = pos;
        if !extend {
            self.selection.anchor = pos;
        }
        self.typing_style = None;
        self.history.break_coalescing();
    }

    pub fn select(&mut self, anchor: TextPos, head: TextPos) {
        self.selection = Selection::new(self.doc.clamp_pos(anchor), self.doc.clamp_pos(head));
        self.typing_style = None;
        self.history.break_coalescing();
    }

    pub fn select_all(&mut self) {
        let (a, b) = (self.doc.first_pos(), self.doc.last_pos());
        self.select(a, b);
    }

    fn para_len(&self, path: ParaPath) -> usize {
        self.doc.paragraph(path).map_or(0, Paragraph::char_len)
    }

    /// Position one character to the left of `pos` (crossing paragraphs).
    pub fn pos_left(&self, pos: TextPos) -> TextPos {
        if pos.offset > 0 {
            TextPos::new(pos.path, pos.offset - 1)
        } else if let Some(prev) = self.doc.prev_path(pos.path) {
            self.doc.end_of(prev)
        } else {
            pos
        }
    }

    /// Position one character to the right of `pos` (crossing paragraphs).
    pub fn pos_right(&self, pos: TextPos) -> TextPos {
        if pos.offset < self.para_len(pos.path) {
            TextPos::new(pos.path, pos.offset + 1)
        } else if let Some(next) = self.doc.next_path(pos.path) {
            TextPos::start_of(next)
        } else {
            pos
        }
    }

    /// Start of the previous word.
    pub fn pos_word_left(&self, pos: TextPos) -> TextPos {
        if pos.offset == 0 {
            return self.pos_left(pos);
        }
        let chars: Vec<char> = self
            .doc
            .paragraph(pos.path)
            .map(|p| p.plain_text().chars().collect())
            .unwrap_or_default();
        let mut i = pos.offset.min(chars.len());
        while i > 0 && !is_word_char(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
        TextPos::new(pos.path, i)
    }

    /// End of the next word.
    pub fn pos_word_right(&self, pos: TextPos) -> TextPos {
        let chars: Vec<char> = self
            .doc
            .paragraph(pos.path)
            .map(|p| p.plain_text().chars().collect())
            .unwrap_or_default();
        if pos.offset >= chars.len() {
            return self.pos_right(pos);
        }
        let mut i = pos.offset;
        while i < chars.len() && !is_word_char(chars[i]) {
            i += 1;
        }
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        TextPos::new(pos.path, i)
    }

    pub fn move_left(&mut self, extend: bool, word: bool) {
        if !extend && self.has_selection() {
            let (s, _) = self.selection.ordered();
            return self.move_to(s, false);
        }
        let p = if word {
            self.pos_word_left(self.caret())
        } else {
            self.pos_left(self.caret())
        };
        self.move_to(p, extend);
    }

    pub fn move_right(&mut self, extend: bool, word: bool) {
        if !extend && self.has_selection() {
            let (_, e) = self.selection.ordered();
            return self.move_to(e, false);
        }
        let p = if word {
            self.pos_word_right(self.caret())
        } else {
            self.pos_right(self.caret())
        };
        self.move_to(p, extend);
    }

    /// Selects the word around `pos`.
    pub fn select_word_at(&mut self, pos: TextPos) {
        let chars: Vec<char> = self
            .doc
            .paragraph(pos.path)
            .map(|p| p.plain_text().chars().collect())
            .unwrap_or_default();
        let at = pos.offset.min(chars.len());
        let word = |c: char| is_word_char(c);
        let target = at < chars.len() && word(chars[at]) || at > 0 && word(chars[at - 1]);
        let mut s = at;
        let mut e = at;
        if target {
            while s > 0 && word(chars[s - 1]) {
                s -= 1;
            }
            while e < chars.len() && word(chars[e]) {
                e += 1;
            }
        } else if e < chars.len() {
            e += 1;
        }
        self.select(TextPos::new(pos.path, s), TextPos::new(pos.path, e));
    }

    /// Selects the whole paragraph containing `pos`.
    pub fn select_paragraph_at(&mut self, pos: TextPos) {
        self.select(TextPos::start_of(pos.path), self.doc.end_of(pos.path));
    }

    /// The per-paragraph character ranges covered by the selection, in
    /// reading order. For a collapsed caret this is the caret's paragraph
    /// with an empty range.
    pub fn selected_ranges(&self) -> Vec<ParaRange> {
        match self.selection_kind() {
            SelectionKind::Caret(p) => vec![ParaRange {
                path: p.path,
                start: p.offset,
                end: p.offset,
            }],
            SelectionKind::Range { start, end } => self
                .doc
                .para_paths()
                .into_iter()
                .filter(|p| *p >= start.path && *p <= end.path)
                .map(|path| ParaRange {
                    path,
                    start: if path == start.path { start.offset } else { 0 },
                    end: if path == end.path {
                        end.offset
                    } else {
                        self.para_len(path)
                    },
                })
                .collect(),
            SelectionKind::Cells { block, rows, cols } => self
                .doc
                .para_paths()
                .into_iter()
                .filter(|p| {
                    p.block == block
                        && p.cell.is_some_and(|(r, c)| {
                            (rows.0..=rows.1).contains(&r) && (cols.0..=cols.1).contains(&c)
                        })
                })
                .map(|path| ParaRange {
                    path,
                    start: 0,
                    end: self.para_len(path),
                })
                .collect(),
        }
    }

    /// The plain text of the current selection.
    pub fn selected_text(&self) -> String {
        self.copy_selection()
            .map(|c| c.plain_text())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Text editing
    // ------------------------------------------------------------------

    /// The style that text typed at the caret will get.
    pub fn style_for_typing(&self) -> CharStyle {
        if let Some(s) = &self.typing_style {
            return s.clone();
        }
        let pos = self.caret();
        self.doc
            .paragraph(pos.path)
            .map(|p| p.style_at(pos.offset))
            .unwrap_or_default()
    }

    /// Deletes the selected content (if any). Returns `true` if something was removed.
    /// Does not record history; callers do.
    pub(crate) fn delete_selection_raw(&mut self) -> bool {
        match self.selection_kind() {
            SelectionKind::Caret(_) => false,
            SelectionKind::Range { start, end } => {
                self.delete_range_raw(start, end);
                self.selection = Selection::caret(start);
                true
            }
            SelectionKind::Cells { block, rows, cols } => {
                if let Some(t) = self.doc.blocks.get_mut(block).and_then(Block::as_table_mut) {
                    for r in rows.0..=rows.1 {
                        for c in cols.0..=cols.1 {
                            if let Some(cell) = t.cell_mut(r, c) {
                                *cell = cell.empty_like();
                            }
                        }
                    }
                }
                let pos = TextPos::start_of(ParaPath::cell(block, rows.0, cols.0, 0));
                self.selection = Selection::caret(pos);
                true
            }
        }
    }

    /// Deletes `start..end` where both endpoints are in the same container.
    fn delete_range_raw(&mut self, start: TextPos, end: TextPos) {
        let (a, b) = (start.path, end.path);
        if a == b {
            if let Some(p) = self.doc.paragraph_mut(a) {
                p.delete_range(start.offset, end.offset);
            }
            return;
        }
        // Detach the tail of the end paragraph.
        let tail = match self.doc.paragraph_mut(b) {
            Some(p) => p.split_off(end.offset),
            None => Paragraph::new(),
        };
        if let Some((row, col)) = a.cell {
            let Some(cell) = self
                .doc
                .blocks
                .get_mut(a.block)
                .and_then(Block::as_table_mut)
                .and_then(|t| t.cell_mut(row, col))
            else {
                return;
            };
            cell.paragraphs[a.para].delete_range(start.offset, usize::MAX);
            cell.paragraphs[a.para].append(tail);
            cell.paragraphs
                .drain(a.para + 1..=b.para.min(cell.paragraphs.len() - 1));
        } else {
            if let Some(p) = self.doc.paragraph_mut(a) {
                p.delete_range(start.offset, usize::MAX);
                p.append(tail);
            }
            self.doc.blocks.drain(a.block + 1..=b.block);
        }
    }

    /// Deletes the selection as an undoable edit.
    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        self.begin_edit(EditKind::Other);
        let r = self.delete_selection_raw();
        self.end_edit();
        r
    }

    /// Inserts (possibly multi-line) text at the caret, replacing the selection.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let single_char = text.chars().count() == 1 && !text.contains('\n');
        let kind = if single_char && !self.has_selection() {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.begin_edit(kind);
        self.insert_text_raw(text);
        self.end_edit();
    }

    pub(crate) fn insert_text_raw(&mut self, text: &str) {
        let style = if self.has_selection() {
            // Typing over a selection keeps the style of its first character.
            let (s, _) = self.selection.ordered();
            self.doc
                .paragraph(s.path)
                .and_then(|p| p.style_of_char(s.offset))
                .unwrap_or_else(|| self.style_for_typing())
        } else {
            self.style_for_typing()
        };
        self.delete_selection_raw();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.split_paragraph_raw();
            }
            let pos = self.caret();
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.insert_text(pos.offset, line, &style);
                let new = TextPos::new(pos.path, pos.offset + line.chars().count());
                self.selection = Selection::caret(new);
            }
        }
        self.typing_style = None;
    }

    /// Splits the paragraph at the caret; the caret moves to the new paragraph.
    fn split_paragraph_raw(&mut self) {
        let pos = self.caret();
        let Some(p) = self.doc.paragraph_mut(pos.path) else {
            return;
        };
        let carry = p.style_at(pos.offset);
        let was_heading = p.style.heading > 0;
        let mut tail = p.split_off(pos.offset);
        let tail_empty = tail.is_empty();
        if tail_empty && was_heading {
            // Pressing Enter at the end of a heading continues with body text.
            tail.style.heading = 0;
        }
        let new_path = match pos.path.cell {
            Some((r, c)) => {
                let cell = self
                    .doc
                    .blocks
                    .get_mut(pos.path.block)
                    .and_then(Block::as_table_mut)
                    .and_then(|t| t.cell_mut(r, c))
                    .expect("caret cell exists");
                cell.paragraphs.insert(pos.path.para + 1, tail);
                ParaPath::cell(pos.path.block, r, c, pos.path.para + 1)
            }
            None => {
                self.doc
                    .blocks
                    .insert(pos.path.block + 1, Block::Paragraph(tail));
                ParaPath::top(pos.path.block + 1)
            }
        };
        self.selection = Selection::caret(TextPos::start_of(new_path));
        if tail_empty && !was_heading {
            self.typing_style = Some(carry);
        }
    }

    /// Handles the Enter key.
    pub fn insert_paragraph_break(&mut self) {
        self.begin_edit(EditKind::Other);
        self.delete_selection_raw();
        let pos = self.caret();
        let in_empty_list_item = self
            .doc
            .paragraph(pos.path)
            .is_some_and(|p| p.is_empty() && p.style.list.is_some());
        if in_empty_list_item {
            // Enter on an empty bullet ends the list.
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.style.list = None;
                p.style.indent_level = 0;
            }
        } else {
            let typing = self.typing_style.take();
            self.split_paragraph_raw();
            if let Some(t) = typing {
                self.typing_style = Some(t);
            }
        }
        self.end_edit();
    }

    /// Handles the Backspace key.
    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let pos = self.caret();
        let Some(para) = self.doc.paragraph(pos.path) else {
            return;
        };
        if pos.offset > 0 {
            self.begin_edit(EditKind::Deleting);
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.delete_range(pos.offset - 1, pos.offset);
            }
            self.selection = Selection::caret(TextPos::new(pos.path, pos.offset - 1));
            self.end_edit();
            return;
        }
        if para.style.list.is_some() {
            // First backspace at the start of a list item removes the bullet.
            self.begin_edit(EditKind::Other);
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.style.list = None;
                p.style.indent_level = 0;
            }
            self.end_edit();
            return;
        }
        match pos.path.cell {
            Some((r, c)) => {
                if pos.path.para == 0 {
                    return;
                }
                self.begin_edit(EditKind::Other);
                let cell = self
                    .doc
                    .blocks
                    .get_mut(pos.path.block)
                    .and_then(Block::as_table_mut)
                    .and_then(|t| t.cell_mut(r, c))
                    .expect("caret cell exists");
                let cur = cell.paragraphs.remove(pos.path.para);
                let prev = &mut cell.paragraphs[pos.path.para - 1];
                let off = prev.char_len();
                prev.append(cur);
                let path = ParaPath::cell(pos.path.block, r, c, pos.path.para - 1);
                self.selection = Selection::caret(TextPos::new(path, off));
                self.end_edit();
            }
            None => {
                if pos.path.block == 0 {
                    return;
                }
                let prev_block = pos.path.block - 1;
                match &self.doc.blocks[prev_block] {
                    Block::Table(_) => {
                        // Move into the last cell instead of deleting the table.
                        if let Some(prev) = self.doc.prev_path(pos.path) {
                            let p = self.doc.end_of(prev);
                            self.move_to(p, false);
                        }
                    }
                    Block::Paragraph(_) => {
                        self.begin_edit(EditKind::Other);
                        let Block::Paragraph(cur) = self.doc.blocks.remove(pos.path.block) else {
                            unreachable!()
                        };
                        let prev = self.doc.blocks[prev_block]
                            .as_paragraph_mut()
                            .expect("checked above");
                        let off = prev.char_len();
                        if prev.is_empty() {
                            prev.style = cur.style.clone();
                        }
                        prev.append(cur);
                        self.selection =
                            Selection::caret(TextPos::new(ParaPath::top(prev_block), off));
                        self.end_edit();
                    }
                }
            }
        }
    }

    /// Handles the Delete key.
    pub fn delete_forward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let pos = self.caret();
        let len = self.para_len(pos.path);
        if pos.offset < len {
            self.begin_edit(EditKind::Deleting);
            if let Some(p) = self.doc.paragraph_mut(pos.path) {
                p.delete_range(pos.offset, pos.offset + 1);
            }
            self.end_edit();
            return;
        }
        match pos.path.cell {
            Some((r, c)) => {
                let Some(cell) = self
                    .doc
                    .blocks
                    .get(pos.path.block)
                    .and_then(Block::as_table)
                    .and_then(|t| t.cell(r, c))
                else {
                    return;
                };
                if pos.path.para + 1 >= cell.paragraphs.len() {
                    return;
                }
                self.begin_edit(EditKind::Other);
                let cell = self
                    .doc
                    .blocks
                    .get_mut(pos.path.block)
                    .and_then(Block::as_table_mut)
                    .and_then(|t| t.cell_mut(r, c))
                    .expect("checked above");
                let next = cell.paragraphs.remove(pos.path.para + 1);
                cell.paragraphs[pos.path.para].append(next);
                self.end_edit();
            }
            None => {
                let next = pos.path.block + 1;
                if !matches!(self.doc.blocks.get(next), Some(Block::Paragraph(_))) {
                    return;
                }
                self.begin_edit(EditKind::Other);
                let Block::Paragraph(n) = self.doc.blocks.remove(next) else {
                    unreachable!()
                };
                if let Some(p) = self.doc.paragraph_mut(pos.path) {
                    p.append(n);
                }
                self.end_edit();
            }
        }
    }

    /// Handles the Tab key: next/previous cell in tables, indent in lists,
    /// otherwise inserts a tab character.
    pub fn tab(&mut self, backwards: bool) {
        let pos = self.caret();
        if pos.path.cell.is_some() {
            self.move_to_adjacent_cell(backwards);
            return;
        }
        let in_list = self
            .doc
            .paragraph(pos.path)
            .is_some_and(|p| p.style.list.is_some());
        if in_list {
            self.change_indent(if backwards { -1 } else { 1 });
        } else if !backwards {
            self.insert_text("\t");
        }
    }
}

pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::Table;

    fn session(text: &str) -> EditSession {
        EditSession::new(Document::from_plain_text(text))
    }

    #[test]
    fn typing_and_undo() {
        let mut s = session("");
        for c in "hello".chars() {
            s.insert_text(&c.to_string());
        }
        assert_eq!(s.doc.plain_text(), "hello");
        assert!(s.undo());
        assert_eq!(s.doc.plain_text(), "");
        assert!(s.redo());
        assert_eq!(s.doc.plain_text(), "hello");
    }

    #[test]
    fn enter_and_backspace() {
        let mut s = session("abcd");
        s.move_to(TextPos::new(ParaPath::top(0), 2), false);
        s.insert_paragraph_break();
        assert_eq!(s.doc.plain_text(), "ab\ncd");
        assert_eq!(s.caret(), TextPos::start_of(ParaPath::top(1)));
        s.backspace();
        assert_eq!(s.doc.plain_text(), "abcd");
        assert_eq!(s.caret(), TextPos::new(ParaPath::top(0), 2));
    }

    #[test]
    fn delete_multi_paragraph_selection() {
        let mut s = session("one\ntwo\nthree");
        s.select(
            TextPos::new(ParaPath::top(0), 1),
            TextPos::new(ParaPath::top(2), 2),
        );
        s.delete_selection();
        assert_eq!(s.doc.plain_text(), "oree");
    }

    #[test]
    fn multiline_insert() {
        let mut s = session("ab");
        s.move_to(TextPos::new(ParaPath::top(0), 1), false);
        s.insert_text("x\ny\nz");
        assert_eq!(s.doc.plain_text(), "ax\ny\nzb");
        assert_eq!(s.caret(), TextPos::new(ParaPath::top(2), 1));
    }

    #[test]
    fn delete_across_table_removes_it() {
        let mut s = EditSession::new(Document::from_blocks(vec![
            Block::Paragraph(Paragraph::with_text("abc")),
            Block::Table(Table::new(2, 2)),
            Block::Paragraph(Paragraph::with_text("def")),
        ]));
        s.select(
            TextPos::new(ParaPath::top(0), 1),
            TextPos::new(ParaPath::top(2), 1),
        );
        s.delete_selection();
        assert_eq!(s.doc.blocks.len(), 1);
        assert_eq!(s.doc.plain_text(), "aef");
    }

    #[test]
    fn word_navigation() {
        let s = session("hello brave world");
        let p = s.pos_word_right(TextPos::new(ParaPath::top(0), 0));
        assert_eq!(p.offset, 5);
        let p = s.pos_word_right(p);
        assert_eq!(p.offset, 11);
        let p = s.pos_word_left(p);
        assert_eq!(p.offset, 6);
    }
}
