//! Drives the complete application UI headlessly (no window, no GPU) by
//! feeding synthetic input events to an `egui::Context`.

use egui::{Context, Event, Key, Modifiers, PointerButton, RawInput, Rect, pos2, vec2};
use rusttext::RustTextApp;
use rusttext::document::{Block, ListKind, ParaPath, TextPos};
use rusttext::fonts::FontManager;

struct Harness {
    ctx: Context,
    app: RustTextApp,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let ctx = Context::default();
        let app = RustTextApp::with_fonts(&ctx, FontManager::empty(), None);
        let mut h = Self {
            ctx,
            app,
            time: 0.0,
        };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 900.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(input, |ui| app.show(ui));
        // No renderer: discard texture uploads.
        out.textures_delta.clear();
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_owned())]);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]);
    }

    fn click(&mut self, pos: egui::Pos2) {
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    }

    fn text(&self) -> String {
        self.app.session().doc.plain_text()
    }
}

#[test]
fn typing_editing_and_navigation() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.type_text(" world");
    assert_eq!(h.text(), "Hello world");

    h.key(Key::Enter, Modifiers::NONE);
    h.type_text("Second line");
    assert_eq!(h.text(), "Hello world\nSecond line");

    // Up arrow moves into the first paragraph.
    h.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(h.app.session().caret().path, ParaPath::top(0));
    h.key(Key::End, Modifiers::NONE);
    assert_eq!(h.app.session().caret().offset, 11);
    h.key(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.text(), "Hello worl\nSecond line");

    // Down arrow returns to the second paragraph.
    h.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.app.session().caret().path, ParaPath::top(1));

    // Undo restores the deleted character.
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.text(), "Hello world\nSecond line");
}

#[test]
fn shortcuts_format_text() {
    let mut h = Harness::new();
    h.type_text("bold");
    h.key(Key::A, Modifiers::COMMAND);
    h.key(Key::B, Modifiers::COMMAND);
    let p = h.app.session().doc.paragraph(ParaPath::top(0)).unwrap();
    assert!(p.runs.iter().all(|r| r.style.bold));
}

#[test]
fn clipboard_events() {
    let mut h = Harness::new();
    h.type_text("copy me");
    h.key(Key::A, Modifiers::COMMAND);
    h.frame(vec![Event::Copy]);
    h.key(Key::End, Modifiers::COMMAND);
    h.frame(vec![Event::Paste("copy me".into())]);
    assert_eq!(h.text(), "copy mecopy me");
}

#[test]
fn mouse_click_places_caret() {
    let mut h = Harness::new();
    h.type_text("first paragraph");
    h.key(Key::Enter, Modifiers::NONE);
    h.type_text("second paragraph");
    assert_eq!(h.app.session().caret().path, ParaPath::top(1));
    // The page is centered; the first line of text starts near the top margin.
    // Click far left on the first line: caret goes to the start of paragraph 0.
    let x = 1200.0 / 2.0 - 816.0 / 2.0 + 72.0 + 1.0;
    let mut found = false;
    for y in (60..260).step_by(4) {
        h.click(pos2(x, y as f32));
        if h.app.session().caret() == TextPos::new(ParaPath::top(0), 0) {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "clicking the first line should put the caret in paragraph 0"
    );
}

#[test]
fn tables_and_lists_through_session() {
    let mut h = Harness::new();
    h.type_text("intro");
    {
        let s = h.app.session_mut();
        assert!(s.insert_table(2, 2));
    }
    h.frame(vec![]);
    h.type_text("a");
    h.key(Key::Tab, Modifiers::NONE);
    h.type_text("b");
    h.key(Key::Tab, Modifiers::NONE);
    h.type_text("c");
    // Down from the table into the paragraph below, then make it a list.
    h.key(Key::ArrowDown, Modifiers::NONE);
    h.key(Key::ArrowDown, Modifiers::NONE);
    h.type_text("item");
    h.app.session_mut().toggle_list(ListKind::Bullet);
    h.frame(vec![]);

    let doc = &h.app.session().doc;
    let t = doc.blocks.iter().find_map(Block::as_table).unwrap();
    assert_eq!(t.plain_text(), "a\tb\nc\t");
    let last = doc.blocks.last().unwrap().as_paragraph().unwrap();
    assert_eq!(last.plain_text(), "item");
    assert_eq!(last.style.list, Some(ListKind::Bullet));
}

#[test]
fn long_text_wraps_without_panicking() {
    let mut h = Harness::new();
    let long = "lorem ipsum dolor sit amet ".repeat(80);
    h.type_text(&long);
    h.key(Key::Home, Modifiers::NONE);
    // Home goes to the start of the *visual line*, which is not offset 0
    // because the paragraph wraps.
    assert!(h.app.session().caret().offset > 0);
    h.key(Key::Home, Modifiers::COMMAND);
    assert_eq!(h.app.session().caret().offset, 0);
}
