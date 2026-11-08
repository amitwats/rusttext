//! The application menu bar.

use egui::{Button, Ui};

use super::commands::Command;
use crate::document::{Alignment, ListKind, ToggleAttr};

/// State that decides which menu entries are enabled.
pub struct MenuInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub in_table: bool,
}

fn item(
    ui: &mut Ui,
    cmds: &mut Vec<Command>,
    enabled: bool,
    label: &str,
    shortcut: &str,
    cmd: Command,
) {
    let mut b = Button::new(label);
    if !shortcut.is_empty() {
        b = b.shortcut_text(shortcut);
    }
    if ui.add_enabled(enabled, b).clicked() {
        cmds.push(cmd);
        ui.close();
    }
}

fn shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("Cmd+{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

pub fn menu_bar(ui: &mut Ui, info: &MenuInfo) -> Vec<Command> {
    let mut c = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            item(ui, &mut c, true, "New", &shortcut("N"), Command::New);
            item(ui, &mut c, true, "Open…", &shortcut("O"), Command::Open);
            item(ui, &mut c, true, "Save", &shortcut("S"), Command::Save);
            item(
                ui,
                &mut c,
                true,
                "Save As…",
                &shortcut("Shift+S"),
                Command::SaveAs,
            );
            ui.separator();
            item(ui, &mut c, true, "Exit", &shortcut("Q"), Command::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(
                ui,
                &mut c,
                info.can_undo,
                "Undo",
                &shortcut("Z"),
                Command::Undo,
            );
            item(
                ui,
                &mut c,
                info.can_redo,
                "Redo",
                &shortcut("Y"),
                Command::Redo,
            );
            ui.separator();
            item(
                ui,
                &mut c,
                info.has_selection,
                "Cut",
                &shortcut("X"),
                Command::Cut,
            );
            item(
                ui,
                &mut c,
                info.has_selection,
                "Copy",
                &shortcut("C"),
                Command::Copy,
            );
            item(ui, &mut c, true, "Paste", &shortcut("V"), Command::Paste);
            ui.separator();
            item(
                ui,
                &mut c,
                true,
                "Select All",
                &shortcut("A"),
                Command::SelectAll,
            );
        });
        ui.menu_button("Format", |ui| {
            item(
                ui,
                &mut c,
                true,
                "Bold",
                &shortcut("B"),
                Command::Toggle(ToggleAttr::Bold),
            );
            item(
                ui,
                &mut c,
                true,
                "Italic",
                &shortcut("I"),
                Command::Toggle(ToggleAttr::Italic),
            );
            item(
                ui,
                &mut c,
                true,
                "Underline",
                &shortcut("U"),
                Command::Toggle(ToggleAttr::Underline),
            );
            item(
                ui,
                &mut c,
                true,
                "Strikethrough",
                "",
                Command::Toggle(ToggleAttr::Strikethrough),
            );
            item(
                ui,
                &mut c,
                true,
                "Automatic text color",
                "",
                Command::TextColor(None),
            );
            ui.separator();
            item(
                ui,
                &mut c,
                true,
                "Bulleted list",
                "",
                Command::List(ListKind::Bullet),
            );
            item(
                ui,
                &mut c,
                true,
                "Numbered list",
                "",
                Command::List(ListKind::Numbered),
            );
            item(ui, &mut c, true, "Increase indent", "", Command::Indent(1));
            item(ui, &mut c, true, "Decrease indent", "", Command::Indent(-1));
            ui.separator();
            for a in Alignment::ALL {
                item(
                    ui,
                    &mut c,
                    true,
                    &format!("Align {}", a.label().to_lowercase()),
                    "",
                    Command::Align(a),
                );
            }
            ui.separator();
            ui.menu_button("Paragraph style", |ui| {
                item(ui, &mut c, true, "Normal", "", Command::Heading(0));
                for h in 1..=6u8 {
                    item(
                        ui,
                        &mut c,
                        true,
                        &format!("Heading {h}"),
                        "",
                        Command::Heading(h),
                    );
                }
            });
        });
        ui.menu_button("Table", |ui| {
            item(
                ui,
                &mut c,
                true,
                "Insert table…",
                "",
                Command::ShowInsertTable,
            );
            ui.separator();
            let t = info.in_table;
            item(
                ui,
                &mut c,
                t,
                "Insert row above",
                "",
                Command::InsertRow { below: false },
            );
            item(
                ui,
                &mut c,
                t,
                "Insert row below",
                "",
                Command::InsertRow { below: true },
            );
            item(
                ui,
                &mut c,
                t,
                "Insert column left",
                "",
                Command::InsertColumn { right: false },
            );
            item(
                ui,
                &mut c,
                t,
                "Insert column right",
                "",
                Command::InsertColumn { right: true },
            );
            ui.separator();
            item(ui, &mut c, t, "Select table", "", Command::SelectTable);
            ui.separator();
            item(ui, &mut c, t, "Delete rows", "", Command::DeleteRows);
            item(ui, &mut c, t, "Delete columns", "", Command::DeleteColumns);
            item(ui, &mut c, t, "Delete table", "", Command::DeleteTable);
            ui.separator();
            item(
                ui,
                &mut c,
                t,
                "Clear cell fill",
                "",
                Command::CellBackground(None),
            );
            item(
                ui,
                &mut c,
                t,
                "Automatic cell text color",
                "",
                Command::CellForeground(None),
            );
        });
        ui.menu_button("View", |ui| {
            item(ui, &mut c, true, "Zoom in", &shortcut("+"), Command::ZoomIn);
            item(
                ui,
                &mut c,
                true,
                "Zoom out",
                &shortcut("-"),
                Command::ZoomOut,
            );
            item(
                ui,
                &mut c,
                true,
                "Actual size",
                &shortcut("0"),
                Command::ZoomReset,
            );
        });
        ui.menu_button("Help", |ui| {
            item(ui, &mut c, true, "About RustText", "", Command::About);
        });
    });
    c
}
