//! Table creation and structural editing.

use super::history::EditKind;
use super::model::{Block, Paragraph, Row, Table};
use super::position::{ParaPath, Selection, SelectionKind, TextPos};
use super::session::EditSession;
use super::style::Color;

/// The cells a table command applies to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellTarget {
    pub block: usize,
    pub rows: (usize, usize),
    pub cols: (usize, usize),
}

impl EditSession {
    /// The table cells targeted by the selection: the selected cell
    /// rectangle, or the cell containing the caret.
    pub fn cell_target(&self) -> Option<CellTarget> {
        match self.selection_kind() {
            SelectionKind::Cells { block, rows, cols } => Some(CellTarget { block, rows, cols }),
            _ => {
                let p = self.caret().path;
                let (r, c) = p.cell?;
                Some(CellTarget {
                    block: p.block,
                    rows: (r, r),
                    cols: (c, c),
                })
            }
        }
    }

    pub fn in_table(&self) -> bool {
        self.cell_target().is_some()
    }

    fn table_mut(&mut self, block: usize) -> Option<&mut Table> {
        self.doc.blocks.get_mut(block).and_then(Block::as_table_mut)
    }

    fn table(&self, block: usize) -> Option<&Table> {
        self.doc.blocks.get(block).and_then(Block::as_table)
    }

    /// Inserts a `rows` x `cols` table at the caret (splitting the paragraph).
    /// Returns `false` when the caret is inside a table (nested tables are
    /// not supported).
    pub fn insert_table(&mut self, rows: usize, cols: usize) -> bool {
        if self.caret().path.is_in_table() || self.selection.anchor.path.is_in_table() {
            return false;
        }
        self.begin_edit(EditKind::Other);
        self.delete_selection_raw();
        let pos = self.caret();
        let block = pos.path.block;
        let tail = self
            .doc
            .paragraph_mut(pos.path)
            .map(|p| p.split_off(pos.offset))
            .unwrap_or_default();
        self.doc
            .blocks
            .insert(block + 1, Block::Table(Table::new(rows, cols)));
        self.doc.blocks.insert(block + 2, Block::Paragraph(tail));
        self.selection = Selection::caret(TextPos::start_of(ParaPath::cell(block + 1, 0, 0, 0)));
        self.end_edit();
        true
    }

    /// Inserts a row above the first targeted row or below the last one.
    pub fn insert_row(&mut self, below: bool) {
        let Some(t) = self.cell_target() else { return };
        let caret_col = self.caret().path.cell.map_or(0, |(_, c)| c);
        self.begin_edit(EditKind::Other);
        let reference = if below { t.rows.1 } else { t.rows.0 };
        let at = if below { t.rows.1 + 1 } else { t.rows.0 };
        if let Some(table) = self.table_mut(t.block) {
            let row = Row {
                cells: table.rows[reference]
                    .cells
                    .iter()
                    .map(|c| c.empty_like())
                    .collect(),
            };
            table.rows.insert(at, row);
        }
        self.selection =
            Selection::caret(TextPos::start_of(ParaPath::cell(t.block, at, caret_col, 0)));
        self.end_edit();
    }

    /// Inserts a column left of the first targeted column or right of the last one.
    pub fn insert_column(&mut self, right: bool) {
        let Some(t) = self.cell_target() else { return };
        let caret_row = self.caret().path.cell.map_or(0, |(r, _)| r);
        self.begin_edit(EditKind::Other);
        let reference = if right { t.cols.1 } else { t.cols.0 };
        let at = if right { t.cols.1 + 1 } else { t.cols.0 };
        if let Some(table) = self.table_mut(t.block) {
            for row in &mut table.rows {
                let cell = row.cells[reference.min(row.cells.len() - 1)].empty_like();
                row.cells.insert(at.min(row.cells.len()), cell);
            }
        }
        self.selection =
            Selection::caret(TextPos::start_of(ParaPath::cell(t.block, caret_row, at, 0)));
        self.end_edit();
    }

    /// Deletes the targeted rows (the whole table if no row would remain).
    pub fn delete_rows(&mut self) {
        let Some(t) = self.cell_target() else { return };
        let Some(table) = self.table(t.block) else {
            return;
        };
        if t.rows.1 - t.rows.0 + 1 >= table.row_count() {
            return self.delete_table();
        }
        self.begin_edit(EditKind::Other);
        let mut r = t.rows.0;
        if let Some(table) = self.table_mut(t.block) {
            table.rows.drain(t.rows.0..=t.rows.1);
            r = r.min(table.row_count() - 1);
        }
        self.selection =
            Selection::caret(TextPos::start_of(ParaPath::cell(t.block, r, t.cols.0, 0)));
        self.end_edit();
    }

