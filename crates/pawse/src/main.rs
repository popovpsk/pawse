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
pub mod cover_mode_view;
pub mod cover_volume;
pub mod error_bridge;
pub mod footer;
pub mod keyboard_shortcuts;
pub mod library_service;
pub mod library_views;
pub mod library_watcher;
pub mod localization;
pub mod main_view;
pub mod media_bridge;
pub mod next_button;
pub mod now_playing;
pub mod onboarding_view;
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
#[cfg(not(target_os = "macos"))]
pub mod single_instance;
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

fn build_window_options(cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.0), px(400.0))),
        titlebar: Some(TitleBar::title_bar_options()),
        app_id: Some("pawse".into()),
        #[cfg(target_os = "linux")]
        window_background: WindowBackgroundAppearance::Transparent,
        #[cfg(target_os = "linux")]
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}

fn open_main_window(cx: &mut App, run_startup_tasks: bool) {
    let options = build_window_options(cx);
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| MainView::new(window, cx));
        let root = cx.new(|cx| Root::new(view, window, cx));
        if run_startup_tasks {
            restore_engine_state(cx);
            window.on_next_frame(|_window, cx| {
                let folders = cx
                    .global::<crate::settings_store::SettingsStore>()
                    .music_folders()
                    .to_vec();
                if !folders.is_empty() {
                    cx.global::<Services>().library.clear_and_rescan(folders);
                }
                crate::library_watcher::rebuild(cx);
            });
        }
        root
    })
    .expect("Failed to open window");
}

pub fn open_onboarding_window(cx: &mut App) {
    let options = build_window_options(cx);
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| crate::onboarding_view::OnboardingView::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("Failed to open onboarding window");
}

fn open_initial_window(cx: &mut App, run_startup_tasks: bool) {
    if cx
        .global::<crate::settings_store::SettingsStore>()
        .onboarding_complete()
    {
        open_main_window(cx, run_startup_tasks);
    } else {
        open_onboarding_window(cx);
    }
}

fn main() {
    #[cfg(not(target_os = "macos"))]
    let single_instance = match single_instance::acquire() {
        single_instance::Acquire::Duplicate => return,
        single_instance::Acquire::First(listener) => listener,
    };

    let app = Application::new().with_assets(ui_resources::assets::Assets);

    #[cfg(target_os = "macos")]
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_initial_window(cx, false);
        }
    });

    app.run(move |cx| {
        let log_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("pawse")
            .join("logs");
        let notices = diagnostics::init(diagnostics::Config {
            log_dir,
            ..Default::default()
        });

        gpui_component::init(cx);
        crate::playlist_popup::init(cx);
        crate::keyboard_shortcuts::init(cx);
        crate::error_bridge::spawn_notice_forwarder(cx, notices);

        let settings_store = crate::settings_store::SettingsStore::load();
        crate::settings_store::apply_startup_theme(&settings_store, cx);
        cx.set_global(settings_store);
        let save_executor = cx.background_executor().clone();
        cx.global_mut::<crate::settings_store::SettingsStore>()
            .start_background_writer(save_executor);
        crate::localization::sync_active_lang(cx);

        #[cfg(not(target_os = "linux"))]
        {
            let auto_update_enabled = cx
                .global::<crate::settings_store::SettingsStore>()
                .auto_update();
            updater::init(cx, env!("CARGO_PKG_VERSION"), auto_update_enabled);
        }

        ui_resources::themes::register_bundled_themes(cx, |cx| {
            let choice = cx.global::<crate::settings_store::SettingsStore>().theme();
            if let crate::settings_store::ThemeChoice::Named(name) = choice {
                crate::settings_store::apply_named_theme(&name, cx);
            }
        });

        let services = Services::initialize(cx);

        let engine_manager = services.engine_manager.clone();
        let engine_event_bus = services.engine_event_bus.clone();
        let current_position_ms = services.current_position_ms.clone();
        let current_duration_ms = services.current_duration_ms.clone();
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
                queue.restore(crate::playback_queue::QueueRestore {
                    tracks: stored.queue.into_iter().map(std::rc::Rc::new).collect(),
                    original_order: stored
                        .original_queue
                        .map(|v| v.into_iter().map(std::rc::Rc::new).collect()),
                    current_index: stored.current_index,
                    shuffle: stored.shuffle,
                    repeat: stored.repeat.into(),
                    source: stored.source.into(),
                    custom: stored.custom,
                });
            }
        }

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                #[cfg(not(target_os = "macos"))]
                cx.quit();
            }
        })
        .detach();

        cx.on_app_quit(|cx| {
            let state = cx.global::<Services>().snapshot_playback();
            let _ = cx
                .global_mut::<crate::settings_store::SettingsStore>()
                .save_playback_blocking(state);
            cx.global::<Services>().shutdown();
            diagnostics::flush();
            async {}
        })
        .detach();

        cx.on_action(|_: &crate::app_menu::Rescan, cx| {
            let folders = cx
                .global::<crate::settings_store::SettingsStore>()
                .music_folders()
                .to_vec();
            if !folders.is_empty() {
                cx.global::<Services>()
                    .library
                    .request_rescan(folders, true, true);
            }
        });

        cx.on_action(|_: &crate::app_menu::Quit, cx| {
            cx.quit();
        });

        #[cfg(target_os = "macos")]
        cx.on_action(|_: &crate::app_menu::Hide, cx| cx.hide());
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &crate::app_menu::HideOthers, cx| cx.hide_other_apps());
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &crate::app_menu::ShowAll, cx| cx.unhide_other_apps());

        #[cfg(target_os = "macos")]
        cx.on_action(|_: &crate::app_menu::Minimize, cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.minimize_window());
            }
        });
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &crate::app_menu::Zoom, cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.zoom_window());
            }
        });
        cx.on_action(|_: &crate::app_menu::OpenRepository, cx| {
            cx.open_url(crate::app_menu::REPOSITORY_URL);
        });

        #[cfg(not(target_os = "linux"))]
        cx.on_action(|_: &updater::CheckForUpdates, cx| updater::check_now(cx));

        cx.activate(true);
        crate::app_menu::set_menus(cx);

        open_initial_window(cx, true);

        #[cfg(not(target_os = "macos"))]
        single_instance::install(cx, single_instance);

        #[cfg(target_os = "macos")]
        crate::media_bridge::setup(cx);

        cx.spawn(async move |cx| {
            run_engine_events_bus(
                cx,
                engine_manager,
                engine_event_bus,
                current_position_ms,
                current_duration_ms,
                is_playing,
            )
            .await;
        })
        .detach();
    });
}
