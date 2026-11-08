//! DOCX serialization.

use std::fmt::Write as _;
use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::document::style::heading_font_size;
use crate::document::{Alignment, Block, Cell, Color, Document, ListKind, Paragraph, Table};
use crate::formats::{Result, xml_escape};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Table width in twentieths of a point (6.5 inches).
const TABLE_WIDTH: usize = 9360;
pub(super) const BULLET_NUM_ID: u32 = 1;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#;

fn styles_xml() -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{W_NS}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="80"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>"#
    );
    for level in 1..=6u8 {
        let sz = (heading_font_size(level) * 2.0).round() as u32;
        let _ = write!(
            s,
            r#"<w:style w:type="paragraph" w:styleId="Heading{level}"><w:name w:val="heading {level}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="{}"/></w:pPr><w:rPr><w:b/><w:sz w:val="{sz}"/></w:rPr></w:style>"#,
            level - 1
        );
    }
    s.push_str(r#"<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr></w:style></w:styles>"#);
    s
}

fn numbering_xml(numbered_instances: u32) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="{W_NS}">"#
    );
    for (abs, bullet) in [(0, true), (1, false)] {
        let _ = write!(
            s,
            r#"<w:abstractNum w:abstractNumId="{abs}"><w:multiLevelType w:val="hybridMultilevel"/>"#
        );
        for lvl in 0..9 {
            let (fmt, text) = if bullet {
                let symbols = ["•", "◦", "▪"];
                ("bullet", symbols[lvl % 3].to_owned())
            } else {
                ("decimal", format!("%{}.", lvl + 1))
            };
            let _ = write!(
                s,
                r#"<w:lvl w:ilvl="{lvl}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
                720 * (lvl + 1)
            );
        }
        s.push_str("</w:abstractNum>");
    }
    let _ = write!(
        s,
        r#"<w:num w:numId="{BULLET_NUM_ID}"><w:abstractNumId w:val="0"/></w:num>"#
    );
    for i in 0..numbered_instances {
        let _ = write!(
            s,
            r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>"#,
            i + 2
        );
    }
    s.push_str("</w:numbering>");
    s
}

struct Writer {
    out: String,
    /// Number of numbered-list instances emitted so far.
    numbered_instances: u32,
}

