//! The in-memory document tree.
//!
//! ```text
//! Document
//!  └─ Vec<Block>
//!      ├─ Block::Paragraph(Paragraph)      ── Vec<Run> + ParagraphStyle
//!      └─ Block::Table(Table)              ── Vec<Row>
//!                                              └─ Vec<Cell> ── Vec<Paragraph> + CellStyle
//! ```
//!
//! All text offsets inside a paragraph are **character** offsets (Unicode
//! scalar values), never byte offsets.

use super::style::{CellStyle, CharStyle, ParagraphStyle};

/// A span of text sharing the same character formatting.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Run {
    pub text: String,
    pub style: CharStyle,
}

impl Run {
    pub fn new(text: impl Into<String>, style: CharStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, CharStyle::default())
    }

    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }
}

/// Converts a character offset into a byte offset of `s` (clamped to the end).
pub fn byte_offset(s: &str, char_offset: usize) -> usize {
    s.char_indices()
        .nth(char_offset)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// A paragraph: a sequence of runs and paragraph-level formatting.
///
/// Paragraph text never contains `'\n'`; line breaks are represented by
/// separate paragraphs.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub style: ParagraphStyle,
}

impl Paragraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(text: impl Into<String>) -> Self {
        let mut p = Self::new();
        p.runs.push(Run::plain(text));
        p.normalize();
        p
    }

    pub fn from_runs(runs: Vec<Run>, style: ParagraphStyle) -> Self {
        let mut p = Self { runs, style };
        p.normalize();
        p
    }

    pub fn char_len(&self) -> usize {
        self.runs.iter().map(Run::char_len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }

    pub fn plain_text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Merges adjacent runs with identical styles and removes empty runs.
    pub fn normalize(&mut self) {
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            if run.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.style == run.style => last.text.push_str(&run.text),
                _ => out.push(run),
            }
        }
        self.runs = out;
    }

    /// Ensures a run boundary exists at `offset` and returns the index of the
    /// first run starting at or after `offset`.
    fn split_run_at(&mut self, offset: usize) -> usize {
        let mut pos = 0;
        for i in 0..self.runs.len() {
            let len = self.runs[i].char_len();
            if offset == pos {
                return i;
            }
            if offset < pos + len {
                let b = byte_offset(&self.runs[i].text, offset - pos);
                let tail = self.runs[i].text.split_off(b);
                let style = self.runs[i].style.clone();
                self.runs.insert(i + 1, Run::new(tail, style));
                return i + 1;
            }
            pos += len;
        }
        self.runs.len()
    }

    /// The character style in effect for text typed at `offset`: the style of
    /// the character before the offset, or of the first character if the
    /// offset is at the start.
    pub fn style_at(&self, offset: usize) -> CharStyle {
        let mut pos = 0;
        for run in &self.runs {
            let len = run.char_len();
            if offset > pos && offset <= pos + len {
                return run.style.clone();
            }
            pos += len;
        }
        self.runs
            .first()
            .map(|r| r.style.clone())
            .unwrap_or_else(|| self.default_style())
    }

    /// The character style of the character starting at `offset` (if any).
    pub fn style_of_char(&self, offset: usize) -> Option<CharStyle> {
        let mut pos = 0;
        for run in &self.runs {
            let len = run.char_len();
            if offset >= pos && offset < pos + len {
                return Some(run.style.clone());
            }
            pos += len;
        }
        None
    }

    /// The style to use when the paragraph is empty.
    pub fn default_style(&self) -> CharStyle {
        let mut s = CharStyle::default();
        if self.style.heading > 0 {
            s.bold = true;
            s.font_size = super::style::heading_font_size(self.style.heading);
        }
        s
    }

    /// Inserts `text` (which must not contain newlines) at `offset` using `style`.
    pub fn insert_text(&mut self, offset: usize, text: &str, style: &CharStyle) {
        if text.is_empty() {
            return;
        }
        let idx = self.split_run_at(offset.min(self.char_len()));
        self.runs.insert(idx, Run::new(text, style.clone()));
        self.normalize();
    }

    /// Inserts runs at `offset`.
    pub fn insert_runs(&mut self, offset: usize, runs: Vec<Run>) {
        let idx = self.split_run_at(offset.min(self.char_len()));
        for (k, r) in runs.into_iter().enumerate() {
            self.runs.insert(idx + k, r);
        }
        self.normalize();
    }

    /// Removes the characters in `start..end`.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        let len = self.char_len();
        let (start, end) = (start.min(len), end.min(len));
        if start >= end {
            return;
        }
        let a = self.split_run_at(start);
        let b = self.split_run_at(end);
        self.runs.drain(a..b);
        self.normalize();
    }

    /// Returns a copy of the runs in `start..end`.
    pub fn slice_runs(&self, start: usize, end: usize) -> Vec<Run> {
        let mut copy = self.clone();
        let len = copy.char_len();
        let (start, end) = (start.min(len), end.min(len));
        if start >= end {
            return Vec::new();
        }
        let a = copy.split_run_at(start);
        let b = copy.split_run_at(end);
        copy.runs.drain(a..b).collect()
    }

    /// Returns a new paragraph with the same paragraph style containing the
    /// text in `start..end`.
    pub fn slice(&self, start: usize, end: usize) -> Paragraph {
        Paragraph::from_runs(self.slice_runs(start, end), self.style.clone())
    }

    /// Splits the paragraph at `offset`; `self` keeps the head and the tail is
    /// returned (with the same paragraph style).
    pub fn split_off(&mut self, offset: usize) -> Paragraph {
        let idx = self.split_run_at(offset.min(self.char_len()));
        let tail = self.runs.split_off(idx);
        self.normalize();
        Paragraph::from_runs(tail, self.style.clone())
    }

    /// Appends the runs of `other` to the end of this paragraph.
    pub fn append(&mut self, other: Paragraph) {
        self.runs.extend(other.runs);
        self.normalize();
    }

    /// Applies `f` to the character style of every character in `start..end`.
    pub fn apply_style(&mut self, start: usize, end: usize, f: &dyn Fn(&mut CharStyle)) {
        let len = self.char_len();
        let (start, end) = (start.min(len), end.min(len));
        if start >= end {
            return;
        }
        let a = self.split_run_at(start);
        let b = self.split_run_at(end);
        for run in &mut self.runs[a..b] {
            f(&mut run.style);
        }
        self.normalize();
    }

    /// Applies `f` to every run.
    pub fn apply_style_all(&mut self, f: &dyn Fn(&mut CharStyle)) {
        for run in &mut self.runs {
            f(&mut run.style);
        }
        self.normalize();
    }

    /// Iterates over the styles of the characters in `start..end` (one item
    /// per run overlapping the range).
    pub fn styles_in(&self, start: usize, end: usize) -> Vec<&CharStyle> {
        let mut out = Vec::new();
        let mut pos = 0;
        for run in &self.runs {
            let len = run.char_len();
            if pos < end && pos + len > start {
                out.push(&run.style);
            }
            pos += len;
        }
        out
    }
}

