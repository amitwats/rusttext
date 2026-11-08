//! System font discovery and registration with egui.
//!
//! [`FontManager`] enumerates the installed font families with `fontdb`
//! and lazily loads a family (regular / bold / italic / bold-italic faces)
//! into egui the first time it is used. Missing bold or italic faces are
//! synthesized by the renderer (see [`ResolvedFont`]).
//!
//! egui applies new fonts at the start of the *next* frame, so a family is
//! only reported as usable once that has happened; until then text falls
//! back to the default font for a single frame.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// Name used by importers (e.g. Markdown code blocks) to request a monospace font.
pub const MONOSPACE_ALIAS: &str = "Monospace";

/// Preferred default document fonts, in order.
const DEFAULT_CANDIDATES: &[&str] = &[
    "Calibri",
    "Segoe UI",
    "Helvetica Neue",
    "Helvetica",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
    "Ubuntu",
    "Cantarell",
    "Roboto",
];

#[derive(Clone, Copy, Debug, Default)]
struct Variants {
    bold: bool,
    italic: bool,
    bold_italic: bool,
}

/// The egui font family to use for a character style, and which effects
/// must be synthesized because the font has no matching face.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFont {
    pub family: FontFamily,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
}

pub struct FontManager {
    db: fontdb::Database,
    /// Sorted, de-duplicated family names of the installed fonts.
    families: Vec<String>,
    /// Lower-case name → canonical name.
    lookup: HashMap<String, String>,
    defs: FontDefinitions,
    registered: HashMap<String, Variants>,
    /// Families registered but not yet applied by egui.
    staged: Vec<String>,
    /// Families usable for layout in the current frame.
    active: HashSet<String>,
    dirty: bool,
    default_family: Option<String>,
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

fn key(family: &str, variant: &str) -> String {
    format!("{family}#{variant}")
}

impl FontManager {
    /// Scans the system fonts. This takes a moment on systems with many fonts.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self::with_database(db)
    }

    /// A manager that knows no system fonts (useful for tests).
    pub fn empty() -> Self {
        Self::with_database(fontdb::Database::new())
    }

    fn with_database(db: fontdb::Database) -> Self {
        let mut families: Vec<String> = db
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .filter(|n| !n.starts_with('.'))
            .collect();
        families.sort_by_key(|a| a.to_lowercase());
        families.dedup();
        let lookup = families
            .iter()
            .map(|f| (f.to_lowercase(), f.clone()))
            .collect();
        let mut m = Self {
            db,
            families,
            lookup,
            defs: FontDefinitions::default(),
            registered: HashMap::new(),
            staged: Vec::new(),
            active: HashSet::new(),
            dirty: false,
            default_family: None,
        };
        m.default_family = DEFAULT_CANDIDATES
            .iter()
            .find_map(|c| m.canonical(c))
            .or_else(|| m.families.first().cloned());
        if let Some(d) = m.default_family.clone() {
            m.request(&d);
        }
        m
    }

    /// All installed font family names (sorted).
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// The family used for text without an explicit font.
    pub fn default_family(&self) -> Option<&str> {
        self.default_family.as_deref()
    }

    /// The installed family matching `name` case-insensitively.
    pub fn canonical(&self, name: &str) -> Option<String> {
        self.lookup.get(&name.to_lowercase()).cloned()
    }

    pub fn is_installed(&self, name: &str) -> bool {
        name == MONOSPACE_ALIAS || self.lookup.contains_key(&name.to_lowercase())
    }

    fn load_face(
        &self,
        family: &str,
        weight: fontdb::Weight,
        style: fontdb::Style,
    ) -> Option<(FontData, bool, bool)> {
        let names = [fontdb::Family::Name(family)];
        let id = self.db.query(&fontdb::Query {
            families: &names,
            weight,
            stretch: fontdb::Stretch::Normal,
            style,
        })?;
        let info = self.db.face(id)?;
        let is_bold = info.weight.0 >= 600;
        let is_italic = info.style != fontdb::Style::Normal;
        let (bytes, index) = self
            .db
            .with_face_data(id, |data, index| (data.to_vec(), index))?;
        let mut fd = FontData::from_owned(bytes);
        fd.index = index;
        Some((fd, is_bold, is_italic))
    }

