//! The egui front-end of the editing engine.
//!
//! * [`layout`] – document → positioned galleys (wrapping, alignment, tables)
//! * [`render`] – painting of the layout, selection and caret
//! * [`widget`] – [`EditorView`]: input handling and the glue between the two

pub mod layout;
pub mod render;
pub mod widget;

pub use widget::{EditorOutput, EditorView};
