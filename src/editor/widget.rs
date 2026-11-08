//! The interactive rich-text editor widget.
//!
//! [`EditorView::show`] lays out the document, handles mouse and keyboard
//! input by calling commands on the [`EditSession`], and paints the result.

use egui::{CursorIcon, Event, EventFilter, Id, Key, Modifiers, Pos2, Rect, Sense, Ui, pos2, vec2};

use super::layout::{DocLayout, LayoutCtx, layout_document};
use super::render::{Theme, paint_document, paint_page};
use crate::document::{EditSession, TextPos, ToggleAttr};
use crate::fonts::FontManager;

/// Page width at 100% zoom (US Letter at 96 DPI).
pub const PAGE_WIDTH: f32 = 816.0;
/// Page margin at 100% zoom.
pub const PAGE_MARGIN: f32 = 72.0;
/// Gap between the page and the window edges.
const OUTER_GAP: f32 = 16.0;

/// Things the editor reports back to the application.
#[derive(Default, Debug)]
pub struct EditorOutput {
    /// The document or selection changed.
    pub changed: bool,
}

/// Persistent (cross-frame) state of the editor widget.
pub struct EditorView {
    id: Id,
    pub zoom: f32,
    theme: Theme,
    /// Remembered x position for repeated up/down movement.
    preferred_x: Option<f32>,
    last_activity: f64,
    dragging: bool,
    scroll_to_caret: bool,
    /// Grab keyboard focus on the next frame (after toolbar interaction).
    focus_requested: bool,
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            id: Id::unique("rusttext-editor"),
            zoom: 1.0,
            theme: Theme::default(),
            preferred_x: None,
            last_activity: 0.0,
            dragging: false,
            scroll_to_caret: true,
            focus_requested: true,
        }
    }

    pub fn id(&self) -> Id {
        self.id
    }

    /// Give the editor keyboard focus again (e.g. after a toolbar click).
    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    /// Scroll the caret into view on the next frame.
    pub fn reveal_caret(&mut self) {
        self.scroll_to_caret = true;
    }

    fn layout(
        &self,
        ui: &Ui,
        session: &EditSession,
        fonts: &FontManager,
        origin: Pos2,
        width: f32,
    ) -> DocLayout {
        let lc = LayoutCtx {
            ctx: ui.ctx(),
            fonts,
            zoom: self.zoom,
        };
        layout_document(&lc, &session.doc, origin, width)
    }

    /// Shows the editor. Must be placed inside a vertical `ScrollArea`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut EditSession,
        fonts: &FontManager,
    ) -> EditorOutput {
        let mut out = EditorOutput::default();
        let zoom = self.zoom;
        let avail = ui.available_rect_before_wrap();
        let page_w = (PAGE_WIDTH * zoom)
            .min(avail.width() - 2.0 * OUTER_GAP)
            .max(120.0);
        let margin = (PAGE_MARGIN * zoom).min(page_w * 0.08).max(12.0);
        let page_left = avail.center().x - page_w / 2.0;
        let page_top = avail.top() + OUTER_GAP;
        let origin = pos2(page_left + margin, page_top + margin);
        let text_w = page_w - 2.0 * margin;

        let mut layout = self.layout(ui, session, fonts, origin, text_w);
        let min_page_h = page_w * 11.0 / 8.5;
        let page_h = (layout.bottom - page_top + margin).max(min_page_h);
        let page = Rect::from_min_size(pos2(page_left, page_top), vec2(page_w, page_h));
        let full = Rect::from_min_max(avail.min, pos2(avail.max.x, page.bottom() + OUTER_GAP));
        let response = ui.interact(full, self.id, Sense::click_and_drag());
        // Reserve the space so the scroll area knows the content height.
        ui.allocate_rect(full, Sense::hover());

        if self.focus_requested {
            ui.memory_mut(|m| m.request_focus(self.id));
            self.focus_requested = false;
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }

        // ---------------- Mouse ----------------
        let now = ui.input(|i| i.time);
        let rev_before = session.revision;
        let sel_before = session.selection;
        if let Some(pointer) = response.interact_pointer_pos() {
            let shift = ui.input(|i| i.modifiers.shift);
            if response.triple_clicked() {
                if let Some(p) = layout.hit_test(pointer) {
                    session.select_paragraph_at(p);
                }
            } else if response.double_clicked() {
                if let Some(p) = layout.hit_test(pointer) {
                    session.select_word_at(p);
                }
            } else if ui.input(|i| i.pointer.primary_pressed()) {
                response.request_focus();
                if let Some(p) = layout.hit_test(pointer) {
                    session.move_to(p, shift);
                }
                self.dragging = true;
                self.preferred_x = None;
            } else if self.dragging
                && response.dragged()
                && let Some(p) = layout.hit_test(pointer)
            {
                let anchor = session.selection.anchor;
                session.select(anchor, p);
                self.scroll_to_caret = true;
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            self.dragging = false;
        }

        // ---------------- Keyboard ----------------
        // A disabled editor (e.g. behind a modal dialog) ignores the keyboard.
        let has_focus = ui.is_enabled() && ui.memory(|m| m.has_focus(self.id));
        if has_focus {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                )
            });
            let events = ui.input(|i| i.events.clone());
            for ev in events {
                if session.revision != rev_before || session.selection != sel_before {
                    // Keep geometry current for layout-dependent movement.
                    layout = self.layout(ui, session, fonts, origin, text_w);
                }
                self.handle_event(ui, session, &layout, &ev);
            }
        }

        if session.revision != rev_before || session.selection != sel_before {
            out.changed = true;
            self.last_activity = now;
            self.scroll_to_caret = true;
            layout = self.layout(ui, session, fonts, origin, text_w);
        }

        // ---------------- Paint ----------------
        let painter = ui.painter_at(ui.clip_rect());
        paint_page(&painter, page, &self.theme);
        let blink_on = ((now - self.last_activity) % 1.0) < 0.55;
        paint_document(
            &painter,
            &layout,
            session,
            &self.theme,
            has_focus && blink_on,
            zoom,
        );
        if has_focus {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }

        if self.scroll_to_caret {
            if let Some(r) = layout.caret_rect(session.caret()) {
                ui.scroll_to_rect(r.expand2(vec2(4.0, 24.0 * zoom)), None);
            }
            self.scroll_to_caret = false;
        }
        out
    }

    fn handle_event(&mut self, ui: &Ui, s: &mut EditSession, layout: &DocLayout, ev: &Event) {
        match ev {
            Event::Text(t) => {
                let m = ui.input(|i| i.modifiers);
                if !(m.command && !m.alt) && !t.chars().all(char::is_control) {
                    s.insert_text(t);
                    self.preferred_x = None;
                }
            }
            Event::Ime(egui::ImeEvent::Commit(t)) => {
                s.insert_text(t);
                self.preferred_x = None;
            }
            Event::Copy => {
                if let Some(t) = s.copy() {
                    ui.ctx().copy_text(t);
                }
            }
            Event::Cut => {
                if let Some(t) = s.cut() {
                    ui.ctx().copy_text(t);
                }
            }
            Event::Paste(t) => {
                s.paste_from(Some(t));
                self.preferred_x = None;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => self.handle_key(ui, s, layout, *key, *modifiers),
            _ => {}
        }
    }

    fn handle_key(
        &mut self,
        ui: &Ui,
        s: &mut EditSession,
        layout: &DocLayout,
        key: Key,
        m: Modifiers,
    ) {
        let word = if cfg!(target_os = "macos") {
            m.alt
        } else {
            m.ctrl
        };
        let extend = m.shift;
        let vertical = matches!(
            key,
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown
        );
        if !vertical {
            self.preferred_x = None;
        }
        // Editing shortcuts.
        if m.command {
            match key {
                Key::A => return s.select_all(),
                Key::B => return s.toggle_attr(ToggleAttr::Bold),
                Key::I => return s.toggle_attr(ToggleAttr::Italic),
                Key::U => return s.toggle_attr(ToggleAttr::Underline),
                Key::Z if m.shift => {
                    s.redo();
                    return;
                }
                Key::Z => {
                    s.undo();
                    return;
                }
                Key::Y => {
                    s.redo();
                    return;
                }
                _ => {}
            }
        }
        match key {
            Key::ArrowLeft => s.move_left(extend, word),
            Key::ArrowRight => s.move_right(extend, word),
            Key::ArrowUp | Key::ArrowDown => {
                let Some(caret) = layout.caret_rect(s.caret()) else {
                    return;
                };
                let x = *self.preferred_x.get_or_insert(caret.left());
                match layout.vertical_neighbor(caret, x, key == Key::ArrowUp) {
                    Some(p) => s.move_to(p, extend),
                    None if key == Key::ArrowUp => {
                        let first = s.doc.first_pos();
                        s.move_to(first, extend);
                    }
                    None => {
                        let last = s.doc.last_pos();
                        s.move_to(last, extend);
                    }
                }
            }
            Key::PageUp | Key::PageDown => {
                let Some(caret) = layout.caret_rect(s.caret()) else {
                    return;
                };
                let x = *self.preferred_x.get_or_insert(caret.left());
                let h = ui.clip_rect().height() * 0.9;
                let y = if key == Key::PageUp {
                    caret.center().y - h
                } else {
                    caret.center().y + h
                };
                if let Some(p) = layout.hit_test(pos2(x, y)) {
                    s.move_to(p, extend);
                }
            }
            Key::Home | Key::End => {
                let home = key == Key::Home;
                if m.command {
                    let p = if home {
                        s.doc.first_pos()
                    } else {
                        s.doc.last_pos()
                    };
                    s.move_to(p, extend);
                } else if let Some(p) = line_edge(layout, s.caret(), home) {
                    s.move_to(p, extend);
                }
            }
            Key::Backspace => {
                if word && !s.has_selection() {
                    let to = s.pos_word_left(s.caret());
                    s.move_to(to, true);
                }
                s.backspace();
            }
            Key::Delete => {
                if word && !s.has_selection() {
                    let to = s.pos_word_right(s.caret());
                    s.move_to(to, true);
                }
                s.delete_forward();
            }
            Key::Enter => s.insert_paragraph_break(),
            Key::Tab => s.tab(m.shift),
            Key::Escape => {
                let c = s.caret();
                s.move_to(c, false);
            }
            _ => {}
        }
    }
}

/// Start or end of the visual line containing `pos`.
fn line_edge(layout: &DocLayout, pos: TextPos, home: bool) -> Option<TextPos> {
    let p = layout.para(pos.path)?;
    let rows = p.rows();
    let row = rows
        .iter()
        .find(|(_, s, e)| pos.offset >= *s && pos.offset <= *e)
        .or(rows.last())?;
    Some(TextPos::new(pos.path, if home { row.1 } else { row.2 }))
}
