//! Renders the application UI to a PNG without a window or GPU.
//!
//! ```sh
//! cargo run --example screenshot -- out.png [document]
//! ```
//!
//! Without a document a built-in feature tour is shown. Useful for
//! documentation screenshots and for checking visual changes on headless
//! machines (e.g. CI). The rasterizer is a deliberately simple CPU
//! triangle filler: good enough to judge layout, not pixel-exact.

use std::collections::HashMap;
use std::io::Write;

use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{
    Color32, ColorImage, Context, Event, Key, Modifiers, RawInput, Rect, TextureId, pos2, vec2,
};
use rusttext::RustTextApp;
use rusttext::document::{Alignment, Color, ListKind, ToggleAttr};
use rusttext::fonts::FontManager;

const W: usize = 1200;
const H: usize = 860;

fn frame(ctx: &Context, app: &mut RustTextApp, events: Vec<Event>, t: f64) -> egui::FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(W as f32, H as f32),
        )),
        time: Some(t),
        events,
        ..Default::default()
    };
    ctx.run_ui(input, |ui| app.show(ui))
}

fn build_tour(app: &mut RustTextApp) {
    let s = app.session_mut();
    s.insert_text("RustText feature tour");
    s.set_heading(1);
    s.insert_paragraph_break();
    s.insert_text("Text can be ");
    let styled = |s: &mut rusttext::document::EditSession,
                  text: &str,
                  f: &dyn Fn(&mut rusttext::document::EditSession)| {
        f(s);
        s.insert_text(text);
        s.typing_style = None;
        let st = rusttext::document::CharStyle::default();
        s.typing_style = Some(st);
    };
    styled(s, "bold", &|s| s.toggle_attr(ToggleAttr::Bold));
    s.insert_text(", ");
    styled(s, "italic", &|s| s.toggle_attr(ToggleAttr::Italic));
    s.insert_text(", ");
    styled(s, "underlined", &|s| s.toggle_attr(ToggleAttr::Underline));
    s.insert_text(", ");
    styled(s, "struck through", &|s| {
        s.toggle_attr(ToggleAttr::Strikethrough)
    });
    s.insert_text(", ");
    styled(s, "red", &|s| {
        s.set_text_color(Some(Color::rgb(220, 30, 30)))
    });
    s.insert_text(", in a ");
    styled(s, "serif font", &|s| {
        s.set_font_family(Some("DejaVu Serif".into()))
    });
    s.insert_text(" or ");
    styled(s, "larger", &|s| s.set_font_size(18.0));
    s.insert_text(".");
    s.insert_paragraph_break();
    s.insert_text("This paragraph is centered.");
    s.set_alignment(Alignment::Center);
    s.insert_paragraph_break();
    s.set_alignment(Alignment::Right);
    s.insert_text("And this one is right aligned.");
    s.insert_paragraph_break();
    s.set_alignment(Alignment::Left);
    s.toggle_list(ListKind::Bullet);
    s.insert_text("Bullet one");
    s.insert_paragraph_break();
    s.insert_text("Bullet two");
    s.insert_paragraph_break();
    s.change_indent(1);
    s.insert_text("Nested bullet");
    s.insert_paragraph_break();
    s.change_indent(-1);
    s.toggle_list(ListKind::Numbered);
    s.insert_text("First step");
    s.insert_paragraph_break();
    s.insert_text("Second step");
    s.insert_paragraph_break();
    s.toggle_list(ListKind::Numbered);
    s.insert_table(3, 3);
    for (i, t) in [
        "Product", "Qty", "Price", "Apples", "3", "1.20", "Pears", "5", "2.10",
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            s.tab(false);
        }
        s.insert_text(t);
    }
    // Header row styling.
    let first = rusttext::document::TextPos::start_of(rusttext::document::ParaPath::cell(
        s.caret().path.block,
        0,
        0,
        0,
    ));
    let last = rusttext::document::TextPos::start_of(rusttext::document::ParaPath::cell(
        s.caret().path.block,
        0,
        2,
        0,
    ));
    s.select(first, last);
    s.set_cell_background(Some(Color::rgb(0, 112, 192)));
    s.set_cell_foreground(Some(Color::WHITE));
    s.toggle_attr(ToggleAttr::Bold);
    s.set_alignment(Alignment::Center);
    let c = s.doc.last_pos();
    s.move_to(c, false);
    s.insert_text("Tables support cell colors, alignment and formatting.");
}

/// Minimal PNG encoder (uncompressed deflate blocks).
fn write_png(path: &str, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for row in rgba.chunks(w * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&png)
}

