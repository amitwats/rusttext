//! Markdown (`.md`) — CommonMark plus GitHub tables and strikethrough.
//!
//! Supported: headings, bold, italic, strikethrough, underline (as
//! `<u>…</u>`), bullet & numbered lists (nested), tables with column
//! alignment, line breaks inside cells (`<br>`).
//! Not representable in Markdown (lost on save): fonts, sizes, colors,
//! paragraph alignment outside tables and cell colors.

use pulldown_cmark::{Alignment as MdAlign, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{Codec, Result};
use crate::document::style::heading_font_size;
use crate::document::{
    Alignment, Block, Cell, CharStyle, Document, ListKind, Paragraph, ParagraphStyle, Run, Table,
};

pub struct MarkdownCodec;

impl Codec for MarkdownCodec {
    fn read(&self, bytes: &[u8]) -> Result<Document> {
        let text = super::txt::decode_text(bytes);
        Ok(parse(&text))
    }

    fn write(&self, doc: &Document) -> Result<Vec<u8>> {
        Ok(render(doc).into_bytes())
    }
}

// ----------------------------------------------------------------------
// Reading
// ----------------------------------------------------------------------

#[derive(Default)]
struct TableBuilder {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Option<Cell>,
}

#[derive(Default)]
struct MdReader {
    blocks: Vec<Block>,
    cur: Option<Paragraph>,
    bold: u32,
    italic: u32,
    strike: u32,
    underline: u32,
    heading: u8,
    lists: Vec<ListKind>,
    in_item: bool,
    code_block: bool,
    quote_depth: u8,
    table: Option<TableBuilder>,
}

impl MdReader {
    fn char_style(&self) -> CharStyle {
        let mut s = CharStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strikethrough: self.strike > 0,
            underline: self.underline > 0,
            ..Default::default()
        };
        if self.heading > 0 {
            s.bold = true;
            s.font_size = heading_font_size(self.heading);
        }
        if self.code_block {
            s.font_family = Some("Monospace".into());
        }
        s
    }

    fn start_paragraph(&mut self, list: bool) {
        self.finish_paragraph();
        let mut style = ParagraphStyle {
            heading: self.heading,
            ..Default::default()
        };
        if list && let Some(kind) = self.lists.last() {
            style.list = Some(*kind);
        }
        if !self.lists.is_empty() {
            style.indent_level = (self.lists.len() - 1) as u8;
        } else if self.quote_depth > 0 {
            style.indent_level = self.quote_depth;
        }
        if let Some(t) = &self.table {
            let col = t.row.len();
            style.alignment = t.aligns.get(col).copied().unwrap_or_default();
        }
        self.cur = Some(Paragraph {
            runs: Vec::new(),
            style,
        });
    }

    fn finish_paragraph(&mut self) {
        let Some(mut p) = self.cur.take() else { return };
        p.normalize();
        if let Some(t) = &mut self.table
            && let Some(cell) = &mut t.cell
        {
            cell.paragraphs.push(p);
            return;
        }
        self.blocks.push(Block::Paragraph(p));
    }

    fn text(&mut self, text: &str) {
        if self.cur.is_none() {
            self.start_paragraph(false);
        }
        let style = self.char_style();
        if let Some(p) = &mut self.cur {
            p.runs.push(Run::new(text, style));
        }
    }

    fn hard_break(&mut self) {
        let style = self.cur.as_ref().map(|p| p.style.clone());
        self.finish_paragraph();
        let mut style = style.unwrap_or_default();
        style.list = None;
        self.cur = Some(Paragraph {
            runs: Vec::new(),
            style,
        });
    }

    fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.code_block {
                    let lines: Vec<&str> = t.trim_end_matches('\n').split('\n').collect();
                    for line in lines {
                        self.start_paragraph(false);
                        self.text(line);
                        self.finish_paragraph();
                    }
                } else {
                    self.text(&t);
                }
            }
            Event::Code(t) => self.text(&t),
            Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.hard_break(),
            Event::InlineHtml(h) | Event::Html(h) => {
                let tag = h.trim().to_ascii_lowercase().replace(' ', "");
                match tag.as_str() {
                    "<u>" | "<ins>" => self.underline += 1,
                    "</u>" | "</ins>" => self.underline = self.underline.saturating_sub(1),
                    "<br>" | "<br/>" => self.hard_break(),
                    _ => {}
                }
            }
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(r) => self.text(&format!("[^{r}]")),
            Event::Rule => {
                self.finish_paragraph();
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                // The first paragraph of a list item carries the bullet.
                let fresh_item = self.in_item && self.cur.as_ref().is_some_and(|p| p.is_empty());
                if fresh_item {
                    self.in_item = false;
                } else {
                    self.start_paragraph(false);
                }
            }
            Tag::Heading { level, .. } => {
                self.heading = heading_level(level);
                self.start_paragraph(false);
            }
            Tag::BlockQuote(_) => {
                self.finish_paragraph();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(_) => {
                self.finish_paragraph();
                self.code_block = true;
            }
            Tag::List(start) => {
                // An item that only contains a nested list keeps its (empty) bullet.
                self.finish_paragraph();
                self.in_item = false;
                self.lists.push(if start.is_some() {
                    ListKind::Numbered
                } else {
                    ListKind::Bullet
                });
            }
            Tag::Item => {
                self.start_paragraph(true);
                self.in_item = true;
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Table(aligns) => {
                self.finish_paragraph();
                self.table = Some(TableBuilder {
                    aligns: aligns.into_iter().map(from_md_align).collect(),
                    ..Default::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table {
                    t.cell = Some(Cell {
                        paragraphs: Vec::new(),
                        style: Default::default(),
                    });
                }
                self.start_paragraph(false);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.finish_paragraph(),
            TagEnd::Heading(_) => {
                self.finish_paragraph();
                self.heading = 0;
            }
            TagEnd::BlockQuote(_) => {
                self.finish_paragraph();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.finish_paragraph();
                self.code_block = false;
            }
            TagEnd::List(_) => {
                self.finish_paragraph();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.finish_paragraph();
                self.in_item = false;
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::TableCell => {
                self.finish_paragraph();
                if let Some(t) = &mut self.table
                    && let Some(mut cell) = t.cell.take()
                {
                    cell.ensure_paragraph();
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take()
                    && !t.rows.is_empty()
                {
                    self.blocks.push(Block::Table(Table::from_cells(t.rows)));
                }
            }
            _ => {}
        }
    }
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn from_md_align(a: MdAlign) -> Alignment {
    match a {
        MdAlign::Center => Alignment::Center,
        MdAlign::Right => Alignment::Right,
        MdAlign::Left | MdAlign::None => Alignment::Left,
    }
}

/// Parses Markdown text into a document.
pub fn parse(text: &str) -> Document {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = MdReader::default();
    for ev in Parser::new_ext(text, opts) {
        r.event(ev);
    }
    r.finish_paragraph();
    Document::from_blocks(r.blocks)
}

// ----------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------

fn escape(text: &str, at_line_start: bool) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for (i, c) in text.chars().enumerate() {
        let special = matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~'
        ) || (i == 0 && at_line_start && matches!(c, '#' | '-' | '+' | '='));
        if special {
            out.push('\\');
        }
        out.push(c);
    }
    if at_line_start {
        // "1." at the start of a line would become a numbered list.
        let digits = out.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && out[digits..].starts_with(['.', ')']) {
            out.insert(digits, '\\');
        }
    }
    out
}

/// Renders the runs of a paragraph as inline Markdown.
fn inline(p: &Paragraph, skip_heading_style: bool) -> String {
    // Merge runs that only differ in attributes Markdown cannot express.
    let mut segs: Vec<(String, bool, bool, bool, bool)> = Vec::new();
    for r in &p.runs {
        let bold = r.style.bold && !skip_heading_style;
        let key = (
            bold,
            r.style.italic,
            r.style.underline,
            r.style.strikethrough,
        );
        match segs.last_mut() {
            Some(l) if (l.1, l.2, l.3, l.4) == key => l.0.push_str(&r.text),
            _ => segs.push((r.text.clone(), key.0, key.1, key.2, key.3)),
        }
    }
    let mut out = String::new();
    for (i, (text, bold, italic, underline, strike)) in segs.into_iter().enumerate() {
        let trimmed_start = text.trim_start();
        let lead = &text[..text.len() - trimmed_start.len()];
        let core = trimmed_start.trim_end();
        let trail = &trimmed_start[core.len()..];
        let at_start = i == 0 && lead.is_empty();
        out.push_str(lead);
        if core.is_empty() {
            out.push_str(trail);
            continue;
        }
        let (mut open, mut close) = (String::new(), String::new());
        if underline {
            open.push_str("<u>");
            close.insert_str(0, "</u>");
        }
        if strike {
            open.push_str("~~");
            close.insert_str(0, "~~");
        }
        if bold {
            open.push_str("**");
            close.insert_str(0, "**");
        }
        if italic {
            open.push('*');
            close.insert(0, '*');
        }
        out.push_str(&open);
        out.push_str(&escape(core, at_start && open.is_empty()));
        out.push_str(&close);
        out.push_str(trail);
    }
    out
}

fn md_align(a: Alignment) -> &'static str {
    match a {
        Alignment::Center => ":---:",
        Alignment::Right => "---:",
        Alignment::Left | Alignment::Justify => "---",
    }
}

fn render_table(t: &Table) -> String {
    let cell_text = |c: &Cell| {
        let s = c
            .paragraphs
            .iter()
            .map(|p| inline(p, false))
            .collect::<Vec<_>>()
            .join("<br>");
        if s.is_empty() { " ".to_owned() } else { s }
    };
    let mut lines = Vec::new();
    for (i, row) in t.rows.iter().enumerate() {
        let cells: Vec<String> = row.cells.iter().map(cell_text).collect();
        lines.push(format!("| {} |", cells.join(" | ")));
        if i == 0 {
            let aligns: Vec<&str> = row
                .cells
                .iter()
                .map(|c| {
                    md_align(
                        c.paragraphs
                            .first()
                            .map(|p| p.style.alignment)
                            .unwrap_or_default(),
                    )
                })
                .collect();
            lines.push(format!("| {} |", aligns.join(" | ")));
        }
    }
    lines.join("\n")
}

/// Renders a document as Markdown text.
pub fn render(doc: &Document) -> String {
    let mut out = String::new();
    let mut counters = [0usize; 16];
    // Kind of the current top-level list, while the previous block was a list item.
    let mut top_kind: Option<ListKind> = None;
    for block in &doc.blocks {
        let (text, list) = match block {
            Block::Paragraph(p) => {
                if let Some(kind) = p.style.list {
                    let lvl = (p.style.indent_level as usize).min(15);
                    let indent = "    ".repeat(lvl);
                    let marker = match kind {
                        ListKind::Bullet => "- ".to_owned(),
                        ListKind::Numbered => {
                            counters[lvl] += 1;
                            counters[lvl + 1..].iter_mut().for_each(|c| *c = 0);
                            format!("{}. ", counters[lvl])
                        }
                    };
                    (
                        format!("{indent}{marker}{}", inline(p, false)),
                        Some((kind, p.style.indent_level)),
                    )
                } else if p.style.heading > 0 {
                    counters = [0; 16];
                    let h = "#".repeat(p.style.heading.min(6) as usize);
                    (format!("{h} {}", inline(p, true)), None)
                } else {
                    counters = [0; 16];
                    if p.is_empty() {
                        continue;
                    }
                    (inline(p, false), None)
                }
            }
            Block::Table(t) => {
                counters = [0; 16];
                (render_table(t), None)
            }
        };
        if !out.is_empty() {
            // Items of the same list are on consecutive lines; a change of list
            // type at the top level needs a blank line to start a new list.
            let same_list = match (top_kind, list) {
                (Some(top), Some((kind, level))) => level > 0 || kind == top,
                _ => false,
            };
            out.push_str(if same_list { "\n" } else { "\n\n" });
        }
        out.push_str(&text);
        top_kind = match list {
            Some((kind, 0)) => Some(kind),
            Some(_) => top_kind.or(list.map(|l| l.0)),
            None => None,
        };
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_formatting() {
        let doc = parse("# Title\n\nSome **bold** and *italic* and ~~gone~~ and <u>under</u>.\n");
        let h = doc.blocks[0].as_paragraph().unwrap();
        assert_eq!(h.style.heading, 1);
        assert_eq!(h.plain_text(), "Title");
        let p = doc.blocks[1].as_paragraph().unwrap();
        assert_eq!(p.plain_text(), "Some bold and italic and gone and under.");
        assert!(p.runs.iter().any(|r| r.style.bold && r.text == "bold"));
        assert!(p.runs.iter().any(|r| r.style.italic && r.text == "italic"));
        assert!(
            p.runs
                .iter()
                .any(|r| r.style.strikethrough && r.text == "gone")
        );
        assert!(
            p.runs
                .iter()
                .any(|r| r.style.underline && r.text == "under")
        );
    }

    #[test]
    fn parse_lists() {
        let doc = parse("- a\n- b\n    - c\n\n1. one\n2. two\n");
        let ps: Vec<&Paragraph> = doc.all_paragraphs().filter(|p| !p.is_empty()).collect();
        assert_eq!(ps.len(), 5);
        assert_eq!(ps[0].style.list, Some(ListKind::Bullet));
        assert_eq!(ps[2].style.indent_level, 1);
        assert_eq!(ps[3].style.list, Some(ListKind::Numbered));
    }

    #[test]
    fn parse_table() {
        let doc = parse("| a | b |\n| :---: | ---: |\n| 1 | 2 |\n");
        let t = doc.blocks.iter().find_map(Block::as_table).unwrap();
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.cell(1, 1).unwrap().plain_text(), "2");
        assert_eq!(
            t.cell(1, 0).unwrap().paragraphs[0].style.alignment,
            Alignment::Center
        );
    }

    #[test]
    fn roundtrip() {
        let src = "# Heading\n\nHello **bold** *it* ~~s~~ <u>u</u>\n\n- x\n- y\n\n1. one\n2. two\n\n| a | b |\n| --- | :---: |\n| 1 | 2 |\n";
        let doc = parse(src);
        let out = render(&doc);
        assert_eq!(parse(&out), doc);
        assert_eq!(out, src);
    }

    #[test]
    fn escapes_special_characters() {
        let doc = Document::from_plain_text("1. not a list *nor emphasis*");
        let out = render(&doc);
        assert_eq!(parse(&out).plain_text(), "1. not a list *nor emphasis*");
    }
}
