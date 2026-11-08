//! DOCX parsing.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::document::style::heading_font_size;
use crate::document::{
    Alignment, Block, Cell, CharStyle, Color, Document, ListKind, Paragraph, Run, Table,
};
use crate::formats::{FormatError, Result};

/// Value of the attribute with the given local name (namespace prefix ignored).
fn attr(e: &BytesStart<'_>, local: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == local)
            .then(|| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(|v| v.into_owned())
            })
            .flatten()
    })
}

/// OOXML "on/off" property: absent `w:val` means on.
fn on_off(e: &BytesStart<'_>) -> bool {
    !matches!(
        attr(e, "val").as_deref(),
        Some("0" | "false" | "off" | "none")
    )
}

fn local_name(e: &BytesStart<'_>) -> String {
    e.local_name().as_ref().to_owned()
}

fn read_part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Finds the main document part via `_rels/.rels` (falls back to the default name).
fn main_part_name(zip: &mut zip::ZipArchive<Cursor<&[u8]>>) -> String {
    let default = "word/document.xml".to_owned();
    let Some(rels) = read_part(zip, "_rels/.rels") else {
        return default;
    };
    let mut r = Reader::from_str(&rels);
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if local_name(&e) == "Relationship" => {
                if attr(&e, "Type").is_some_and(|t| t.ends_with("/officeDocument"))
                    && let Some(t) = attr(&e, "Target")
                {
                    return t.trim_start_matches('/').to_owned();
                }
            }
            Ok(Event::Eof) | Err(_) => return default,
            _ => {}
        }
    }
}

/// numId → (ilvl → is_bullet)
#[derive(Default)]
struct Numbering {
    nums: HashMap<String, String>,
    abstract_levels: HashMap<String, HashMap<u8, bool>>,
}

