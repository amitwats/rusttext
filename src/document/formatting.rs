//! Character and paragraph formatting commands.

use super::history::EditKind;
use super::position::SelectionKind;
use super::session::EditSession;
use super::style::{
    Alignment, CharStyle, DEFAULT_FONT_SIZE, ListKind, MAX_INDENT_LEVEL, ParagraphStyle,
    heading_font_size,
};

/// A character attribute that can be toggled on and off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToggleAttr {
    Bold,
    Italic,
    Underline,
    Strikethrough,
}

impl ToggleAttr {
    fn get(self, s: &CharStyle) -> bool {
        match self {
            ToggleAttr::Bold => s.bold,
            ToggleAttr::Italic => s.italic,
            ToggleAttr::Underline => s.underline,
            ToggleAttr::Strikethrough => s.strikethrough,
        }
    }

    fn set(self, s: &mut CharStyle, v: bool) {
        match self {
            ToggleAttr::Bold => s.bold = v,
            ToggleAttr::Italic => s.italic = v,
            ToggleAttr::Underline => s.underline = v,
            ToggleAttr::Strikethrough => s.strikethrough = v,
        }
    }
}

impl EditSession {
    /// The character style shown in the toolbar: the typing style for a
    /// caret, or the style of the first selected character.
    pub fn current_char_style(&self) -> CharStyle {
        if !self.has_selection() {
            return self.style_for_typing();
        }
        for r in self.selected_ranges() {
            if let Some(p) = self.doc.paragraph(r.path)
                && let Some(s) = p.style_of_char(r.start)
                && r.start < r.end
            {
                return s;
            }
        }
        self.style_for_typing()
    }

    /// The paragraph style of the paragraph containing the caret.
    pub fn current_paragraph_style(&self) -> ParagraphStyle {
        self.doc
            .paragraph(self.caret().path)
            .map(|p| p.style.clone())
            .unwrap_or_default()
    }

    /// True when the attribute is set on all selected text (or on the typing style).
    pub fn attr_active(&self, attr: ToggleAttr) -> bool {
        if !self.has_selection() {
            return attr.get(&self.style_for_typing());
        }
        let mut any = false;
        for r in self.selected_ranges() {
            if let Some(p) = self.doc.paragraph(r.path) {
                for s in p.styles_in(r.start, r.end) {
                    any = true;
                    if !attr.get(s) {
                        return false;
                    }
                }
            }
        }
        any
    }

    /// Applies `f` to the selected characters. With a collapsed caret, `f`
    /// modifies the style used for the next typed text.
    pub fn apply_char_style(&mut self, f: &dyn Fn(&mut CharStyle)) {
        if !self.has_selection() {
            let mut s = self.style_for_typing();
            f(&mut s);
            self.typing_style = Some(s);
            // An empty paragraph has no runs to carry the style, so the
            // typing style is the only place it can live; nothing else to do.
            return;
        }
        self.begin_edit(EditKind::Other);
        let full_cells = matches!(self.selection_kind(), SelectionKind::Cells { .. });
        for r in self.selected_ranges() {
            if let Some(p) = self.doc.paragraph_mut(r.path) {
                if full_cells {
                    p.apply_style_all(f);
                } else {
                    p.apply_style(r.start, r.end, f);
                }
            }
        }
        self.end_edit();
    }

    pub fn toggle_attr(&mut self, attr: ToggleAttr) {
        let on = !self.attr_active(attr);
        self.apply_char_style(&move |s| attr.set(s, on));
    }

    pub fn set_font_family(&mut self, family: Option<String>) {
        self.apply_char_style(&move |s| s.font_family = family.clone());
    }

    pub fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(4.0, 400.0);
        self.apply_char_style(&move |s| s.font_size = size);
    }

    pub fn set_text_color(&mut self, color: Option<super::style::Color>) {
        self.apply_char_style(&move |s| s.color = color);
    }

    /// Applies `f` to every paragraph touched by the selection.
    pub fn apply_paragraph_style(&mut self, f: &dyn Fn(&mut ParagraphStyle)) {
        self.begin_edit(EditKind::Other);
        for r in self.selected_ranges() {
            if let Some(p) = self.doc.paragraph_mut(r.path) {
                f(&mut p.style);
            }
        }
        self.end_edit();
    }

    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.apply_paragraph_style(&move |s| s.alignment = alignment);
    }

    /// Turns the selected paragraphs into a list of `kind`, or removes the
    /// list if they all already are.
    pub fn toggle_list(&mut self, kind: ListKind) {
        let all = self.selected_ranges().iter().all(|r| {
            self.doc
                .paragraph(r.path)
                .is_some_and(|p| p.style.list == Some(kind))
        });
        self.apply_paragraph_style(&move |s| {
            if all {
                s.list = None;
                s.indent_level = 0;
            } else {
                s.list = Some(kind);
                s.heading = 0;
            }
        });
    }

    /// Increases (`delta > 0`) or decreases the list nesting level.
    pub fn change_indent(&mut self, delta: i32) {
        self.apply_paragraph_style(&move |s| {
            let lvl = (s.indent_level as i32 + delta).clamp(0, MAX_INDENT_LEVEL as i32);
            s.indent_level = lvl as u8;
        });
    }

    /// Makes the selected paragraphs headings of `level` (0 = body text),
    /// adjusting their font size and weight.
    pub fn set_heading(&mut self, level: u8) {
        let level = level.min(6);
        self.begin_edit(EditKind::Other);
        for r in self.selected_ranges() {
            if let Some(p) = self.doc.paragraph_mut(r.path) {
                p.style.heading = level;
                if level > 0 {
                    p.style.list = None;
                }
                let size = heading_font_size(level);
                p.apply_style_all(&|s| {
                    s.font_size = if level > 0 { size } else { DEFAULT_FONT_SIZE };
                    s.bold = level > 0;
                });
            }
        }
        self.typing_style = None;
        self.end_edit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::Document;
    use crate::document::position::{ParaPath, TextPos};

    #[test]
    fn toggle_bold_on_selection() {
        let mut s = EditSession::new(Document::from_plain_text("hello world"));
        s.select(
            TextPos::new(ParaPath::top(0), 0),
            TextPos::new(ParaPath::top(0), 5),
        );
        s.toggle_attr(ToggleAttr::Bold);
        let p = s.doc.paragraph(ParaPath::top(0)).unwrap();
        assert_eq!(p.runs.len(), 2);
        assert!(p.runs[0].style.bold);
        assert!(s.attr_active(ToggleAttr::Bold));
        s.toggle_attr(ToggleAttr::Bold);
        assert_eq!(s.doc.paragraph(ParaPath::top(0)).unwrap().runs.len(), 1);
    }

    #[test]
    fn typing_style_applies_to_new_text() {
        let mut s = EditSession::new(Document::from_plain_text("ab"));
        s.move_to(TextPos::new(ParaPath::top(0), 2), false);
        s.toggle_attr(ToggleAttr::Italic);
        s.insert_text("c");
        let p = s.doc.paragraph(ParaPath::top(0)).unwrap();
        assert_eq!(p.runs.len(), 2);
        assert!(p.runs[1].style.italic);
    }

    #[test]
    fn list_toggle() {
        let mut s = EditSession::new(Document::from_plain_text("a\nb"));
        s.select_all();
        s.toggle_list(ListKind::Bullet);
        assert!(
            s.doc
                .all_paragraphs()
                .all(|p| p.style.list == Some(ListKind::Bullet))
        );
        s.toggle_list(ListKind::Bullet);
        assert!(s.doc.all_paragraphs().all(|p| p.style.list.is_none()));
    }
}
