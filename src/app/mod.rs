//! The eframe application: window chrome (menus, toolbar, status bar),
//! file handling and command dispatch around the [`EditorView`].

pub mod commands;
pub mod dialogs;
pub mod file_ops;
pub mod icons;
pub mod menu;
pub mod toolbar;

use std::path::{Path, PathBuf};

use egui::{CentralPanel, Color32, Frame, Key, KeyboardShortcut, Modifiers, Panel, ScrollArea, Ui};

use crate::document::{Document, EditSession, ToggleAttr};
use crate::editor::EditorView;
use crate::fonts::FontManager;
use crate::formats::{self, FileFormat};
use commands::{Command, needs_table};
use dialogs::{Dialog, PendingAction, UnsavedChoice};
use toolbar::{ToolbarInfo, ToolbarState};

const ZOOM_STEPS: &[f32] = &[
    0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];
const STATUS_SECONDS: f64 = 5.0;

pub struct RustTextApp {
    session: EditSession,
    editor: EditorView,
    fonts: FontManager,
    toolbar: ToolbarState,
    path: Option<PathBuf>,
    format: FileFormat,
    dialog: Option<Dialog>,
    status: Option<(String, f64)>,
    allow_close: bool,
    title: String,
    /// Document revision whose fonts have been registered.
    fonts_revision: Option<u64>,
}

