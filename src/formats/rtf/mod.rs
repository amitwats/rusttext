//! Rich Text Format (`.rtf`).
//!
//! The writer produces RTF 1.x that WordPad, Word, LibreOffice and TextEdit
//! open. The reader understands the subset of RTF needed for the editor's
//! features: fonts, sizes, colors, bold/italic/underline/strikethrough,
//! alignment, legacy (`\pn`) and Word (`\listtext`) lists, tables with cell
//! shading, and Unicode escapes. Unknown destinations are skipped.

mod reader;
mod writer;

use super::{Codec, Result};
use crate::document::Document;

pub struct RtfCodec;

impl Codec for RtfCodec {
    fn read(&self, bytes: &[u8]) -> Result<Document> {
        reader::parse(bytes)
    }

    fn write(&self, doc: &Document) -> Result<Vec<u8>> {
        Ok(writer::write(doc).into_bytes())
    }
}

/// Maps a Windows-1252 byte to a Unicode character.
pub fn cp1252_to_char(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        Alignment, Block, Cell, CharStyle, Color, ListKind, Paragraph, ParagraphStyle, Run, Table,
    };

    fn sample() -> Document {
        let bold = CharStyle {
            bold: true,
            ..Default::default()
        };
        let fancy = CharStyle {
            italic: true,
            underline: true,
            strikethrough: true,
            font_family: Some("Courier New".into()),
            font_size: 18.0,
            color: Some(Color::rgb(200, 10, 20)),
            ..Default::default()
        };
        let mut p1 = Paragraph::from_runs(
            vec![
                Run::plain("Plain "),
                Run::new("bold", bold.clone()),
                Run::new(" fancy {}\\ é ☃ 😀", fancy),
            ],
            ParagraphStyle {
                alignment: Alignment::Center,
                ..Default::default()
            },
        );
        p1.normalize();
        let bullet = Paragraph::from_runs(
            vec![Run::plain("item")],
            ParagraphStyle {
                list: Some(ListKind::Bullet),
                indent_level: 1,
                ..Default::default()
            },
        );
        let num = Paragraph::from_runs(
            vec![Run::plain("first")],
            ParagraphStyle {
                list: Some(ListKind::Numbered),
                ..Default::default()
            },
        );
        let mut cell = Cell::with_text("c1\nsecond line");
        cell.style.background = Some(Color::rgb(255, 255, 0));
        let mut right = Cell::with_text("c2");
        right.paragraphs[0].style.alignment = Alignment::Right;
        let table = Table::from_cells(vec![
            vec![cell, right],
            vec![Cell::with_text("c3"), Cell::default()],
        ]);
        Document::from_blocks(vec![
            Block::Paragraph(p1),
            Block::Paragraph(bullet),
            Block::Paragraph(num),
            Block::Table(table),
            Block::Paragraph(Paragraph::with_text("end\ttab")),
        ])
    }

    #[test]
    fn roundtrip() {
        let doc = sample();
        let bytes = RtfCodec.write(&doc).unwrap();
        let back = RtfCodec.read(&bytes).unwrap();
        assert_eq!(back, doc, "\n{}", String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn reads_wordpad_style_rtf() {
        let src = br"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fnil\fcharset0 Calibri;}{\f1\fswiss Arial;}}
{\colortbl ;\red255\green0\blue0;}
{\*\generator Riched20 10.0.19041}\viewkind4\uc1
\pard\sa200\sl276\slmult1\qc\f0\fs22\lang9 Hello \b bold\b0  \cf1 red\cf0\par
\pard{\pntext\f0 1.\tab}{\*\pn\pnlvlbody\pnf0\pnindent0\pnstart1\pndec{\pntxta.}}
\fi-360\li720 one\par
{\pntext\f0 2.\tab}two\par
\pard caf\'e9 \u8364?\par
}";
        let doc = RtfCodec.read(src).unwrap();
        let ps: Vec<&Paragraph> = doc.all_paragraphs().collect();
        assert_eq!(ps[0].plain_text(), "Hello bold red");
        assert_eq!(ps[0].style.alignment, Alignment::Center);
        assert!(ps[0].runs.iter().any(|r| r.text == "bold" && r.style.bold));
        assert!(
            ps[0]
                .runs
                .iter()
                .any(|r| r.text == "red" && r.style.color == Some(Color::rgb(255, 0, 0)))
        );
        assert_eq!(ps[1].style.list, Some(ListKind::Numbered));
        assert_eq!(ps[1].plain_text(), "one");
        assert_eq!(ps[2].style.list, Some(ListKind::Numbered));
        assert_eq!(ps[3].plain_text(), "café €");
        assert_eq!(ps[3].style.list, None);
    }
}
