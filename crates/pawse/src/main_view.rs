use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, DispatchPhase, DragMoveEvent, Empty, Entity, EntityId, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, px, svg,
};
use gpui_component::{
    ActiveTheme, Icon, Root, Sizable, Size, StyledExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    setting::SettingPage,
    theme::ThemeRegistry,
};

use crate::audio_settings::AudioSettings;
use crate::footer::{Footer, ToggleQueueEvent};
use crate::library_views::library_view::{LibraryRootTab, LibraryView, LibraryViewEvent};
use crate::media_bridge::MediaBridge;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::playlist_popup::PlaylistPopup;
use crate::queue_view::QueueView;
use crate::settings_store::SettingsStore;
use crate::settings_view::ThemePickerState;
use ui_components::fade::{FadeEdge, fade_overlay};

const HEADER_HEIGHT: f32 = 44.0;
const FOOTER_HEIGHT: f32 = 80.0;
const FADE_HEIGHT: f32 = 16.0;
const QUEUE_WIDTH_DEFAULT: f32 = 360.0;
const QUEUE_WIDTH_MIN: f32 = 200.0;
const QUEUE_WIDTH_MAX: f32 = 560.0;

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
    show_queue: bool,
    queue_width: f32,
    queue_resize_origin: Option<(Pixels, f32)>,
    settings_pages: Vec<SettingPage>,
    search_input: Entity<InputState>,
    _theme_picker: Entity<ThemePickerState>,
    _media_bridge: Entity<MediaBridge>,
    _library_subscription: Subscription,
    _search_subscription: Subscription,
    _footer_subscription: Subscription,
    _footer_album_subscription: Subscription,
    _footer_artist_subscription: Subscription,
    _shuffle_subscription: gpui::Subscription,
    _theme_registry_subscription: gpui::Subscription,
    _theme_picker_subscription: gpui::Subscription,
    _settings_observer: gpui::Subscription,
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
                LibraryViewEvent::OpenSettingsRequested => {
                    this.show_settings = true;
                    cx.notify();
                }
            },
        );

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search artists, albums, tracks"));

        let search_subscription = cx.subscribe(&search_input, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    let query = this.search_input.read(cx).value().to_string();
                    library_view.update(cx, |v, cx| v.apply_search(&query, cx));
                }
            }
        });

        let theme_picker: Entity<ThemePickerState> = cx.new(|cx| ThemePickerState::new(cx));

        let theme_registry_subscription = cx.observe_global::<ThemeRegistry>({
            let theme_picker = theme_picker.clone();
            move |this, cx| {
                theme_picker.update(cx, |state, cx| {
                    state.options = ThemePickerState::build_options(&*cx);
                    cx.notify();
                });
                this.settings_pages =
                    crate::settings_view::build_settings_pages(&*cx, theme_picker.clone());
                cx.notify();
            }
        });

        let theme_picker_subscription = cx.observe(&theme_picker, |_, _, cx| {
            cx.notify();
        });

        let settings_pages = crate::settings_view::build_settings_pages(&*cx, theme_picker.clone());

        let footer = cx.new(|cx| Footer::new(window, cx));
        let footer_subscription = cx.subscribe(&footer, |this, _, event: &ToggleQueueEvent, cx| {
            this.show_queue = event.show;
            cx.notify();
        });

        let footer_album_subscription = cx.subscribe_in(&footer, window, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToAlbumRequested, window, cx| {
                this.show_settings = false;
                library_view.update(cx, |view, cx| {
                    view.navigate_to_album(event.album_id, window, cx);
                });
            }
        });

        let footer_artist_subscription = cx.subscribe(&footer, {
            let library_view = library_view.clone();
            move |this, _, event: &NavigateToArtistRequested, cx| {
                this.show_settings = false;
                library_view.update(cx, |view, cx| {
                    view.navigate_to_artist(event.artist_id, cx);
                });
            }
        });

        let queue_view = cx.new(|cx| QueueView::new(window, cx));

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

        let settings_observer = cx.observe_global::<SettingsStore>(|_, cx| {
            // Re-render so the tab strip flips visibility when feature flags
            // are toggled in settings.
            cx.notify();
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
            show_queue: false,
            queue_width: QUEUE_WIDTH_DEFAULT,
            queue_resize_origin: None,
            settings_pages,
            search_input,
            _theme_picker: theme_picker,
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
            _library_subscription: library_subscription,
            _search_subscription: search_subscription,
            _footer_subscription: footer_subscription,
            _footer_album_subscription: footer_album_subscription,
            _footer_artist_subscription: footer_artist_subscription,
            _shuffle_subscription: shuffle_subscription,
            _theme_registry_subscription: theme_registry_subscription,
            _theme_picker_subscription: theme_picker_subscription,
            _settings_observer: settings_observer,
        }
    }

    fn clear_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let library_view = self.library_view.clone();
        library_view.update(cx, |v, cx| v.apply_search("", cx));
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let library_view = self.library_view.clone();
        let show_settings = self.show_settings;
        let has_back = show_settings || self.is_drilled_in;
        let current_tab = self.current_tab;

        let back_button = div()
            .id("back_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.clear_search(window, cx);
                if this.show_settings {
                    this.show_settings = false;
                    cx.notify();
                } else {
                    library_view.update(cx, |view, cx| view.go_back(cx));
                }
            }))
            .child(
                svg()
                    .path("icons/back.svg")
                    .size(px(22.))
                    .text_color(cx.theme().foreground),
            );

        let liked_enabled = cx.global::<SettingsStore>().liked_enabled();
        let playlists_enabled = cx.global::<SettingsStore>().playlists_enabled();

        let left_group = div()
            .flex_1()
            .flex()
            .items_center()
            .h_full()
            .gap_1()
            .when(has_back, |d| d.child(back_button))
            .when(!has_back, |d| {
                d.child(tab_icon_button(
                    "tab_albums",
                    "icons/s1-albums.svg",
                    "Albums",
                    current_tab == LibraryRootTab::Albums,
                    LibraryRootTab::Albums,
                    cx,
                ))
                .child(tab_icon_button(
                    "tab_artists",
                    "icons/s1-artists.svg",
                    "Artists",
                    current_tab == LibraryRootTab::Artists,
                    LibraryRootTab::Artists,
                    cx,
                ))
                .when(liked_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_liked",
                        "icons/s1-heart-fill.svg",
                        "Liked",
                        current_tab == LibraryRootTab::Liked,
                        LibraryRootTab::Liked,
                        cx,
                    ))
                })
                .when(playlists_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_playlists",
                        "icons/s1-playlists.svg",
                        "Playlists",
                        current_tab == LibraryRootTab::Playlists,
                        LibraryRootTab::Playlists,
                        cx,
                    ))
                })
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
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_2()
                    .bg(cx.theme().title_bar)
                    .child(left_group)
                    .when(!show_settings, |d| {
                        d.child(
                            div().w(px(260.)).child(
                                Input::new(&self.search_input)
                                    .with_size(Size::Medium)
                                    .focus_bordered(false)
                                    .rounded_full()
                                    .cleanable(true)
                                    .bg(cx.theme().title_bar),
                            ),
                        )
                    })
                    .child(right_group),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .bg(cx.theme().title_bar)
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .ml_4()
                            .when(!self.show_queue, |d| d.mr_4())
                            .child(if show_settings {
                                // Match gpui-component's StoryContainer wrap chain
                                // exactly: outer `size_full` + Scrollable, then inner
                                // `size_full`, then Settings. The Scrollable wrap is
                                // what gives `h_resizable` panels a definite-width
                                // parent so flex_basis on the sidebar panel resolves.
                                div()
                                    .size_full()
                                    .overflow_y_scrollbar()
                                    .child(div().size_full().child(
                                        crate::settings_view::settings_widget(
                                            self.settings_pages.clone(),
                                            cx,
                                        ),
                                    ))
                                    .into_any_element()
                            } else {
                                self.library_view.clone().into_any_element()
                            }),
                    )
                    .when(self.show_queue, |d| {
                        let queue_width = self.queue_width;
                        d.child(
                            div()
                                .w(px(queue_width))
                                .flex_shrink_0()
                                .border_l(px(1.))
                                .border_color(cx.theme().border)
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
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(FOOTER_HEIGHT))
                    .child(self.footer.clone()),
            )
            .when(!show_settings, |d| {
                d.child(fade_overlay(
                    FadeEdge::Top,
                    cx.theme().title_bar,
                    FADE_HEIGHT,
                    HEADER_HEIGHT,
                ))
                .child(fade_overlay(
                    FadeEdge::Bottom,
                    cx.theme().background,
                    FADE_HEIGHT,
                    FOOTER_HEIGHT,
                ))
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
        .tooltip("Settings")
        .on_click(cx.listener(|this, _, window, cx| {
            this.clear_search(window, cx);
            this.show_settings = true;
            cx.notify();
        }))
}

fn tab_icon_button(
    id: &'static str,
    icon_path: &'static str,
    _tooltip: &'static str,
    active: bool,
    tab: LibraryRootTab,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let theme = cx.theme();
    let active_bg = theme.secondary;
    let hover_bg = theme.muted;
    let fg = if active {
        theme.primary
    } else {
        theme.foreground
    };

    div()
        .id(id)
        .cursor_pointer()
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(active, |d| d.bg(active_bg))
        .hover(|s| s.bg(hover_bg))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.clear_search(window, cx);
            this.library_view
                .update(cx, |view, cx| view.select_tab(tab, cx));
            cx.notify();
        }))
        .child(svg().path(icon_path).size(px(20.)).text_color(fg))
}
