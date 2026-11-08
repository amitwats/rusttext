//! Office Open XML word processing documents (`.docx`).
//!
//! A `.docx` file is a ZIP container. The writer emits the minimal set of
//! parts Word requires (`document.xml`, `styles.xml`, `numbering.xml` and the
//! relationship / content-type files). The reader streams `document.xml`
//! with `quick-xml`, using `numbering.xml` to tell bullets from numbers and
//! `styles.xml` to detect headings.

mod reader;
mod writer;

use super::{Codec, Result};
use crate::document::Document;

pub struct DocxCodec;

impl Codec for DocxCodec {
    fn read(&self, bytes: &[u8]) -> Result<Document> {
        reader::read(bytes)
    }

    fn write(&self, doc: &Document) -> Result<Vec<u8>> {
        writer::write(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        Alignment, Block, Cell, CharStyle, Color, ListKind, Paragraph, ParagraphStyle, Run, Table,
    };

    fn sample() -> Document {
        let fancy = CharStyle {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            font_family: Some("Georgia".into()),
            font_size: 15.5,
            color: Some(Color::rgb(1, 2, 3)),
        };
        let heading = Paragraph::from_runs(
            vec![Run::new(
                "Title",
                CharStyle {
                    bold: true,
                    font_size: 24.0,
                    ..Default::default()
                },
            )],
            ParagraphStyle {
                heading: 1,
                ..Default::default()
            },
        );
        let body = Paragraph::from_runs(
            vec![Run::plain("a <&> \"b\"\tc "), Run::new("fancy", fancy)],
            ParagraphStyle {
                alignment: Alignment::Justify,
                ..Default::default()
            },
        );
        let li = |kind, lvl, t: &str| {
            Paragraph::from_runs(
                vec![Run::plain(t)],
                ParagraphStyle {
                    list: Some(kind),
                    indent_level: lvl,
                    ..Default::default()
                },
            )
        };
        let mut c1 = Cell::with_text("x\ny");
        c1.style.background = Some(Color::rgb(0, 255, 0));
        let mut c2 = Cell::with_text("z");
        c2.paragraphs[0].style.alignment = Alignment::Center;
        Document::from_blocks(vec![
            Block::Paragraph(heading),
            Block::Paragraph(body),
            Block::Paragraph(li(ListKind::Bullet, 0, "b1")),
            Block::Paragraph(li(ListKind::Bullet, 1, "b2")),
            Block::Paragraph(li(ListKind::Numbered, 0, "n1")),
            Block::Paragraph(li(ListKind::Numbered, 0, "n2")),
            Block::Table(Table::from_cells(vec![vec![c1, c2]])),
            Block::Paragraph(Paragraph::with_text("end")),
        ])
    }

    #[test]
    fn roundtrip() {
        let doc = sample();
        let bytes = DocxCodec.write(&doc).unwrap();
        let back = DocxCodec.read(&bytes).unwrap();
        assert_eq!(back, doc);
    }

    /// Builds a .docx from raw XML parts, like files produced by Word.
    fn docx_from_parts(document: &str, numbering: Option<&str>) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        if let Some(n) = numbering {
            zip.start_file("word/numbering.xml", opts).unwrap();
            zip.write_all(n.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_word_style_markup() {
        let document = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Title"/><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:t>Doc</w:t></w:r></w:p>
<w:p><w:bookmarkStart w:id="0" w:name="x"/><w:r><w:rPr><w:b/><w:color w:val="auto"/></w:rPr><w:t xml:space="preserve">Hello </w:t></w:r><w:proofErr w:type="spellStart"/><w:hyperlink r:id="rId5" xmlns:r="r"><w:r><w:rPr><w:b w:val="0"/><w:u w:val="none"/></w:rPr><w:t>link</w:t></w:r></w:hyperlink><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:t>1</w:t></w:r><w:del w:id="1"><w:r><w:delText>gone</w:delText></w:r></w:del><w:ins w:id="2"><w:r><w:t>&amp;new&#x21;</w:t></w:r></w:ins><w:r><mc:AlternateContent><mc:Choice><w:drawing><w:txbxContent><w:p><w:r><w:t>textbox</w:t></w:r></w:p></w:txbxContent></w:drawing></mc:Choice></mc:AlternateContent></w:r><w:r><w:br/><w:t>line2</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="7"/></w:numPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>numbered</w:t></w:r></w:p>
<w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>
</w:body></w:document>"#;
        let numbering = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"><w:numFmt w:val="lowerRoman"/></w:lvl></w:abstractNum><w:num w:numId="7"><w:abstractNumId w:val="3"/></w:num></w:numbering>"#;
        let doc = DocxCodec
            .read(&docx_from_parts(document, Some(numbering)))
            .unwrap();
        let ps: Vec<&Paragraph> = doc.all_paragraphs().collect();
        assert_eq!(ps[0].style.heading, 1);
        assert_eq!(ps[1].plain_text(), "Hello link1&new!");
        assert!(ps[1].runs[0].style.bold);
        assert!(!ps[1].runs[1].style.bold);
        assert_eq!(ps[1].runs[0].style.color, None);
        assert_eq!(ps[2].plain_text(), "line2");
        assert_eq!(ps[3].plain_text(), "numbered");
        assert_eq!(ps[3].style.list, Some(ListKind::Numbered));
    }

    #[test]
    fn rejects_garbage() {
        assert!(DocxCodec.read(b"not a zip").is_err());
    }
}
