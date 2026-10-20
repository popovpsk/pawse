use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext, Context, DispatchPhase,
    DragMoveEvent, Empty, Entity, EntityId, FocusHandle, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    Pixels, Render, StatefulInteractiveElement, StyleRefinement, Styled, Subscription, Window,
    canvas, div, ease_out_quint, px, svg,
};
use gpui_component::{
    StyledExt,
    input::{InputEvent, InputState},
    slider::{SliderEvent, SliderState},
    theme::ThemeRegistry,
    tooltip::Tooltip,
};

mod header;
mod stage;

use crate::audio_settings::AudioSettings;
use crate::cover_backdrop::{self, CoverBackdrop};
use crate::cover_mode_view::CoverModeView;
use crate::cover_skin::CoverSkin;
use crate::cover_volume::CoverVolume;
use crate::footer::{Footer, OpenSleepTimerEvent, ToggleLyricsEvent, ToggleQueueEvent};
use crate::keyboard_shortcuts::{
    ExitCoverMode, NextTrack, PlayPause, PreviousTrack, SeekBackward, SeekForward, VolumeDown,
    VolumeUp,
};
use crate::library_service::LibraryEvent;
use crate::library_views::library_view::{LibraryRootTab, LibraryView, LibraryViewEvent};
use crate::library_views::view_menu::ViewMenuTab;
use crate::localization::LangChanged;
use crate::localization::tr;
use crate::lyrics_view::LyricsView;
#[cfg(not(target_os = "macos"))]
use crate::media_bridge::MediaBridge;
use crate::now_playing::{
    NavigateToAlbumRequested, NavigateToArtistRequested, ToggleCoverModeRequested,
};
use crate::playlist_popup::PlaylistPopup;
use crate::queue_view::QueueView;
use crate::scrobble_settings::{ScrobbleInputs, ScrobbleUiState};
use crate::settings_store::{BlurBackground, SettingsStore, ui_scale};
use crate::settings_view::SettingsSliders;
use crate::theme_colors::Colors;

const FOOTER_HEIGHT: f32 = 80.;
const QUEUE_WIDTH_DEFAULT: f32 = 360.;
const QUEUE_WIDTH_MIN: f32 = 280.;
const QUEUE_WIDTH_MAX: f32 = 560.;
const LYRICS_WIDTH_DEFAULT: f32 = 360.;
const LYRICS_WIDTH_MIN: f32 = 280.;
const LYRICS_WIDTH_MAX: f32 = 560.;
const QUEUE_ANIM: Duration = Duration::from_millis(200);
const PANEL_BORDER: f32 = 1.;

pub(crate) fn footer_height(cx: &App) -> f32 {
    FOOTER_HEIGHT * ui_scale(cx)
}

#[derive(Clone, Copy, PartialEq)]
struct TabColors {
    active_bg: Hsla,
    hover_bg: Hsla,
    primary: Hsla,
    foreground: Hsla,
}

#[derive(Clone)]
struct DragQueueResize(EntityId);

impl Render for DragQueueResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
struct DragLyricsResize(EntityId);

impl Render for DragLyricsResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct MainView {
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    queue_view: Entity<QueueView>,
    lyrics_view: Entity<LyricsView>,
    playlist_popup: Entity<PlaylistPopup>,
    is_drilled_in: bool,
    current_tab: LibraryRootTab,
    view_menu_tab: Option<ViewMenuTab>,
    show_settings: bool,
    show_tools: bool,
    tools_view: Entity<crate::tools::ToolsView>,
    cover_mode: bool,
    cover_mode_view: Entity<CoverModeView>,
    cover_backdrop: Entity<CoverBackdrop>,
    cover_volume: Entity<CoverVolume>,
    _cover_skin: Entity<CoverSkin>,
    show_queue: bool,
    queue_width: f32,
    queue_resize_origin: Option<(Pixels, f32)>,
    queue_closing: bool,
    _queue_close_task: Option<gpui::Task<()>>,
    show_lyrics: bool,
    lyrics_width: f32,
    lyrics_resize_origin: Option<(Pixels, f32)>,
    lyrics_closing: bool,
    _lyrics_close_task: Option<gpui::Task<()>>,
    _lyrics_slider: Entity<SliderState>,
    _lyrics_slider_observe: Subscription,
    _lyrics_slider_subscription: Subscription,
    _blur_intensity_slider: Entity<SliderState>,
    _blur_intensity_slider_observe: Subscription,
    _blur_intensity_slider_subscription: Subscription,
    _blur_interface_opacity_slider: Entity<SliderState>,
    _blur_interface_opacity_slider_observe: Subscription,
    _blur_interface_opacity_slider_subscription: Subscription,
    settings_screen: Entity<crate::settings_view::SettingsScreen>,
    _screens_observe: Subscription,
    search_input: Entity<InputState>,
    _remote_port_input: Entity<InputState>,
    _remote_port_subscription: Subscription,
    _scrobble_ui: Entity<ScrobbleUiState>,
    library_sources: Entity<crate::library_sources::LibrarySources>,
    _library_sources_observe: Subscription,
    _scrobble_ui_observe: Subscription,
    _scrobble_rules_observe: Subscription,
    _scrobble_imports_subscription: Subscription,
    _scrobble_inputs: ScrobbleInputs,
    _scrobble_token_subscription: Subscription,
    _scrobble_status_observe: Option<Subscription>,
    #[cfg(not(target_os = "macos"))]
    _media_bridge: Entity<MediaBridge>,
    _library_subscription: Subscription,
    _scan_subscription: Subscription,
    _search_subscription: Subscription,
    _footer_subscription: Subscription,
    _footer_lyrics_subscription: Subscription,
    _footer_sleep_timer_subscription: Subscription,
    _footer_album_subscription: Subscription,
    _footer_artist_subscription: Subscription,
    _footer_cover_subscription: Subscription,
    _cover_album_subscription: Subscription,
    _cover_artist_subscription: Subscription,
    _cover_observe_subscription: Subscription,
    _cover_lyrics_toggle_subscription: Subscription,
    _cover_queue_toggle_subscription: Subscription,
    _cover_backdrop_observe: Subscription,
    _shuffle_subscription: gpui::Subscription,
    _theme_registry_subscription: gpui::Subscription,
    _settings_observer: gpui::Subscription,
    _lang_subscription: Subscription,
    _activation_subscription: gpui::Subscription,
    _window_bounds_subscription: gpui::Subscription,
    updater: Option<Entity<updater::AutoUpdater>>,
    _updater_observer: Option<gpui::Subscription>,
    _sleep_timer_controls_observe: Subscription,
    header: Entity<header::HeaderHost>,
    #[cfg(target_os = "macos")]
    traffic_light: (f32, f32),
    focus_handle: FocusHandle,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_view = cx.new(|cx| LibraryView::new(window, cx));
        let view_menu_tab = library_view.read(cx).view_menu_tab();

