use gpui::*;
use gpui_component::*;

use crate::{
    main_view::MainView,
    services::{Services, run_engine_events_bus},
};

pub mod app_menu;
pub mod assets;
pub mod audio_settings;
pub mod cover_art_cache;
pub mod footer;
pub mod library_service;
pub mod library_views;
pub mod main_view;
pub mod media_bridge;
pub mod next_button;
pub mod now_playing;
pub mod play_button;
pub mod playback_queue;
pub mod prev_button;
pub mod services;
pub mod settings_store;
pub mod settings_view;
pub mod track_progress_slider;
pub mod volume;

fn main() {
    let app = Application::new().with_assets(assets::Assets);

    app.run(move |cx| {
        #[cfg(target_os = "macos")]
        macos_integration::app_icon::set_application_icon();

        gpui_component::init(cx);

        let settings_store = crate::settings_store::SettingsStore::load();
        crate::settings_store::apply_startup_theme(&settings_store, cx);
        cx.set_global(settings_store);

        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.0), px(400.0))),
            ..Default::default()
        };
        let services = Services::initialize(cx);

        let engine_manager = services.engine_manager.clone();
        let engine_event_bus = services.engine_event_bus.clone();
        cx.set_global(services);

        {
            let store = cx.global::<crate::settings_store::SettingsStore>();
            if let Some(folder) = store.music_folder().cloned() {
                let library = cx.global::<Services>().library.clone();
                if !library.has_tracks() {
                    library.clear_and_rescan(folder);
                }
            }
        }

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.on_app_quit(|cx| {
            cx.global::<Services>().shutdown();
            async {}
        })
        .detach();

        cx.on_action(|_: &crate::app_menu::Rescan, cx| {
            crate::settings_view::pick_folder_and_rescan(cx);
        });

        cx.on_action(|_: &crate::app_menu::Quit, cx| {
            cx.quit();
        });

        cx.activate(true);
        cx.set_menus(crate::app_menu::app_menus());

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| MainView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();

        cx.spawn(async move |cx| {
            run_engine_events_bus(cx, engine_manager, engine_event_bus).await;
        })
        .detach();
    });
}
