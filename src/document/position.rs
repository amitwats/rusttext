//! Addressing text inside a [`Document`]: paragraph paths, caret positions
//! and selections.

use super::model::{Block, Document, Paragraph};

/// Identifies one paragraph of the document: either a top-level paragraph
/// block or a paragraph inside a table cell.
///
/// The derived ordering is the reading order (block, row, column, paragraph).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ParaPath {
    /// Index of the block in [`Document::blocks`].
    pub block: usize,
    /// `(row, column)` when the paragraph lives in a table cell.
    pub cell: Option<(usize, usize)>,
    /// Paragraph index inside the cell (always 0 for top-level paragraphs).
    pub para: usize,
}

impl ParaPath {
    pub const fn top(block: usize) -> Self {
        Self {
            block,
            cell: None,
            para: 0,
        }
    }

    pub const fn cell(block: usize, row: usize, col: usize, para: usize) -> Self {
        Self {
            block,
            cell: Some((row, col)),
            para,
        }
    }

    pub fn is_in_table(&self) -> bool {
        self.cell.is_some()
    }

    /// True when both paths are in the same container (the top-level flow,
    /// or the same table cell).
    pub fn same_container(&self, other: &ParaPath) -> bool {
        match (self.cell, other.cell) {
            (None, None) => true,
            (Some(a), Some(b)) => self.block == other.block && a == b,
            _ => false,
        }
    }
}

/// A caret position: a paragraph and a character offset within it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct TextPos {
    pub path: ParaPath,
    pub offset: usize,
}

impl TextPos {
    pub const fn new(path: ParaPath, offset: usize) -> Self {
        Self { path, offset }
    }

    pub const fn start_of(path: ParaPath) -> Self {
        Self { path, offset: 0 }
    }
}

impl Default for TextPos {
    fn default() -> Self {
        Self::start_of(ParaPath::top(0))
    }
}

/// A selection between an anchor (where selecting started) and the head
/// (where the caret is).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Selection {
    pub anchor: TextPos,
    pub head: TextPos,
}

/// The resolved meaning of a [`Selection`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionKind {
    /// No selection, just a caret.
    Caret(TextPos),
    /// A contiguous range of text with `start < end`. Both endpoints are in
    /// the same container. For the top-level flow, any table between the
    /// endpoints is selected entirely.
    Range { start: TextPos, end: TextPos },
    /// A rectangular block of table cells (inclusive ranges).
    Cells {
        block: usize,
        rows: (usize, usize),
        cols: (usize, usize),
    },
}

impl Selection {
    pub fn caret(pos: TextPos) -> Self {
        Self {
            anchor: pos,
            head: pos,
        }
    }

