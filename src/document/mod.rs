//! GUI-independent document model and editing engine.
//!
//! * [`model`]     – the document tree (blocks, paragraphs, runs, tables)
//! * [`style`]     – character / paragraph / cell formatting
//! * [`position`]  – caret positions and selections
//! * [`session`]   – [`EditSession`]: editing commands, navigation
//! * [`formatting`], [`table_ops`], [`clipboard`] – more commands on [`EditSession`]
//! * [`history`]   – undo / redo

pub mod clipboard;
pub mod formatting;
pub mod history;
pub mod model;
pub mod position;
pub mod session;
pub mod style;
pub mod table_ops;

pub use clipboard::ClipContent;
pub use formatting::ToggleAttr;
pub use model::{Block, Cell, Document, Paragraph, Row, Run, Table};
pub use position::{ParaPath, Selection, SelectionKind, TextPos};
pub use session::{EditSession, ParaRange};
pub use style::{Alignment, CellStyle, CharStyle, Color, ListKind, ParagraphStyle};