    /// Requests that `name` becomes available for rendering. Returns `false`
    /// if the family is not installed.
    pub fn request(&mut self, name: &str) -> bool {
        let Some(name) = self.canonical(name) else {
            return false;
        };
        if self.registered.contains_key(&name) {
            return true;
        }
        let fallbacks = self
            .defs
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        let mut variants = Variants::default();
        let add = |defs: &mut FontDefinitions, variant: &str, fd: FontData| {
            let k = key(&name, variant);
            defs.font_data.insert(k.clone(), Arc::new(fd));
            let mut chain = vec![k.clone()];
            chain.extend(fallbacks.iter().cloned());
            defs.families.insert(FontFamily::Name(k.into()), chain);
        };
        use fontdb::{Style, Weight};
        let Some((regular, _, _)) = self.load_face(&name, Weight::NORMAL, Style::Normal) else {
            return false;
        };
        add(&mut self.defs, "r", regular);
        if let Some((fd, b, _)) = self.load_face(&name, Weight::BOLD, Style::Normal)
            && b
        {
            add(&mut self.defs, "b", fd);
            variants.bold = true;
        }
        if let Some((fd, _, i)) = self.load_face(&name, Weight::NORMAL, Style::Italic)
            && i
        {
            add(&mut self.defs, "i", fd);
            variants.italic = true;
        }
        if let Some((fd, b, i)) = self.load_face(&name, Weight::BOLD, Style::Italic)
            && b
            && i
        {
            add(&mut self.defs, "bi", fd);
            variants.bold_italic = true;
        }
        log::debug!("registered font family {name}: {variants:?}");
        self.registered.insert(name.clone(), variants);
        self.staged.push(name);
        self.dirty = true;
        true
    }

    /// Requests every font used by a document.
    pub fn request_all<'a>(&mut self, names: impl IntoIterator<Item = &'a String>) {
        for n in names {
            self.request(n);
        }
    }

    /// Must be called once at the beginning of every frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        if self.dirty {
            ctx.set_fonts(self.defs.clone());
            self.dirty = false;
            ctx.request_repaint();
            return;
        }
        // Fonts set during the previous frame are active now.
        if !self.staged.is_empty() {
            self.active.extend(self.staged.drain(..));
            ctx.request_repaint();
        }
    }

    /// Maps a document font request to an egui font family.
    pub fn resolve(&self, family: Option<&str>, bold: bool, italic: bool) -> ResolvedFont {
        let fallback = |family| ResolvedFont {
            family,
            synthetic_bold: bold,
            synthetic_italic: italic,
        };
        let name = match family {
            Some(n) if n.eq_ignore_ascii_case(MONOSPACE_ALIAS) => {
                return fallback(FontFamily::Monospace);
            }
            Some(n) => self.canonical(n).or_else(|| self.default_family.clone()),
            None => self.default_family.clone(),
        };
        let Some(name) = name else {
            return fallback(FontFamily::Proportional);
        };
        let (Some(v), true) = (self.registered.get(&name), self.active.contains(&name)) else {
            return fallback(FontFamily::Proportional);
        };
        let (variant, sb, si) = match (bold, italic) {
            (true, true) if v.bold_italic => ("bi", false, false),
            (true, true) if v.bold => ("b", false, true),
            (true, true) if v.italic => ("i", true, false),
            (true, true) => ("r", true, true),
            (true, false) if v.bold => ("b", false, false),
            (true, false) => ("r", true, false),
            (false, true) if v.italic => ("i", false, false),
            (false, true) => ("r", false, true),
            (false, false) => ("r", false, false),
        };
        ResolvedFont {
            family: FontFamily::Name(key(&name, variant).into()),
            synthetic_bold: sb,
            synthetic_italic: si,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fonts_fall_back() {
        let m = FontManager::empty();
        let r = m.resolve(Some("Nope"), true, false);
        assert_eq!(r.family, FontFamily::Proportional);
        assert!(r.synthetic_bold);
        assert_eq!(
            m.resolve(Some("monospace"), false, false).family,
            FontFamily::Monospace
        );
    }
}
