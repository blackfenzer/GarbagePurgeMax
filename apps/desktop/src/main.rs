mod workspace;

use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use workspace::Workspace;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,storage_clinic=info"),
    )
    .init();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
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
