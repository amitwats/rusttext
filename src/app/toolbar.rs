//! The formatting toolbar.

use egui::{Button, Color32, ComboBox, DragValue, RichText, ScrollArea, Ui, vec2};

use super::commands::Command;
use super::icons::{Icon, icon_button};
use crate::document::{Alignment, CellStyle, CharStyle, ListKind, ParagraphStyle, ToggleAttr};
use crate::editor::layout::{from_color32, to_color32};

const FONT_SIZES: &[f32] = &[
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0,
];

/// A palette of commonly used colors (Office-like).
pub const PALETTE: &[[u8; 3]] = &[
    [0, 0, 0],
    [67, 67, 67],
    [102, 102, 102],
    [153, 153, 153],
    [204, 204, 204],
    [255, 255, 255],
    [192, 0, 0],
    [255, 0, 0],
    [255, 192, 0],
    [255, 255, 0],
    [146, 208, 80],
    [0, 176, 80],
    [0, 176, 240],
    [0, 112, 192],
    [0, 32, 96],
    [112, 48, 160],
    [255, 153, 204],
    [255, 204, 153],
    [255, 242, 204],
    [226, 239, 218],
    [221, 235, 247],
    [237, 226, 246],
    [252, 228, 214],
    [242, 242, 242],
];

/// UI state of the toolbar that persists between frames.
#[derive(Default)]
pub struct ToolbarState {
    pub font_filter: String,
    custom_text: Option<Color32>,
    custom_fill: Option<Color32>,
    custom_cell_text: Option<Color32>,
}

/// What the toolbar needs to know about the current selection.
pub struct ToolbarInfo<'a> {
    pub char_style: CharStyle,
    pub para_style: ParagraphStyle,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub cell: Option<CellStyle>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub families: &'a [String],
    pub default_family: Option<&'a str>,
}

