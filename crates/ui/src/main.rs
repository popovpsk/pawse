use gpui::*;
use gpui_component::*;

use crate::{
    main_view::MainView,
    services::{run_engine_events_bus, Services},
};

pub mod footer;
pub mod main_view;
pub mod play_button;
pub mod services;
pub mod volume;

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(300.0), px(200.0))),
            ..Default::default()
        };
        let services = Services::initialize(cx);

        let engine_manager = services.engine_manager.clone();
        let engine_event_bus = services.engine_event_bus.clone();
        cx.set_global(services);

        cx.spawn(async move |cx| {
            run_engine_events_bus(cx, engine_manager, engine_event_bus).await;
        })
        .detach();

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| MainView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
