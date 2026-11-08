//! Saves a feature-rich document to disk in every format and loads it back.
//!
//! Set `RUSTTEXT_SAMPLE_DIR=<dir>` to keep the generated files for manual
//! inspection in other applications (Word, LibreOffice, a Markdown viewer…).

use rusttext::document::{
    Alignment, Block, Cell, CharStyle, Color, Document, ListKind, Paragraph, ParagraphStyle, Run,
    Table,
};
use rusttext::formats::{self, FileFormat};

fn rich_document() -> Document {
    let style = |f: &dyn Fn(&mut CharStyle)| {
        let mut s = CharStyle::default();
        f(&mut s);
        s
    };
    let para = |runs: Vec<Run>, f: &dyn Fn(&mut ParagraphStyle)| {
        let mut ps = ParagraphStyle::default();
        f(&mut ps);
        Paragraph::from_runs(runs, ps)
    };
    let mut header = Vec::new();
    for t in ["Name", "Value"] {
        let mut c = Cell::with_text(t);
        c.style.background = Some(Color::rgb(0, 112, 192));
        c.paragraphs[0].style.alignment = Alignment::Center;
        c.paragraphs[0].apply_style_all(&|s| s.bold = true);
        header.push(c);
    }
    let mut value = Cell::with_text("42");
    value.paragraphs[0].style.alignment = Alignment::Right;
    Document::from_blocks(vec![
        Block::Paragraph(para(
            vec![Run::new(
                "Report",
                style(&|s| {
                    s.bold = true;
                    s.font_size = 24.0;
                }),
            )],
            &|p| p.heading = 1,
        )),
        Block::Paragraph(para(
            vec![
                Run::plain("Mixed "),
                Run::new("bold", style(&|s| s.bold = true)),
                Run::plain(" "),
                Run::new("italic", style(&|s| s.italic = true)),
                Run::plain(" "),
                Run::new("underline", style(&|s| s.underline = true)),
                Run::plain(" "),
                Run::new("strike", style(&|s| s.strikethrough = true)),
                Run::plain(" "),
                Run::new(
                    "colored serif",
                    style(&|s| {
                        s.color = Some(Color::rgb(200, 0, 100));
                        s.font_family = Some("Times New Roman".into());
                        s.font_size = 14.0;
                    }),
                ),
                Run::plain(" unicode: café ✓ 😀"),
            ],
            &|p| p.alignment = Alignment::Justify,
        )),
        Block::Paragraph(para(vec![Run::plain("Centered")], &|p| {
            p.alignment = Alignment::Center
        })),
        Block::Paragraph(para(vec![Run::plain("Bullet")], &|p| {
            p.list = Some(ListKind::Bullet)
        })),
        Block::Paragraph(para(vec![Run::plain("Nested")], &|p| {
            p.list = Some(ListKind::Bullet);
            p.indent_level = 1;
        })),
        Block::Paragraph(para(vec![Run::plain("Step one")], &|p| {
            p.list = Some(ListKind::Numbered)
        })),
        Block::Paragraph(para(vec![Run::plain("Step two")], &|p| {
            p.list = Some(ListKind::Numbered)
        })),
        Block::Table(Table::from_cells(vec![
            header,
            vec![Cell::with_text("answer"), value],
        ])),
        Block::Paragraph(Paragraph::with_text("The end.")),
    ])
}

fn roundtrip(format: FileFormat) -> (Document, Document) {
    let doc = rich_document();
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(format!("sample.{}", format.default_extension()));
    formats::save(&doc, &path, format).unwrap();
    if let Ok(keep) = std::env::var("RUSTTEXT_SAMPLE_DIR") {
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::copy(
            &path,
            std::path::Path::new(&keep).join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    let (back, detected) = formats::load(&path).unwrap();
    assert_eq!(detected, format);
    (doc, back)
}

#[test]
fn docx_preserves_everything() {
    let (doc, back) = roundtrip(FileFormat::Docx);
    assert_eq!(back, doc);
}

#[test]
fn rtf_preserves_everything() {
    let (doc, back) = roundtrip(FileFormat::Rtf);
    assert_eq!(back, doc);
}

#[test]
fn markdown_preserves_structure() {
    let (doc, back) = roundtrip(FileFormat::Markdown);
    assert_eq!(back.plain_text(), doc.plain_text());
    let lists: Vec<_> = back
        .all_paragraphs()
        .map(|p| (p.style.list, p.style.indent_level))
        .collect();
    let expected: Vec<_> = doc
        .all_paragraphs()
        .map(|p| (p.style.list, p.style.indent_level))
        .collect();
    assert_eq!(lists, expected);
    let bold = |d: &Document| {
        d.all_paragraphs()
            .flat_map(|p| p.runs.iter())
            .filter(|r| r.style.bold)
            .count()
    };
    assert!(bold(&back) >= 1);
    let table = back.blocks.iter().find_map(Block::as_table).unwrap();
    // GFM stores one alignment per column, taken from the header row.
    assert_eq!(
        table.cell(1, 1).unwrap().paragraphs[0].style.alignment,
        Alignment::Center
    );
}

#[test]
fn plain_text_preserves_text() {
    let (doc, back) = roundtrip(FileFormat::PlainText);
    let text = back.plain_text();
    assert!(text.contains("Mixed bold italic underline strike colored serif unicode: café ✓ 😀"));
    assert!(text.contains("• Bullet"));
    assert!(text.contains("2. Step two"));
    assert!(text.contains("answer\t42"));
    assert_eq!(doc.blocks.len() - 1, back.blocks.len() - 1 - 1); // table becomes two lines
}

#[test]
fn saving_never_leaves_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.docx");
    formats::save(&rich_document(), &path, FileFormat::Docx).unwrap();
    formats::save(&rich_document(), &path, FileFormat::Docx).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
}
