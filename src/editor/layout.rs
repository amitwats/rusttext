//! Converts a [`Document`] into positioned egui galleys.
//!
//! Each paragraph becomes one [`egui::Galley`] (egui handles shaping, word
//! wrapping and alignment). Tables are laid out as a grid of equally wide
//! columns whose rows grow to fit their tallest cell.
//!
//! All coordinates are absolute screen coordinates of the current frame.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Galley, Pos2, Rect, Stroke, pos2, vec2};

use crate::document::{
    Alignment, Block, CharStyle, Color, Document, ListKind, ParaPath, Paragraph, Table, TextPos,
};
use crate::fonts::FontManager;

/// Points (1/72 inch) to screen pixels at 96 DPI.
pub const PT_TO_PX: f32 = 96.0 / 72.0;
/// Indentation per list level, in pixels at 100% zoom.
pub const LIST_INDENT: f32 = 24.0;
/// Inner padding of table cells.
pub const CELL_PADDING: f32 = 5.0;
/// Space after body paragraphs.
pub const PARAGRAPH_SPACING: f32 = 4.0;
/// Width of a tab character.
pub const TAB_WIDTH: f32 = 32.0;

pub fn to_color32(c: Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

pub fn from_color32(c: Color32) -> Color {
    Color::rgb(c.r(), c.g(), c.b())
}

/// List marker drawn left of a list paragraph.
#[derive(Clone, Debug)]
pub enum Marker {
    /// Bullet shape for the given nesting level, centered at the position.
    Bullet {
        center: Pos2,
        radius: f32,
        level: u8,
        color: Color32,
    },
    /// Number text, right-aligned at the position.
    Number { galley: Arc<Galley>, pos: Pos2 },
}

/// Layout of one paragraph.
#[derive(Clone, Debug)]
pub struct ParaLayout {
    pub path: ParaPath,
    pub galley: Arc<Galley>,
    /// Same layout, used to paint synthetic bold (see `render.rs`).
    pub bold_overlay: Option<Arc<Galley>>,
    /// Anchor at which `galley` is painted (depends on alignment).
    pub galley_pos: Pos2,
    /// Area owned by this paragraph (container width, includes spacing below).
    pub rect: Rect,
    /// Height of an empty line in this paragraph's font.
    pub line_height: f32,
    /// Left edge of the text area (after list indentation).
    pub text_left: f32,
    pub text_width: f32,
    pub alignment: Alignment,
    pub char_len: usize,
    pub marker: Option<Marker>,
}

impl ParaLayout {
    /// Rectangle of the caret at `offset` (zero width, line height tall).
    pub fn caret_rect(&self, offset: usize) -> Rect {
        let r = self
            .galley
            .pos_from_cursor(CCursor::new(offset.min(self.char_len)))
            .translate(self.galley_pos.to_vec2());
        if self.char_len == 0 || r.height() < 1.0 {
            let x = match self.alignment {
                Alignment::Center => self.text_left + self.text_width / 2.0,
                Alignment::Right => self.text_left + self.text_width,
                _ => self.text_left,
            };
            let top = self.galley_pos.y;
            return Rect::from_min_size(pos2(x, top), vec2(0.0, self.line_height));
        }
        r
    }

    /// Character offset closest to `pos`.
    pub fn offset_at(&self, pos: Pos2) -> usize {
        if self.char_len == 0 {
            return 0;
        }
        let local = pos - self.galley_pos;
        self.galley
            .cursor_from_pos(local)
            .index
            .0
            .min(self.char_len)
    }

    /// Absolute rectangles of the galley rows with their char ranges.
    pub fn rows(&self) -> Vec<(Rect, usize, usize)> {
        let mut out = Vec::new();
        let mut idx = 0;
        for row in &self.galley.rows {
            let n = row.char_count_excluding_newline().0;
            let total = row.char_count_including_newline().0;
            let rect = row.rect().translate(self.galley_pos.to_vec2());
            out.push((rect, idx, idx + n));
            idx += total;
        }
        if out.is_empty() {
            out.push((self.caret_rect(0), 0, 0));
        }
        out
    }

    /// X coordinate of the caret at `offset` within row `row_start`.
    pub fn x_of(&self, offset: usize) -> f32 {
        self.caret_rect(offset).left()
    }
}

#[derive(Clone, Debug)]
pub struct CellLayout {
    pub block: usize,
    pub row: usize,
    pub col: usize,
    pub rect: Rect,
    pub background: Option<Color32>,
}

#[derive(Clone, Debug)]
pub struct TableLayout {
    pub block: usize,
    pub rect: Rect,
    pub col_edges: Vec<f32>,
    pub row_edges: Vec<f32>,
}

/// The complete layout of a document.
#[derive(Clone, Debug, Default)]
pub struct DocLayout {
    pub paras: Vec<ParaLayout>,
    pub cells: Vec<CellLayout>,
    pub tables: Vec<TableLayout>,
    /// Bottom of the content.
    pub bottom: f32,
    index: HashMap<ParaPath, usize>,
}

impl DocLayout {
    pub fn para(&self, path: ParaPath) -> Option<&ParaLayout> {
        self.index.get(&path).map(|&i| &self.paras[i])
    }

    pub fn caret_rect(&self, pos: TextPos) -> Option<Rect> {
        Some(self.para(pos.path)?.caret_rect(pos.offset))
    }

    /// The text position closest to `pos`.
    pub fn hit_test(&self, pos: Pos2) -> Option<TextPos> {
        let best = self.paras.iter().min_by(|a, b| {
            let da = a.rect.distance_sq_to_pos(pos);
            let db = b.rect.distance_sq_to_pos(pos);
            da.total_cmp(&db)
        })?;
        Some(TextPos::new(best.path, best.offset_at(pos)))
    }

    /// Position on the visual line above (`up`) or below the caret, keeping
    /// the horizontal position `x` as far as possible. Works across
    /// paragraphs and table cells.
    pub fn vertical_neighbor(&self, caret: Rect, x: f32, up: bool) -> Option<TextPos> {
        let mut best: Option<((bool, f32, f32), &ParaLayout, Rect)> = None;
        for p in &self.paras {
            for (row, _, _) in p.rows() {
                let cy = row.center().y;
                let dy = if up {
                    if cy >= caret.top() {
                        continue;
                    }
                    caret.top() - row.bottom()
                } else {
                    if cy <= caret.bottom() {
                        continue;
                    }
                    row.top() - caret.bottom()
                };
                let dx = if x < p.rect.left() {
                    p.rect.left() - x
                } else if x > p.rect.right() {
                    x - p.rect.right()
                } else {
                    0.0
                };
                let key = (dx > 0.5, dy.max(0.0), dx);
                let better = match &best {
                    None => true,
                    Some((k, _, _)) => (key.0, key.1, key.2)
                        .partial_cmp(&(k.0, k.1, k.2))
                        .is_some_and(|o| o.is_lt()),
                };
                if better {
                    best = Some((key, p, row));
                }
            }
        }
        let (_, p, row) = best?;
        let target = pos2(
            x.clamp(row.left(), row.right().max(row.left())),
            row.center().y,
        );
        Some(TextPos::new(p.path, p.offset_at(target)))
    }
}

/// Parameters shared by all layout functions.
pub struct LayoutCtx<'a> {
    pub ctx: &'a egui::Context,
    pub fonts: &'a FontManager,
    pub zoom: f32,
}