impl RustTextApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        Self::with_fonts(&cc.egui_ctx, FontManager::new(), path)
    }

    /// Creates the application with the given font manager (tests use
    /// [`FontManager::empty`] to skip the system font scan).
    pub fn with_fonts(ctx: &egui::Context, fonts: FontManager, path: Option<PathBuf>) -> Self {
        ctx.set_theme(egui::Theme::Light);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = Self {
            session: EditSession::new(Document::new()),
            editor: EditorView::new(),
            fonts,
            toolbar: ToolbarState::default(),
            path: None,
            format: FileFormat::Docx,
            dialog: None,
            status: None,
            allow_close: false,
            title: String::new(),
            fonts_revision: None,
        };
        if let Some(p) = path {
            if p.exists() {
                app.load(&p);
            } else {
                // Treat a missing path as "new file with this name".
                app.format = FileFormat::from_path(&p).unwrap_or(FileFormat::Docx);
                app.path = Some(p);
            }
        }
        app
    }

    fn set_status(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        let msg = msg.into();
        log::info!("{msg}");
        self.status = Some((msg, ctx.input(|i| i.time)));
    }

    fn load(&mut self, path: &Path) {
        match formats::load(path) {
            Ok((doc, format)) => {
                self.fonts.request_all(doc.font_families().iter());
                self.session.load(doc);
                self.path = Some(path.to_path_buf());
                self.format = format;
                self.editor.reveal_caret();
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                self.status = Some((format!("Opened {name}"), 0.0));
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(format!(
                    "Could not open {}:\n{e}",
                    path.display()
                )));
            }
        }
    }

    fn save_to(&mut self, ctx: &egui::Context, path: PathBuf, format: FileFormat) -> bool {
        match formats::save(&self.session.doc, &path, format) {
            Ok(()) => {
                self.session.modified = false;
                let note = match format {
                    FileFormat::PlainText => " (formatting is not stored in plain text)",
                    FileFormat::Markdown => {
                        " (fonts, colors and alignment are not stored in Markdown)"
                    }
                    _ => "",
                };
                self.set_status(ctx, format!("Saved {}{note}", path.display()));
                self.path = Some(path);
                self.format = format;
                true
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(format!(
                    "Could not save {}:\n{e}",
                    path.display()
                )));
                false
            }
        }
    }

    fn save(&mut self, ctx: &egui::Context) -> bool {
        match self.path.clone() {
            Some(p) => {
                let format = FileFormat::from_path(&p).unwrap_or(self.format);
                self.save_to(ctx, p, format)
            }
            None => self.save_as(ctx),
        }
    }

    fn save_as(&mut self, ctx: &egui::Context) -> bool {
        match file_ops::pick_save(self.path.as_deref(), self.format) {
            Some((path, format)) => self.save_to(ctx, path, format),
            None => false,
        }
    }

    /// Runs `action`, first asking about unsaved changes if necessary.
    fn request(&mut self, ctx: &egui::Context, action: PendingAction) {
        if self.session.modified {
            self.dialog = Some(Dialog::UnsavedChanges(action));
        } else {
            self.perform(ctx, action);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: PendingAction) {
        match action {
            PendingAction::New => {
                self.session.load(Document::new());
                self.path = None;
                self.format = FileFormat::Docx;
                self.set_status(ctx, "New document");
            }
            PendingAction::Open => {
                if let Some(p) = file_ops::pick_open(self.path.as_deref()) {
                    self.load(&p);
                }
            }
            PendingAction::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn zoom(&mut self, dir: i32) {
        let z = self.editor.zoom;
        self.editor.zoom = match dir {
            0 => 1.0,
            d if d > 0 => ZOOM_STEPS
                .iter()
                .copied()
                .find(|s| *s > z + 0.01)
                .unwrap_or(z),
            _ => ZOOM_STEPS
                .iter()
                .rev()
                .copied()
                .find(|s| *s < z - 0.01)
                .unwrap_or(z),
        };
    }

    /// Carries out a command.
    pub fn execute(&mut self, ctx: &egui::Context, cmd: Command) {
        if needs_table(&cmd) && !self.session.in_table() {
            self.set_status(ctx, "Place the cursor in a table first");
            return;
        }
        let s = &mut self.session;
        match cmd {
            Command::New => self.request(ctx, PendingAction::New),
            Command::Open => self.request(ctx, PendingAction::Open),
            Command::Save => {
                self.save(ctx);
            }
            Command::SaveAs => {
                self.save_as(ctx);
            }
            Command::Quit => self.request(ctx, PendingAction::Quit),
            Command::Undo => {
                s.undo();
            }
            Command::Redo => {
                s.redo();
            }
            Command::Cut => {
                if let Some(t) = s.cut() {
                    ctx.copy_text(t);
                }
            }
            Command::Copy => {
                if let Some(t) = s.copy() {
                    ctx.copy_text(t);
                }
            }
            Command::Paste => {
                let text = file_ops::clipboard_text();
                s.paste_from(text.as_deref());
            }
            Command::SelectAll => s.select_all(),
            Command::Toggle(a) => s.toggle_attr(a),
            Command::FontFamily(f) => {
                if let Some(name) = &f {
                    self.fonts.request(name);
                }
                s.set_font_family(f);
            }
            Command::FontSize(size) => s.set_font_size(size),
            Command::TextColor(c) => s.set_text_color(c),
            Command::Align(a) => s.set_alignment(a),
            Command::List(k) => s.toggle_list(k),
            Command::Indent(d) => s.change_indent(d),
            Command::Heading(h) => s.set_heading(h),
            Command::ShowInsertTable => {
                if s.caret().path.is_in_table() {
                    self.set_status(ctx, "Tables cannot be nested");
                } else {
                    self.dialog = Some(Dialog::InsertTable { rows: 3, cols: 3 });
                }
            }
            Command::InsertTable { rows, cols } => {
                if !s.insert_table(rows, cols) {
                    self.set_status(ctx, "Tables cannot be nested");
                }
            }
            Command::InsertRow { below } => s.insert_row(below),
            Command::InsertColumn { right } => s.insert_column(right),
            Command::DeleteRows => s.delete_rows(),
            Command::DeleteColumns => s.delete_columns(),
            Command::DeleteTable => s.delete_table(),
            Command::SelectTable => s.select_table(),
            Command::CellBackground(c) => s.set_cell_background(c),
            Command::CellForeground(c) => s.set_cell_foreground(c),
            Command::ZoomIn => self.zoom(1),
            Command::ZoomOut => self.zoom(-1),
            Command::ZoomReset => self.zoom(0),
            Command::About => self.dialog = Some(Dialog::About),
        }
        self.editor.request_focus();
        self.editor.reveal_caret();
    }

    /// Global keyboard shortcuts (editing shortcuts are handled by the editor).
    fn shortcuts(&self, ctx: &egui::Context) -> Vec<Command> {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let table: [(Modifiers, Key, Command); 8] = [
            (cmd_shift, Key::S, Command::SaveAs),
            (cmd, Key::S, Command::Save),
            (cmd, Key::O, Command::Open),
            (cmd, Key::N, Command::New),
            (cmd, Key::Q, Command::Quit),
            (cmd, Key::Plus, Command::ZoomIn),
            (cmd, Key::Minus, Command::ZoomOut),
            (cmd, Key::Num0, Command::ZoomReset),
        ];
        let mut out = Vec::new();
        ctx.input_mut(|i| {
            for (m, k, c) in table {
                if i.consume_shortcut(&KeyboardShortcut::new(m, k)) {
                    out.push(c);
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Equals)) {
                out.push(Command::ZoomIn);
            }
        });
        out
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        let title = format!(
            "{}{} — RustText",
            if self.session.modified { "• " } else { "" },
            name
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            let msg = match &self.status {
                Some((m, t)) if *t == 0.0 || now - t < STATUS_SECONDS => m.clone(),
                _ => self
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Untitled document".into()),
            };
            if let Some((_, t)) = &mut self.status
                && *t == 0.0
            {
                *t = now; // start the timeout for messages set before the first frame
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{:.0}%", self.editor.zoom * 100.0));
                ui.separator();
                ui.label(self.format.label());
                ui.separator();
                let caret = self.session.caret();
                let where_ = match caret.path.cell {
                    Some((r, c)) => format!("Table cell {}:{}", r + 1, c + 1),
                    None => format!("Paragraph {}", caret.path.block + 1),
                };
                ui.label(format!("{where_}, column {}", caret.offset + 1));
                ui.separator();
                ui.label(format!(
                    "{} words, {} characters",
                    self.session.doc.word_count(),
                    self.session.doc.char_count()
                ));
                ui.separator();
                // The message takes the remaining space on the left and is
                // truncated rather than overlapping the counters.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(msg).truncate());
                });
            });
        });
    }
}

