//! Plain text (`.txt`). Formatting is discarded when saving; lists are
//! written with textual markers and tables as tab-separated rows.

use super::{Codec, Result};
use crate::document::{Block, Document, ListKind};

pub struct TxtCodec;

/// Decodes text bytes: UTF-8 (with or without BOM), UTF-16 with BOM, and
/// falls back to Windows-1252/Latin-1 for invalid UTF-8.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes
            .iter()
            .map(|&b| super::rtf::cp1252_to_char(b))
            .collect(),
    }
}

impl Codec for TxtCodec {
    fn read(&self, bytes: &[u8]) -> Result<Document> {
        Ok(Document::from_plain_text(&decode_text(bytes)))
    }

    fn write(&self, doc: &Document) -> Result<Vec<u8>> {
        let mut lines: Vec<String> = Vec::new();
        let mut counters = [0usize; 16];
        for block in &doc.blocks {
            match block {
                Block::Paragraph(p) => {
                    let lvl = p.style.indent_level as usize;
                    let indent = "    ".repeat(lvl);
                    let marker = match p.style.list {
                        Some(ListKind::Bullet) => {
                            counters[lvl.min(15)] = 0;
                            "• ".to_owned()
                        }
                        Some(ListKind::Numbered) => {
                            let l = lvl.min(15);
                            counters[l] += 1;
                            counters[l + 1..].iter_mut().for_each(|c| *c = 0);
                            format!("{}. ", counters[l])
                        }
                        None => {
                            counters = [0; 16];
                            String::new()
                        }
                    };
                    lines.push(format!("{indent}{marker}{}", p.plain_text()));
                }
                Block::Table(t) => {
                    counters = [0; 16];
                    lines.push(t.plain_text());
                }
            }
        }
        // Paragraphs are separated (not terminated) by newlines, so a file
        // ending in a newline round-trips exactly.
        Ok(lines.join("\n").into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let doc = TxtCodec.read(b"line 1\r\nline 2\n").unwrap();
        assert_eq!(doc.plain_text(), "line 1\nline 2\n");
        let out = TxtCodec.write(&doc).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "line 1\nline 2\n");
    }

    #[test]
    fn decodes_utf16_and_latin1() {
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(decode_text(&[b'c', b'a', b'f', 0xE9]), "café");
    }
}
