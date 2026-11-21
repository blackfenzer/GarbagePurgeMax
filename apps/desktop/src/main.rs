mod assets;
mod file_icons;
mod workspace;

use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use workspace::Workspace;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,storage_clinic=info"),
    )
    .init();

    // `tests/ui_smoke.rs` starts the binary this way: the macOS platform must be created on
    // the main thread, which the test harness cannot offer.
    #[cfg(feature = "ui-smoke")]
    if let Some(shots) = std::env::var_os("STORAGE_CLINIC_UI_SMOKE") {
        if let Err(error) = workspace::ui_smoke::run(std::path::Path::new(&shots)) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("UI smoke run passed");
        return;
    }

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // Single-window app: without this, closing the window leaves the process running,
            // and a relaunch becomes a second instance competing for the scan database.
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            let bounds = Bounds::centered(None, size(px(1100.0), px(700.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.0), px(520.0))),
                ..WindowOptions::default()
            };

            match gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Storage Clinic");
                cx.new(|cx| Workspace::new(window, cx))
            }) {
                Ok(_) => {
                    log::info!("Opened Storage Clinic");
                    cx.activate(true);
                }
                Err(error) => {
                    log::error!("Could not open Storage Clinic: {error}");
                    cx.quit();
                }
            }
        });
}