fn rasterize(prims: &[ClippedPrimitive], textures: &HashMap<TextureId, ColorImage>) -> Vec<u8> {
    let mut px = vec![[0f32; 4]; W * H];
    for p in prims {
        let Primitive::Mesh(mesh) = &p.primitive else {
            continue;
        };
        let tex = textures.get(&mesh.texture_id);
        let clip = p.clip_rect.intersect(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(W as f32, H as f32),
        ));
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v = [
                &mesh.vertices[tri[0] as usize],
                &mesh.vertices[tri[1] as usize],
                &mesh.vertices[tri[2] as usize],
            ];
            let (a, b, c) = (v[0].pos, v[1].pos, v[2].pos);
            let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
            if area.abs() < 1e-6 {
                continue;
            }
            let minx = a.x.min(b.x).min(c.x).max(clip.min.x).floor() as i32;
            let maxx = a.x.max(b.x).max(c.x).min(clip.max.x).ceil() as i32;
            let miny = a.y.min(b.y).min(c.y).max(clip.min.y).floor() as i32;
            let maxy = a.y.max(b.y).max(c.y).min(clip.max.y).ceil() as i32;
            for y in miny.max(0)..maxy.min(H as i32) {
                for x in minx.max(0)..maxx.min(W as i32) {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.x - fx) * (c.y - fy) - (c.x - fx) * (b.y - fy)) / area;
                    let w1 = ((c.x - fx) * (a.y - fy) - (a.x - fx) * (c.y - fy)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let lerp = |f: &dyn Fn(&egui::epaint::Vertex) -> f32| {
                        w0 * f(v[0]) + w1 * f(v[1]) + w2 * f(v[2])
                    };
                    let mut src = [0f32; 4];
                    for (k, s) in src.iter_mut().enumerate() {
                        *s = lerp(&|vx| vx.color.to_array()[k] as f32 / 255.0);
                    }
                    if let Some(t) = tex {
                        let u = (lerp(&|vx| vx.uv.x) * t.size[0] as f32) as usize;
                        let vv = (lerp(&|vx| vx.uv.y) * t.size[1] as f32) as usize;
                        let tc = t.pixels[vv.min(t.size[1] - 1) * t.size[0] + u.min(t.size[0] - 1)]
                            .to_array();
                        for k in 0..4 {
                            src[k] *= tc[k] as f32 / 255.0;
                        }
                    }
                    let d = &mut px[y as usize * W + x as usize];
                    for k in 0..4 {
                        d[k] = src[k] + d[k] * (1.0 - src[3]);
                    }
                }
            }
        }
    }
    px.iter()
        .flat_map(|p| {
            let a = p[3].max(1e-6);
            [
                (p[0] / a * 255.0) as u8,
                (p[1] / a * 255.0) as u8,
                (p[2] / a * 255.0) as u8,
                255,
            ]
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "screenshot.png".into());
    let doc = args.next().filter(|d| d != "-");
    let select = args.next().is_some_and(|a| a == "select");

    let ctx = Context::default();
    let mut app = RustTextApp::with_fonts(&ctx, FontManager::new(), doc.map(Into::into));
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    let mut last = None;
    for i in 0..6 {
        if i == 1 && std::env::args().nth(2).is_none_or(|d| d == "-") {
            build_tour(&mut app);
        }
        if i == 5 && select {
            // Demonstrate selection rendering: from mid-paragraph into the table.
            let s = app.session_mut();
            let a = rusttext::document::TextPos::new(rusttext::document::ParaPath::top(1), 5);
            let b = rusttext::document::TextPos::new(rusttext::document::ParaPath::top(6), 4);
            s.select(a, b);
        }
        let events = if i == 4 {
            vec![Event::Key {
                key: Key::End,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }]
        } else {
            vec![]
        };
        let mut o = frame(&ctx, &mut app, events, i as f64 * 0.1);
        for (id, deltas) in &o.textures_delta.set {
            for delta in deltas {
                let egui::ImageData::Color(img) = &delta.image;
                match delta.pos {
                    None => {
                        textures.insert(*id, img.as_ref().clone());
                    }
                    Some([x0, y0]) => {
                        let t = textures.get_mut(id).expect("patch for known texture");
                        for y in 0..img.size[1] {
                            for x in 0..img.size[0] {
                                t.pixels[(y0 + y) * t.size[0] + x0 + x] =
                                    img.pixels[y * img.size[0] + x];
                            }
                        }
                    }
                }
            }
        }
        o.textures_delta.clear();
        last = Some(o);
    }
    let o = last.expect("rendered");
    let prims = ctx.tessellate(o.shapes, o.pixels_per_point);
    let rgba = rasterize(&prims, &textures);
    write_png(&out, W, H, &rgba).expect("write png");
    let _ = Color32::WHITE;
    println!("wrote {out}");
}