/// A drop-down color chooser. Returns `Some(choice)` when a color (or
/// "automatic" = `None`) was picked.
fn color_menu(
    ui: &mut Ui,
    label: RichText,
    tooltip: &str,
    auto_label: &str,
    custom: &mut Option<Color32>,
) -> Option<Option<Color32>> {
    let mut picked = None;
    ui.menu_button(label, |ui| {
        if ui.button(auto_label).clicked() {
            picked = Some(None);
            ui.close();
        }
        ui.separator();
        egui::Grid::new(ui.next_auto_id())
            .spacing(vec2(3.0, 3.0))
            .show(ui, |ui| {
                for (i, rgb) in PALETTE.iter().enumerate() {
                    let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let swatch = Button::new("").fill(c).min_size(vec2(18.0, 18.0));
                    if ui
                        .add(swatch)
                        .on_hover_text(format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
                        .clicked()
                    {
                        picked = Some(Some(c));
                        ui.close();
                    }
                    if i % 6 == 5 {
                        ui.end_row();
                    }
                }
            });
        ui.separator();
        ui.label("Custom:");
        let c = custom.get_or_insert(Color32::from_rgb(0, 112, 192));
        egui::color_picker::color_picker_color32(ui, c, egui::color_picker::Alpha::Opaque);
        if ui.button("Apply custom color").clicked() {
            picked = Some(Some(*c));
            ui.close();
        }
    })
    .response
    .on_hover_text(tooltip);
    picked
}

fn toggle(ui: &mut Ui, text: RichText, on: bool, tooltip: &str) -> bool {
    ui.add(Button::new(text).selected(on).min_size(vec2(26.0, 24.0)))
        .on_hover_text(tooltip)
        .clicked()
}

pub fn toolbar(ui: &mut Ui, state: &mut ToolbarState, info: &ToolbarInfo<'_>) -> Vec<Command> {
    let mut cmds = Vec::new();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // --- Edit ---
        if ui
            .button("New")
            .on_hover_text("New document (Ctrl+N)")
            .clicked()
        {
            cmds.push(Command::New);
        }
        if ui
            .button("Open")
            .on_hover_text("Open a file (Ctrl+O)")
            .clicked()
        {
            cmds.push(Command::Open);
        }
        if ui.button("Save").on_hover_text("Save (Ctrl+S)").clicked() {
            cmds.push(Command::Save);
        }
        ui.separator();
        if ui
            .add_enabled(info.can_undo, Button::new("Undo"))
            .on_hover_text("Undo (Ctrl+Z)")
            .clicked()
        {
            cmds.push(Command::Undo);
        }
        if ui
            .add_enabled(info.can_redo, Button::new("Redo"))
            .on_hover_text("Redo (Ctrl+Y)")
            .clicked()
        {
            cmds.push(Command::Redo);
        }
        ui.separator();
        if ui
            .add_enabled(info.has_selection, Button::new("Cut"))
            .on_hover_text("Cut (Ctrl+X)")
            .clicked()
        {
            cmds.push(Command::Cut);
        }
        if ui
            .add_enabled(info.has_selection, Button::new("Copy"))
            .on_hover_text("Copy (Ctrl+C)")
            .clicked()
        {
            cmds.push(Command::Copy);
        }
        if ui.button("Paste").on_hover_text("Paste (Ctrl+V)").clicked() {
            cmds.push(Command::Paste);
        }
        ui.separator();

        // --- Paragraph style ---
        let heading = info.para_style.heading;
        let name = |h: u8| {
            if h == 0 {
                "Normal".to_owned()
            } else {
                format!("Heading {h}")
            }
        };
        ComboBox::from_id_salt("para-style")
            .width(92.0)
            .selected_text(name(heading))
            .show_ui(ui, |ui| {
                for h in 0..=6u8 {
                    if ui.selectable_label(heading == h, name(h)).clicked() {
                        cmds.push(Command::Heading(h));
                    }
                }
            });

        // --- Font family ---
        let current =
            info.char_style.font_family.clone().unwrap_or_else(|| {
                format!("{} (default)", info.default_family.unwrap_or("Default"))
            });
        ComboBox::from_id_salt("font-family")
            .width(170.0)
            .selected_text(current)
            .height(360.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.font_filter)
                        .hint_text("Search fonts…")
                        .desired_width(200.0),
                );
                if ui
                    .selectable_label(info.char_style.font_family.is_none(), "(Default font)")
                    .clicked()
                {
                    cmds.push(Command::FontFamily(None));
                }
                let filter = state.font_filter.to_lowercase();
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for f in info
                        .families
                        .iter()
                        .filter(|f| filter.is_empty() || f.to_lowercase().contains(&filter))
                    {
                        let selected = info.char_style.font_family.as_deref() == Some(f.as_str());
                        if ui.selectable_label(selected, f).clicked() {
                            cmds.push(Command::FontFamily(Some(f.clone())));
                        }
                    }
                });
            });

        // --- Font size ---
        let mut size = info.char_style.font_size;
        ComboBox::from_id_salt("font-size")
            .width(44.0)
            .selected_text(format!("{size}"))
            .show_ui(ui, |ui| {
                for &s in FONT_SIZES {
                    if ui.selectable_label(size == s, format!("{s}")).clicked() {
                        cmds.push(Command::FontSize(s));
                    }
                }
            });
        let drag = ui.add(
            DragValue::new(&mut size)
                .range(4.0..=400.0)
                .speed(0.25)
                .suffix(" pt"),
        );
        if drag.changed() && !drag.dragged() || drag.drag_stopped() {
            cmds.push(Command::FontSize(size));
        }
        ui.separator();

        // --- Character toggles ---
        if toggle(ui, RichText::new("B").strong(), info.bold, "Bold (Ctrl+B)") {
            cmds.push(Command::Toggle(ToggleAttr::Bold));
        }
        if toggle(
            ui,
            RichText::new("I").italics(),
            info.italic,
            "Italic (Ctrl+I)",
        ) {
            cmds.push(Command::Toggle(ToggleAttr::Italic));
        }
        if toggle(
            ui,
            RichText::new("U").underline(),
            info.underline,
            "Underline (Ctrl+U)",
        ) {
            cmds.push(Command::Toggle(ToggleAttr::Underline));
        }
        if toggle(
            ui,
            RichText::new("S").strikethrough(),
            info.strikethrough,
            "Strikethrough",
        ) {
            cmds.push(Command::Toggle(ToggleAttr::Strikethrough));
        }
        let text_color = info
            .char_style
            .color
            .map(to_color32)
            .unwrap_or(Color32::BLACK);
        let label = RichText::new("A")
            .strong()
            .color(text_color)
            .background_color(Color32::from_gray(235));
        if let Some(c) = color_menu(ui, label, "Text color", "Automatic", &mut state.custom_text) {
            cmds.push(Command::TextColor(c.map(from_color32)));
        }
        ui.separator();

        // --- Paragraph ---
        let align = info.para_style.alignment;
        for (a, icon, tip) in [
            (Alignment::Left, Icon::AlignLeft, "Align left"),
            (Alignment::Center, Icon::AlignCenter, "Center"),
            (Alignment::Right, Icon::AlignRight, "Align right"),
            (Alignment::Justify, Icon::AlignJustify, "Justify"),
        ] {
            if icon_button(ui, icon, align == a, tip).clicked() {
                cmds.push(Command::Align(a));
            }
        }
        let list = info.para_style.list;
        if icon_button(
            ui,
            Icon::Bullets,
            list == Some(ListKind::Bullet),
            "Bulleted list",
        )
        .clicked()
        {
            cmds.push(Command::List(ListKind::Bullet));
        }
        if icon_button(
            ui,
            Icon::Numbering,
            list == Some(ListKind::Numbered),
            "Numbered list",
        )
        .clicked()
        {
            cmds.push(Command::List(ListKind::Numbered));
        }
        if icon_button(
            ui,
            Icon::IndentLess,
            false,
            "Decrease indent (Shift+Tab in lists)",
        )
        .clicked()
        {
            cmds.push(Command::Indent(-1));
        }
        if icon_button(
            ui,
            Icon::IndentMore,
            false,
            "Increase indent (Tab in lists)",
        )
        .clicked()
        {
            cmds.push(Command::Indent(1));
        }
        ui.separator();

        // --- Tables ---
        if icon_button(ui, Icon::Table, false, "Insert table…").clicked() {
            cmds.push(Command::ShowInsertTable);
        }
        let in_table = info.cell.is_some();
        ui.add_enabled_ui(in_table, |ui| {
            let b = |ui: &mut Ui, t: &str, tip: &str| ui.button(t).on_hover_text(tip).clicked();
            if b(ui, "Row+ above", "Insert a row above") {
                cmds.push(Command::InsertRow { below: false });
            }
            if b(ui, "Row+ below", "Insert a row below") {
                cmds.push(Command::InsertRow { below: true });
            }
            if b(ui, "Col+ left", "Insert a column to the left") {
                cmds.push(Command::InsertColumn { right: false });
            }
            if b(ui, "Col+ right", "Insert a column to the right") {
                cmds.push(Command::InsertColumn { right: true });
            }
            if b(ui, "Row-", "Delete the selected rows") {
                cmds.push(Command::DeleteRows);
            }
            if b(ui, "Col-", "Delete the selected columns") {
                cmds.push(Command::DeleteColumns);
            }
            let fill = info
                .cell
                .as_ref()
                .and_then(|c| c.background)
                .map(to_color32)
                .unwrap_or(Color32::WHITE);
            let label = RichText::new(" Fill ")
                .background_color(fill)
                .color(Color32::BLACK);
            if let Some(c) = color_menu(
                ui,
                label,
                "Cell background color",
                "No fill",
                &mut state.custom_fill,
            ) {
                cmds.push(Command::CellBackground(c.map(from_color32)));
            }
            let fg = info
                .cell
                .as_ref()
                .and_then(|c| c.foreground)
                .map(to_color32)
                .unwrap_or(Color32::BLACK);
            let label = RichText::new(" Text ")
                .color(fg)
                .background_color(Color32::from_gray(235));
            if let Some(c) = color_menu(
                ui,
                label,
                "Cell text color",
                "Automatic",
                &mut state.custom_cell_text,
            ) {
                cmds.push(Command::CellForeground(c.map(from_color32)));
            }
        });
    });
    cmds
}
