//! Every user action is a [`Command`]. Menus, the toolbar and keyboard
//! shortcuts only *produce* commands; [`super::RustTextApp::execute`] is the
//! single place that carries them out. This keeps the UI code declarative
//! and makes new actions easy to add (see `develop.md`).

use crate::document::{Alignment, Color, ListKind, ToggleAttr};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    // File
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    // Character formatting
    Toggle(ToggleAttr),
    FontFamily(Option<String>),
    FontSize(f32),
    TextColor(Option<Color>),
    // Paragraph formatting
    Align(Alignment),
    List(ListKind),
    Indent(i32),
    Heading(u8),
    // Tables
    ShowInsertTable,
    InsertTable { rows: usize, cols: usize },
    InsertRow { below: bool },
    InsertColumn { right: bool },
    DeleteRows,
    DeleteColumns,
    DeleteTable,
    SelectTable,
    CellBackground(Option<Color>),
    CellForeground(Option<Color>),
    // View
    ZoomIn,
    ZoomOut,
    ZoomReset,
    About,
}

/// Commands that only work with the caret inside a table.
pub fn needs_table(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::InsertRow { .. }
            | Command::InsertColumn { .. }
            | Command::DeleteRows
            | Command::DeleteColumns
            | Command::DeleteTable
            | Command::SelectTable
            | Command::CellBackground(_)
            | Command::CellForeground(_)
    )
}