    /// Deletes the targeted columns (the whole table if no column would remain).
    pub fn delete_columns(&mut self) {
        let Some(t) = self.cell_target() else { return };
        let Some(table) = self.table(t.block) else {
            return;
        };
        if t.cols.1 - t.cols.0 + 1 >= table.col_count() {
            return self.delete_table();
        }
        self.begin_edit(EditKind::Other);
        if let Some(table) = self.table_mut(t.block) {
            for row in &mut table.rows {
                let end = t.cols.1.min(row.cells.len() - 1);
                row.cells.drain(t.cols.0..=end);
            }
        }
        self.selection = Selection::caret(TextPos::start_of(ParaPath::cell(
            t.block, t.rows.0, t.cols.0, 0,
        )));
        self.end_edit();
    }

    /// Selects every cell of the table containing the caret.
    pub fn select_table(&mut self) {
        let Some(t) = self.cell_target() else { return };
        let Some(table) = self.table(t.block) else {
            return;
        };
        let (rows, cols) = (table.row_count(), table.col_count());
        let start = TextPos::start_of(ParaPath::cell(t.block, 0, 0, 0));
        let end = TextPos::start_of(ParaPath::cell(t.block, rows - 1, cols - 1, 0));
        self.select(start, end);
    }

    /// Removes the table containing the caret.
    pub fn delete_table(&mut self) {
        let Some(t) = self.cell_target() else { return };
        self.begin_edit(EditKind::Other);
        self.doc.blocks.remove(t.block);
        // Merge the (always present) paragraphs around the old table if the
        // one after it is empty, to avoid leaving stray blank lines behind.
        if matches!(self.doc.blocks.get(t.block), Some(Block::Paragraph(p)) if p.is_empty())
            && t.block > 0
            && matches!(self.doc.blocks.get(t.block - 1), Some(Block::Paragraph(_)))
        {
            self.doc.blocks.remove(t.block);
        }
        // Document::normalize guarantees a paragraph right before every table.
        let block = t.block.saturating_sub(1);
        let pos = match self.doc.blocks.get(block) {
            Some(Block::Paragraph(p)) => TextPos::new(ParaPath::top(block), p.char_len()),
            _ => TextPos::start_of(ParaPath::top(0)),
        };
        self.selection = Selection::caret(pos);
        self.end_edit();
    }

    /// Applies `f` to the style of every targeted cell.
    fn apply_cells(&mut self, f: &dyn Fn(&mut super::model::Cell)) {
        let Some(t) = self.cell_target() else { return };
        self.begin_edit(EditKind::Other);
        if let Some(table) = self.table_mut(t.block) {
            for r in t.rows.0..=t.rows.1 {
                for c in t.cols.0..=t.cols.1 {
                    if let Some(cell) = table.cell_mut(r, c) {
                        f(cell);
                    }
                }
            }
        }
        self.end_edit();
    }

    pub fn set_cell_background(&mut self, color: Option<Color>) {
        self.apply_cells(&move |c| c.style.background = color);
    }

    /// Sets the cell text color. Explicit run colors in the cell are cleared
    /// so the new color takes effect on all text.
    pub fn set_cell_foreground(&mut self, color: Option<Color>) {
        self.apply_cells(&move |c| {
            c.style.foreground = color;
            for p in &mut c.paragraphs {
                p.apply_style_all(&|s| s.color = None);
            }
        });
    }

    /// The style of the cell containing the caret.
    pub fn current_cell_style(&self) -> Option<super::style::CellStyle> {
        let t = self.cell_target()?;
        Some(self.table(t.block)?.cell(t.rows.0, t.cols.0)?.style.clone())
    }

    /// Moves to the next (or previous) cell; Tab in the last cell appends a row.
    pub fn move_to_adjacent_cell(&mut self, backwards: bool) {
        let p = self.caret().path;
        let Some((r, c)) = p.cell else { return };
        let Some(table) = self.table(p.block) else {
            return;
        };
        let (rows, cols) = (table.row_count(), table.col_count());
        let idx = r * cols + c;
        let target = if backwards {
            match idx.checked_sub(1) {
                Some(i) => i,
                None => return,
            }
        } else if idx + 1 >= rows * cols {
            self.insert_row(true);
            let path = ParaPath::cell(p.block, rows, 0, 0);
            self.move_to(TextPos::start_of(path), false);
            return;
        } else {
            idx + 1
        };
        let (nr, nc) = (target / cols, target % cols);
        // Select the content of the destination cell, like word processors do.
        let n_paras = table.cell(nr, nc).map_or(1, |c| c.paragraphs.len());
        let start = TextPos::start_of(ParaPath::cell(p.block, nr, nc, 0));
        let end = self
            .doc
            .end_of(ParaPath::cell(p.block, nr, nc, n_paras - 1));
        self.select(start, end);
    }

