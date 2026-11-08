//! Modal dialogs.

use egui::{Context, DragValue, Id, Modal, RichText};

use super::commands::Command;

/// What to do after the user resolved the "unsaved changes" prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    New,
    Open,
    Quit,
}

/// The currently open dialog, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    InsertTable { rows: usize, cols: usize },
    UnsavedChanges(PendingAction),
    Error(String),
    About,
}

/// Result of the unsaved-changes prompt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// What a dialog asks the application to do this frame.
#[derive(Debug, Default)]
pub struct DialogOutcome {
    pub close: bool,
    pub command: Option<Command>,
    pub unsaved: Option<(UnsavedChoice, PendingAction)>,
}

pub fn show(ctx: &Context, dialog: &mut Dialog) -> DialogOutcome {
    let mut out = DialogOutcome::default();
    let modal = Modal::new(Id::unique("rusttext-dialog")).show(ctx, |ui| {
        ui.set_min_width(300.0);
        match dialog {
            Dialog::InsertTable { rows, cols } => {
                ui.heading("Insert table");
                ui.add_space(6.0);
                egui::Grid::new("insert-table-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Rows:");
                        ui.add(DragValue::new(rows).range(1..=200));
                        ui.end_row();
                        ui.label("Columns:");
                        ui.add(DragValue::new(cols).range(1..=30));
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Insert").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        out.command = Some(Command::InsertTable {
                            rows: *rows,
                            cols: *cols,
                        });
                        out.close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        out.close = true;
                    }
                });
            }
            Dialog::UnsavedChanges(action) => {
                ui.heading("Unsaved changes");
                ui.label("The document has unsaved changes. Save them first?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let mut choice = None;
                    if ui.button("Save").clicked() {
                        choice = Some(UnsavedChoice::Save);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(UnsavedChoice::Discard);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(UnsavedChoice::Cancel);
                    }
                    if let Some(c) = choice {
                        out.unsaved = Some((c, action.clone()));
                        out.close = true;
                    }
                });
            }
            Dialog::Error(msg) => {
                ui.heading(RichText::new("Error").color(ui.visuals().error_fg_color));
                ui.label(msg.as_str());
                ui.add_space(8.0);
                if ui.button("OK").clicked() {
                    out.close = true;
                }
            }
            Dialog::About => {
                ui.heading("RustText");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A basic rich text editor written in Rust.");
                ui.label("Formats: DOCX, RTF, Markdown and plain text.");
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    out.close = true;
                }
            }
        }
    });
    if modal.should_close() && out.unsaved.is_none() {
        if let Dialog::UnsavedChanges(action) = dialog {
            out.unsaved = Some((UnsavedChoice::Cancel, action.clone()));
        }
        out.close = true;
    }
    out
}
