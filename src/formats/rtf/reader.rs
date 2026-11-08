//! RTF parsing: a tokenizer followed by a small state machine.

use super::cp1252_to_char;
use crate::document::{
    Alignment, Block, Cell, CharStyle, Color, Document, ListKind, Paragraph, ParagraphStyle, Run,
    Table,
};
use crate::formats::{FormatError, Result};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    GroupStart,
    GroupEnd,
    Word(String, Option<i32>),
    Symbol(char),
    Hex(u8),
    Text(u8),
}

fn tokenize(src: &[u8]) -> Vec<Token> {
    let mut out = Vec::with_capacity(src.len() / 2);
    let mut i = 0;
    while i < src.len() {
        let b = src[i];
        match b {
            b'{' => out.push(Token::GroupStart),
            b'}' => out.push(Token::GroupEnd),
            b'\r' | b'\n' => {}
            b'\\' => {
                i += 1;
                let Some(&c) = src.get(i) else { break };
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < src.len() && src[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let name = String::from_utf8_lossy(&src[start..i]).into_owned();
                    let num_start = i;
                    if i < src.len() && src[i] == b'-' {
                        i += 1;
                    }
                    while i < src.len() && src[i].is_ascii_digit() {
                        i += 1;
                    }
                    let param = std::str::from_utf8(&src[num_start..i])
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                        .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
                    // A single space delimits the control word and is consumed.
                    if i < src.len() && src[i] == b' ' {
                        i += 1;
                    }
                    out.push(Token::Word(name, param));
                    continue;
                } else if c == b'\'' {
                    let hex = src
                        .get(i + 1..i + 3)
                        .and_then(|h| std::str::from_utf8(h).ok());
                    if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        out.push(Token::Hex(v));
                        i += 3;
                        continue;
                    }
                } else if c == b'\r' || c == b'\n' {
                    // "\<newline>" is equivalent to \par.
                    out.push(Token::Word("par".into(), None));
                } else {
                    out.push(Token::Symbol(c as char));
                }
            }
            _ => out.push(Token::Text(b)),
        }
        i += 1;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dest {
    Normal,
    FontTable,
    ColorTable,
    /// Legacy list marker text (`\pntext`): ignored, but marks a list item.
    PnText,
    /// Word list marker text (`\listtext`): collected to guess the list kind.
    ListText,
    /// Legacy list definition (`\*\pn`).
    Pn,
    Skip,
}

#[derive(Clone, Debug)]
struct GroupState {
    style: CharStyle,
    dest: Dest,
    uc: usize,
    /// The previous token was `\*` (ignorable destination follows).
    star: bool,
}

#[derive(Default)]
struct ParaProps {
    alignment: Alignment,
    list: Option<ListKind>,
    left_indent: i32,
    in_table: bool,
    heading: u8,
}

struct Parser {
    stack: Vec<GroupState>,
    state: GroupState,
    fonts: Vec<(i32, String)>,
    font_name_buf: String,
    font_num: i32,
    default_font: i32,
    colors: Vec<Option<Color>>,
    color_buf: (Option<u8>, Option<u8>, Option<u8>),
    skip_chars: usize,
    high_surrogate: Option<u16>,
    para: ParaProps,
    runs: Vec<Run>,
    list_text: String,
    blocks: Vec<Block>,
    // Table assembly.
    table: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell_paras: Vec<Paragraph>,
    cell_bgs: Vec<Option<Color>>,
    pending_bg: Option<Color>,
}

const SKIP_DESTINATIONS: &[&str] = &[
    "stylesheet",
    "info",
    "pict",
    "header",
    "headerl",
    "headerr",
    "headerf",
    "footer",
    "footerl",
    "footerr",
    "footerf",
    "footnote",
    "annotation",
    "object",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "xmlnstbl",
    "listtable",
    "listoverridetable",
    "revtbl",
    "rsidtbl",
    "generator",
    "mmathPr",
    "fldinst",
    "shp",
    "shpinst",
    "nonshppict",
    "filetbl",
    "operator",
    "author",
    "title",
    "subject",
    "keywords",
    "comment",
    "doccomm",
    "bkmkstart",
    "bkmkend",
    "xe",
    "tc",
    "txe",
    "atnid",
    "atnauthor",
    "pgdsctbl",
    "wgrffmtfilter",
];

impl Parser {
    fn new() -> Self {
        let state = GroupState {
            style: CharStyle::default(),
            dest: Dest::Normal,
            uc: 1,
            star: false,
        };
        Self {
            stack: Vec::new(),
            state,
            fonts: Vec::new(),
            font_name_buf: String::new(),
            font_num: 0,
            default_font: 0,
            colors: Vec::new(),
            color_buf: (None, None, None),
            skip_chars: 0,
            high_surrogate: None,
            para: ParaProps::default(),
            runs: Vec::new(),
            list_text: String::new(),
            blocks: Vec::new(),
            table: Vec::new(),
            row: Vec::new(),
            cell_paras: Vec::new(),
            cell_bgs: Vec::new(),
            pending_bg: None,
        }
    }

    fn font_name(&self, num: i32) -> Option<String> {
        if num == self.default_font {
            return None;
        }
        self.fonts
            .iter()
            .find(|(n, _)| *n == num)
            .map(|(_, name)| name.clone())
    }

    fn push_char(&mut self, c: char) {
        match self.state.dest {
            Dest::Normal => {
                if !self.para.in_table && !self.table.is_empty() && self.runs.is_empty() {
                    self.close_table();
                }
                match self.runs.last_mut() {
                    Some(r) if r.style == self.state.style => r.text.push(c),
                    _ => self
                        .runs
                        .push(Run::new(c.to_string(), self.state.style.clone())),
                }
            }
            Dest::FontTable => {
                if c == ';' {
                    let name = self.font_name_buf.trim().to_owned();
                    self.fonts.push((self.font_num, name));
                    self.font_name_buf.clear();
                } else {
                    self.font_name_buf.push(c);
                }
            }
            Dest::ColorTable => {
                if c == ';' {
                    let (r, g, b) = self.color_buf;
                    let color = if r.is_none() && g.is_none() && b.is_none() {
                        None
                    } else {
                        Some(Color::rgb(r.unwrap_or(0), g.unwrap_or(0), b.unwrap_or(0)))
                    };
                    self.colors.push(color);
                    self.color_buf = (None, None, None);
                }
            }
            Dest::ListText => self.list_text.push(c),
            Dest::PnText | Dest::Pn | Dest::Skip => {}
        }
    }

    fn text_char(&mut self, c: char) {
        if self.skip_chars > 0 {
            self.skip_chars -= 1;
            return;
        }
        self.push_char(c);
    }

    fn take_paragraph(&mut self) -> Paragraph {
        let mut style = ParagraphStyle {
            alignment: self.para.alignment,
            list: self.para.list,
            heading: self.para.heading,
            ..Default::default()
        };
        let step = 720;
        let levels = (self.para.left_indent / step).max(0) as u8;
        style.indent_level = if style.list.is_some() {
            levels.saturating_sub(1)
        } else {
            levels
        }
        .min(crate::document::style::MAX_INDENT_LEVEL);
        Paragraph::from_runs(std::mem::take(&mut self.runs), style)
    }

    fn end_paragraph(&mut self) {
        let p = self.take_paragraph();
        if self.para.in_table {
            self.cell_paras.push(p);
        } else {
            if !self.table.is_empty() {
                self.close_table();
            }
            self.blocks.push(Block::Paragraph(p));
        }
        // Legacy list markers are per paragraph unless \pard keeps them.
    }

    fn end_cell(&mut self) {
        if !self.runs.is_empty() || self.cell_paras.is_empty() {
            let p = self.take_paragraph();
            self.cell_paras.push(p);
        }
        let idx = self.row.len();
        let mut cell = Cell {
            paragraphs: std::mem::take(&mut self.cell_paras),
            style: Default::default(),
        };
        cell.style.background = self.cell_bgs.get(idx).copied().flatten();
        self.row.push(cell);
    }

    fn end_row(&mut self) {
        if !self.cell_paras.is_empty() || !self.runs.is_empty() {
            self.end_cell();
        }
        if !self.row.is_empty() {
            let row = std::mem::take(&mut self.row);
            self.table.push(row);
        }
    }

    fn close_table(&mut self) {
        if !self.row.is_empty() {
            self.end_row();
        }
        if !self.table.is_empty() {
            let t = Table::from_cells(std::mem::take(&mut self.table));
            self.blocks.push(Block::Table(t));
        }
    }

    fn word(&mut self, name: &str, param: Option<i32>) {
        self.skip_chars = 0;
        let star = std::mem::take(&mut self.state.star);
        let on = param.is_none_or(|p| p != 0);

        // Destination control words.
        match name {
            "fonttbl" => return self.state.dest = Dest::FontTable,
            "colortbl" => return self.state.dest = Dest::ColorTable,
            "pntext" => {
                self.state.dest = Dest::PnText;
                if self.para.list.is_none() {
                    self.para.list = Some(ListKind::Bullet);
                }
                return;
            }
            "listtext" => {
                self.state.dest = Dest::ListText;
                self.list_text.clear();
                return;
            }
            "pn" => return self.state.dest = Dest::Pn,
            _ => {}
        }
        if star || SKIP_DESTINATIONS.contains(&name) {
            if self.state.dest != Dest::Pn {
                self.state.dest = Dest::Skip;
            }
            return;
        }

        match self.state.dest {
            Dest::Skip | Dest::PnText => return,
            Dest::Pn => {
                match name {
                    "pnlvlblt" => self.para.list = Some(ListKind::Bullet),
                    "pnlvlbody" | "pndec" | "pnucltr" | "pnlcltr" | "pnucrm" | "pnlcrm" => {
                        self.para.list = Some(ListKind::Numbered)
                    }
                    _ => {}
                }
                return;
            }
            Dest::FontTable => {
                if name == "f" {
                    self.font_num = param.unwrap_or(0);
                    self.font_name_buf.clear();
                }
                return;
            }
            Dest::ColorTable => {
                let v = param.unwrap_or(0).clamp(0, 255) as u8;
                match name {
                    "red" => self.color_buf.0 = Some(v),
                    "green" => self.color_buf.1 = Some(v),
                    "blue" => self.color_buf.2 = Some(v),
                    _ => {}
                }
                return;
            }
            Dest::ListText | Dest::Normal => {}
        }

        let style = &mut self.state.style;
        match name {
            // Document.
            "deff" => self.default_font = param.unwrap_or(0),
            "uc" => self.state.uc = param.unwrap_or(1).max(0) as usize,
            // Characters.
            "plain" => *style = CharStyle::default(),
            "b" => style.bold = on,
            "i" => style.italic = on,
            "ul" | "uld" | "uldb" | "ulw" | "uldash" | "ulth" | "ulwave" => style.underline = on,
            "ulnone" => style.underline = false,
            "strike" | "striked" => style.strikethrough = on,
            "fs" => style.font_size = param.unwrap_or(24) as f32 / 2.0,
            "f" => {
                let f = self.font_name(param.unwrap_or(0));
                self.state.style.font_family = f;
            }
            "cf" => {
                let c = self
                    .colors
                    .get(param.unwrap_or(0).max(0) as usize)
                    .copied()
                    .flatten();
                self.state.style.color = c;
            }
            "u" => {
                let v = param.unwrap_or(0);
                let unit = if v < 0 { (v + 65536) as u16 } else { v as u16 };
                let ch = if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit);
                    None
                } else if (0xDC00..0xE000).contains(&unit) {
                    self.high_surrogate
                        .take()
                        .and_then(|h| char::decode_utf16([h, unit]).next()?.ok())
                } else {
                    char::from_u32(unit as u32)
                };
                if let Some(ch) = ch {
                    self.push_char(ch);
                }
                self.skip_chars = self.state.uc;
            }
            "tab" => self.push_char('\t'),
            "emdash" => self.push_char('—'),
            "endash" => self.push_char('–'),
            "bullet" => self.push_char('•'),
            "lquote" => self.push_char('‘'),
            "rquote" => self.push_char('’'),
            "ldblquote" => self.push_char('“'),
            "rdblquote" => self.push_char('”'),
            // Paragraphs.
            "par" | "line" => self.end_paragraph(),
            "pard" => self.para = ParaProps::default(),
            "ql" => self.para.alignment = Alignment::Left,
            "qc" => self.para.alignment = Alignment::Center,
            "qr" => self.para.alignment = Alignment::Right,
            "qj" => self.para.alignment = Alignment::Justify,
            "li" => self.para.left_indent = param.unwrap_or(0),
            "outlinelevel" => {
                let l = param.unwrap_or(0);
                if (0..6).contains(&l) {
                    self.para.heading = l as u8 + 1;
                }
            }
            "ls" => {
                if self.para.list.is_none() {
                    self.para.list = Some(ListKind::Bullet);
                }
            }
            "ilvl" => self.para.left_indent = 720 * (param.unwrap_or(0) + 1),
            // Tables.
            "intbl" => self.para.in_table = true,
            "trowd" => {
                self.cell_bgs.clear();
                self.pending_bg = None;
            }
            "clcbpat" | "clcbpatraw" => {
                self.pending_bg = self
                    .colors
                    .get(param.unwrap_or(0).max(0) as usize)
                    .copied()
                    .flatten();
            }
            "cellx" => {
                let bg = self.pending_bg.take();
                self.cell_bgs.push(bg);
            }
            "cell" => self.end_cell(),
            "row" => self.end_row(),
            _ => {}
        }
    }

    fn group_start(&mut self) {
        self.stack.push(self.state.clone());
        self.state.star = false;
        self.skip_chars = 0;
    }

    fn group_end(&mut self) {
        let ending = self.state.dest;
        if ending == Dest::FontTable && !self.font_name_buf.trim().is_empty() {
            let name = self.font_name_buf.trim().to_owned();
            self.fonts.push((self.font_num, name));
            self.font_name_buf.clear();
        }
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
        if ending == Dest::ListText && self.state.dest != Dest::ListText {
            let numbered = self.list_text.chars().any(|c| c.is_alphanumeric());
            self.para.list = Some(if numbered {
                ListKind::Numbered
            } else {
                ListKind::Bullet
            });
        }
        self.skip_chars = 0;
    }

    fn finish(mut self) -> Document {
        if !self.runs.is_empty() {
            self.end_paragraph();
        }
        self.close_table();
        // RTF files conventionally end with \par; drop the implied empty paragraph.
        Document::from_blocks(self.blocks)
    }
}

/// Parses RTF bytes into a document.
pub fn parse(bytes: &[u8]) -> Result<Document> {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    if !bytes[start..].starts_with(b"{\\rtf") {
        return Err(FormatError::Malformed("missing {\\rtf header".into()));
    }
    let mut p = Parser::new();
    for tok in tokenize(&bytes[start..]) {
        match tok {
            Token::GroupStart => p.group_start(),
            Token::GroupEnd => p.group_end(),
            Token::Word(name, param) => p.word(&name, param),
            Token::Symbol(c) => {
                p.skip_chars = 0;
                match c {
                    '*' => p.state.star = true,
                    '~' => p.push_char('\u{A0}'),
                    '_' => p.push_char('\u{2011}'),
                    '-' => {}
                    '\\' | '{' | '}' => p.push_char(c),
                    _ => {}
                }
            }
            Token::Hex(b) => p.text_char(cp1252_to_char(b)),
            Token::Text(b) => p.text_char(cp1252_to_char(b)),
        }
    }
    Ok(p.finish())
}