        let library_subscription = cx.subscribe_in(
            &library_view,
            window,
            move |this: &mut MainView, _, event: &LibraryViewEvent, window, cx| match event {
                LibraryViewEvent::StateChanged => {
                    let view = this.library_view.read(cx);
                    this.is_drilled_in = view.is_drilled_in();
                    this.view_menu_tab = view.view_menu_tab();
                    if let Some(tab) = view.current_tab() {
                        this.current_tab = tab;
                    }
                    this.clear_search(window, cx);
                    cx.notify();
                }
                LibraryViewEvent::OpenLibrarySettings => {
                    let page = this.settings_screen.read(cx).pages().library;
                    this.open_settings(page, window, cx);
                }
            },
        );

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().search_placeholder.clone()));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let search_subscription = cx.subscribe_in(&search_input, window, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let query = this.search_input.read(cx).value().to_string();
                    library_view.update(cx, |v, cx| v.apply_search(&query, cx));
                }
                InputEvent::Blur
                    // Only reclaim focus for keyboard shortcuts when nothing
                    // else took it; otherwise we'd steal focus from a popup or
                    // picker opened while the search box was focused.
                    if window.focused(cx).is_none() => {
                        this.focus_handle.focus(window, cx);
                    }
                _ => {}
            }
        });

        // The search placeholder is set imperatively on the input, so a
        // language change must re-set it (a plain repaint won't).
        let lang_event_bus = cx
            .global::<crate::services::Services>()
            .lang_event_bus
            .clone();
        let lang_subscription = cx.subscribe_in(
            &lang_event_bus,
            window,
            |this, _, _: &LangChanged, window, cx| {
                this.search_input.update(cx, |input, cx| {
                    input.set_placeholder(tr().search_placeholder.clone(), window, cx);
                });
            },
        );

        let saved_lyrics_size = cx.global::<SettingsStore>().lyrics_font_size();
        let lyrics_slider: Entity<SliderState> = cx.new(|_| {
            SliderState::new()
                .min(crate::settings_store::LYRICS_FONT_SIZE_MIN)
                .max(crate::settings_store::LYRICS_FONT_SIZE_MAX)
                .step(1.)
                .default_value(saved_lyrics_size)
        });
        let lyrics_slider_observe = cx.observe(&lyrics_slider, |_, _, cx| cx.notify());
        let lyrics_slider_subscription =
            cx.subscribe(&lyrics_slider, |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                if let Err(e) = cx
                    .global_mut::<SettingsStore>()
                    .set_lyrics_font_size(value.start())
                {
                    crate::settings_store::notify_save_error(cx, e);
                }
                this.lyrics_view.update(cx, |_, cx| cx.notify());
            });

        let saved_blur_intensity = cx.global::<SettingsStore>().blur_intensity();
        let blur_intensity_slider: Entity<SliderState> = cx.new(|_| {
            SliderState::new()
                .min(crate::settings_store::BLUR_INTENSITY_MIN)
                .max(crate::settings_store::BLUR_INTENSITY_MAX)
                .step(1.)
                .default_value(saved_blur_intensity)
        });
        let blur_intensity_slider_observe =
            cx.observe(&blur_intensity_slider, |_, _, cx| cx.notify());
        let blur_intensity_slider_subscription =
            cx.subscribe(&blur_intensity_slider, |_, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                if let Err(e) = cx
                    .global_mut::<SettingsStore>()
                    .set_blur_intensity(value.start())
                {
                    crate::settings_store::notify_save_error(cx, e);
                }
            });

        let saved_blur_interface_opacity = cx.global::<SettingsStore>().blur_interface_opacity();
        let blur_interface_opacity_slider: Entity<SliderState> = cx.new(|_| {
            SliderState::new()
                .min(crate::settings_store::BLUR_INTERFACE_OPACITY_MIN)
                .max(crate::settings_store::BLUR_INTERFACE_OPACITY_MAX)
                .step(1.)
                .default_value(saved_blur_interface_opacity)
        });
        let blur_interface_opacity_slider_observe =
            cx.observe(&blur_interface_opacity_slider, |_, _, cx| cx.notify());
        let blur_interface_opacity_slider_subscription = cx.subscribe(
            &blur_interface_opacity_slider,
            |_, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                if let Err(e) = cx
                    .global_mut::<SettingsStore>()
                    .set_blur_interface_opacity(value.start())
                {
                    crate::settings_store::notify_save_error(cx, e);
                }
            },
        );

        let remote_port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(cx.global::<SettingsStore>().remote_port().to_string())
                .validate(|s, _| s.parse::<u16>().is_ok())
        });
        let remote_port_subscription = cx.subscribe_in(
            &remote_port_input,
            window,
            |_, input, ev: &InputEvent, window, cx| {
                if !matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    return;
                }
                let stored = cx.global::<SettingsStore>().remote_port();
                let raw = input.read(cx).value();
                match raw.trim().parse::<u16>() {
                    Ok(port) if port != 0 => {
                        if port != stored {
                            if let Err(e) = cx.global_mut::<SettingsStore>().set_remote_port(port) {
                                crate::settings_store::notify_save_error(cx, e);
                            } else {
                                crate::services::apply_remote_state(cx);
                            }
                        }
                    }
                    _ => {
                        input.update(cx, |s, cx| s.set_value(stored.to_string(), window, cx));
                    }
                }
            },
        );

        let library_sources = cx.new(crate::library_sources::LibrarySources::new);
        let library_page = crate::settings_view::LibraryPage {
            sources: library_sources.clone(),
            subsonic_inputs: crate::remote_settings::ServerInputs::new(window, cx),
            jellyfin_inputs: crate::remote_settings::ServerInputs::new(window, cx),
            dlna_address: crate::dlna_settings::address_input(window, cx),
            torrent_magnet: crate::torrent_settings::magnet_input(window, cx),
        };
        let library_sources_observe = cx.observe(&library_sources, |_, _, cx| cx.notify());
        let scrobble_ui: Entity<ScrobbleUiState> = cx.new(|_| ScrobbleUiState::new());
        let scrobble_ui_observe = cx.observe(&scrobble_ui, |_, _, cx| cx.notify());
        let scrobble_imports_subscription =
            crate::scrobble_settings::watch_server_imports(scrobble_ui.clone(), cx);
        let scrobble_rules =
            cx.new(|cx| crate::scrobble_rules_settings::ScrobbleRules::new(window, cx));
        let scrobble_rules_observe = cx.observe(&scrobble_rules, |_, _, cx| cx.notify());
        let scrobble_inputs = ScrobbleInputs {
            rules: scrobble_rules,
            token: cx.new(|cx| InputState::new(window, cx).masked(true)),
            api_root: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(scrobble::LISTENBRAINZ_ROOT)
                    .default_value(
                        cx.global::<SettingsStore>()
                            .scrobble()
                            .listenbrainz
                            .api_root
                            .clone(),
                    )
            }),
        };
        let scrobble_token_subscription = cx.subscribe_in(&scrobble_inputs.token, window, {
            let scrobble_ui = scrobble_ui.clone();
            let scrobble_inputs = scrobble_inputs.clone();
            move |_, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    crate::scrobble_settings::connect_listenbrainz_from_inputs(
                        cx,
                        scrobble_ui.clone(),
                        &scrobble_inputs,
                    );
                }
                _ => {}
            }
        });
        let scrobble_status = cx
            .try_global::<crate::scrobble_bridge::ScrobbleService>()
            .map(|service| service.status());
        let scrobble_status_observe =
            scrobble_status.map(|status| cx.observe(&status, |_, _, cx| cx.notify()));

        let sleep_timer_controls =
            cx.new(|cx| crate::sleep_timer::controls::SleepTimerControls::new(window, cx));
        let sleep_timer_controls_observe =
            cx.observe(&sleep_timer_controls, |_, _, cx| cx.notify());

        let theme_registry_subscription = cx.observe_global::<ThemeRegistry>({
            let lyrics_slider = lyrics_slider.clone();
            let blur_intensity_slider = blur_intensity_slider.clone();
            let blur_interface_opacity_slider = blur_interface_opacity_slider.clone();
            let remote_port_input = remote_port_input.clone();
            let scrobble_ui = scrobble_ui.clone();
            let scrobble_inputs = scrobble_inputs.clone();
            let library_page = library_page.clone();
            let sleep_timer_controls = sleep_timer_controls.clone();
            move |this, cx| {
                let pages = crate::settings_view::build_settings_pages(
                    SettingsSliders {
                        lyrics: lyrics_slider.clone(),
                        blur_intensity: blur_intensity_slider.clone(),
                        blur_interface_opacity: blur_interface_opacity_slider.clone(),
                    },
                    remote_port_input.clone(),
                    scrobble_ui.clone(),
                    scrobble_inputs.clone(),
                    library_page.clone(),
                    sleep_timer_controls.clone(),
                    cx,
                );
                this.settings_screen
                    .update(cx, |screen, _| screen.set_pages(pages));
                cx.notify();
            }
        });

        let settings_pages = crate::settings_view::build_settings_pages(
            SettingsSliders {
                lyrics: lyrics_slider.clone(),
                blur_intensity: blur_intensity_slider.clone(),
                blur_interface_opacity: blur_interface_opacity_slider.clone(),
            },
            remote_port_input.clone(),
            scrobble_ui.clone(),
            scrobble_inputs.clone(),
            library_page.clone(),
            sleep_timer_controls.clone(),
            cx,
        );

        let footer = cx.new(|cx| Footer::new(window, cx));
        let footer_subscription = cx.subscribe(&footer, |this, _, event: &ToggleQueueEvent, cx| {
            this.set_queue_visible(event.show, cx);
        });
        let footer_lyrics_subscription =
            cx.subscribe(&footer, |this, _, event: &ToggleLyricsEvent, cx| {
                this.set_lyrics_visible(event.show, cx);
            });

        let footer_sleep_timer_subscription = cx.subscribe_in(
            &footer,
            window,
            |this, _, _: &OpenSleepTimerEvent, window, cx| {
                this.open_tools_page(crate::tools::TIMER_PAGE, window, cx);
            },
        );

        let cover_volume_source = footer.read(cx).volume().clone();
        let cover_volume = cx.new(|cx| CoverVolume::new(cover_volume_source, window, cx));

        let footer_album_subscription = cx.subscribe(&footer, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToAlbumRequested, cx| {
                this.close_screens();
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, cx);
                });
            }
        });

        let footer_artist_subscription = cx.subscribe(&footer, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToArtistRequested, cx| {
                this.close_screens();
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_artist(event.artist_id, event.track_id, cx);
                });
            }
        });

        let footer_cover_subscription = cx.subscribe_in(
            &footer,
            window,
            |this, _, _: &ToggleCoverModeRequested, window, cx| {
                this.toggle_cover_mode(window, cx);
            },
        );

        let queue_view = cx.new(|cx| QueueView::new(window, cx));
        let lyrics_view = cx.new(|cx| LyricsView::new(window, cx));

        let cover_mode_view = cx.new(|cx| CoverModeView::new(cover_volume.clone(), window, cx));
        let cover_lyrics_toggle_subscription = cx.subscribe(
            &cover_mode_view,
            |this, _, event: &ToggleLyricsEvent, cx| {
                this.footer
                    .update(cx, |f, cx| f.set_show_lyrics(event.show, cx));
                this.set_lyrics_visible(event.show, cx);
            },
        );
        let cover_queue_toggle_subscription =
            cx.subscribe(&cover_mode_view, |this, _, event: &ToggleQueueEvent, cx| {
                this.footer
                    .update(cx, |f, cx| f.set_show_queue(event.show, cx));
                this.set_queue_visible(event.show, cx);
            });
        let cover_album_subscription = cx.subscribe(&cover_mode_view, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, event: &NavigateToAlbumRequested, cx| {
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, cx);
                });
            }
        });
        let cover_artist_subscription = cx.subscribe(&cover_mode_view, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, event: &NavigateToArtistRequested, cx| {
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_artist(event.artist_id, event.track_id, cx);
                });
            }
        });
        let cover_backdrop = cx.new(CoverBackdrop::new);
        let cover_backdrop_observe = cx.observe(&cover_backdrop, |_, _, cx| cx.notify());
        let cover_skin = cx.new(CoverSkin::new);

        let cover_observe_subscription = cx.observe(&cover_mode_view, |this, view, cx| {
            let hidden = !view.read(cx).controls_shown();
            if hidden {
                this.cover_volume.update(cx, |cv, cx| cv.collapse(cx));
            }
            cx.notify();
        });

        // ShuffleButton::on_click calls cx.notify() after reordering the queue.
        // Observe that entity so QueueView stays in sync with the shuffled order.
        let shuffle_button = footer.read(cx).shuffle_button.clone();
        let shuffle_subscription = cx.observe(&shuffle_button, {
            let queue_view = queue_view.clone();
            move |_, _, cx| {
                queue_view.update(cx, |qv, cx| qv.refresh_tracks(cx));
            }
        });

        let playlist_popup = cx.new(|cx| PlaylistPopup::new(window, cx));

        let settings_observer = cx.observe_global::<SettingsStore>({
            let lyrics_slider = lyrics_slider.clone();
            let blur_intensity_slider = blur_intensity_slider.clone();
            let blur_interface_opacity_slider = blur_interface_opacity_slider.clone();
            let remote_port_input = remote_port_input.clone();
            let scrobble_ui = scrobble_ui.clone();
            let scrobble_inputs = scrobble_inputs.clone();
            let library_page = library_page.clone();
            let sleep_timer_controls = sleep_timer_controls.clone();
            move |this, cx| {
                let grouping = cx.global::<SettingsStore>().artists_grouping();
                let library = &cx.global::<crate::services::Services>().library;
                if library.artists_grouping() != grouping {
                    library.set_artists_grouping(grouping);
                    crate::services::publish_remote_library_change(cx);
                }
                let pages = crate::settings_view::build_settings_pages(
                    SettingsSliders {
                        lyrics: lyrics_slider.clone(),
                        blur_intensity: blur_intensity_slider.clone(),
                        blur_interface_opacity: blur_interface_opacity_slider.clone(),
                    },
                    remote_port_input.clone(),
                    scrobble_ui.clone(),
                    scrobble_inputs.clone(),
                    library_page.clone(),
                    sleep_timer_controls.clone(),
                    cx,
                );
                this.settings_screen
                    .update(cx, |screen, _| screen.set_pages(pages));
                cx.notify();
            }
        });

        let activation_subscription = cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                let folders = cx
                    .global::<crate::settings_store::SettingsStore>()
                    .music_folders()
                    .to_vec();
                if !folders.is_empty() {
                    cx.global::<crate::services::Services>()
                        .library
                        .request_rescan(folders, false, false);
                }
                crate::remote_settings::sync_offline(cx);
            }
        });

        let window_bounds_subscription = cx.observe_window_bounds(window, |_, window, cx| {
            crate::window_placement::remember(window, cx);
        });

        let library_event_bus = cx
            .global::<crate::services::Services>()
            .library_event_bus
            .clone();
        let scan_subscription = cx.subscribe(
            &library_event_bus,
            |this: &mut MainView, _, event: &LibraryEvent, cx| {
                if this.show_settings
                    && matches!(
                        event,
                        LibraryEvent::ScanStarted | LibraryEvent::ScanComplete
                    )
                {
                    cx.notify();
                }
            },
        );

        let updater = updater::handle(cx);
        let updater_observer = updater
            .as_ref()
            .map(|entity| cx.observe(entity, |_, _, cx| cx.notify()));

        let header = {
            let main_view = cx.weak_entity();
            let parts = header::HeaderParts {
                audio_settings: cx.new(|cx| AudioSettings::new(window, cx)),
                search_input: search_input.clone(),
                scan_indicator: crate::library_scan_indicator::indicator(cx),
            };
            cx.new(|cx| header::HeaderHost::new(main_view, parts, cx))
        };

        Self {
            library_view,
            footer,
            queue_view,
            lyrics_view,
            playlist_popup,
            is_drilled_in: false,
            current_tab: LibraryRootTab::Albums,
            view_menu_tab,
            show_settings: false,
            show_tools: false,
            tools_view: cx
                .new(|cx| crate::tools::ToolsView::new(sleep_timer_controls.clone(), window, cx)),
            cover_mode: false,
            cover_mode_view,
            cover_backdrop,
            cover_volume,
            _cover_skin: cover_skin,
            show_queue: false,
            queue_width: QUEUE_WIDTH_DEFAULT,
            queue_resize_origin: None,
            queue_closing: false,
            _queue_close_task: None,
            show_lyrics: false,
            lyrics_width: LYRICS_WIDTH_DEFAULT,
            lyrics_resize_origin: None,
            lyrics_closing: false,
            _lyrics_close_task: None,
            _lyrics_slider: lyrics_slider,
            _lyrics_slider_observe: lyrics_slider_observe,
            _lyrics_slider_subscription: lyrics_slider_subscription,
            _blur_intensity_slider: blur_intensity_slider,
            _blur_intensity_slider_observe: blur_intensity_slider_observe,
            _blur_intensity_slider_subscription: blur_intensity_slider_subscription,
            _blur_interface_opacity_slider: blur_interface_opacity_slider,
            _blur_interface_opacity_slider_observe: blur_interface_opacity_slider_observe,
            _blur_interface_opacity_slider_subscription: blur_interface_opacity_slider_subscription,
            settings_screen: cx.new(|_| crate::settings_view::SettingsScreen::new(settings_pages)),
            _screens_observe: cx.observe_self(|this, cx| {
                if this.show_settings {
                    this.settings_screen.update(cx, |_, cx| cx.notify());
                } else if this.show_tools {
                    this.tools_view.update(cx, |_, cx| cx.notify());
                }
            }),
            search_input,
            _remote_port_input: remote_port_input,
            _remote_port_subscription: remote_port_subscription,
            _scrobble_ui: scrobble_ui,
            library_sources: library_sources.clone(),
            _library_sources_observe: library_sources_observe,
            _scrobble_ui_observe: scrobble_ui_observe,
            _scrobble_rules_observe: scrobble_rules_observe,
            _scrobble_imports_subscription: scrobble_imports_subscription,
            _scrobble_inputs: scrobble_inputs,
            _scrobble_token_subscription: scrobble_token_subscription,
            _scrobble_status_observe: scrobble_status_observe,
            #[cfg(not(target_os = "macos"))]
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
            _library_subscription: library_subscription,
            _scan_subscription: scan_subscription,
            _search_subscription: search_subscription,
            _footer_subscription: footer_subscription,
            _footer_lyrics_subscription: footer_lyrics_subscription,
            _footer_sleep_timer_subscription: footer_sleep_timer_subscription,
            _footer_album_subscription: footer_album_subscription,
            _footer_artist_subscription: footer_artist_subscription,
            _footer_cover_subscription: footer_cover_subscription,
            _cover_album_subscription: cover_album_subscription,
            _cover_artist_subscription: cover_artist_subscription,
            _cover_observe_subscription: cover_observe_subscription,
            _cover_lyrics_toggle_subscription: cover_lyrics_toggle_subscription,
            _cover_queue_toggle_subscription: cover_queue_toggle_subscription,
            _cover_backdrop_observe: cover_backdrop_observe,
            _shuffle_subscription: shuffle_subscription,
            _theme_registry_subscription: theme_registry_subscription,
            _settings_observer: settings_observer,
            _lang_subscription: lang_subscription,
            _activation_subscription: activation_subscription,
            _window_bounds_subscription: window_bounds_subscription,
            updater,
            _updater_observer: updater_observer,
            _sleep_timer_controls_observe: sleep_timer_controls_observe,
            header,
            #[cfg(target_os = "macos")]
            traffic_light: (
                crate::window_title_bar::TRAFFIC_LIGHT_X,
                crate::window_title_bar::TRAFFIC_LIGHT_Y,
            ),
            focus_handle,
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_traffic_lights(&mut self, window: &mut Window, bar_height: f32, hidden: bool) {
        let x = if hidden {
            crate::window_title_bar::TRAFFIC_LIGHT_HIDDEN_X
        } else {
            crate::window_title_bar::TRAFFIC_LIGHT_X
        };
        let y = crate::window_title_bar::traffic_light_y(bar_height);
        let (old_x, old_y) = self.traffic_light;
        if (old_x - x).abs() < 0.01 && (old_y - y).abs() < 0.01 {
            return;
        }
        self.traffic_light = (x, y);
        window.on_next_frame(move |window, _| {
            window.set_traffic_light_position(gpui::point(px(x), px(y)));
        });
    }

    fn clear_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let library_view = self.library_view.clone();
        library_view.update(cx, |v, cx| v.apply_search("", cx));
        self.focus_handle.focus(window, cx);
    }

    fn on_seek_forward(&mut self, _: &SeekForward, _: &mut Window, cx: &mut Context<Self>) {
        self.footer.update(cx, |f, cx| {
            f.progress().update(cx, |p, cx| p.seek_step(1, cx));
        });
    }

    fn on_seek_backward(&mut self, _: &SeekBackward, _: &mut Window, cx: &mut Context<Self>) {
        self.footer.update(cx, |f, cx| {
            f.progress().update(cx, |p, cx| p.seek_step(-1, cx));
        });
    }

    fn on_next_track(&mut self, _: &NextTrack, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_next(cx);
    }

    fn on_previous_track(&mut self, _: &PreviousTrack, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_previous(cx);
    }

    fn on_volume_up(&mut self, _: &VolumeUp, _: &mut Window, cx: &mut Context<Self>) {
        self.footer.update(cx, |f, cx| {
            f.volume()
                .update(cx, |v, cx| v.nudge(crate::volume::VOLUME_STEP, cx));
        });
    }

    fn on_volume_down(&mut self, _: &VolumeDown, _: &mut Window, cx: &mut Context<Self>) {
        self.footer.update(cx, |f, cx| {
            f.volume()
                .update(cx, |v, cx| v.nudge(-crate::volume::VOLUME_STEP, cx));
        });
    }

    fn on_play_pause(&mut self, _: &PlayPause, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::toggle_play_pause(cx);
    }

    fn open_settings(&mut self, page_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_at(page_ix, None, window, cx);
    }

    fn open_settings_at(
        &mut self,
        page_ix: usize,
        anchor: Option<gpui::SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_overlays(window, cx);
        self.show_tools = false;
        self.show_settings = true;
        self.settings_screen
            .update(cx, |screen, _| screen.show(page_ix, anchor));
        self.library_sources
            .update(cx, |sources, cx| sources.refresh_cache(cx));
        cx.notify();
    }

    fn open_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_overlays(window, cx);
        self.show_settings = false;
        self.show_tools = true;
        cx.notify();
    }

    fn open_tools_page(&mut self, page_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tools(window, cx);
        self.tools_view
            .update(cx, |view, cx| view.show_page(page_ix, cx));
    }

    fn close_screens(&mut self) {
        self.show_settings = false;
        self.show_tools = false;
    }

    fn leave_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_search(window, cx);
        self.set_cover_mode(false, cx);
    }

    fn toggle_cover_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_screens();
        self.clear_search(window, cx);
        self.set_cover_mode(!self.cover_mode, cx);
    }

    fn set_cover_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.cover_mode == on {
            return;
        }
        self.cover_mode = on;
        self.cover_mode_view
            .update(cx, |view, cx| view.set_active(on, cx));
        if !on {
            self.cover_volume.update(cx, |v, cx| v.collapse(cx));
        }
        cx.notify();
    }

    fn set_queue_visible(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_queue == show {
            return;
        }
        self.show_queue = show;
        self.queue_view.update(cx, |qv, _| qv.set_visible(show));
        if show {
            self.queue_closing = false;
            self._queue_close_task = None;
        } else {
            self.queue_closing = true;
            self._queue_close_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(QUEUE_ANIM).await;
                let _ = this.update(cx, |this, cx| {
                    this.queue_closing = false;
                    this._queue_close_task = None;
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn set_lyrics_visible(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_lyrics == show {
            return;
        }
        self.show_lyrics = show;
        self.lyrics_view
            .update(cx, |lv, cx| lv.set_visible(show, cx));
        if show {
            self.lyrics_closing = false;
            self._lyrics_close_task = None;
        } else {
            self.lyrics_closing = true;
            self._lyrics_close_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(QUEUE_ANIM).await;
                let _ = this.update(cx, |this, cx| {
                    this.lyrics_closing = false;
                    this._lyrics_close_task = None;
                    this.lyrics_view.update(cx, |lv, cx| lv.release_blur(cx));
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn on_open_scrobbling_settings(
        &mut self,
        _: &crate::settings_view::OpenScrobblingSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_at(
            self.settings_screen.read(cx).pages().scrobbling,
            Some(gpui::SharedString::from(
                crate::scrobble_settings::MEDIA_SERVERS_ANCHOR,
            )),
            window,
            cx,
        );
    }

    fn on_open_sleep_timer_settings(
        &mut self,
        _: &crate::settings_view::OpenSleepTimerSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_at(
            self.settings_screen.read(cx).pages().general,
            Some(gpui::SharedString::from(
                crate::sleep_timer::settings::SETTINGS_ANCHOR,
            )),
            window,
            cx,
        );
    }

    fn on_open_ai_playlist(
        &mut self,
        _: &crate::tools::OpenAiPlaylist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_tools(window, cx);
        self.tools_view
            .update(cx, |view, cx| view.show_ai_playlist(cx));
    }

    fn on_exit_cover_mode(&mut self, _: &ExitCoverMode, _: &mut Window, cx: &mut Context<Self>) {
        if self.cover_mode {
            self.set_cover_mode(false, cx);
        } else {
            cx.propagate();
        }
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let show_settings = self.show_settings;
        let show_tools = self.show_tools;
        let show_screen = show_settings || show_tools;
        let cover_mode = self.cover_mode;
        if cover_mode {
            let (lyrics, queue) = (self.show_lyrics, self.show_queue);
            self.cover_mode_view
                .update(cx, |view, _| view.set_panels(lyrics, queue));
        }
        let (chrome_visible, controls_t) = {
            let view = self.cover_mode_view.read(cx);
            (view.chrome_visible(), view.controls_progress(cx))
        };

        let title_bar = Colors::title_bar(cx);
        let background = Colors::background(cx);
        let blur = cx.global::<SettingsStore>().blur_background();
        let backdrop = (!show_screen && (cover_mode || blur == BlurBackground::AllViews))
            .then(|| self.cover_backdrop.read(cx).frame())
            .flatten();
        let has_backdrop = backdrop.is_some();
        if backdrop.as_ref().is_some_and(|frame| frame.progress < 1.) {
            window.request_animation_frame();
        }
        let veil_content = has_backdrop && !cover_mode;
        cover_backdrop::set_active(
            backdrop
                .as_ref()
                .map_or(0., cover_backdrop::Backdrop::presence),
            cx,
        );
        let veil = cover_backdrop::veil_factor(cx);
        let stage = stage::Stage::new(
            cover_mode,
            self.cover_mode_view.read(cx),
            has_backdrop,
            title_bar,
            background,
            veil,
            window,
            cx,
        );
        let bar_bg = cover_backdrop::chrome_bg(title_bar, veil);
        let panel_bg = cover_backdrop::panel_bg(background, veil);
        let muted = Colors::muted(cx);
        let foreground = Colors::foreground(cx);
        let tab_colors = TabColors {
            active_bg: cover_backdrop::inset_bg(Colors::secondary(cx), veil),
            hover_bg: cover_backdrop::inset_bg(muted, veil),
            primary: Colors::primary(cx),
            foreground,
        };

        let settings = cx.global::<SettingsStore>();
        let scale = settings.font_scale().ui_scale();
        let header_in_title_bar = settings.header_in_title_bar();
        let title_bar_height = header::TITLE_BAR_HEIGHT * scale;
        let header_in_bar =
            header_in_title_bar && (!window.is_fullscreen() || cfg!(target_os = "linux"));
        let show_chrome = !cover_mode || chrome_visible;
        let lights_bar = if header_in_title_bar {
            title_bar_height
        } else {
            crate::window_title_bar::HEIGHT
        };
        let hold_zone = if cfg!(target_os = "macos") && !window.is_fullscreen() {
            lights_bar
        } else {
            0.
        };
        #[cfg(target_os = "macos")]
        self.sync_traffic_lights(window, lights_bar, stage.lights_hidden);

        let header_state = show_chrome.then(|| header::HeaderState {
            placement: if header_in_bar {
                header::Placement::TitleBar
            } else {
                header::Placement::Below(bar_bg)
            },
            colors: tab_colors,
            title_bar,
            veil,
            scale,
            show_settings,
            show_tools,
            cover_mode,
            is_drilled_in: self.is_drilled_in,
            current_tab: self.current_tab,
            view_menu_tab: self.view_menu_tab,
            update_ready: self
                .updater
                .as_ref()
                .is_some_and(|entity| entity.read(cx).has_staged_update()),
        });
        let header_below = header_state
            .filter(|_| !header_in_bar)
            .map(|state| self.header_block(state, cx));

        let border = Colors::border(cx);
        let bar_height =
            crate::window_title_bar::bar_height(window, header_in_bar.then_some(title_bar_height));
        let mut window_title_bar =
            crate::window_title_bar::WindowTitleBar::new().bg(stage.title_bar_bg);
        if header_in_bar {
            window_title_bar = window_title_bar
                .height(title_bar_height)
                .content(header_state.map(|state| self.header_block(state, cx)));
        }

        div()
            .id("main_view")
            .v_flex()
            .size_full()
            .relative()
            .overflow_hidden()
            .on_action(cx.listener(Self::on_seek_forward))
            .on_action(cx.listener(Self::on_seek_backward))
            .on_action(cx.listener(Self::on_next_track))
            .on_action(cx.listener(Self::on_previous_track))
            .on_action(cx.listener(Self::on_volume_up))
            .on_action(cx.listener(Self::on_volume_down))
            .on_action(cx.listener(Self::on_play_pause))
            .on_action(cx.listener(Self::on_exit_cover_mode))
            .on_action(cx.listener(Self::on_open_scrobbling_settings))
            .on_action(cx.listener(Self::on_open_sleep_timer_settings))
            .on_action(cx.listener(Self::on_open_ai_playlist))
            .on_drag_move(
                cx.listener(move |this, e: &DragMoveEvent<DragQueueResize>, _, cx| {
                    if e.drag(cx).0 != entity_id {
                        return;
                    }
                    let Some((start_x, start_width)) = this.queue_resize_origin else {
                        return;
                    };
                    let delta = (start_x - e.event.position.x) / px(1.);
                    let new_width = (start_width + delta).clamp(QUEUE_WIDTH_MIN, QUEUE_WIDTH_MAX);
                    if (this.queue_width - new_width).abs() > 0.5 {
                        this.queue_width = new_width;
                        cx.notify();
                    }
                }),
            )
            .on_drag_move(
                cx.listener(move |this, e: &DragMoveEvent<DragLyricsResize>, _, cx| {
                    if e.drag(cx).0 != entity_id {
                        return;
                    }
                    let Some((start_x, start_width)) = this.lyrics_resize_origin else {
                        return;
                    };
                    let delta = (start_x - e.event.position.x) / px(1.);
                    let new_width = (start_width + delta).clamp(LYRICS_WIDTH_MIN, LYRICS_WIDTH_MAX);
                    if (this.lyrics_width - new_width).abs() > 0.5 {
                        this.lyrics_width = new_width;
                        cx.notify();
                    }
                }),
            )
            .when(cover_mode, |d| {
                d.on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                    let held = e.position.y < px(hold_zone);
                    this.cover_mode_view
                        .update(cx, |view, cx| view.handle_mouse_move(held, cx));
                }))
            })
            .when(has_backdrop, |d| d.bg(background))
            .when_some(backdrop, |d, frame| {
                d.child(cover_backdrop::layers(frame, background))
            })
            .children((self.show_queue || self.queue_closing).then(|| {
                sliding(
                    div().absolute().top_0().right_0().h(px(bar_height)).when(
                        stage.queue_to_top,
                        |d| {
                            d.bg(panel_bg)
                                .border_l(px(PANEL_BORDER))
                                .border_color(border)
                        },
                    ),
                    self.queue_width,
                    self.queue_closing,
                    ("queue-top-slide-in", "queue-top-slide-out"),
                )
            }))
            .child(window_title_bar)
            .child({
                let (pane_strip, area_strip) = match stage.strip {
                    Some(strip) if strip.under_lyrics => (Some(strip), None),
                    strip => (None, strip),
                };
                let content = div()
                    .flex_1()
                    .overflow_hidden()
                    .when(!cover_mode, |d| d.ml_4())
                    .when(!cover_mode && !self.show_queue && !self.show_lyrics, |d| {
                        d.mr_4()
                    })
                    .when(cover_mode, |d| d.relative())
                    .when_some(area_strip, |d, strip| d.child(strip.layer()))
                    .child(if cover_mode {
                        self.cover_mode_view.clone().into_any_element()
                    } else if show_settings {
                        crate::view_cache::full(&self.settings_screen)
                    } else if show_tools {
                        crate::view_cache::full(&self.tools_view)
                    } else {
                        crate::view_cache::full(&self.library_view)
                    })
                    .when(cover_mode && (chrome_visible || controls_t > 0.), |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .when(!chrome_visible, |d| d.opacity(controls_t))
                                .child(cover_chrome_button(chrome_visible, tab_colors, cx))
                                .child(cover_spectrum_button(tab_colors, cx)),
                        )
                    });
                let lyrics_changed = self
                    .lyrics_view
                    .update(cx, |lv, _| lv.set_presentation(stage.lyrics));
                let lyrics_panel = (self.show_lyrics || self.lyrics_closing).then(|| {
                    side_panel(
                        crate::view_cache::block(
                            &self.lyrics_view,
                            StyleRefinement::default().size_full(),
                            !lyrics_changed,
                        ),
                        self.lyrics_width,
                        self.lyrics_closing,
                        (!veil_content && !stage.bare_lyrics).then_some(panel_bg),
                        if stage.bare_lyrics {
                            gpui::transparent_black()
                        } else {
                            border
                        },
                        resize_handle("lyrics_resize_handle")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, _| {
                                    this.lyrics_resize_origin =
                                        Some((e.position.x, this.lyrics_width));
                                }),
                            )
                            .on_drag(DragLyricsResize(entity_id), |drag, _, _, cx| {
                                cx.new(|_| drag.clone())
                            }),
                        ("lyrics-slide-in", "lyrics-slide-out"),
                    )
                });
                let queue_panel = (self.show_queue || self.queue_closing).then(|| {
                    side_panel(
                        crate::view_cache::full(&self.queue_view),
                        self.queue_width,
                        self.queue_closing,
                        (!veil_content).then_some(panel_bg),
                        border,
                        resize_handle("queue_resize_handle")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, _| {
                                    this.queue_resize_origin =
                                        Some((e.position.x, this.queue_width));
                                }),
                            )
                            .on_drag(DragQueueResize(entity_id), |drag, _, _, cx| {
                                cx.new(|_| drag.clone())
                            }),
                        ("queue-slide-in", "queue-slide-out"),
                    )
                });
                let middle = div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .when(!has_backdrop, |d| d.bg(background))
                    .when(veil_content, |d| d.bg(panel_bg))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .relative()
                            .when_some(pane_strip, |d, strip| d.child(strip.layer()))
                            .child(content)
                            .children(lyrics_panel),
                    )
                    .children(queue_panel);

                let footer_bar = div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(footer_height(cx)))
                    .bg(bar_bg)
                    .child(crate::view_cache::full(&self.footer));

                div()
                    .id("main_content")
                    .v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .key_context(crate::keyboard_shortcuts::CONTEXT)
                    .track_focus(&self.focus_handle)
                    .children(header_below)
                    .child(middle)
                    .when(show_chrome, |d| d.child(footer_bar))
            })
            .child({
                let entity = cx.entity();
                let cover = self.cover_mode_view.clone();
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            if e.button != MouseButton::Left {
                                return;
                            }
                            entity.update(cx, |this, _| {
                                this.queue_resize_origin = None;
                                this.lyrics_resize_origin = None;
                            });
                        });
                        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            cover.update(cx, |view, _| view.release_hold());
                        });
                    },
                )
                .absolute()
                .size_full()
            })
            .child(self.playlist_popup.clone())
    }
}