/// A table cell. Always contains at least one paragraph.
#[derive(Clone, PartialEq, Debug)]
pub struct Cell {
    pub paragraphs: Vec<Paragraph>,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            paragraphs: vec![Paragraph::new()],
            style: CellStyle::default(),
        }
    }
}

impl Cell {
    pub fn with_text(text: &str) -> Self {
        Self {
            paragraphs: text.split('\n').map(Paragraph::with_text).collect(),
            style: CellStyle::default(),
        }
    }

    pub fn plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(Paragraph::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn ensure_paragraph(&mut self) {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(Paragraph::new());
        }
    }

    /// An empty cell that keeps the formatting (style, alignment, first run style) of `self`.
    pub fn empty_like(&self) -> Cell {
        let first = self.paragraphs.first();
        let mut p = Paragraph::new();
        if let Some(first) = first {
            p.style = first.style.clone();
            p.style.list = None;
        }
        Cell {
            paragraphs: vec![p],
            style: self.style.clone(),
        }
    }
}

/// A table row.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
}

/// A rectangular table. All rows have the same number of cells
/// (see [`Table::normalize`]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Table {
    pub rows: Vec<Row>,
}

impl Table {
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            rows: (0..rows)
                .map(|_| Row {
                    cells: (0..cols).map(|_| Cell::default()).collect(),
                })
                .collect(),
        }
    }

    pub fn from_cells(cells: Vec<Vec<Cell>>) -> Self {
        let mut t = Self {
            rows: cells.into_iter().map(|cells| Row { cells }).collect(),
        };
        t.normalize();
        t
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn col_count(&self) -> usize {
        self.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0)
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.rows.get(row)?.cells.get(col)
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        self.rows.get_mut(row)?.cells.get_mut(col)
    }

    /// Makes the table rectangular with at least one row and one column and
    /// guarantees each cell has a paragraph.
    pub fn normalize(&mut self) {
        if self.rows.is_empty() {
            self.rows.push(Row::default());
        }
        let cols = self.col_count().max(1);
        for row in &mut self.rows {
            while row.cells.len() < cols {
                row.cells.push(Cell::default());
            }
            for cell in &mut row.cells {
                cell.ensure_paragraph();
            }
        }
    }

    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| c.plain_text().replace('\n', " "))
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A top-level block of the document.
#[derive(Clone, PartialEq, Debug)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

impl Block {
    pub fn as_paragraph(&self) -> Option<&Paragraph> {
        match self {
            Block::Paragraph(p) => Some(p),
            Block::Table(_) => None,
        }
    }

    pub fn as_paragraph_mut(&mut self) -> Option<&mut Paragraph> {
        match self {
            Block::Paragraph(p) => Some(p),
            Block::Table(_) => None,
        }
    }

    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Block::Table(t) => Some(t),
            Block::Paragraph(_) => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut Table> {
        match self {
            Block::Table(t) => Some(t),
            Block::Paragraph(_) => None,
        }
    }

    pub fn plain_text(&self) -> String {
        match self {
            Block::Paragraph(p) => p.plain_text(),
            Block::Table(t) => t.plain_text(),
        }
    }
}

