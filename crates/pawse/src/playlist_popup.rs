use std::collections::HashSet;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, ElementId, Entity, EventEmitter, FocusHandle, Global,
    InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, Pixels, Point,
    ScrollHandle, StatefulInteractiveElement, Styled, Subscription, Window, actions, anchored,
    deferred, div, point, px, svg,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    v_flex,
};

use crate::theme_colors::Colors;

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct OpenAddToPlaylist {
    pub track_id: i64,
    pub anchor: Point<Pixels>,
}

actions!(playlist_popup, [ClosePlaylistPopup]);

const POPUP_KEY_CONTEXT: &str = "PlaylistPopup";

/// Register the popup's escape binding once at app startup. Scoped to the
/// `PlaylistPopup` key context so it only fires while the popup is rendered
/// and a descendant (the popup container itself or one of its inputs) is
/// focused. The deepest-context input binding still runs first; we rely on
/// Input's `cx.propagate()` to let this binding fire next.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        ClosePlaylistPopup,
        Some(POPUP_KEY_CONTEXT),
    )]);
}

pub struct PlaylistPopupBus;

impl EventEmitter<OpenAddToPlaylist> for PlaylistPopupBus {}
impl Global for PlaylistPopupBus {}

/// Per-window popup that lets the user add a track to one of the existing
/// playlists, search/filter that list, or create a new playlist on the fly.
///
/// One instance lives in `Services` so any track-row "+" button can open it.
/// The popup itself is a `Render`er placed at the top of the view tree by
/// `MainView`; when `open == false` it returns an empty element.
pub struct PlaylistPopup {
    open: bool,
    track_id: Option<i64>,
    anchor: Point<Pixels>,
    playlists: Vec<music_library::PlaylistSummary>,
    containing: HashSet<i64>,
    /// Filter text mirrored from `filter_input`. Used for cheap fuzzy matching.
    filter: String,
    filter_input: Entity<InputState>,
    creating: bool,
    create_input: Entity<InputState>,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    _filter_subscription: Subscription,
    _create_subscription: Subscription,
    _library_subscription: Subscription,
    _bus_subscription: Subscription,
}

impl PlaylistPopup {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().filter_playlists.clone()));
        // No `clean_on_escape`: Input's escape handler propagates without
        // clean_on_escape, letting the popup's ClosePlaylistPopup action fire
        // next. Cancel button stays the explicit way to discard pending text.
        let create_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().playlist_name.clone()));

        let filter_subscription = cx.subscribe(&filter_input, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let value = this.filter_input.read(cx).value().to_string();
                this.filter = value;
                cx.notify();
            }
        });

        let create_subscription = cx.subscribe(&create_input, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.commit_create(cx);
            }
        });

        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let popup_bus = services.playlist_popup_bus.clone();
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                let refresh = match event {
                    LibraryEvent::PlaylistsChanged => true,
                    LibraryEvent::ScanComplete { changed } => *changed,
                    _ => false,
                };
                if refresh && this.open {
                    this.refresh_lists(cx);
                    cx.notify();
                }
            });

        let bus_subscription = cx.subscribe_in(
            &popup_bus,
            window,
            |this, _, ev: &OpenAddToPlaylist, window, cx| {
                this.open(ev.track_id, ev.anchor, window, cx);
            },
        );

        Self {
            open: false,
            track_id: None,
            anchor: point(px(0.), px(0.)),
            playlists: Vec::new(),
            containing: HashSet::new(),
            filter: String::new(),
            filter_input,
            creating: false,
            create_input,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            _filter_subscription: filter_subscription,
            _create_subscription: create_subscription,
            _library_subscription: library_subscription,
            _bus_subscription: bus_subscription,
        }
    }

    fn open(
        &mut self,
        track_id: i64,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = true;
        self.track_id = Some(track_id);
        self.anchor = anchor;
        self.creating = false;
        self.filter.clear();
        self.filter_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.create_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.refresh_lists(cx);
        self.focus_handle.focus(window);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.track_id = None;
        self.creating = false;
        cx.notify();
    }

    fn refresh_lists(&mut self, cx: &mut Context<Self>) {
        let Some(track_id) = self.track_id else {
            return;
        };
        let services = cx.global::<Services>();
        self.playlists = services.library.playlists();
        self.containing = services
            .library
            .playlists_containing_track(track_id)
            .into_iter()
            .collect();
    }

    fn filtered_playlists(&self) -> Vec<&music_library::PlaylistSummary> {
        if self.filter.is_empty() {
            return self.playlists.iter().collect();
        }
        let needle = self.filter.to_lowercase();
        self.playlists
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&needle))
            .collect()
    }

    fn commit_create(&mut self, cx: &mut Context<Self>) {
        let name = self.create_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let services = cx.global::<Services>();
        let Some(track_id) = self.track_id else {
            return;
        };
        let library = services.library.clone();
        let Some(playlist_id) = library.create_playlist(&name) else {
            return;
        };
        library.add_track_to_playlist(playlist_id, track_id);
        self.close(cx);
    }

    fn add_to(&mut self, playlist_id: i64, cx: &mut Context<Self>) {
        let Some(track_id) = self.track_id else {
            return;
        };
        if self.containing.contains(&playlist_id) {
            self.close(cx);
            return;
        }
        cx.global::<Services>()
            .library
            .add_track_to_playlist(playlist_id, track_id);
        self.close(cx);
    }
}

