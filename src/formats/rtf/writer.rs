//! RTF serialization.

use std::fmt::Write as _;

use crate::document::{
    Alignment, Block, Cell, CharStyle, Color, Document, ListKind, Paragraph, Table,
};

/// Fallback font name written as font 0 for runs that use the default font.
pub(super) const DEFAULT_FONT_NAME: &str = "Calibri";
/// Twips (1/1440 inch) per list level indentation.
const LIST_INDENT: i32 = 360;
/// Total table width in twips (6.5 inches).
const TABLE_WIDTH: i32 = 9360;

struct Tables {
    fonts: Vec<String>,
    colors: Vec<Color>,
}

impl Tables {
    fn font(&mut self, name: &Option<String>) -> usize {
        match name {
            None => 0,
            Some(n) => {
                if let Some(i) = self.fonts.iter().skip(1).position(|f| f == n) {
                    return i + 1;
                }
                self.fonts.push(n.clone());
                self.fonts.len() - 1
            }
        }
    }

    /// Color index (1-based; 0 is "auto").
    fn color(&mut self, c: Color) -> usize {
        if let Some(i) = self.colors.iter().position(|x| *x == c) {
            return i + 1;
        }
        self.colors.push(c);
        self.colors.len()
    }
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\t' => out.push_str("\\tab "),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{}?", *unit as i16);
                }
            }
        }
    }
}

fn write_runs(out: &mut String, p: &Paragraph, cell_fg: Option<Color>, t: &mut Tables) {
    for run in &p.runs {
        let s: &CharStyle = &run.style;
        out.push('{');
        let _ = write!(
            out,
            "\\f{}\\fs{}",
            t.font(&s.font_family),
            (s.font_size * 2.0).round() as i32
        );
        if s.bold {
            out.push_str("\\b");
        }
        if s.italic {
            out.push_str("\\i");
        }
        if s.underline {
            out.push_str("\\ul");
        }
        if s.strikethrough {
            out.push_str("\\strike");
        }
        if let Some(c) = s.color.or(cell_fg) {
            let _ = write!(out, "\\cf{}", t.color(c));
        }
        out.push(' ');
        escape_into(out, &run.text);
        out.push('}');
    }
}

fn paragraph_props(out: &mut String, p: &Paragraph, in_table: bool, number: usize) {
    out.push_str("\\pard\\plain");
    if in_table {
        out.push_str("\\intbl");
    }
    if let Some(kind) = p.style.list {
        let li = LIST_INDENT * 2 * (p.style.indent_level as i32 + 1);
        match kind {
            ListKind::Bullet => {
                out.push_str("{\\pntext\\f0 \\u8226?\\tab}");
                out.push_str("{\\*\\pn\\pnlvlblt\\pnf0\\pnindent360{\\pntxtb\\u8226?}}");
            }
            ListKind::Numbered => {
                let _ = write!(out, "{{\\pntext\\f0 {number}.\\tab}}");
                out.push_str("{\\*\\pn\\pnlvlbody\\pnf0\\pnindent360\\pnstart1\\pndec{\\pntxta.}}");
            }
        }
        let _ = write!(out, "\\fi-{LIST_INDENT}\\li{li}");
    } else if p.style.indent_level > 0 {
        let _ = write!(out, "\\li{}", LIST_INDENT * 2 * p.style.indent_level as i32);
    }
    if p.style.heading > 0 {
        let _ = write!(out, "\\outlinelevel{}", p.style.heading - 1);
    }
    // End with a control word so the following space is just a delimiter.
    out.push_str(match p.style.alignment {
        Alignment::Left => "\\ql ",
        Alignment::Center => "\\qc ",
        Alignment::Right => "\\qr ",
        Alignment::Justify => "\\qj ",
    });
}

fn write_table(out: &mut String, table: &Table, t: &mut Tables) {
    let cols = table.col_count().max(1) as i32;
    let width = TABLE_WIDTH / cols;
    for row in &table.rows {
        out.push_str("\\trowd\\trgaph108\\trleft0");
        for (ci, cell) in row.cells.iter().enumerate() {
            for side in ["t", "l", "b", "r"] {
                let _ = write!(out, "\\clbrdr{side}\\brdrs\\brdrw10");
            }
            if let Some(bg) = cell.style.background {
                let _ = write!(out, "\\clcbpat{}", t.color(bg));
            }
            let _ = write!(out, "\\cellx{}", width * (ci as i32 + 1));
        }
        out.push('\n');
        for cell in &row.cells {
            write_cell(out, cell, t);
        }
        out.push_str("\\row\n");
    }
}

fn write_cell(out: &mut String, cell: &Cell, t: &mut Tables) {
    let mut number = 0;
    for (i, p) in cell.paragraphs.iter().enumerate() {
        number = next_number(p, number);
        paragraph_props(out, p, true, number);
        write_runs(out, p, cell.style.foreground, t);
        if i + 1 < cell.paragraphs.len() {
            out.push_str("\\par\n");
        }
    }
    out.push_str("\\cell\n");
}

fn next_number(p: &Paragraph, prev: usize) -> usize {
    match p.style.list {
        Some(ListKind::Numbered) => prev + 1,
        _ => 0,
    }
}

/// Serializes `doc` as RTF.
pub fn write(doc: &Document) -> String {
    let mut t = Tables {
        fonts: vec![DEFAULT_FONT_NAME.to_owned()],
        colors: Vec::new(),
    };
    let mut body = String::new();
    let mut number = 0;
    for block in &doc.blocks {
        match block {
            Block::Paragraph(p) => {
                number = next_number(p, number);
                paragraph_props(&mut body, p, false, number);
                write_runs(&mut body, p, None, &mut t);
                body.push_str("\\par\n");
            }
            Block::Table(table) => {
                number = 0;
                write_table(&mut body, table, &mut t);
            }
        }
    }

    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1\n{\\fonttbl");
    for (i, f) in t.fonts.iter().enumerate() {
        let _ = write!(out, "{{\\f{i}\\fnil\\fcharset0 ");
        escape_into(&mut out, f);
        out.push_str(";}");
    }
    out.push_str("}\n{\\colortbl ;");
    for c in &t.colors {
        let _ = write!(out, "\\red{}\\green{}\\blue{};", c.r, c.g, c.b);
    }
    out.push_str("}\n{\\*\\generator RustText;}\\viewkind4\n");
    out.push_str(&body);
    out.push('}');
    out
}
