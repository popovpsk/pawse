#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use audio_output::AudioOutput;
use gpui::*;
use gpui_component::*;

use crate::{
    main_view::MainView,
    services::{Services, run_engine_events_bus},
};

pub mod app_menu;
pub mod audio_settings;
pub mod cover_art_cache;
pub mod footer;
pub mod library_service;
pub mod library_views;
pub mod localization;
pub mod main_view;
pub mod media_bridge;
pub mod next_button;
pub mod now_playing;
pub mod play_button;
pub mod playback_queue;
pub mod playlist_popup;
pub mod prev_button;
pub mod queue_view;
pub mod repeat_button;
pub mod services;
pub mod settings_store;
pub mod settings_view;
pub mod shuffle_button;
pub mod theme_colors;
pub mod track_list;
pub mod track_progress_slider;
pub mod volume;
pub mod window_title_bar;

fn restore_engine_state(cx: &mut App) {
    let stored_position_ms = cx
        .global::<crate::settings_store::SettingsStore>()
        .playback()
        .position_ms;
    let services = cx.global::<Services>();
    let queue = services.playback_queue.borrow();
    let Some(track) = queue.current_track().cloned() else {
        return;
    };
    drop(queue);

    let path = std::path::PathBuf::from(&track.path);
    let start_offset = if track.start_offset_ms > 0 {
        Some(std::time::Duration::from_millis(
            track.start_offset_ms as u64,
        ))
    } else {
        None
    };
    let duration = track
        .duration_ms
        .map(|ms| std::time::Duration::from_millis(ms as u64));
    services
        .engine_manager
        .set_track_with_offset(path, start_offset, duration);
    if stored_position_ms > 0
        && let Some(dur_ms) = track.duration_ms
        && dur_ms > 0
    {
        let ratio = (stored_position_ms as f32 / dur_ms as f32).clamp(0.0, 1.0);
        services
            .current_position_ms
            .store(stored_position_ms, std::sync::atomic::Ordering::Relaxed);
        services.engine_manager.seek(ratio);
    }
}

fn main() {
    let app = Application::new().with_assets(ui_resources::assets::Assets);

    app.run(move |cx| {
        gpui_component::init(cx);
        crate::playlist_popup::init(cx);

        let settings_store = crate::settings_store::SettingsStore::load();
        crate::settings_store::apply_startup_theme(&settings_store, cx);
        cx.set_global(settings_store);

        ui_resources::themes::register_bundled_themes(cx, |cx| {
            let choice = cx.global::<crate::settings_store::SettingsStore>().theme();
            if let crate::settings_store::ThemeChoice::Named(name) = choice {
                crate::settings_store::apply_named_theme(&name, cx);
            }
        });

        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.0), px(400.0))),
            titlebar: Some(TitleBar::title_bar_options()),
            app_id: Some("pawse".into()),
            #[cfg(target_os = "linux")]
            window_background: WindowBackgroundAppearance::Transparent,
            #[cfg(target_os = "linux")]
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        let services = Services::initialize(cx);

        let engine_manager = services.engine_manager.clone();
        let engine_event_bus = services.engine_event_bus.clone();
        let current_position_ms = services.current_position_ms.clone();
        let is_playing = services.is_playing.clone();
        cx.set_global(services);

        {
            let (stored, initial_volume) = {
                let store = cx.global::<crate::settings_store::SettingsStore>();
                (store.playback().clone(), store.volume())
            };
            let services = cx.global::<Services>();
            services.output.set_volume(initial_volume);
            {
                let mut queue = services.playback_queue.borrow_mut();
                queue.restore(
                    stored.queue.into_iter().map(std::rc::Rc::new).collect(),
                    stored
                        .original_queue
                        .map(|v| v.into_iter().map(std::rc::Rc::new).collect()),
                    stored.current_index,
                    stored.shuffle,
                    stored.repeat.into(),
                    stored.source.into(),
                );
            }
        }

        {
            let store = cx.global::<crate::settings_store::SettingsStore>();
            let folders = store.music_folders().to_vec();
            if !folders.is_empty() {
                let library = cx.global::<Services>().library.clone();
                if !library.has_tracks() {
                    library.clear_and_rescan(folders);
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
            let state = cx.global::<Services>().snapshot_playback();
            let _ = cx
                .global_mut::<crate::settings_store::SettingsStore>()
                .set_playback(state);
            cx.global::<Services>().shutdown();
            async {}
        })
        .detach();

        cx.on_action(|_: &crate::app_menu::Rescan, cx| {
            let folders = cx
                .global::<crate::settings_store::SettingsStore>()
                .music_folders()
                .to_vec();
            if !folders.is_empty() {
                cx.global::<Services>().library.clear_and_rescan(folders);
            }
        });

        cx.on_action(|_: &crate::app_menu::Quit, cx| {
            cx.quit();
        });

        cx.activate(true);
        crate::app_menu::set_menus(cx);

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| MainView::new(window, cx));
                let root = cx.new(|cx| Root::new(view, window, cx));
                restore_engine_state(cx);
                root
            })
            .expect("Failed to open window");
        })
        .detach();

        cx.spawn(async move |cx| {
            run_engine_events_bus(
                cx,
                engine_manager,
                engine_event_bus,
                current_position_ms,
                is_playing,
            )
            .await;
        })
        .detach();
    });
}