impl gpui::Render for PlaylistPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let popover_bg = Colors::popover(cx);
        let border_color = Colors::border(cx);
        let accent = Colors::accent(cx);
        let accent_fg = Colors::accent_foreground(cx);
        let secondary = Colors::secondary(cx);
        let muted_fg = Colors::muted_foreground(cx);
        let foreground = Colors::foreground(cx);
        let primary = Colors::primary(cx);

        let viewport = window.viewport_size();
        let entity_handle = cx.entity();

        // Full-window backdrop that closes the popup on any click outside it.
        let backdrop = {
            let popup = entity_handle.clone();
            div()
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(viewport.width)
                .h(viewport.height)
                .occlude()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    popup.update(cx, |state, cx| state.close(cx));
                })
        };

        let filtered = self.filtered_playlists();

        let list_items: Vec<AnyElement> = if filtered.is_empty() {
            vec![
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(if self.playlists.is_empty() {
                        tr().no_playlists_yet.clone()
                    } else {
                        tr().no_playlists_match.clone()
                    })
                    .into_any_element(),
            ]
        } else {
            filtered
                .iter()
                .map(|p| {
                    let playlist_id = p.id;
                    let name = p.name.clone();
                    let already = self.containing.contains(&playlist_id);
                    let popup = entity_handle.clone();
                    div()
                        .id(ElementId::NamedInteger(
                            "playlist-popup-item".into(),
                            playlist_id as u64,
                        ))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_sm()
                        .when(already, |d| d.text_color(muted_fg))
                        .hover(|s| s.bg(secondary))
                        .child(div().flex_1().truncate().child(name))
                        .when(already, |d| {
                            d.child(
                                svg()
                                    .path("icons/check.svg")
                                    .size(px(14.))
                                    .text_color(primary),
                            )
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, _, cx| {
                            popup.update(cx, |state, cx| state.add_to(playlist_id, cx));
                        })
                        .into_any_element()
                })
                .collect()
        };

        let create_row: AnyElement = if self.creating {
            let popup = entity_handle.clone();
            let cancel_popup = entity_handle.clone();
            v_flex()
                .gap_2()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(border_color)
                .child(
                    div()
                        .text_xs()
                        .text_color(muted_fg)
                        .child(tr().name_new_playlist.clone()),
                )
                .child(Input::new(&self.create_input).cleanable(false))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            div()
                                .id("playlist-popup-cancel")
                                .px_2()
                                .py_1()
                                .text_sm()
                                .text_color(muted_fg)
                                .cursor_pointer()
                                .rounded(px(4.))
                                .hover(|s| s.bg(secondary))
                                .child(tr().cancel.clone())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _, cx| {
                                    cancel_popup.update(cx, |state, cx| {
                                        state.creating = false;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            div()
                                .id("playlist-popup-create-confirm")
                                .px_2()
                                .py_1()
                                .text_sm()
                                .text_color(accent_fg)
                                .bg(accent)
                                .cursor_pointer()
                                .rounded(px(4.))
                                .child(tr().create.clone())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _, cx| {
                                    popup.update(cx, |state, cx| state.commit_create(cx));
                                }),
                        ),
                )
                .into_any_element()
        } else {
            let popup = entity_handle.clone();
            div()
                .id("playlist-popup-new-row")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .border_t_1()
                .border_color(border_color)
                .text_sm()
                .text_color(foreground)
                .cursor_pointer()
                .hover(|s| s.bg(secondary))
                .child(
                    svg()
                        .path("icons/s1-plus.svg")
                        .size(px(14.))
                        .text_color(foreground),
                )
                .child(div().child(tr().create_new_playlist.clone()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    popup.update(cx, |state, cx| {
                        state.creating = true;
                        state.create_input.update(cx, |s, cx| {
                            s.set_value("", window, cx);
                            s.focus(window, cx);
                        });
                        cx.notify();
                    });
                })
                .into_any_element()
        };

        let popup_content = v_flex()
            .id("playlist-popup-content")
            .key_context(POPUP_KEY_CONTEXT)
            .bg(popover_bg)
            .border_1()
            .border_color(border_color)
            .rounded(px(8.))
            .shadow_md()
            .w(px(280.))
            .occlude()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ClosePlaylistPopup, _, cx| this.close(cx)))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(border_color)
                    .child(Input::new(&self.filter_input).cleanable(true)),
            )
            .child(
                div()
                    .id("playlist-popup-list")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .p_1()
                    .children(list_items),
            )
            .child(create_row);

        let popup_layer = deferred(
            anchored()
                .snap_to_window_with_margin(px(8.))
                .position(self.anchor)
                .child(div().occlude().child(popup_content)),
        )
        .with_priority(2);

        let backdrop_layer =
            deferred(anchored().position(point(px(0.), px(0.))).child(backdrop)).with_priority(1);

        div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .size_full()
            .child(backdrop_layer)
            .child(popup_layer)
            .into_any_element()
    }
}
