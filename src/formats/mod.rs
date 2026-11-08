//! Reading and writing documents in different file formats.
//!
//! Every format implements the [`Codec`] trait; [`FileFormat`] maps file
//! extensions to codecs. To add a new format implement [`Codec`] in a new
//! module and register it in [`FileFormat`] (see `develop.md`).

pub mod docx;
pub mod markdown;
pub mod rtf;
pub mod txt;

use std::path::Path;

use crate::document::Document;

/// Errors produced while reading or writing documents.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid ZIP container: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid XML: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("malformed document: {0}")]
    Malformed(String),
    #[error("unsupported file type '{0}'")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, FormatError>;

/// A file format reader/writer.
pub trait Codec {
    /// Parses a document from raw file bytes.
    fn read(&self, bytes: &[u8]) -> Result<Document>;
    /// Serializes a document to raw file bytes.
    fn write(&self, doc: &Document) -> Result<Vec<u8>>;
}

/// The file formats supported by the editor.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FileFormat {
    PlainText,
    Markdown,
    Rtf,
    Docx,
}

impl FileFormat {
    pub const ALL: [FileFormat; 4] = [
        FileFormat::Docx,
        FileFormat::Rtf,
        FileFormat::Markdown,
        FileFormat::PlainText,
    ];

    /// Human readable name (used in file dialogs and the status bar).
    pub fn label(self) -> &'static str {
        match self {
            FileFormat::PlainText => "Plain text",
            FileFormat::Markdown => "Markdown",
            FileFormat::Rtf => "Rich Text Format",
            FileFormat::Docx => "Word document",
        }
    }

    /// File extensions (without dot); the first one is the default.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileFormat::PlainText => &["txt", "text", "log"],
            FileFormat::Markdown => &["md", "markdown"],
            FileFormat::Rtf => &["rtf"],
            FileFormat::Docx => &["docx"],
        }
    }

    pub fn default_extension(self) -> &'static str {
        self.extensions()[0]
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }

    /// True if the format can store character formatting.
    pub fn is_rich(self) -> bool {
        !matches!(self, FileFormat::PlainText)
    }

    pub fn codec(self) -> Box<dyn Codec> {
        match self {
            FileFormat::PlainText => Box::new(txt::TxtCodec),
            FileFormat::Markdown => Box::new(markdown::MarkdownCodec),
            FileFormat::Rtf => Box::new(rtf::RtfCodec),
            FileFormat::Docx => Box::new(docx::DocxCodec),
        }
    }
}

/// Loads a document, choosing the format from the file extension
/// (unknown extensions are read as plain text).
pub fn load(path: &Path) -> Result<(Document, FileFormat)> {
    let bytes = std::fs::read(path)?;
    let format = FileFormat::from_path(path).unwrap_or(FileFormat::PlainText);
    let doc = format.codec().read(&bytes)?;
    Ok((doc, format))
}

/// Saves a document in the given format.
pub fn save(doc: &Document, path: &Path, format: FileFormat) -> Result<()> {
    let bytes = format.codec().write(doc)?;
    // Write to a temporary file first so a failure never truncates the original.
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, &bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Some filesystems do not support atomic replacement; fall back.
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(path, &bytes).map_err(|_| e)?;
    }
    Ok(())
}

/// Escapes text for XML content and attribute values.
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_mapping() {
        assert_eq!(FileFormat::from_extension("DOCX"), Some(FileFormat::Docx));
        assert_eq!(FileFormat::from_extension("md"), Some(FileFormat::Markdown));
        assert_eq!(FileFormat::from_extension("rtf"), Some(FileFormat::Rtf));
        assert_eq!(
            FileFormat::from_extension("txt"),
            Some(FileFormat::PlainText)
        );
        assert_eq!(FileFormat::from_extension("exe"), None);
    }
}