fn side_panel(
    content: AnyElement,
    width: f32,
    closing: bool,
    bg: Option<Hsla>,
    border: Hsla,
    handle: impl IntoElement,
    (slide_in, slide_out): (&'static str, &'static str),
) -> impl IntoElement {
    let panel = div()
        .flex_shrink_0()
        .border_l(px(PANEL_BORDER))
        .border_color(border)
        .relative()
        .when_some(bg, |d, bg| d.bg(bg))
        .child(
            div().size_full().overflow_hidden().child(
                div()
                    .h_full()
                    .flex_shrink_0()
                    .w(px(width - PANEL_BORDER))
                    .child(content),
            ),
        )
        .child(handle);
    sliding(panel, width, closing, (slide_in, slide_out))
}

fn sliding<E: IntoElement + Styled + 'static>(
    element: E,
    width: f32,
    closing: bool,
    (slide_in, slide_out): (&'static str, &'static str),
) -> gpui::AnimationElement<E> {
    element.with_animation(
        if closing { slide_out } else { slide_in },
        Animation::new(QUEUE_ANIM).with_easing(ease_out_quint()),
        move |this, delta| {
            let factor = if closing { 1.0 - delta } else { delta };
            this.w(px(width * factor))
        },
    )
}

fn resize_handle(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .absolute()
        .left(px(-3.))
        .top(px(0.))
        .h_full()
        .w(px(6.))
        .cursor(gpui::CursorStyle::ResizeLeftRight)
}