impl Numbering {
    fn parse(xml: &str) -> Self {
        let mut n = Numbering::default();
        let mut r = Reader::from_str(xml);
        let mut cur_abs: Option<String> = None;
        let mut cur_lvl: u8 = 0;
        let mut cur_num: Option<String> = None;
        loop {
            match r.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => match local_name(&e).as_str() {
                    "abstractNum" => cur_abs = attr(&e, "abstractNumId"),
                    "lvl" => cur_lvl = attr(&e, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                    "numFmt" => {
                        if let Some(a) = &cur_abs {
                            let bullet = attr(&e, "val").as_deref() == Some("bullet");
                            n.abstract_levels
                                .entry(a.clone())
                                .or_default()
                                .insert(cur_lvl, bullet);
                        }
                    }
                    "num" => cur_num = attr(&e, "numId"),
                    "abstractNumId" => {
                        if let (Some(num), Some(v)) = (&cur_num, attr(&e, "val")) {
                            n.nums.insert(num.clone(), v);
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => match e.local_name().as_ref() {
                    "abstractNum" => cur_abs = None,
                    "num" => cur_num = None,
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        n
    }

    fn kind(&self, num_id: &str, ilvl: u8) -> ListKind {
        let bullet = self
            .nums
            .get(num_id)
            .and_then(|a| self.abstract_levels.get(a))
            .and_then(|levels| levels.get(&ilvl).or_else(|| levels.get(&0)))
            .copied()
            .unwrap_or(true);
        if bullet {
            ListKind::Bullet
        } else {
            ListKind::Numbered
        }
    }
}

/// styleId → heading level, from `styles.xml`.
fn heading_styles(xml: &str) -> HashMap<String, u8> {
    let mut map = HashMap::new();
    let mut r = Reader::from_str(xml);
    let mut cur: Option<String> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match local_name(&e).as_str() {
                "style" => cur = attr(&e, "styleId"),
                "name" => {
                    if let (Some(id), Some(name)) = (&cur, attr(&e, "val")) {
                        let lower = name.to_ascii_lowercase();
                        let level = if lower == "title" {
                            Some(1)
                        } else {
                            lower
                                .strip_prefix("heading ")
                                .and_then(|l| l.trim().parse::<u8>().ok())
                        };
                        if let Some(l) = level.filter(|l| (1..=6).contains(l)) {
                            map.insert(id.clone(), l);
                        }
                    }
                }
                "outlineLvl" => {
                    if let (Some(id), Some(v)) =
                        (&cur, attr(&e, "val").and_then(|v| v.parse::<u8>().ok()))
                        && v < 6
                    {
                        map.entry(id.clone()).or_insert(v + 1);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == "style" => cur = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Elements whose content is never part of the visible body text.
const SKIPPED: &[&str] = &[
    "drawing",
    "pict",
    "object",
    "AlternateContent",
    "txbxContent",
    "del",
    "moveFrom",
    "instrText",
    "delText",
    "sectPr",
    "footnoteReference",
    "endnoteReference",
    "commentReference",
    "fldData",
];

#[derive(Default)]
struct BodyReader {
    numbering: Numbering,
    headings: HashMap<String, u8>,
    blocks: Vec<Block>,

    skip_depth: usize,
    tbl_depth: usize,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Option<Cell>,
    in_tcpr: bool,

    para: Option<Paragraph>,
    num_id: Option<String>,
    ilvl: u8,
    in_ppr: bool,

    in_run: bool,
    in_rpr: bool,
    run_style: CharStyle,
    in_text: bool,
}

impl BodyReader {
    fn push_text(&mut self, s: &str) {
        if !self.in_run || s.is_empty() {
            return;
        }
        let style = self.run_style.clone();
        let p = self.para.get_or_insert_with(Paragraph::new);
        match p.runs.last_mut() {
            Some(r) if r.style == style => r.text.push_str(s),
            _ => p.runs.push(Run::new(s, style)),
        }
    }

    fn finish_paragraph(&mut self) {
        let Some(mut p) = self.para.take() else {
            return;
        };
        if let Some(id) = self.num_id.take().filter(|id| id != "0") {
            p.style.list = Some(self.numbering.kind(&id, self.ilvl));
            p.style.indent_level = self.ilvl.min(crate::document::style::MAX_INDENT_LEVEL);
        }
        self.ilvl = 0;
        p.normalize();
        match &mut self.cell {
            Some(cell) if self.tbl_depth > 0 => cell.paragraphs.push(p),
            _ => self.blocks.push(Block::Paragraph(p)),
        }
    }

    /// A line break inside a paragraph: continue in a new paragraph with the same style.
    fn line_break(&mut self) {
        let style = self
            .para
            .as_ref()
            .map(|p| p.style.clone())
            .unwrap_or_default();
        let (num, ilvl) = (self.num_id.clone(), self.ilvl);
        self.finish_paragraph();
        let mut style = style;
        style.list = None;
        self.para = Some(Paragraph {
            runs: Vec::new(),
            style,
        });
        // Continuation lines are not new list items.
        let _ = (num, ilvl);
    }

    fn start(&mut self, e: &BytesStart<'_>, empty: bool) {
        let name = local_name(e);
        if self.skip_depth > 0 {
            if !empty {
                self.skip_depth += 1;
            }
            return;
        }
        if SKIPPED.contains(&name.as_str()) {
            if !empty {
                self.skip_depth = 1;
            }
            return;
        }
        match name.as_str() {
            "p" => {
                self.finish_paragraph();
                self.para = Some(Paragraph::new());
                self.num_id = None;
                self.ilvl = 0;
                if empty {
                    self.finish_paragraph();
                }
            }
            "pPr" if !empty => self.in_ppr = true,
            "pStyle" if self.in_ppr && !self.in_rpr => {
                if let Some(v) = attr(e, "val") {
                    // Prefer styles.xml; fall back to Word's built-in style ids.
                    let level = self.headings.get(&v).copied().or_else(|| {
                        let lower = v.to_ascii_lowercase();
                        if lower == "title" {
                            return Some(1);
                        }
                        lower
                            .strip_prefix("heading")
                            .and_then(|l| l.parse::<u8>().ok())
                            .filter(|l| (1..=6).contains(l))
                    });
                    if let (Some(l), Some(p)) = (level, &mut self.para) {
                        p.style.heading = l;
                    }
                }
            }
            "jc" if self.in_ppr && !self.in_rpr => {
                let a = match attr(e, "val").as_deref() {
                    Some("center") => Alignment::Center,
                    Some("right" | "end") => Alignment::Right,
                    Some("both" | "distribute" | "justify") => Alignment::Justify,
                    _ => Alignment::Left,
                };
                if let Some(p) = &mut self.para {
                    p.style.alignment = a;
                }
            }
            "ilvl" if self.in_ppr => {
                self.ilvl = attr(e, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "numId" if self.in_ppr => self.num_id = attr(e, "val"),
            "r" if !empty => {
                self.in_run = true;
                let heading = self.para.as_ref().map_or(0, |p| p.style.heading);
                self.run_style = CharStyle::default();
                if heading > 0 {
                    self.run_style.bold = true;
                    self.run_style.font_size = heading_font_size(heading);
                }
            }
            "rPr" if !empty => self.in_rpr = true,
            "b" | "i" | "strike" | "dstrike" | "u" | "color" | "sz" | "rFonts"
                if self.in_rpr && !self.in_ppr =>
            {
                let s = &mut self.run_style;
                match name.as_str() {
                    "b" => s.bold = on_off(e),
                    "i" => s.italic = on_off(e),
                    "strike" | "dstrike" => s.strikethrough = on_off(e),
                    "u" => s.underline = on_off(e),
                    "color" => s.color = attr(e, "val").and_then(|v| Color::from_hex(&v)),
                    "sz" => {
                        if let Some(v) = attr(e, "val").and_then(|v| v.parse::<f32>().ok()) {
                            s.font_size = v / 2.0;
                        }
                    }
                    "rFonts" => {
                        if let Some(f) = attr(e, "ascii")
                            .or_else(|| attr(e, "hAnsi"))
                            .or_else(|| attr(e, "cs"))
                            .or_else(|| attr(e, "eastAsia"))
                        {
                            s.font_family = Some(f);
                        }
                    }
                    _ => {}
                }
            }
            "t" if self.in_run && !empty => self.in_text = true,
            "tab" if self.in_run && !self.in_ppr => self.push_text("\t"),
            "br" | "cr" if self.in_run => self.line_break(),
            "noBreakHyphen" if self.in_run => self.push_text("\u{2011}"),
            "tbl" if !empty => {
                self.finish_paragraph();
                self.tbl_depth += 1;
                if self.tbl_depth == 1 {
                    self.rows.clear();
                }
            }
            "tr" if self.tbl_depth == 1 && !empty => self.row.clear(),
            "tc" if self.tbl_depth == 1 && !empty => {
                self.cell = Some(Cell {
                    paragraphs: Vec::new(),
                    style: Default::default(),
                });
            }
            "tcPr" if self.tbl_depth == 1 && !empty => self.in_tcpr = true,
            "shd" if self.in_tcpr => {
                if let (Some(fill), Some(cell)) = (attr(e, "fill"), &mut self.cell) {
                    cell.style.background = Color::from_hex(&fill);
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            return;
        }
        match name {
            "p" => self.finish_paragraph(),
            "pPr" => self.in_ppr = false,
            "r" => {
                self.in_run = false;
                self.in_text = false;
            }
            "rPr" => self.in_rpr = false,
            "t" => self.in_text = false,
            "tcPr" => self.in_tcpr = false,
            "tc" if self.tbl_depth == 1 => {
                self.finish_paragraph();
                if let Some(mut cell) = self.cell.take() {
                    cell.ensure_paragraph();
                    self.row.push(cell);
                }
            }
            "tr" if self.tbl_depth == 1 => {
                let row = std::mem::take(&mut self.row);
                if !row.is_empty() {
                    self.rows.push(row);
                }
            }
            "tbl" => {
                self.finish_paragraph();
                self.tbl_depth = self.tbl_depth.saturating_sub(1);
                if self.tbl_depth == 0 && !self.rows.is_empty() {
                    let rows = std::mem::take(&mut self.rows);
                    self.blocks.push(Block::Table(Table::from_cells(rows)));
                }
            }
            _ => {}
        }
    }

    fn parse(mut self, xml: &str) -> Result<Document> {
        let mut r = Reader::from_str(xml);
        loop {
            match r.read_event()? {
                Event::Start(e) => self.start(&e, false),
                Event::Empty(e) => self.start(&e, true),
                Event::End(e) => self.end(e.local_name().as_ref()),
                Event::Text(t) if self.in_text && self.skip_depth == 0 => {
                    let s = t.xml10_content().into_owned();
                    self.push_text(&s);
                }
                Event::CData(t) if self.in_text && self.skip_depth == 0 => {
                    let s = t.into_inner().into_owned();
                    self.push_text(&s);
                }
                Event::GeneralRef(r) if self.in_text && self.skip_depth == 0 => {
                    let c = match r.resolve_char_ref().ok().flatten() {
                        Some(c) => Some(c),
                        None => match &*r {
                            "lt" => Some('<'),
                            "gt" => Some('>'),
                            "amp" => Some('&'),
                            "quot" => Some('"'),
                            "apos" => Some('\''),
                            _ => None,
                        },
                    };
                    if let Some(c) = c {
                        self.push_text(&c.to_string());
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        self.finish_paragraph();
        Ok(Document::from_blocks(self.blocks))
    }
}

/// Parses DOCX bytes into a document.
pub fn read(bytes: &[u8]) -> Result<Document> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let main = main_part_name(&mut zip);
    let xml = read_part(&mut zip, &main)
        .ok_or_else(|| FormatError::Malformed(format!("missing {main}")))?;
    let dir = main.rsplit_once('/').map_or("", |(d, _)| d);
    let part = |n: &str| {
        if dir.is_empty() {
            n.to_owned()
        } else {
            format!("{dir}/{n}")
        }
    };
    let numbering = read_part(&mut zip, &part("numbering.xml"))
        .map(|x| Numbering::parse(&x))
        .unwrap_or_default();
    let headings = read_part(&mut zip, &part("styles.xml"))
        .map(|x| heading_styles(&x))
        .unwrap_or_default();
    let reader = BodyReader {
        numbering,
        headings,
        ..Default::default()
    };
    reader.parse(&xml)
}
