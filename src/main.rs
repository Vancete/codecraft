// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::Ordering;

use codecraft::{app, single_instance};

fn main() -> eframe::Result {
    let open_arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    // A file opened while CodeCraft runs goes to a tab of that window.
    if let Some(path) = open_arg.as_deref()
        && path.is_file()
        && single_instance::forward(path)
    {
        return Ok(());
    }
    app::preload();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CodeCraft")
            // Matches a codecraft.desktop, so Linux desktops show its icon and group its windows.
            .with_app_id("codecraft")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true)
            .with_icon(app::window_icon()),
        // Runs after eframe restored the last session's size and position.
        window_builder: Some(Box::new(|builder| {
            if cfg!(windows) && builder.maximized == Some(true) {
                app::MAXIMIZE_WHEN_SHOWN.store(true, Ordering::Relaxed);
                builder.with_maximized(false)
            } else {
                builder
            }
        })),
        ..Default::default()
    };
    eframe::run_native(
        "codecraft",
        options,
        Box::new(|cc| {
            let mut app = app::App::new(cc, open_arg);
            app.take_files_from(single_instance::listen(cc.egui_ctx.clone()));
            Ok(Box::new(app))
        }),
    )
}
