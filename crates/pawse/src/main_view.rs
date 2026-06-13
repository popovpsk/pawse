use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt as _, AppContext, Context, DispatchPhase, DragMoveEvent, Empty, Entity,
    EntityId, FocusHandle, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, StatefulInteractiveElement,
    Styled, Subscription, Window, canvas, div, ease_out_quint, px, svg,
};
use gpui_component::{
    Icon, Root, Sizable, Size, StyledExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    theme::ThemeRegistry,
    tooltip::Tooltip,
};

use crate::audio_settings::AudioSettings;
use crate::cover_mode_view::CoverModeView;
use crate::footer::{Footer, ToggleQueueEvent};
use crate::keyboard_shortcuts::{
    ExitCoverMode, NextTrack, PlayPause, PreviousTrack, SeekBackward, SeekForward, VolumeDown,
    VolumeUp,
};
use crate::library_views::library_view::{LibraryRootTab, LibraryView, LibraryViewEvent};
use crate::localization::LangChanged;
use crate::localization::tr;
#[cfg(not(target_os = "macos"))]
use crate::media_bridge::MediaBridge;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::playlist_popup::PlaylistPopup;
use crate::queue_view::QueueView;
use crate::settings_store::SettingsStore;
use crate::settings_view::{LangPickerState, ThemePickerState};
use crate::theme_colors::Colors;
use ui_components::settings::SettingPage;

const HEADER_HEIGHT: f32 = 44.;
const FOOTER_HEIGHT: f32 = 80.;
const QUEUE_WIDTH_DEFAULT: f32 = 360.;
const QUEUE_WIDTH_MIN: f32 = 280.;
const QUEUE_WIDTH_MAX: f32 = 560.;
const QUEUE_ANIM: Duration = Duration::from_millis(200);

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