impl eframe::App for RustTextApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl RustTextApp {
    /// The editing session (document, selection, history).
    pub fn session(&self) -> &EditSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut EditSession {
        &mut self.session
    }

    /// Draws the whole application UI into `ui` for one frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // Make sure every font used by the document (typed, pasted, loaded,
        // undone…) is registered with egui.
        if self.fonts_revision != Some(self.session.revision) {
            self.fonts
                .request_all(self.session.doc.font_families().iter());
            self.fonts_revision = Some(self.session.revision);
        }
        self.fonts.begin_frame(&ctx);

        if ctx.input(|i| i.viewport().close_requested())
            && self.session.modified
            && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::UnsavedChanges(PendingAction::Quit));
        }

        let mut cmds = if self.dialog.is_none() {
            self.shortcuts(&ctx)
        } else {
            Vec::new()
        };

        let s = &self.session;
        let menu_info = menu::MenuInfo {
            can_undo: s.history.can_undo(),
            can_redo: s.history.can_redo(),
            has_selection: s.has_selection(),
            in_table: s.in_table(),
        };
        Panel::top("menu").show(ui, |ui| cmds.extend(menu::menu_bar(ui, &menu_info)));

        let info = ToolbarInfo {
            char_style: s.current_char_style(),
            para_style: s.current_paragraph_style(),
            bold: s.attr_active(ToggleAttr::Bold),
            italic: s.attr_active(ToggleAttr::Italic),
            underline: s.attr_active(ToggleAttr::Underline),
            strikethrough: s.attr_active(ToggleAttr::Strikethrough),
            cell: s.current_cell_style(),
            can_undo: menu_info.can_undo,
            can_redo: menu_info.can_redo,
            has_selection: menu_info.has_selection,
            families: self.fonts.families(),
            default_family: self.fonts.default_family(),
        };
        Panel::top("toolbar")
            .frame(Frame::side_top_panel(ui.style()).inner_margin(6.0))
            .show(ui, |ui| {
                cmds.extend(toolbar::toolbar(ui, &mut self.toolbar, &info))
            });

        Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        // Run toolbar/menu commands before the editor paints this frame.
        for c in cmds {
            self.execute(&ctx, c);
        }

        let interactive = self.dialog.is_none();
        CentralPanel::no_frame()
            .frame(Frame::new().fill(Color32::from_gray(225)))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_enabled_ui(interactive, |ui| {
                            self.editor.show(ui, &mut self.session, &self.fonts);
                        });
                    });
            });

        if let Some(dialog) = &mut self.dialog {
            let outcome = dialogs::show(&ctx, dialog);
            if outcome.close {
                self.dialog = None;
            }
            if let Some(cmd) = outcome.command {
                self.execute(&ctx, cmd);
            }
            if let Some((choice, action)) = outcome.unsaved {
                match choice {
                    UnsavedChoice::Save => {
                        if self.save(&ctx) {
                            self.perform(&ctx, action);
                        }
                    }
                    UnsavedChoice::Discard => self.perform(&ctx, action),
                    UnsavedChoice::Cancel => {}
                }
            }
        }

        self.update_title(&ctx);
    }
}