impl Writer {
    fn run(
        &mut self,
        text: &str,
        style: &crate::document::CharStyle,
        fallback_color: Option<Color>,
    ) {
        let o = &mut self.out;
        o.push_str("<w:r><w:rPr>");
        if let Some(f) = &style.font_family {
            let f = xml_escape(f);
            let _ = write!(
                o,
                r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/>"#
            );
        }
        if style.bold {
            o.push_str("<w:b/><w:bCs/>");
        }
        if style.italic {
            o.push_str("<w:i/><w:iCs/>");
        }
        if style.strikethrough {
            o.push_str("<w:strike/>");
        }
        if let Some(c) = style.color.or(fallback_color) {
            let _ = write!(o, r#"<w:color w:val="{}"/>"#, c.to_hex());
        }
        let sz = (style.font_size * 2.0).round() as u32;
        let _ = write!(o, r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#);
        if style.underline {
            o.push_str(r#"<w:u w:val="single"/>"#);
        }
        o.push_str("</w:rPr>");
        for (i, part) in text.split('\t').enumerate() {
            if i > 0 {
                o.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                let _ = write!(o, r#"<w:t xml:space="preserve">{}</w:t>"#, xml_escape(part));
            }
        }
        o.push_str("</w:r>");
    }

    fn paragraph(&mut self, p: &Paragraph, num_id: Option<u32>, fallback_color: Option<Color>) {
        self.out.push_str("<w:p><w:pPr>");
        if p.style.heading > 0 {
            let _ = write!(
                self.out,
                r#"<w:pStyle w:val="Heading{}"/>"#,
                p.style.heading.min(6)
            );
        }
        if let Some(id) = num_id {
            let _ = write!(
                self.out,
                r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{id}"/></w:numPr>"#,
                p.style.indent_level
            );
        } else if p.style.indent_level > 0 {
            let _ = write!(
                self.out,
                r#"<w:ind w:left="{}"/>"#,
                720 * p.style.indent_level as u32
            );
        }
        let jc = match p.style.alignment {
            Alignment::Left => "left",
            Alignment::Center => "center",
            Alignment::Right => "right",
            Alignment::Justify => "both",
        };
        let _ = write!(self.out, r#"<w:jc w:val="{jc}"/></w:pPr>"#);
        for r in &p.runs {
            self.run(&r.text, &r.style, fallback_color);
        }
        self.out.push_str("</w:p>");
    }

    /// Writes a sequence of paragraphs, assigning list numbering instances.
    fn paragraphs<'a>(
        &mut self,
        paras: impl Iterator<Item = &'a Paragraph>,
        fallback: Option<Color>,
    ) {
        let mut prev_numbered = false;
        let mut current_num = 0;
        for p in paras {
            let num_id = match p.style.list {
                Some(ListKind::Bullet) => {
                    // Nested bullets inside a numbered list keep the list open.
                    prev_numbered = prev_numbered && p.style.indent_level > 0;
                    Some(BULLET_NUM_ID)
                }
                Some(ListKind::Numbered) => {
                    if !prev_numbered {
                        self.numbered_instances += 1;
                        current_num = self.numbered_instances + 1;
                    }
                    prev_numbered = true;
                    Some(current_num)
                }
                None => {
                    prev_numbered = false;
                    None
                }
            };
            self.paragraph(p, num_id, fallback);
        }
    }

    fn table(&mut self, t: &Table) {
        let cols = t.col_count().max(1);
        let w = TABLE_WIDTH / cols;
        self.out.push_str(r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid>"#);
        for _ in 0..cols {
            let _ = write!(self.out, r#"<w:gridCol w:w="{w}"/>"#);
        }
        self.out.push_str("</w:tblGrid>");
        for row in &t.rows {
            self.out.push_str("<w:tr>");
            for cell in &row.cells {
                self.cell(cell, w);
            }
            self.out.push_str("</w:tr>");
        }
        self.out.push_str("</w:tbl>");
    }

    fn cell(&mut self, cell: &Cell, width: usize) {
        let _ = write!(
            self.out,
            r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>"#
        );
        if let Some(bg) = cell.style.background {
            let _ = write!(
                self.out,
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                bg.to_hex()
            );
        }
        self.out.push_str("</w:tcPr>");
        self.paragraphs(cell.paragraphs.iter(), cell.style.foreground);
        if cell.paragraphs.is_empty() {
            self.out.push_str("<w:p/>");
        }
        self.out.push_str("</w:tc>");
    }
}

fn document_xml(doc: &Document) -> (String, u32) {
    let mut w = Writer {
        out: format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body>"#
        ),
        numbered_instances: 0,
    };
    let mut i = 0;
    while i < doc.blocks.len() {
        match &doc.blocks[i] {
            Block::Paragraph(_) => {
                // Group consecutive paragraphs so lists number continuously.
                let start = i;
                while i < doc.blocks.len() && matches!(doc.blocks[i], Block::Paragraph(_)) {
                    i += 1;
                }
                w.paragraphs(
                    doc.blocks[start..i].iter().filter_map(Block::as_paragraph),
                    None,
                );
                continue;
            }
            Block::Table(t) => w.table(t),
        }
        i += 1;
    }
    w.out.push_str(r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#);
    (w.out, w.numbered_instances)
}

/// Serializes `doc` as a DOCX (ZIP) byte buffer.
pub fn write(doc: &Document) -> Result<Vec<u8>> {
    let (document, numbered) = document_xml(doc);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let parts: [(&str, String); 6] = [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", ROOT_RELS.to_owned()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.to_owned()),
        ("word/document.xml", document),
        ("word/styles.xml", styles_xml()),
        ("word/numbering.xml", numbering_xml(numbered)),
    ];
    for (name, content) in parts {
        zip.start_file(name, opts)?;
        zip.write_all(content.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}
