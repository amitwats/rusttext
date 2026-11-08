//! Toolbar icons drawn with vector shapes, so they look the same on every
//! platform regardless of which fonts are installed.

use egui::{Color32, Painter, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
    Bullets,
    Numbering,
    IndentMore,
    IndentLess,
    Table,
}

const SIZE: Vec2 = vec2(26.0, 24.0);

fn lines(p: &Painter, r: Rect, widths: &[f32], align: f32, color: Color32) {
    let n = widths.len() as f32;
    for (i, w) in widths.iter().enumerate() {
        let y = r.top() + (i as f32 + 0.5) * r.height() / n;
        let len = r.width() * w;
        let x0 = r.left() + (r.width() - len) * align;
        p.line_segment([pos2(x0, y), pos2(x0 + len, y)], Stroke::new(1.6, color));
    }
}

fn paint(p: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let r = rect.shrink2(vec2(6.0, 6.0));
    match icon {
        Icon::AlignLeft => lines(p, r, &[1.0, 0.6, 1.0, 0.6], 0.0, color),
        Icon::AlignCenter => lines(p, r, &[1.0, 0.6, 1.0, 0.6], 0.5, color),
        Icon::AlignRight => lines(p, r, &[1.0, 0.6, 1.0, 0.6], 1.0, color),
        Icon::AlignJustify => lines(p, r, &[1.0, 1.0, 1.0, 1.0], 0.0, color),
        Icon::Bullets | Icon::Numbering => {
            let text = Rect::from_min_max(pos2(r.left() + 6.0, r.top()), r.max);
            lines(p, text, &[1.0, 1.0, 1.0], 0.0, color);
            for i in 0..3 {
                let y = r.top() + (i as f32 + 0.5) * r.height() / 3.0;
                if icon == Icon::Bullets {
                    p.circle_filled(pos2(r.left() + 1.5, y), 1.6, color);
                } else {
                    let x = r.left() + 1.5;
                    p.line_segment(
                        [pos2(x, y - 2.0), pos2(x, y + 2.0)],
                        Stroke::new(1.2, color),
                    );
                }
            }
        }
        Icon::IndentMore | Icon::IndentLess => {
            let text = Rect::from_min_max(pos2(r.left() + 6.0, r.top()), r.max);
            lines(p, text, &[1.0, 0.7, 0.7, 1.0], 1.0, color);
            let cy = r.center().y;
            let (a, b) = if icon == Icon::IndentMore {
                (r.left(), r.left() + 4.0)
            } else {
                (r.left() + 4.0, r.left())
            };
            p.add(egui::Shape::convex_polygon(
                vec![pos2(a, cy - 3.0), pos2(b, cy), pos2(a, cy + 3.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Table => {
            p.rect_stroke(r, 1.0, Stroke::new(1.3, color), egui::StrokeKind::Inside);
            let (x, y) = (r.center().x, r.center().y);
            p.line_segment(
                [pos2(x, r.top()), pos2(x, r.bottom())],
                Stroke::new(1.0, color),
            );
            p.line_segment(
                [pos2(r.left(), y), pos2(r.right(), y)],
                Stroke::new(1.0, color),
            );
        }
    }
}

/// A small toggle-style button showing `icon`.
pub fn icon_button(ui: &mut Ui, icon: Icon, selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(SIZE, Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    let color = if ui.is_enabled() {
        visuals.fg_stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    paint(ui.painter(), icon, rect, color);
    response.on_hover_text(tooltip)
}