    pub fn new(anchor: TextPos, head: TextPos) -> Self {
        Self { anchor, head }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// `(min, max)` of anchor and head in reading order.
    pub fn ordered(&self) -> (TextPos, TextPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Interprets the selection against `doc`.
    pub fn kind(&self, doc: &Document) -> SelectionKind {
        if self.is_collapsed() {
            return SelectionKind::Caret(self.head);
        }
        let (start, end) = self.ordered();
        let (a, b) = (start.path, end.path);
        if a.same_container(&b) {
            return SelectionKind::Range { start, end };
        }
        if let (Some((r0, c0)), Some((r1, c1))) = (a.cell, b.cell)
            && a.block == b.block
        {
            return SelectionKind::Cells {
                block: a.block,
                rows: (r0.min(r1), r0.max(r1)),
                cols: (c0.min(c1), c0.max(c1)),
            };
        }
        // Mixed selection between the flow and table cells: widen to whole tables.
        let start = if a.cell.is_some() {
            // The paragraph before a table always exists (see Document::normalize).
            let prev = a.block.saturating_sub(1);
            let len = doc
                .paragraph(ParaPath::top(prev))
                .map_or(0, Paragraph::char_len);
            TextPos::new(ParaPath::top(prev), len)
        } else {
            start
        };
        let end = if b.cell.is_some() {
            TextPos::new(ParaPath::top(b.block + 1), 0)
        } else {
            end
        };
        if start == end {
            SelectionKind::Caret(end)
        } else {
            SelectionKind::Range { start, end }
        }
    }
}

impl Document {
    pub fn paragraph(&self, path: ParaPath) -> Option<&Paragraph> {
        match (self.blocks.get(path.block)?, path.cell) {
            (Block::Paragraph(p), None) => Some(p),
            (Block::Table(t), Some((r, c))) => t.cell(r, c)?.paragraphs.get(path.para),
            _ => None,
        }
    }

    pub fn paragraph_mut(&mut self, path: ParaPath) -> Option<&mut Paragraph> {
        match (self.blocks.get_mut(path.block)?, path.cell) {
            (Block::Paragraph(p), None) => Some(p),
            (Block::Table(t), Some((r, c))) => t.cell_mut(r, c)?.paragraphs.get_mut(path.para),
            _ => None,
        }
    }

    /// Every paragraph path in reading order.
    pub fn para_paths(&self) -> Vec<ParaPath> {
        let mut out = Vec::new();
        for (bi, block) in self.blocks.iter().enumerate() {
            match block {
                Block::Paragraph(_) => out.push(ParaPath::top(bi)),
                Block::Table(t) => {
                    for (ri, row) in t.rows.iter().enumerate() {
                        for (ci, cell) in row.cells.iter().enumerate() {
                            for pi in 0..cell.paragraphs.len() {
                                out.push(ParaPath::cell(bi, ri, ci, pi));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn first_pos(&self) -> TextPos {
        TextPos::start_of(
            self.para_paths()
                .first()
                .copied()
                .unwrap_or(ParaPath::top(0)),
        )
    }

    pub fn last_pos(&self) -> TextPos {
        let path = self
            .para_paths()
            .last()
            .copied()
            .unwrap_or(ParaPath::top(0));
        TextPos::new(path, self.paragraph(path).map_or(0, Paragraph::char_len))
    }

    pub fn end_of(&self, path: ParaPath) -> TextPos {
        TextPos::new(path, self.paragraph(path).map_or(0, Paragraph::char_len))
    }

    /// The path following `path` in reading order.
    pub fn next_path(&self, path: ParaPath) -> Option<ParaPath> {
        let paths = self.para_paths();
        let i = paths.iter().position(|p| *p == path)?;
        paths.get(i + 1).copied()
    }

    /// The path preceding `path` in reading order.
    pub fn prev_path(&self, path: ParaPath) -> Option<ParaPath> {
        let paths = self.para_paths();
        let i = paths.iter().position(|p| *p == path)?;
        i.checked_sub(1).map(|i| paths[i])
    }

    /// Returns a valid position as close as possible to `pos`.
    pub fn clamp_pos(&self, pos: TextPos) -> TextPos {
        if let Some(p) = self.paragraph(pos.path) {
            return TextPos::new(pos.path, pos.offset.min(p.char_len()));
        }
        // Try progressively coarser fallbacks.
        if let Some((r, c)) = pos.path.cell
            && let Some(Block::Table(t)) = self.blocks.get(pos.path.block)
        {
            let r = r.min(t.row_count().saturating_sub(1));
            let c = c.min(t.col_count().saturating_sub(1));
            if let Some(cell) = t.cell(r, c) {
                let pi = pos.path.para.min(cell.paragraphs.len().saturating_sub(1));
                return self.end_of(ParaPath::cell(pos.path.block, r, c, pi));
            }
        }
        let block = pos.path.block.min(self.blocks.len().saturating_sub(1));
        match &self.blocks[block] {
            Block::Paragraph(p) => TextPos::new(ParaPath::top(block), pos.offset.min(p.char_len())),
            Block::Table(_) => TextPos::start_of(ParaPath::cell(block, 0, 0, 0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::{Block, Table};

    fn doc() -> Document {
        Document::from_blocks(vec![
            Block::Paragraph(Paragraph::with_text("before")),
            Block::Table(Table::new(2, 2)),
            Block::Paragraph(Paragraph::with_text("after")),
        ])
    }

    #[test]
    fn reading_order() {
        let d = doc();
        let paths = d.para_paths();
        assert_eq!(paths.len(), 6);
        assert_eq!(paths[1], ParaPath::cell(1, 0, 0, 0));
        assert_eq!(paths[2], ParaPath::cell(1, 0, 1, 0));
        assert!(paths.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn cell_selection() {
        let d = doc();
        let s = Selection::new(
            TextPos::start_of(ParaPath::cell(1, 1, 1, 0)),
            TextPos::start_of(ParaPath::cell(1, 0, 0, 0)),
        );
        assert_eq!(
            s.kind(&d),
            SelectionKind::Cells {
                block: 1,
                rows: (0, 1),
                cols: (0, 1)
            }
        );
    }

    #[test]
    fn mixed_selection_widens() {
        let d = doc();
        let s = Selection::new(
            TextPos::new(ParaPath::top(0), 2),
            TextPos::start_of(ParaPath::cell(1, 0, 1, 0)),
        );
        match s.kind(&d) {
            SelectionKind::Range { start, end } => {
                assert_eq!(start, TextPos::new(ParaPath::top(0), 2));
                assert_eq!(end, TextPos::new(ParaPath::top(2), 0));
            }
            k => panic!("unexpected {k:?}"),
        }
    }
}