/// A complete document.
#[derive(Clone, PartialEq, Debug)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            blocks: vec![Block::Paragraph(Paragraph::new())],
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a document from plain text, one paragraph per line.
    pub fn from_plain_text(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut doc = Self {
            blocks: text
                .split('\n')
                .map(|l| Block::Paragraph(Paragraph::with_text(l)))
                .collect(),
        };
        doc.normalize();
        doc
    }

    pub fn from_blocks(blocks: Vec<Block>) -> Self {
        let mut doc = Self { blocks };
        doc.normalize();
        doc
    }

    /// Enforces the structural invariants the editor relies on:
    /// * the document has at least one block,
    /// * it never starts or ends with a table and two tables are never
    ///   adjacent (so there is always a paragraph to place the caret in),
    /// * tables are rectangular and every cell has a paragraph.
    pub fn normalize(&mut self) {
        let mut out = Vec::with_capacity(self.blocks.len() + 2);
        for block in self.blocks.drain(..) {
            if let Block::Table(mut t) = block {
                if t.rows.is_empty() {
                    continue;
                }
                t.normalize();
                if !matches!(out.last(), Some(Block::Paragraph(_))) {
                    out.push(Block::Paragraph(Paragraph::new()));
                }
                out.push(Block::Table(t));
            } else {
                out.push(block);
            }
        }
        if !matches!(out.last(), Some(Block::Paragraph(_))) {
            out.push(Block::Paragraph(Paragraph::new()));
        }
        // A leading empty paragraph that only exists to precede a table is
        // fine; but avoid creating one when the file had none and the first
        // block is a paragraph already.
        self.blocks = out;
    }

    /// Plain-text rendition of the document (tables as tab-separated rows).
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Number of whitespace-separated words.
    pub fn word_count(&self) -> usize {
        self.plain_text().split_whitespace().count()
    }

    /// Number of characters (excluding paragraph breaks).
    pub fn char_count(&self) -> usize {
        self.all_paragraphs().map(Paragraph::char_len).sum()
    }

    /// Iterates over every paragraph in reading order (including table cells).
    pub fn all_paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.blocks
            .iter()
            .flat_map(|b| -> Box<dyn Iterator<Item = &Paragraph>> {
                match b {
                    Block::Paragraph(p) => Box::new(std::iter::once(p)),
                    Block::Table(t) => Box::new(
                        t.rows
                            .iter()
                            .flat_map(|r| r.cells.iter())
                            .flat_map(|c| c.paragraphs.iter()),
                    ),
                }
            })
    }

    /// All distinct font families referenced by the document.
    pub fn font_families(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .all_paragraphs()
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| r.style.font_family.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> CharStyle {
        CharStyle {
            bold: true,
            ..Default::default()
        }
    }

    #[test]
    fn insert_and_delete() {
        let mut p = Paragraph::with_text("Hello world");
        p.insert_text(5, ",", &CharStyle::default());
        assert_eq!(p.plain_text(), "Hello, world");
        assert_eq!(p.runs.len(), 1);
        p.insert_text(0, "¡", &bold());
        assert_eq!(p.plain_text(), "¡Hello, world");
        assert_eq!(p.runs.len(), 2);
        p.delete_range(0, 1);
        assert_eq!(p.runs.len(), 1);
        p.delete_range(5, 100);
        assert_eq!(p.plain_text(), "Hello");
    }

    #[test]
    fn apply_style_splits_and_merges() {
        let mut p = Paragraph::with_text("abcdef");
        p.apply_style(2, 4, &|s| s.bold = true);
        assert_eq!(p.runs.len(), 3);
        assert_eq!(p.runs[1].text, "cd");
        p.apply_style(0, 6, &|s| s.bold = true);
        assert_eq!(p.runs.len(), 1);
        assert!(p.runs[0].style.bold);
    }

    #[test]
    fn split_and_append() {
        let mut p = Paragraph::with_text("abcdef");
        let tail = p.split_off(2);
        assert_eq!(p.plain_text(), "ab");
        assert_eq!(tail.plain_text(), "cdef");
        p.append(tail);
        assert_eq!(p.plain_text(), "abcdef");
        assert_eq!(p.runs.len(), 1);
    }

    #[test]
    fn style_at_uses_previous_char() {
        let mut p = Paragraph::with_text("ab");
        p.apply_style(0, 1, &|s| s.bold = true);
        assert!(p.style_at(1).bold);
        assert!(!p.style_at(2).bold);
        assert!(p.style_at(0).bold);
    }

    #[test]
    fn normalize_surrounds_tables() {
        let doc = Document::from_blocks(vec![
            Block::Table(Table::new(1, 1)),
            Block::Table(Table::new(1, 1)),
        ]);
        assert_eq!(doc.blocks.len(), 5);
        assert!(matches!(doc.blocks[0], Block::Paragraph(_)));
        assert!(matches!(doc.blocks[2], Block::Paragraph(_)));
        assert!(matches!(doc.blocks[4], Block::Paragraph(_)));
    }
}