impl LayoutCtx<'_> {
    fn px(&self, v: f32) -> f32 {
        v * self.zoom
    }

    fn text_format(&self, style: &CharStyle, default_color: Color32) -> (TextFormat, bool) {
        let resolved = self
            .fonts
            .resolve(style.font_family.as_deref(), style.bold, style.italic);
        let size = (style.font_size * PT_TO_PX * self.zoom).max(1.0);
        let color = style.color.map(to_color32).unwrap_or(default_color);
        let line = Stroke::new((size / 14.0).clamp(1.0, 4.0), color);
        let fmt = TextFormat {
            font_id: FontId::new(size, resolved.family),
            color,
            italics: resolved.synthetic_italic,
            underline: if style.underline { line } else { Stroke::NONE },
            strikethrough: if style.strikethrough {
                line
            } else {
                Stroke::NONE
            },
            valign: Align::BOTTOM,
            ..Default::default()
        };
        (fmt, resolved.synthetic_bold)
    }

    fn line_height(&self, style: &CharStyle) -> f32 {
        let (fmt, _) = self.text_format(style, Color32::BLACK);
        self.ctx.fonts_mut(|f| f.row_height(&fmt.font_id))
    }

    /// Lays out one paragraph whose container starts at `(x, y)` and is `width` wide.
    #[allow(clippy::too_many_arguments)]
    fn paragraph(
        &self,
        path: ParaPath,
        p: &Paragraph,
        x: f32,
        y: f32,
        width: f32,
        default_color: Color32,
        number: Option<usize>,
        spacing: f32,
    ) -> ParaLayout {
        let indent_levels =
            p.style.indent_level as f32 + if p.style.list.is_some() { 1.0 } else { 0.0 };
        let indent = self.px(LIST_INDENT) * indent_levels;
        let text_left = x + indent;
        let text_width = (width - indent).max(self.px(20.0));

        let mut job = LayoutJob::default();
        job.wrap.max_width = text_width;
        job.halign = match p.style.alignment {
            Alignment::Center => Align::Center,
            Alignment::Right => Align::RIGHT,
            _ => Align::LEFT,
        };
        job.justify = p.style.alignment == Alignment::Justify;
        let mut overlay = LayoutJob {
            wrap: job.wrap.clone(),
            halign: job.halign,
            justify: job.justify,
            ..Default::default()
        };
        let mut any_synthetic_bold = false;

        for run in &p.runs {
            let (fmt, synth_bold) = self.text_format(&run.style, default_color);
            any_synthetic_bold |= synth_bold;
            let mut ofmt = fmt.clone();
            ofmt.underline = Stroke::NONE;
            ofmt.strikethrough = Stroke::NONE;
            if !synth_bold {
                ofmt.color = Color32::TRANSPARENT;
            }
            // Tabs get their own section with extra spacing to look like a tab stop.
            for (i, part) in run.text.split('\t').enumerate() {
                if i > 0 {
                    let mut tab = fmt.clone();
                    tab.extra_letter_spacing = self.px(TAB_WIDTH);
                    job.append(" ", 0.0, tab.clone());
                    tab.color = Color32::TRANSPARENT;
                    tab.underline = Stroke::NONE;
                    tab.strikethrough = Stroke::NONE;
                    overlay.append(" ", 0.0, tab);
                }
                if !part.is_empty() {
                    job.append(part, 0.0, fmt.clone());
                    overlay.append(part, 0.0, ofmt.clone());
                }
            }
        }

        let base_style = p
            .runs
            .first()
            .map(|r| r.style.clone())
            .unwrap_or_else(|| p.default_style());
        let line_height = self.line_height(&base_style);
        job.first_row_min_height = line_height;
        overlay.first_row_min_height = line_height;

        let galley = self.ctx.fonts_mut(|f| f.layout_job(job));
        let bold_overlay =
            any_synthetic_bold.then(|| self.ctx.fonts_mut(|f| f.layout_job(overlay)));

        let anchor_x = match p.style.alignment {
            Alignment::Center => text_left + text_width / 2.0,
            Alignment::Right => text_left + text_width,
            _ => text_left,
        };
        let galley_pos = pos2(anchor_x, y);
        let text_height = galley.rect.height().max(line_height);
        let rect = Rect::from_min_size(pos2(x, y), vec2(width, text_height + spacing));

        let marker = p.style.list.map(|kind| {
            let first_row = galley
                .rows
                .first()
                .map(|r| r.rect().translate(galley_pos.to_vec2()))
                .unwrap_or_else(|| Rect::from_min_size(galley_pos, vec2(0.0, line_height)));
            let color = base_style.color.map(to_color32).unwrap_or(default_color);
            match kind {
                ListKind::Bullet => {
                    let size = base_style.font_size * PT_TO_PX * self.zoom;
                    Marker::Bullet {
                        center: pos2(
                            text_left - self.px(LIST_INDENT) * 0.5,
                            first_row.bottom() - size * 0.45,
                        ),
                        radius: (size * 0.16).max(1.5),
                        level: p.style.indent_level,
                        color,
                    }
                }
                ListKind::Numbered => {
                    let mut s = base_style.clone();
                    s.underline = false;
                    s.strikethrough = false;
                    let (fmt, _) = self.text_format(&s, color);
                    let text = format!("{}.", number.unwrap_or(1));
                    let g = self
                        .ctx
                        .fonts_mut(|f| f.layout_job(LayoutJob::single_section(text, fmt)));
                    let pos = pos2(
                        text_left - self.px(4.0) - g.rect.width(),
                        first_row.bottom() - g.rect.height(),
                    );
                    Marker::Number { galley: g, pos }
                }
            }
        });

        ParaLayout {
            path,
            galley,
            bold_overlay,
            galley_pos,
            rect,
            line_height,
            text_left,
            text_width,
            alignment: p.style.alignment,
            char_len: p.char_len(),
            marker,
        }
    }

    /// Lays out a sequence of paragraphs (top-level run or a cell) and
    /// returns the y coordinate below them.
    #[allow(clippy::too_many_arguments)]
    fn flow<'p>(
        &self,
        out: &mut DocLayout,
        paras: impl Iterator<Item = (ParaPath, &'p Paragraph)>,
        x: f32,
        mut y: f32,
        width: f32,
        default_color: Color32,
        spacing: f32,
        counters: &mut [usize; 16],
    ) -> f32 {
        for (path, p) in paras {
            let level = (p.style.indent_level as usize).min(14);
            let number = match p.style.list {
                Some(ListKind::Numbered) => {
                    counters[level] += 1;
                    counters[level + 1..].iter_mut().for_each(|c| *c = 0);
                    Some(counters[level])
                }
                Some(ListKind::Bullet) => {
                    counters[level + 1..].iter_mut().for_each(|c| *c = 0);
                    None
                }
                None => {
                    *counters = [0; 16];
                    None
                }
            };
            if p.style.heading > 0 && !out.paras.is_empty() {
                y += self.px(6.0);
            }
            let pl = self.paragraph(path, p, x, y, width, default_color, number, spacing);
            y = pl.rect.bottom();
            out.index.insert(path, out.paras.len());
            out.paras.push(pl);
        }
        y
    }

    fn table(
        &self,
        out: &mut DocLayout,
        block: usize,
        t: &Table,
        x: f32,
        y: f32,
        width: f32,
    ) -> f32 {
        let cols = t.col_count().max(1);
        let col_w = width / cols as f32;
        let pad = self.px(CELL_PADDING);
        let col_edges: Vec<f32> = (0..=cols).map(|c| x + c as f32 * col_w).collect();
        let mut row_edges = vec![y];
        let mut row_top = y;
        for (ri, row) in t.rows.iter().enumerate() {
            let mut row_bottom = row_top + self.px(18.0);
            for (ci, cell) in row.cells.iter().enumerate() {
                let fg = cell
                    .style
                    .foreground
                    .map(to_color32)
                    .unwrap_or(Color32::BLACK);
                let mut counters = [0usize; 16];
                let paras = cell
                    .paragraphs
                    .iter()
                    .enumerate()
                    .map(|(pi, p)| (ParaPath::cell(block, ri, ci, pi), p));
                let bottom = self.flow(
                    out,
                    paras,
                    col_edges[ci] + pad,
                    row_top + pad,
                    col_w - 2.0 * pad,
                    fg,
                    self.px(2.0),
                    &mut counters,
                );
                row_bottom = row_bottom.max(bottom + pad);
            }
            for (ci, cell) in row.cells.iter().enumerate() {
                out.cells.push(CellLayout {
                    block,
                    row: ri,
                    col: ci,
                    rect: Rect::from_min_max(
                        pos2(col_edges[ci], row_top),
                        pos2(col_edges[ci + 1], row_bottom),
                    ),
                    background: cell.style.background.map(to_color32),
                });
            }
            row_top = row_bottom;
            row_edges.push(row_top);
        }
        out.tables.push(TableLayout {
            block,
            rect: Rect::from_min_max(pos2(x, y), pos2(x + width, row_top)),
            col_edges,
            row_edges,
        });
        row_top
    }
}

/// Lays out `doc` with its top-left content corner at `origin` and the given text width.
pub fn layout_document(lc: &LayoutCtx<'_>, doc: &Document, origin: Pos2, width: f32) -> DocLayout {
    let mut out = DocLayout {
        bottom: origin.y,
        ..Default::default()
    };
    let mut y = origin.y;
    let mut counters = [0usize; 16];
    let spacing = lc.px(PARAGRAPH_SPACING);
    for (bi, block) in doc.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(p) => {
                y = lc.flow(
                    &mut out,
                    std::iter::once((ParaPath::top(bi), p)),
                    origin.x,
                    y,
                    width,
                    Color32::BLACK,
                    spacing,
                    &mut counters,
                );
            }
            Block::Table(t) => {
                counters = [0; 16];
                y = lc.table(&mut out, bi, t, origin.x, y + lc.px(6.0), width) + lc.px(8.0);
            }
        }
        out.bottom = y;
    }
    out
}
