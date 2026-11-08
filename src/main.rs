//! RustText executable entry point.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

fn print_help() {
    println!(
        "RustText {}\n\nUSAGE:\n    rusttext [FILE]\n\nSupported formats: .docx .rtf .md .txt\n\nOPTIONS:\n    -h, --help       Print help\n    -V, --version    Print version",
        env!("CARGO_PKG_VERSION")
    );
}

/// A simple generated window icon (a page with text lines).
fn app_icon() -> egui::IconData {
    const N: u32 = 64;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let i = ((y * N + x) * 4) as usize;
            let page = (10..54).contains(&x) && (4..60).contains(&y);
            let line = page && (16..48).contains(&x) && (14..52).contains(&y) && (y - 14) % 8 < 3;
            let short = line && y >= 46 && x > 36;
            let c: [u8; 4] = if line && !short {
                [28, 144, 237, 255]
            } else if page {
                [250, 250, 250, 255]
            } else {
                [0, 0, 0, 0]
            };
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    egui::IconData {
        rgba,
        width: N,
        height: N,
    }
}

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut path: Option<PathBuf> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return std::process::ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("rusttext {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => {}
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustText")
            .with_app_id("rusttext")
            .with_inner_size([1150.0, 820.0])
            .with_min_inner_size([520.0, 360.0])
            .with_icon(Arc::new(app_icon())),
        ..Default::default()
    };
    let result = eframe::run_native(
        "RustText",
        options,
        Box::new(move |cc| Ok(Box::new(rusttext::RustTextApp::new(cc, path)))),
    );
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("RustText could not start: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
