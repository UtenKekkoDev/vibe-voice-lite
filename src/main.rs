#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod audio;
mod config;
mod credentials;
mod holdkey;
mod icons;
mod qwen_asr;
mod sensevoice;
mod services;
mod settings;
mod single_instance;
mod speech;
mod state;
mod task;
mod tray;
mod ui;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, WindowLevel};
use std::fs;
use std::sync::Arc;

const WINDOW_TITLE: &str = ui::WINDOW_TITLE;
const FONT_CANDIDATES: [&str; 3] = [
    r"c:\Windows\Fonts\msyh.ttc",
    r"c:\Windows\Fonts\msyhl.ttc",
    r"c:\Windows\Fonts\simhei.ttf",
];

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    for path in FONT_CANDIDATES {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };

        fonts.font_data.insert(
            "windows_cjk".to_string(),
            Arc::new(FontData::from_owned(bytes)),
        );

        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "windows_cjk".to_string());
        }
        if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
            family.insert(0, "windows_cjk".to_string());
        }

        ctx.set_fonts(fonts);
        return;
    }
}

fn main() -> eframe::Result<()> {
    config::lifecycle("start interview-serena-2");
    let Some(_instance) = single_instance::InstanceGuard::acquire() else {
        return Ok(());
    };

    // The whole product is this one bar: no main window, no taskbar entry.
    let viewport = egui::ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
        .with_icon(icons::window())
        .with_inner_size(ui::BAR_SIZE)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_active(false)
        .with_window_level(WindowLevel::AlwaysOnTop);

    let options = eframe::NativeOptions {
        // glow cannot composite transparent windows on Windows; wgpu can.
        renderer: eframe::Renderer::Wgpu,
        viewport,
        ..Default::default()
    };

    let result = eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(ui::VoiceLiteApp::new(&cc.egui_ctx)))
        }),
    );
    config::lifecycle("event loop returned");
    result
}