fn cover_spectrum_button(colors: TabColors, cx: &mut Context<MainView>) -> impl IntoElement {
    let visible = cx.global::<SettingsStore>().spectrum_visible();
    let available = cx
        .global::<crate::services::Services>()
        .player
        .has_signal_tap();
    let strings = ui_resources::i18n::spectrum_strings();
    let label = if !available {
        strings.unavailable.clone()
    } else if visible {
        strings.hide.clone()
    } else {
        strings.show.clone()
    };
    let fg = if visible && available {
        colors.primary
    } else {
        colors.foreground
    };
    let hover_bg = colors.hover_bg;

    div()
        .id("cover_spectrum_toggle")
        .absolute()
        .top(px(52.))
        .left(px(12.))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(!available, |d| d.opacity(0.45))
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .hover(move |s| s.bg(hover_bg))
        .on_click(move |_, _, cx| {
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .set_spectrum_visible(!visible)
            {
                crate::settings_store::notify_save_error(cx, e);
            }
        })
        .child(
            svg()
                .path("icons/s1-spectrum.svg")
                .size(px(20.))
                .text_color(fg),
        )
}

fn cover_chrome_button(
    chrome_visible: bool,
    colors: TabColors,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let icon = if chrome_visible {
        "icons/s1-expand.svg"
    } else {
        "icons/s1-collapse.svg"
    };
    let fg = colors.foreground;
    let hover_bg = colors.hover_bg;

    div()
        .id("cover_chrome_toggle")
        .absolute()
        .top(px(12.))
        .left(px(12.))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .tooltip(|window, cx| Tooltip::new(tr().full_screen.clone()).build(window, cx))
        .hover(move |s| s.bg(hover_bg))
        .on_click(cx.listener(|this, _, _, cx| {
            this.cover_mode_view
                .update(cx, |view, cx| view.toggle_chrome(cx));
            this.cover_volume.update(cx, |v, cx| v.collapse(cx));
        }))
        .child(svg().path(icon).size(px(20.)).text_color(fg))
}
