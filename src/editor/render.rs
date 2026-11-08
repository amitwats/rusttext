//! Painting a [`DocLayout`]: page, cell shading, selection, text, list
//! markers, table grid and caret.

use std::collections::HashMap;

use egui::{Color32, Painter, Rect, Shape, Stroke, pos2, vec2};

use super::layout::{DocLayout, Marker};
use crate::document::{EditSession, ParaPath, SelectionKind};

/// Visual parameters of the editor.
pub struct Theme {
    pub page: Color32,
    pub page_border: Color32,
    pub selection: Color32,
    pub cell_selection: Color32,
    pub grid: Color32,
    pub caret: Color32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            page: Color32::WHITE,
            page_border: Color32::from_gray(200),
            selection: Color32::from_rgba_unmultiplied(51, 144, 255, 90),
            cell_selection: Color32::from_rgba_unmultiplied(51, 144, 255, 70),
            grid: Color32::from_gray(150),
            caret: Color32::BLACK,
        }
    }
}

pub fn paint_page(painter: &Painter, page: Rect, theme: &Theme) {
    let shadow = page.translate(vec2(2.0, 3.0));
    painter.rect_filled(shadow, 2.0, Color32::from_black_alpha(30));
    painter.rect_filled(page, 2.0, theme.page);
    painter.rect_stroke(
        page,
        2.0,
        Stroke::new(1.0, theme.page_border),
        egui::StrokeKind::Inside,
    );
}

/// Paints the document content. `caret_visible` controls the blinking caret.
pub fn paint_document(
    painter: &Painter,
    layout: &DocLayout,
    session: &EditSession,
    theme: &Theme,
    caret_visible: bool,
    zoom: f32,
) {
    // Cell backgrounds.
    for cell in &layout.cells {
        if let Some(bg) = cell.background {
            painter.rect_filled(cell.rect, 0.0, bg);
        }
    }

    // Selection highlight.
    let kind = session.selection_kind();
    match kind {
        SelectionKind::Cells { block, rows, cols } => {
            for cell in &layout.cells {
                if cell.block == block
                    && (rows.0..=rows.1).contains(&cell.row)
                    && (cols.0..=cols.1).contains(&cell.col)
                {
                    painter.rect_filled(cell.rect.shrink(1.0), 0.0, theme.cell_selection);
                }
            }
        }
        SelectionKind::Range { start, end } => {
            let ranges: HashMap<ParaPath, (usize, usize)> = session
                .selected_ranges()
                .into_iter()
                .map(|r| (r.path, (r.start, r.end)))
                .collect();
            // Tables entirely inside a flow selection are highlighted per cell.
            if start.path.cell.is_none() {
                for cell in &layout.cells {
                    if cell.block > start.path.block && cell.block < end.path.block {
                        painter.rect_filled(cell.rect.shrink(1.0), 0.0, theme.cell_selection);
                    }
                }
            }
            for p in &layout.paras {
                let Some(&(s, e)) = ranges.get(&p.path) else {
                    continue;
                };
                if p.path.cell.is_some() && start.path.cell.is_none() {
                    continue; // already highlighted as whole cells
                }
                let continues = p.path != end.path;
                for (row, rs, re) in p.rows() {
                    let a = s.max(rs);
                    let b = e.min(re);
                    let past_end = continues && e >= re && s <= re;
                    if a > b || (a == b && !past_end) {
                        continue;
                    }
                    let x0 = p.caret_rect(a).left();
                    let mut x1 = p.caret_rect(b).left();
                    if past_end && b == re {
                        x1 += 5.0 * zoom; // show the selected paragraph break
                    }
                    let r = Rect::from_min_max(pos2(x0, row.top()), pos2(x1, row.bottom()));
                    painter.rect_filled(r, 0.0, theme.selection);
                }
            }
        }
        SelectionKind::Caret(_) => {}
    }

    // Text and list markers.
    for p in &layout.paras {
        painter.galley(p.galley_pos, p.galley.clone(), Color32::BLACK);
        if let Some(overlay) = &p.bold_overlay {
            let offset = (0.45 * zoom).clamp(0.4, 1.5);
            painter.galley(
                p.galley_pos + vec2(offset, 0.0),
                overlay.clone(),
                Color32::TRANSPARENT,
            );
        }
        match &p.marker {
            Some(Marker::Bullet {
                center,
                radius,
                level,
                color,
            }) => match level % 3 {
                0 => {
                    painter.circle_filled(*center, *radius, *color);
                }
                1 => {
                    painter.circle_stroke(*center, *radius, Stroke::new(1.0, *color));
                }
                _ => {
                    let r = Rect::from_center_size(*center, vec2(*radius, *radius) * 1.8);
                    painter.rect_filled(r, 0.0, *color);
                }
            },
            Some(Marker::Number { galley, pos }) => {
                painter.galley(*pos, galley.clone(), Color32::BLACK);
            }
            None => {}
        }
    }

    // Table grid.
    for t in &layout.tables {
        let stroke = Stroke::new(1.0, theme.grid);
        let mut shapes = Vec::new();
        for &x in &t.col_edges {
            shapes.push(Shape::line_segment(
                [pos2(x, t.rect.top()), pos2(x, t.rect.bottom())],
                stroke,
            ));
        }
        for &y in &t.row_edges {
            shapes.push(Shape::line_segment(
                [pos2(t.rect.left(), y), pos2(t.rect.right(), y)],
                stroke,
            ));
        }
        painter.extend(shapes);
    }

    // Caret.
    if caret_visible
        && !matches!(kind, SelectionKind::Cells { .. })
        && let Some(r) = layout.caret_rect(session.caret())
    {
        let x = r.left().round() + 0.5;
        painter.line_segment(
            [pos2(x, r.top()), pos2(x, r.bottom())],
            Stroke::new((1.5 * zoom).clamp(1.0, 3.0), theme.caret),
        );
    }
}