    /// Inserts the given cell grid into the table at the caret cell,
    /// overwriting cells and growing the table as necessary.
    pub(crate) fn overwrite_cells_raw(
        &mut self,
        block: usize,
        row: usize,
        col: usize,
        cells: Vec<Vec<super::model::Cell>>,
    ) {
        let Some(table) = self.table_mut(block) else {
            return;
        };
        let need_rows = row + cells.len();
        let need_cols = col + cells.iter().map(Vec::len).max().unwrap_or(0);
        while table.rows.len() < need_rows {
            let template = table.rows.last().cloned().unwrap_or_default();
            table.rows.push(Row {
                cells: template.cells.iter().map(|c| c.empty_like()).collect(),
            });
        }
        if table.col_count() < need_cols {
            for r in &mut table.rows {
                while r.cells.len() < need_cols {
                    let c = r.cells.last().map(|c| c.empty_like()).unwrap_or_default();
                    r.cells.push(c);
                }
            }
        }
        let (mut last_r, mut last_c) = (row, col);
        for (i, src_row) in cells.into_iter().enumerate() {
            for (j, cell) in src_row.into_iter().enumerate() {
                if let Some(dst) = table.cell_mut(row + i, col + j) {
                    *dst = cell;
                    dst.ensure_paragraph();
                    (last_r, last_c) = (row + i, col + j);
                }
            }
        }
        table.normalize();
        let n = table.cell(last_r, last_c).map_or(1, |c| c.paragraphs.len());
        let end_path = ParaPath::cell(block, last_r, last_c, n - 1);
        let start = TextPos::start_of(ParaPath::cell(block, row, col, 0));
        let end = self.doc.end_of(end_path);
        self.selection = Selection::new(start, end);
    }
}

/// Converts a table to paragraphs (used when a table is pasted into a cell).
pub(crate) fn table_to_paragraphs(t: &Table) -> Vec<Paragraph> {
    t.rows
        .iter()
        .map(|r| {
            let mut p = Paragraph::new();
            for (i, c) in r.cells.iter().enumerate() {
                if i > 0 {
                    p.runs.push(super::model::Run::plain("\t"));
                }
                for (k, para) in c.paragraphs.iter().enumerate() {
                    if k > 0 {
                        p.runs.push(super::model::Run::plain(" "));
                    }
                    p.runs.extend(para.runs.iter().cloned());
                }
            }
            p.normalize();
            p
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::Document;

    fn with_table() -> EditSession {
        let mut s = EditSession::new(Document::from_plain_text("intro"));
        s.move_to(TextPos::new(ParaPath::top(0), 5), false);
        assert!(s.insert_table(2, 3));
        s
    }

    #[test]
    fn create_table() {
        let s = with_table();
        let t = s.doc.blocks[1].as_table().unwrap();
        assert_eq!((t.row_count(), t.col_count()), (2, 3));
        assert_eq!(s.caret().path, ParaPath::cell(1, 0, 0, 0));
    }

    #[test]
    fn insert_rows_and_columns() {
        let mut s = with_table();
        s.insert_row(false);
        s.insert_row(true);
        s.insert_column(true);
        s.insert_column(false);
        let t = s.doc.blocks[1].as_table().unwrap();
        assert_eq!((t.row_count(), t.col_count()), (4, 5));
    }

    #[test]
    fn delete_rows_columns_table() {
        let mut s = with_table();
        s.delete_columns();
        assert_eq!(s.doc.blocks[1].as_table().unwrap().col_count(), 2);
        s.delete_rows();
        assert_eq!(s.doc.blocks[1].as_table().unwrap().row_count(), 1);
        s.delete_rows();
        assert!(s.doc.blocks.iter().all(|b| b.as_table().is_none()));
    }

    #[test]
    fn select_and_copy_whole_table() {
        let mut s = with_table();
        s.select_table();
        match s.copy_selection() {
            Some(crate::document::ClipContent::Cells(rows)) => {
                assert_eq!((rows.len(), rows[0].len()), (2, 3));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn cell_colors() {
        let mut s = with_table();
        s.set_cell_background(Some(Color::rgb(255, 0, 0)));
        s.set_cell_foreground(Some(Color::WHITE));
        let cell = s.doc.blocks[1].as_table().unwrap().cell(0, 0).unwrap();
        assert_eq!(cell.style.background, Some(Color::rgb(255, 0, 0)));
        assert_eq!(cell.style.foreground, Some(Color::WHITE));
    }

    #[test]
    fn tab_moves_and_appends_rows() {
        let mut s = with_table();
        for _ in 0..6 {
            s.tab(false);
        }
        let t = s.doc.blocks[1].as_table().unwrap();
        assert_eq!(t.row_count(), 3);
        assert_eq!(s.caret().path, ParaPath::cell(1, 2, 0, 0));
    }
}
