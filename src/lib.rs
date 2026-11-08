//! # RustText
//!
//! A basic rich text editor. The crate is split into layers with one-way
//! dependencies:
//!
//! ```text
//!   app ──► editor ──► document
//!    │        │            ▲
//!    │        └──► fonts   │
//!    └──────────► formats ─┘
//! ```
//!
//! * [`document`] – GUI-independent document model and editing commands
//! * [`formats`]  – TXT / Markdown / RTF / DOCX readers and writers
//! * [`fonts`]    – system font discovery and registration with egui
//! * [`editor`]   – the egui rich-text editing widget
//! * [`app`]      – the desktop application (menus, toolbar, dialogs)

pub mod app;
pub mod document;
pub mod editor;
pub mod fonts;
pub mod formats;

pub use app::RustTextApp;
