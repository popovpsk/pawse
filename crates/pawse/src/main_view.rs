use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt as _, App, AppContext, Context, DispatchPhase, DragMoveEvent, Empty,
    Entity, EntityId, FocusHandle, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, ease_out_quint, px, svg,
};
use gpui_component::{
    Icon, Root, Sizable, Size, StyledExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    slider::{SliderEvent, SliderState},
    theme::ThemeRegistry,
    tooltip::Tooltip,
};

use crate::audio_settings::AudioSettings;
use crate::cover_backdrop::{self, CoverBackdrop};
use crate::cover_mode_view::CoverModeView;
use crate::cover_skin::CoverSkin;
use crate::cover_volume::CoverVolume;
use crate::footer::{Footer, ToggleLyricsEvent, ToggleQueueEvent};
use crate::keyboard_shortcuts::{
    ExitCoverMode, NextTrack, PlayPause, PreviousTrack, SeekBackward, SeekForward, VolumeDown,
    VolumeUp,
};
use crate::library_service::LibraryEvent;
use crate::library_views::library_view::{LibraryRootTab, LibraryView, LibraryViewEvent};
use crate::localization::LangChanged;
use crate::localization::tr;
use crate::lyrics_view::LyricsView;
#[cfg(not(target_os = "macos"))]
use crate::media_bridge::MediaBridge;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::playlist_popup::PlaylistPopup;
use crate::queue_view::QueueView;
use crate::scrobble_settings::{ScrobbleInputs, ScrobbleUiState};
use crate::settings_store::{BlurBackground, SettingsStore, ui_scale};
use crate::settings_view::{LangPickerState, SettingsSliders, ThemePickerState};
use crate::theme_colors::Colors;
use ui_components::settings::SettingPage;

const HEADER_HEIGHT: f32 = 44.;
const FOOTER_HEIGHT: f32 = 80.;
const QUEUE_WIDTH_DEFAULT: f32 = 360.;
const QUEUE_WIDTH_MIN: f32 = 280.;
const QUEUE_WIDTH_MAX: f32 = 560.;
const LYRICS_WIDTH_DEFAULT: f32 = 360.;
const LYRICS_WIDTH_MIN: f32 = 280.;
const LYRICS_WIDTH_MAX: f32 = 560.;
const QUEUE_ANIM: Duration = Duration::from_millis(200);

pub(crate) fn footer_height(cx: &App) -> f32 {
    FOOTER_HEIGHT * ui_scale(cx)
}

