// egui 0.37 deprecated `Id::new` in favour of salt types; the hashed-id usage here is still correct.
#![allow(deprecated)]

mod app;
mod cmds;
mod config;
mod cursor_fx;
mod editor;
mod fonts;
mod hot;
mod icons;
mod reader;
mod search;
mod theme;
mod ui;
mod ui_editor;
mod ui_overlay;
mod ui_side;
mod util;
mod vault;

use eframe::egui;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Carbon")
            .with_app_id("carbon")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_transparent(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Carbon", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
