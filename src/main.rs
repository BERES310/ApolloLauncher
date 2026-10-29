#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod config;
mod icons;
mod launch;
mod model;
mod scanner;
mod ui;
mod util;
mod watcher;

use eframe::egui;

const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.ico");

fn window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON_BYTES).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn main() {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ApolloLauncher")
        .with_inner_size([1100.0, 700.0])
        .with_min_inner_size([480.0, 360.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    // A window that cannot be created has nowhere to report the failure; exit quietly.
    let _ = eframe::run_native(
        "ApolloLauncher",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    );
}