#[derive(Clone, Copy)]
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
    audio_settings: Entity<AudioSettings>,
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    queue_view: Entity<QueueView>,
    lyrics_view: Entity<LyricsView>,
    playlist_popup: Entity<PlaylistPopup>,
    is_drilled_in: bool,
    current_tab: LibraryRootTab,
    show_settings: bool,
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
    settings_pages: Vec<SettingPage>,
    search_input: Entity<InputState>,
    _remote_port_input: Entity<InputState>,
    _remote_port_subscription: Subscription,
    _theme_picker: Entity<ThemePickerState>,
    _lang_picker: Entity<LangPickerState>,
    _scrobble_ui: Entity<ScrobbleUiState>,
    library_sources: Entity<crate::library_sources::LibrarySources>,
    settings_page_ix: usize,
    _library_sources_observe: Subscription,
    _scrobble_ui_observe: Subscription,
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
    _footer_album_subscription: Subscription,
    _footer_artist_subscription: Subscription,
    _cover_album_subscription: Subscription,
    _cover_artist_subscription: Subscription,
    _cover_observe_subscription: Subscription,
    _cover_lyrics_toggle_subscription: Subscription,
    _cover_queue_toggle_subscription: Subscription,
    _cover_backdrop_observe: Subscription,
    _shuffle_subscription: gpui::Subscription,
    _theme_registry_subscription: gpui::Subscription,
    _theme_picker_subscription: gpui::Subscription,
    _lang_picker_subscription: gpui::Subscription,
    _settings_observer: gpui::Subscription,
    _lang_subscription: Subscription,
    _activation_subscription: gpui::Subscription,
    updater: Option<Entity<updater::AutoUpdater>>,
    _updater_observer: Option<gpui::Subscription>,
    focus_handle: FocusHandle,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_view = cx.new(|cx| LibraryView::new(window, cx));

        let library_subscription = cx.subscribe_in(
            &library_view,
            window,
            move |this: &mut MainView, _, event: &LibraryViewEvent, window, cx| match event {
                LibraryViewEvent::StateChanged => {
                    let view = this.library_view.read(cx);
                    this.is_drilled_in = view.is_drilled_in();
                    if let Some(tab) = view.current_tab() {
                        this.current_tab = tab;
                    }
                    this.clear_search(window, cx);
                    cx.notify();
                }
                LibraryViewEvent::OpenLibrarySettings => {
                    this.open_settings(this.settings_pages.len().saturating_sub(1), window, cx);
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

        let theme_picker: Entity<ThemePickerState> = cx.new(|cx| ThemePickerState::new(cx));
        let lang_picker: Entity<LangPickerState> = cx.new(|cx| LangPickerState::new(cx));

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
            subsonic_inputs: crate::subsonic_settings::SubsonicInputs::new(window, cx),
        };
        let library_sources_observe = cx.observe(&library_sources, |_, _, cx| cx.notify());
        let scrobble_ui: Entity<ScrobbleUiState> = cx.new(|_| ScrobbleUiState::new());
        let scrobble_ui_observe = cx.observe(&scrobble_ui, |_, _, cx| cx.notify());
        let scrobble_inputs = ScrobbleInputs {
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

        let theme_registry_subscription = cx.observe_global::<ThemeRegistry>({
            let theme_picker = theme_picker.clone();
            let lang_picker = lang_picker.clone();
            let lyrics_slider = lyrics_slider.clone();
            let blur_intensity_slider = blur_intensity_slider.clone();
            let blur_interface_opacity_slider = blur_interface_opacity_slider.clone();
            let remote_port_input = remote_port_input.clone();
            let scrobble_ui = scrobble_ui.clone();
            let scrobble_inputs = scrobble_inputs.clone();
            let library_page = library_page.clone();
            move |this, cx| {
                theme_picker.update(cx, |state, cx| {
                    state.options = ThemePickerState::build_options(&*cx);
                    cx.notify();
                });
                this.settings_pages = crate::settings_view::build_settings_pages(
                    theme_picker.clone(),
                    lang_picker.clone(),
                    SettingsSliders {
                        lyrics: lyrics_slider.clone(),
                        blur_intensity: blur_intensity_slider.clone(),
                        blur_interface_opacity: blur_interface_opacity_slider.clone(),
                    },
                    remote_port_input.clone(),
                    scrobble_ui.clone(),
                    scrobble_inputs.clone(),
                    library_page.clone(),
                    cx,
                );
                cx.notify();
            }
        });

        let theme_picker_subscription = cx.observe(&theme_picker, |_, _, cx| {
            cx.notify();
        });

        let lang_picker_subscription = cx.observe(&lang_picker, |_, _, cx| {
            cx.notify();
        });

        let settings_pages = crate::settings_view::build_settings_pages(
            theme_picker.clone(),
            lang_picker.clone(),
            SettingsSliders {
                lyrics: lyrics_slider.clone(),
                blur_intensity: blur_intensity_slider.clone(),
                blur_interface_opacity: blur_interface_opacity_slider.clone(),
            },
            remote_port_input.clone(),
            scrobble_ui.clone(),
            scrobble_inputs.clone(),
            library_page.clone(),
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

        let cover_volume_source = footer.read(cx).volume().clone();
        let cover_volume = cx.new(|cx| CoverVolume::new(cover_volume_source, window, cx));

        let footer_album_subscription = cx.subscribe(&footer, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToAlbumRequested, cx| {
                this.show_settings = false;
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, cx);
                });
            }
        });

        let footer_artist_subscription = cx.subscribe(&footer, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToArtistRequested, cx| {
                this.show_settings = false;
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_artist(event.artist_id, cx);
                });
            }
        });

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
                    view.navigate_to_artist(event.artist_id, cx);
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
            let theme_picker = theme_picker.clone();
            let lang_picker = lang_picker.clone();
            let lyrics_slider = lyrics_slider.clone();
            let blur_intensity_slider = blur_intensity_slider.clone();
            let blur_interface_opacity_slider = blur_interface_opacity_slider.clone();
            let remote_port_input = remote_port_input.clone();
            let scrobble_ui = scrobble_ui.clone();
            let scrobble_inputs = scrobble_inputs.clone();
            let library_page = library_page.clone();
            move |this, cx| {
                let grouping = cx.global::<SettingsStore>().artists_grouping();
                cx.global::<crate::services::Services>()
                    .library
                    .set_artists_grouping(grouping);
                this.settings_pages = crate::settings_view::build_settings_pages(
                    theme_picker.clone(),
                    lang_picker.clone(),
                    SettingsSliders {
                        lyrics: lyrics_slider.clone(),
                        blur_intensity: blur_intensity_slider.clone(),
                        blur_interface_opacity: blur_interface_opacity_slider.clone(),
                    },
                    remote_port_input.clone(),
                    scrobble_ui.clone(),
                    scrobble_inputs.clone(),
                    library_page.clone(),
                    cx,
                );
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
                crate::subsonic_settings::sync_offline(cx);
            }
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

        Self {
            audio_settings: cx.new(|cx| AudioSettings::new(window, cx)),
            library_view,
            footer,
            queue_view,
            lyrics_view,
            playlist_popup,
            is_drilled_in: false,
            current_tab: LibraryRootTab::Albums,
            show_settings: false,
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
            settings_pages,
            search_input,
            _remote_port_input: remote_port_input,
            _remote_port_subscription: remote_port_subscription,
            _theme_picker: theme_picker,
            _lang_picker: lang_picker,
            _scrobble_ui: scrobble_ui,
            library_sources: library_sources.clone(),
            settings_page_ix: 0,
            _library_sources_observe: library_sources_observe,
            _scrobble_ui_observe: scrobble_ui_observe,
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
            _footer_album_subscription: footer_album_subscription,
            _footer_artist_subscription: footer_artist_subscription,
            _cover_album_subscription: cover_album_subscription,
            _cover_artist_subscription: cover_artist_subscription,
            _cover_observe_subscription: cover_observe_subscription,
            _cover_lyrics_toggle_subscription: cover_lyrics_toggle_subscription,
            _cover_queue_toggle_subscription: cover_queue_toggle_subscription,
            _cover_backdrop_observe: cover_backdrop_observe,
            _shuffle_subscription: shuffle_subscription,
            _theme_registry_subscription: theme_registry_subscription,
            _theme_picker_subscription: theme_picker_subscription,
            _lang_picker_subscription: lang_picker_subscription,
            _settings_observer: settings_observer,
            _lang_subscription: lang_subscription,
            _activation_subscription: activation_subscription,
            updater,
            _updater_observer: updater_observer,
            focus_handle,
        }
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
        self.leave_overlays(window, cx);
        self.show_settings = true;
        self.settings_page_ix = page_ix;
        self.library_sources
            .update(cx, |sources, cx| sources.refresh_cache(cx));
        cx.notify();
    }

    fn leave_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_search(window, cx);
        self.set_cover_mode(false, cx);
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
                    cx.notify();
                });
            }));
        }
        cx.notify();
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
        let has_back = show_settings || self.is_drilled_in;
        let cover_mode = self.cover_mode;
        let active_tab = (!cover_mode).then_some(self.current_tab);
        if cover_mode {
            let (lyrics, queue) = (self.show_lyrics, self.show_queue);
            self.cover_mode_view
                .update(cx, |view, _| view.set_panels(lyrics, queue));
        }
        let (chrome_visible, controls_t) = {
            let view = self.cover_mode_view.read(cx);
            (view.chrome_visible(), view.controls_progress())
        };

        let title_bar = Colors::title_bar(cx);
        let background = Colors::background(cx);
        let blur = cx.global::<SettingsStore>().blur_background();
        let backdrop = (!show_settings && (cover_mode || blur == BlurBackground::AllViews))
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
        let title_bar_bg = cover_backdrop::chrome_bg(
            if cover_mode && !chrome_visible {
                background
            } else {
                title_bar
            },
            veil,
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
        let liked_enabled = settings.liked_enabled();
        let playlists_enabled = settings.playlists_enabled();
        let scale = settings.font_scale().ui_scale();

        let left_group = div()
            .flex_1()
            .flex()
            .items_center()
            .h_full()
            .gap_1()
            .when(has_back, |d| {
                d.child(back_button(foreground, muted, scale, cx))
            })
            .when(!has_back, |d| {
                d.child(tab_icon_button(
                    "tab_albums",
                    "icons/s1-albums.svg",
                    active_tab == Some(LibraryRootTab::Albums),
                    LibraryRootTab::Albums,
                    tab_colors,
                    scale,
                    cx,
                ))
                .child(tab_icon_button(
                    "tab_artists",
                    "icons/s1-artists.svg",
                    active_tab == Some(LibraryRootTab::Artists),
                    LibraryRootTab::Artists,
                    tab_colors,
                    scale,
                    cx,
                ))
                .when(liked_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_liked",
                        "icons/s1-heart.svg",
                        active_tab == Some(LibraryRootTab::Liked),
                        LibraryRootTab::Liked,
                        tab_colors,
                        scale,
                        cx,
                    ))
                })
                .when(playlists_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_playlists",
                        "icons/s1-playlists.svg",
                        active_tab == Some(LibraryRootTab::Playlists),
                        LibraryRootTab::Playlists,
                        tab_colors,
                        scale,
                        cx,
                    ))
                })
                .child(cover_mode_button(cover_mode, tab_colors, scale, cx))
            });

        let update_ready = self
            .updater
            .as_ref()
            .is_some_and(|entity| entity.read(cx).has_staged_update());

        let right_group = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .h_full()
            .when(update_ready && !show_settings, |d| {
                d.child(update_button(scale, cx))
            })
            .when(!show_settings, |d| d.child(settings_gear_button(scale, cx)))
            .child(self.audio_settings.clone());

        div()
            .id("main_view")
            .v_flex()
            .size_full()
            .relative()
            .overflow_hidden()
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
                d.on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                    this.cover_mode_view
                        .update(cx, |view, cx| view.handle_mouse_move(cx));
                }))
            })
            .when(has_backdrop, |d| d.bg(background))
            .when_some(backdrop, |d, frame| {
                d.child(cover_backdrop::layers(frame, background))
            })
            .child(crate::window_title_bar::WindowTitleBar::new().bg(title_bar_bg))
            .child({
                let header_bar = div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(HEADER_HEIGHT * scale))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_2()
                    .bg(bar_bg)
                    .child(left_group)
                    .when(!show_settings && !cover_mode, |d| {
                        d.child(
                            div().w(px(260.)).child(
                                Input::new(&self.search_input)
                                    .with_size(Size::Medium)
                                    .focus_bordered(false)
                                    .rounded_full()
                                    .bg(cover_backdrop::field_bg(title_bar, veil)),
                            ),
                        )
                    })
                    .child(right_group);

                let middle = div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .when(!has_backdrop, |d| d.bg(background))
                    .when(veil_content, |d| d.bg(panel_bg))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .when(!cover_mode, |d| d.ml_4())
                            .when(!cover_mode && !self.show_queue && !self.show_lyrics, |d| {
                                d.mr_4()
                            })
                            .child(if cover_mode {
                                self.cover_mode_view.clone().into_any_element()
                            } else if show_settings {
                                // Our own tab-based settings widget (ui_components) owns
                                // its scroll + active-tab state; it just needs a
                                // definite-size parent, which `flex_1` provides here.
                                div()
                                    .size_full()
                                    .child(crate::settings_view::settings_widget(
                                        self.settings_pages.clone(),
                                        self.settings_page_ix,
                                    ))
                                    .into_any_element()
                            } else {
                                self.library_view.clone().into_any_element()
                            })
                            .when(cover_mode && (chrome_visible || controls_t > 0.), |d| {
                                d.relative().child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .when(!chrome_visible, |d| d.opacity(controls_t))
                                        .child(cover_chrome_button(chrome_visible, tab_colors, cx)),
                                )
                            }),
                    )
                    .when(self.show_lyrics || self.lyrics_closing, |d| {
                        let lyrics_width = self.lyrics_width;
                        let closing = self.lyrics_closing;
                        d.child(
                            div()
                                .flex_shrink_0()
                                .border_l(px(1.))
                                .border_color(Colors::border(cx))
                                .relative()
                                .when(!veil_content, |d| d.bg(panel_bg))
                                .child(
                                    div()
                                        .size_full()
                                        .overflow_hidden()
                                        .child(self.lyrics_view.clone()),
                                )
                                .child(
                                    div()
                                        .id("lyrics_resize_handle")
                                        .absolute()
                                        .left(px(-3.))
                                        .top(px(0.))
                                        .h_full()
                                        .w(px(6.))
                                        .cursor(gpui::CursorStyle::ResizeLeftRight)
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
                                )
                                .with_animation(
                                    if closing {
                                        "lyrics-slide-out"
                                    } else {
                                        "lyrics-slide-in"
                                    },
                                    Animation::new(QUEUE_ANIM).with_easing(ease_out_quint()),
                                    move |this, delta| {
                                        let factor = if closing { 1.0 - delta } else { delta };
                                        this.w(px(lyrics_width * factor))
                                    },
                                ),
                        )
                    })
                    .when(self.show_queue || self.queue_closing, |d| {
                        let queue_width = self.queue_width;
                        let closing = self.queue_closing;
                        d.child(
                            div()
                                .flex_shrink_0()
                                .border_l(px(1.))
                                .border_color(Colors::border(cx))
                                .relative()
                                .when(!veil_content, |d| d.bg(panel_bg))
                                .child(
                                    div()
                                        .size_full()
                                        .overflow_hidden()
                                        .child(self.queue_view.clone()),
                                )
                                .child(
                                    div()
                                        .id("queue_resize_handle")
                                        .absolute()
                                        .left(px(-3.))
                                        .top(px(0.))
                                        .h_full()
                                        .w(px(6.))
                                        .cursor(gpui::CursorStyle::ResizeLeftRight)
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
                                )
                                .with_animation(
                                    if closing {
                                        "queue-slide-out"
                                    } else {
                                        "queue-slide-in"
                                    },
                                    Animation::new(QUEUE_ANIM).with_easing(ease_out_quint()),
                                    move |this, delta| {
                                        let factor = if closing { 1.0 - delta } else { delta };
                                        this.w(px(queue_width * factor))
                                    },
                                ),
                        )
                    });

                let footer_bar = div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(footer_height(cx)))
                    .bg(bar_bg)
                    .child(self.footer.clone());

                let show_chrome = !cover_mode || chrome_visible;

                div()
                    .id("main_content")
                    .v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .key_context(crate::keyboard_shortcuts::CONTEXT)
                    .track_focus(&self.focus_handle)
                    .on_action(cx.listener(Self::on_seek_forward))
                    .on_action(cx.listener(Self::on_seek_backward))
                    .on_action(cx.listener(Self::on_next_track))
                    .on_action(cx.listener(Self::on_previous_track))
                    .on_action(cx.listener(Self::on_volume_up))
                    .on_action(cx.listener(Self::on_volume_down))
                    .on_action(cx.listener(Self::on_play_pause))
                    .on_action(cx.listener(Self::on_exit_cover_mode))
                    .when(show_chrome, |d| d.child(header_bar))
                    .child(middle)
                    .when(show_chrome, |d| d.child(footer_bar))
            })
            .child({
                let entity = cx.entity();
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
                    },
                )
                .absolute()
                .size_full()
            })
            .children(Root::render_notification_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            .child(self.playlist_popup.clone())
    }
}

