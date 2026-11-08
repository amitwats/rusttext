//! Formatting attributes for characters, paragraphs and table cells.
//!
//! The style types are deliberately independent from any GUI library so the
//! document model and the file-format code can be reused (and tested)
//! without pulling in the editor front-end.

use std::fmt;

/// Default font size in points.
pub const DEFAULT_FONT_SIZE: f32 = 12.0;

/// Opaque 24-bit RGB color.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Formats the color as `RRGGBB` (upper case, no leading `#`).
    pub fn to_hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Parses `RRGGBB` or `#RRGGBB`. Returns `None` for anything else
    /// (including the OOXML keyword `auto`).
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Self::rgb(p(0)?, p(2)?, p(4)?))
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.to_hex())
    }
}

/// Character-level formatting of a [`crate::document::Run`].
#[derive(Clone, PartialEq, Debug)]
pub struct CharStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// Font family name. `None` means "the document default font".
    pub font_family: Option<String>,
    /// Font size in points.
    pub font_size: f32,
    /// Text color. `None` means "automatic" (black, or the cell foreground).
    pub color: Option<Color>,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            color: None,
        }
    }
}

/// Horizontal alignment of a paragraph.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Alignment {
    pub const ALL: [Alignment; 4] = [
        Alignment::Left,
        Alignment::Center,
        Alignment::Right,
        Alignment::Justify,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Alignment::Left => "Left",
            Alignment::Center => "Center",
            Alignment::Right => "Right",
            Alignment::Justify => "Justify",
        }
    }
}

/// Kind of list a paragraph belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// Paragraph-level formatting.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ParagraphStyle {
    pub alignment: Alignment,
    pub list: Option<ListKind>,
    /// Nesting level for lists (0 = top level).
    pub indent_level: u8,
    /// Heading level 1..=6, or 0 for body text.
    pub heading: u8,
}

/// Maximum list nesting depth supported by the editor.
pub const MAX_INDENT_LEVEL: u8 = 8;

/// Default font size (points) and boldness used for a heading level.
pub fn heading_font_size(level: u8) -> f32 {
    match level {
        1 => 24.0,
        2 => 18.0,
        3 => 15.0,
        4 => 13.0,
        5 => 12.0,
        6 => 11.0,
        _ => DEFAULT_FONT_SIZE,
    }
}

/// Formatting of a table cell (in addition to the formatting of its text).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CellStyle {
    /// Cell background (shading). `None` = transparent.
    pub background: Option<Color>,
    /// Default text color for runs without an explicit color.
    pub foreground: Option<Color>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = Color::rgb(0x12, 0xAB, 0xFF);
        assert_eq!(c.to_hex(), "12ABFF");
        assert_eq!(Color::from_hex("#12abff"), Some(c));
        assert_eq!(Color::from_hex("auto"), None);
    }
}
