#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window on Windows

mod audio;
mod icon;
mod notes;
mod pitch;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Bass Tuner")
            .with_inner_size([720.0, 540.0])
            .with_min_inner_size([520.0, 420.0])
            .with_icon(icon::app_icon(512)),
        ..Default::default()
    };
    eframe::run_native(
        "Bass Tuner",
        options,
        Box::new(|cc| Ok(Box::new(ui::TunerApp::new(cc)))),
    )
}
