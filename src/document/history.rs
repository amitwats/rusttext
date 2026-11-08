//! Snapshot based undo / redo.
//!
//! Documents edited in a basic text editor are small, so storing whole
//! snapshots is simple, robust and fast enough. Consecutive typing (or
//! deleting) is coalesced into a single undo step.

use super::model::Document;
use super::position::Selection;

const MAX_UNDO_STEPS: usize = 200;

/// Classifies an edit so consecutive edits of the same kind can be merged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Clone, Debug)]
struct Snapshot {
    doc: Document,
    selection: Selection,
}

#[derive(Default, Debug)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_kind: Option<EditKind>,
}

impl History {
    /// Records the state *before* an edit. Returns `true` when a new undo
    /// step was created (as opposed to being merged with the previous one).
    pub fn record(&mut self, kind: EditKind, doc: &Document, selection: Selection) -> bool {
        self.redo.clear();
        let coalesce = kind != EditKind::Other && self.last_kind == Some(kind);
        self.last_kind = Some(kind);
        if coalesce {
            return false;
        }
        self.undo.push(Snapshot {
            doc: doc.clone(),
            selection,
        });
        if self.undo.len() > MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
        true
    }

    /// Prevents the next edit from being merged with the previous one.
    pub fn break_coalescing(&mut self) {
        self.last_kind = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, doc: &mut Document, selection: &mut Selection) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            doc: std::mem::replace(doc, s.doc),
            selection: std::mem::replace(selection, s.selection),
        });
        self.last_kind = None;
        true
    }

    pub fn redo(&mut self, doc: &mut Document, selection: &mut Selection) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            doc: std::mem::replace(doc, s.doc),
            selection: std::mem::replace(selection, s.selection),
        });
        self.last_kind = None;
        true
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_kind = None;
    }
}
