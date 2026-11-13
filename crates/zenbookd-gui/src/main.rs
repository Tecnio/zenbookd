mod controls;
mod dashboard;
mod link;
mod readout;
mod theme;

use gpui::{
    App, AppContext, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_platform::application;

use crate::dashboard::{Dashboard, Quit};

const WINDOW_WIDTH: f32 = 420.0;
const WINDOW_HEIGHT: f32 = 760.0;
const MIN_WIDTH: f32 = 360.0;
const MIN_HEIGHT: f32 = 420.0;
const APP_ID: &str = "zenbookd";

fn main() {
    application().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-w", Quit, None),
        ]);

        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(APP_ID.into()),
                ..TitlebarOptions::default()
            }),
            app_id: Some(APP_ID.to_string()),
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            ..WindowOptions::default()
        };

        match cx.open_window(options, |window, cx| {
            cx.new(|cx| Dashboard::new(window, cx))
        }) {
            Ok(_) => cx.activate(true),

            Err(err) => {
                eprintln!("zenbookd-gui: could not open a window: {err:#}");
                cx.quit();
            }
        }
    });
}
