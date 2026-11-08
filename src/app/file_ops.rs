//! Native open / save dialogs and the system clipboard.

use std::path::{Path, PathBuf};

use crate::formats::FileFormat;

fn add_filters(mut dialog: rfd::FileDialog, all_first: bool) -> rfd::FileDialog {
    if all_first {
        let all: Vec<&str> = FileFormat::ALL
            .iter()
            .flat_map(|f| f.extensions().iter().copied())
            .collect();
        dialog = dialog.add_filter("All supported documents", &all);
    }
    for f in FileFormat::ALL {
        dialog = dialog.add_filter(f.label(), f.extensions());
    }
    dialog
}

/// Shows the "open file" dialog.
pub fn pick_open(current: Option<&Path>) -> Option<PathBuf> {
    let mut d = add_filters(rfd::FileDialog::new().set_title("Open document"), true);
    if let Some(dir) = current.and_then(Path::parent) {
        d = d.set_directory(dir);
    }
    d.pick_file()
}

/// Shows the "save as" dialog. Adds the format's default extension if the
/// user did not type a known one.
pub fn pick_save(current: Option<&Path>, format: FileFormat) -> Option<(PathBuf, FileFormat)> {
    let file_name = current
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned());
    // Offer the current format first.
    let mut d = rfd::FileDialog::new()
        .set_title("Save document")
        .set_file_name(format!("{file_name}.{}", format.default_extension()))
        .add_filter(format.label(), format.extensions());
    for f in FileFormat::ALL.into_iter().filter(|f| *f != format) {
        d = d.add_filter(f.label(), f.extensions());
    }
    if let Some(dir) = current.and_then(Path::parent) {
        d = d.set_directory(dir);
    }
    let mut path = d.save_file()?;
    let chosen = match FileFormat::from_path(&path) {
        Some(f) => f,
        None => {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            name.push(".");
            name.push(format.default_extension());
            path.set_file_name(name);
            format
        }
    };
    Some((path, chosen))
}

/// Reads text from the system clipboard.
pub fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}