fn settings_gear_button(scale: f32, cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("settings_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40. * scale))
        .h(px(40. * scale))
        .icon(
            Icon::default()
                .path("icons/settings.svg")
                .size(px(20. * scale)),
        )
        .tooltip(tr().settings.clone())
        .on_click(cx.listener(|this, _, window, cx| this.open_settings(0, window, cx)))
}

fn update_button(scale: f32, cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("update_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40. * scale))
        .h(px(40. * scale))
        .icon(
            Icon::default()
                .path("icons/update.svg")
                .size(px(20. * scale)),
        )
        .tooltip(tr().restart_to_update.clone())
        .on_click(cx.listener(|_, _, _, cx| updater::apply_and_restart(cx)))
}

fn back_button(
    fg: Hsla,
    hover_bg: Hsla,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    div()
        .id("back_button")
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .hover(move |style| style.bg(hover_bg))
        .on_click(cx.listener(|this, _, window, cx| {
            this.leave_overlays(window, cx);
            if this.show_settings {
                this.show_settings = false;
                cx.notify();
            } else {
                this.library_view.update(cx, |view, cx| view.go_back(cx));
            }
        }))
        .child(
            svg()
                .path("icons/back.svg")
                .size(px(22. * scale))
                .text_color(fg),
        )
}

fn tab_icon_button(
    id: &'static str,
    icon_path: &'static str,
    active: bool,
    tab: LibraryRootTab,
    colors: TabColors,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let fg = if active {
        colors.primary
    } else {
        colors.foreground
    };
    let active_bg = colors.active_bg;
    let hover_bg = colors.hover_bg;

    div()
        .id(id)
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(active, move |d| d.bg(active_bg))
        .hover(move |s| s.bg(hover_bg))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.leave_overlays(window, cx);
            this.library_view
                .update(cx, |view, cx| view.select_tab(tab, cx));
            cx.notify();
        }))
        .child(svg().path(icon_path).size(px(20. * scale)).text_color(fg))
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

fn cover_mode_button(
    active: bool,
    colors: TabColors,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let fg = if active {
        colors.primary
    } else {
        colors.foreground
    };
    let active_bg = colors.active_bg;
    let hover_bg = colors.hover_bg;

    div()
        .id("tab_cover_mode")
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(active, move |d| d.bg(active_bg))
        .hover(move |s| s.bg(hover_bg))
        .tooltip(|window, cx| Tooltip::new(tr().cover_mode.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.clear_search(window, cx);
            this.set_cover_mode(!this.cover_mode, cx);
        }))
        .child(
            svg()
                .path("icons/s1-cover.svg")
                .size(px(20. * scale))
                .text_color(fg),
        )
}