pub struct MainView {
    audio_settings: Entity<AudioSettings>,
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    queue_view: Entity<QueueView>,
    playlist_popup: Entity<PlaylistPopup>,
    is_drilled_in: bool,
    current_tab: LibraryRootTab,
    show_settings: bool,
    cover_mode: bool,
    cover_mode_view: Entity<CoverModeView>,
    show_queue: bool,
    queue_width: f32,
    queue_resize_origin: Option<(Pixels, f32)>,
    queue_closing: bool,
    _queue_close_task: Option<gpui::Task<()>>,
    settings_pages: Vec<SettingPage>,
    search_input: Entity<InputState>,
    _theme_picker: Entity<ThemePickerState>,
    _lang_picker: Entity<LangPickerState>,
    #[cfg(not(target_os = "macos"))]
    _media_bridge: Entity<MediaBridge>,
    _library_subscription: Subscription,
    _search_subscription: Subscription,
    _footer_subscription: Subscription,
    _footer_album_subscription: Subscription,
    _footer_artist_subscription: Subscription,
    _cover_album_subscription: Subscription,
    _cover_artist_subscription: Subscription,
    _cover_observe_subscription: Subscription,
    _shuffle_subscription: gpui::Subscription,
    _theme_registry_subscription: gpui::Subscription,
    _theme_picker_subscription: gpui::Subscription,
    _lang_picker_subscription: gpui::Subscription,
    _settings_observer: gpui::Subscription,
    _lang_subscription: Subscription,
    _activation_subscription: gpui::Subscription,
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
                LibraryViewEvent::AddMusicFolderRequested => {
                    crate::settings_view::pick_and_add_folder(cx);
                }
            },
        );

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().search_placeholder.clone()));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);

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
                        this.focus_handle.focus(window);
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

        let theme_registry_subscription = cx.observe_global::<ThemeRegistry>({
            let theme_picker = theme_picker.clone();
            let lang_picker = lang_picker.clone();
            move |this, cx| {
                theme_picker.update(cx, |state, cx| {
                    state.options = ThemePickerState::build_options(&*cx);
                    cx.notify();
                });
                this.settings_pages = crate::settings_view::build_settings_pages(
                    theme_picker.clone(),
                    lang_picker.clone(),
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

        let settings_pages =
            crate::settings_view::build_settings_pages(theme_picker.clone(), lang_picker.clone());

        let footer = cx.new(|cx| Footer::new(window, cx));
        let footer_subscription = cx.subscribe(&footer, |this, _, event: &ToggleQueueEvent, cx| {
            this.set_queue_visible(event.show, cx);
        });

        let footer_album_subscription = cx.subscribe_in(&footer, window, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToAlbumRequested, window, cx| {
                this.show_settings = false;
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, window, cx);
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

        let cover_mode_view = cx.new(|cx| CoverModeView::new(window, cx));
        let cover_album_subscription = cx.subscribe_in(&cover_mode_view, window, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, event: &NavigateToAlbumRequested, window, cx| {
                this.set_cover_mode(false, cx);
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, window, cx);
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
        let cover_observe_subscription = cx.observe(&cover_mode_view, |_, _, cx| cx.notify());

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
            move |this, cx| {
                this.settings_pages = crate::settings_view::build_settings_pages(
                    theme_picker.clone(),
                    lang_picker.clone(),
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
                        .request_rescan(folders, false);
                }
            }
        });

        Self {
            audio_settings: cx.new(|cx| AudioSettings::new(window, cx)),
            library_view,
            footer,
            queue_view,
            playlist_popup,
            is_drilled_in: false,
            current_tab: LibraryRootTab::Albums,
            show_settings: false,
            cover_mode: false,
            cover_mode_view,
            show_queue: false,
            queue_width: QUEUE_WIDTH_DEFAULT,
            queue_resize_origin: None,
            queue_closing: false,
            _queue_close_task: None,
            settings_pages,
            search_input,
            _theme_picker: theme_picker,
            _lang_picker: lang_picker,
            #[cfg(not(target_os = "macos"))]
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
            _library_subscription: library_subscription,
            _search_subscription: search_subscription,
            _footer_subscription: footer_subscription,
            _footer_album_subscription: footer_album_subscription,
            _footer_artist_subscription: footer_artist_subscription,
            _cover_album_subscription: cover_album_subscription,
            _cover_artist_subscription: cover_artist_subscription,
            _cover_observe_subscription: cover_observe_subscription,
            _shuffle_subscription: shuffle_subscription,
            _theme_registry_subscription: theme_registry_subscription,
            _theme_picker_subscription: theme_picker_subscription,
            _lang_picker_subscription: lang_picker_subscription,
            _settings_observer: settings_observer,
            _lang_subscription: lang_subscription,
            _activation_subscription: activation_subscription,
            focus_handle,
        }
    }

    fn clear_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let library_view = self.library_view.clone();
        library_view.update(cx, |v, cx| v.apply_search("", cx));
        self.focus_handle.focus(window);
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
        cx.notify();
    }

    fn toggle_cover_queue(&mut self, cx: &mut Context<Self>) {
        let show = !self.show_queue;
        self.footer.update(cx, |f, cx| f.set_show_queue(show, cx));
        self.set_queue_visible(show, cx);
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
        let (chrome_visible, corner_visible) = {
            let view = self.cover_mode_view.read(cx);
            (view.chrome_visible(), view.corner_visible())
        };

        let title_bar = Colors::title_bar(cx);
        let background = Colors::background(cx);
        let title_bar_bg = if cover_mode && !chrome_visible {
            background
        } else {
            title_bar
        };
        let muted = Colors::muted(cx);
        let foreground = Colors::foreground(cx);
        let tab_colors = TabColors {
            active_bg: Colors::secondary(cx),
            hover_bg: muted,
            primary: Colors::primary(cx),
            foreground,
        };

        let settings = cx.global::<SettingsStore>();
        let liked_enabled = settings.liked_enabled();
        let playlists_enabled = settings.playlists_enabled();

        let left_group = div()
            .flex_1()
            .flex()
            .items_center()
            .h_full()
            .gap_1()
            .when(has_back, |d| d.child(back_button(foreground, muted, cx)))
            .when(!has_back, |d| {
                d.child(tab_icon_button(
                    "tab_albums",
                    "icons/s1-albums.svg",
                    active_tab == Some(LibraryRootTab::Albums),
                    LibraryRootTab::Albums,
                    tab_colors,
                    cx,
                ))
                .child(tab_icon_button(
                    "tab_artists",
                    "icons/s1-artists.svg",
                    active_tab == Some(LibraryRootTab::Artists),
                    LibraryRootTab::Artists,
                    tab_colors,
                    cx,
                ))
                .when(liked_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_liked",
                        "icons/s1-heart.svg",
                        active_tab == Some(LibraryRootTab::Liked),
                        LibraryRootTab::Liked,
                        tab_colors,
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
                        cx,
                    ))
                })
                .child(cover_mode_button(cover_mode, tab_colors, cx))
            });

        let right_group = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .h_full()
            .when(!show_settings, |d| d.child(settings_gear_button(cx)))
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
            .when(cover_mode, |d| {
                d.on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                    this.cover_mode_view
                        .update(cx, |view, cx| view.handle_mouse_move(cx));
                }))
            })
            .child(crate::window_title_bar::WindowTitleBar::new().bg(title_bar_bg))
            .child({
                let header_bar = div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_2()
                    .bg(title_bar)
                    .child(left_group)
                    .when(!show_settings && !cover_mode, |d| {
                        d.child(
                            div().w(px(260.)).child(
                                Input::new(&self.search_input)
                                    .with_size(Size::Medium)
                                    .focus_bordered(false)
                                    .rounded_full()
                                    .bg(title_bar),
                            ),
                        )
                    })
                    .child(right_group);

                let middle = div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .bg(background)
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .when(!cover_mode, |d| d.ml_4())
                            .when(!cover_mode && !self.show_queue, |d| d.mr_4())
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
                                    ))
                                    .into_any_element()
                            } else {
                                self.library_view.clone().into_any_element()
                            })
                            .when(cover_mode && (chrome_visible || corner_visible), |d| {
                                d.relative()
                                    .child(cover_chrome_button(chrome_visible, tab_colors, cx))
                                    .when(!chrome_visible, |d| {
                                        d.child(cover_queue_button(self.show_queue, tab_colors, cx))
                                    })
                            }),
                    )
                    .when(self.show_queue || self.queue_closing, |d| {
                        let queue_width = self.queue_width;
                        let closing = self.queue_closing;
                        d.child(
                            div()
                                .flex_shrink_0()
                                .border_l(px(1.))
                                .border_color(Colors::border(cx))
                                .relative()
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
                    .h(px(FOOTER_HEIGHT))
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

fn settings_gear_button(cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("settings_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40.))
        .h(px(40.))
        .icon(Icon::default().path("icons/settings.svg").size(px(20.)))
        .tooltip(tr().settings.clone())
        .on_click(cx.listener(|this, _, window, cx| {
            this.leave_overlays(window, cx);
            this.show_settings = true;
            cx.notify();
        }))
}

fn back_button(fg: Hsla, hover_bg: Hsla, cx: &mut Context<MainView>) -> impl IntoElement {
    div()
        .id("back_button")
        .size(px(36.))
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
        .child(svg().path("icons/back.svg").size(px(22.)).text_color(fg))
}

fn tab_icon_button(
    id: &'static str,
    icon_path: &'static str,
    active: bool,
    tab: LibraryRootTab,
    colors: TabColors,
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
        .size(px(36.))
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
        .child(svg().path(icon_path).size(px(20.)).text_color(fg))
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
        }))
        .child(svg().path(icon).size(px(20.)).text_color(fg))
}

fn cover_queue_button(
    show_queue: bool,
    colors: TabColors,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let fg = if show_queue {
        colors.primary
    } else {
        colors.foreground
    };
    let hover_bg = colors.hover_bg;

    div()
        .id("cover_queue_toggle")
        .absolute()
        .bottom(px(12.))
        .right(px(12.))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .tooltip(|window, cx| Tooltip::new(tr().queue.clone()).build(window, cx))
        .hover(move |s| s.bg(hover_bg))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_cover_queue(cx)))
        .child(
            svg()
                .path("icons/s2-queue.svg")
                .size(px(20.))
                .text_color(fg),
        )
}

fn cover_mode_button(
    active: bool,
    colors: TabColors,
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
        .size(px(36.))
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
                .size(px(20.))
                .text_color(fg),
        )
}
