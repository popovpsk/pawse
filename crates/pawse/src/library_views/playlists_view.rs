use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, ElementId, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString, Size,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, size, svg,
};
use gpui_component::{
    Icon, Sizable, VirtualListScrollHandle,
    button::{Button, ButtonVariants},
    h_flex,
    input::{self, Input, InputEvent, InputState},
    scroll::{ScrollableElement, ScrollbarAxis},
    tooltip::Tooltip,
    v_flex, v_virtual_list,
};
use nucleo_matcher::{Config, Matcher};
use ui_resources::i18n::playlist_import_strings;

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::library_views::playlist_import::{self, PlaylistImport};
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;
use crate::track_list::LIKE_ROW_GROUP;

#[derive(Clone, Debug)]
pub struct PlaylistSelectedEvent {
    pub playlist: music_library::PlaylistSummary,
}

#[derive(Clone, Debug)]
pub struct AllTracksSelectedEvent;

enum PlaylistItem {
    NewPlaylist,
    AllTracks,
    Gap,
    Playlist(usize),
}

struct PlaylistRowData {
    id: i64,
    name: SharedString,
    count_label: SharedString,
}

impl PlaylistRowData {
    fn new(summary: &music_library::PlaylistSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name.clone().into(),
            count_label: tr().n_tracks(summary.track_count).into(),
        }
    }
}

struct PlaylistRowParams {
    border: Hsla,
    list_hover: Hsla,
    muted_fg: Hsla,
    danger_fg: Hsla,
    icon_btn_hover: Hsla,
}

const PLAYLISTS_GAP: f32 = 12.;
const PLAYLIST_ROW_HEIGHT: f32 = 48.;
const ROW_ACTION_SIZE: f32 = 28.;
const ACTIONS_HEIGHT: f32 = 40.;
const ACTIONS_PADDING: f32 = 8.;

pub struct PlaylistsView {
    playlists_all: Vec<music_library::PlaylistSummary>,
    all_tracks_count: i64,
    all_tracks_count_label: SharedString,
    row_data: Vec<PlaylistRowData>,
    items: Vec<PlaylistItem>,
    filter: String,
    matcher: Matcher,
    creating: bool,
    create_has_text: bool,
    create_input: Entity<InputState>,
    pending_delete_id: Option<i64>,
    renaming_id: Option<i64>,
    rename_has_text: bool,
    rename_input: Entity<InputState>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    can_import: bool,
    import: Entity<PlaylistImport>,
    _subscription: Subscription,
    _create_subscription: Subscription,
    _rename_subscription: Subscription,
    _settings_observer: Subscription,
}

impl PlaylistsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_event_bus = cx.global::<Services>().library_event_bus.clone();

        let create_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().new_playlist_name.clone()));

        let create_subscription = cx.subscribe(
            &create_input,
            |this, input, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_create(cx),
                InputEvent::Change => {
                    let has_text = !input.read(cx).value().trim().is_empty();
                    if this.create_has_text != has_text {
                        this.create_has_text = has_text;
                        cx.notify();
                    }
                }
                InputEvent::Blur if input.read(cx).value().trim().is_empty() => {
                    this.cancel_create(cx)
                }
                _ => {}
            },
        );

        let rename_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().playlist_name.clone()));

        let rename_subscription = cx.subscribe(
            &rename_input,
            |this, input, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_rename(cx),
                InputEvent::Change => {
                    let has_text = !input.read(cx).value().trim().is_empty();
                    if this.rename_has_text != has_text {
                        this.rename_has_text = has_text;
                        cx.notify();
                    }
                }
                InputEvent::Blur if this.rename_is_noop(cx) => this.cancel_rename(cx),
                _ => {}
            },
        );

        let playlists_all = cx.global::<Services>().library.playlists();
        let all_tracks_count = cx.global::<Services>().library.track_count();
        let all_tracks_count_label: SharedString = tr().n_tracks(all_tracks_count).into();
        let row_data: Vec<PlaylistRowData> =
            playlists_all.iter().map(PlaylistRowData::new).collect();
        let (items, item_sizes) = Self::build_items(row_data.len(), all_tracks_count > 0);

        let subscription = cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
            let refresh = matches!(
                event,
                LibraryEvent::PlaylistsChanged | LibraryEvent::CatalogChanged
            );
            if refresh {
                let services = cx.global::<Services>();
                this.playlists_all = services.library.playlists();
                this.all_tracks_count = services.library.track_count();
                this.all_tracks_count_label = tr().n_tracks(this.all_tracks_count).into();
                if let Some(id) = this.renaming_id
                    && !this.playlists_all.iter().any(|p| p.id == id)
                {
                    this.renaming_id = None;
                }
                this.recompute_visible();
                cx.notify();
            }
        });

        let settings_observer = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let can_import = playlist_import::available(cx);
            if this.can_import != can_import {
                this.can_import = can_import;
                cx.notify();
            }
        });
        let can_import = playlist_import::available(cx);
        let import = cx.new(PlaylistImport::new);

        Self {
            playlists_all,
            all_tracks_count,
            all_tracks_count_label,
            row_data,
            items,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            creating: false,
            create_has_text: false,
            create_input,
            pending_delete_id: None,
            renaming_id: None,
            rename_has_text: false,
            rename_input,
            item_sizes: Rc::new(item_sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            can_import,
            import,
            _subscription: subscription,
            _create_subscription: create_subscription,
            _rename_subscription: rename_subscription,
            _settings_observer: settings_observer,
        }
    }

    fn build_items(count: usize, all_tracks: bool) -> (Vec<PlaylistItem>, Vec<Size<Pixels>>) {
        let header = size(px(0.), px(PLAYLIST_ROW_HEIGHT));
        let mut items = vec![PlaylistItem::NewPlaylist];
        let mut sizes = vec![header];
        if all_tracks {
            items.push(PlaylistItem::AllTracks);
            sizes.push(header);
        }
        items.push(PlaylistItem::Gap);
        sizes.push(size(px(0.), px(PLAYLISTS_GAP)));
        for ix in 0..count {
            items.push(PlaylistItem::Playlist(ix));
            sizes.push(size(px(0.), px(PLAYLIST_ROW_HEIGHT + 1.)));
        }
        (items, sizes)
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible();
        self.scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    fn recompute_visible(&mut self) {
        if self.filter.is_empty() {
            self.row_data = self
                .playlists_all
                .iter()
                .map(PlaylistRowData::new)
                .collect();
        } else {
            let indices = fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.playlists_all
                    .iter()
                    .enumerate()
                    .map(|(ix, p)| (ix, p.name.as_str())),
            );
            self.row_data = indices
                .into_iter()
                .map(|ix| PlaylistRowData::new(&self.playlists_all[ix]))
                .collect();
        }
        let (items, sizes) = Self::build_items(self.row_data.len(), self.all_tracks_count > 0);
        self.items = items;
        self.item_sizes = Rc::new(sizes);
    }

    fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating = true;
        self.create_has_text = false;
        self.create_input.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn cancel_create(&mut self, cx: &mut Context<Self>) {
        if !self.creating {
            return;
        }
        self.creating = false;
        cx.notify();
    }

    fn commit_create(&mut self, cx: &mut Context<Self>) {
        let name = self.create_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        cx.global::<Services>().library.create_playlist(&name);
        self.creating = false;
        cx.notify();
    }

    fn start_rename(&mut self, playlist_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .playlists_all
            .iter()
            .find(|p| p.id == playlist_id)
            .map(|p| p.name.clone())
        else {
            return;
        };
        self.pending_delete_id = None;
        self.renaming_id = Some(playlist_id);
        self.rename_has_text = !name.trim().is_empty();
        self.rename_input.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming_id.take().is_some() {
            cx.notify();
        }
    }

    fn rename_is_noop(&self, cx: &Context<Self>) -> bool {
        let Some(id) = self.renaming_id else {
            return false;
        };
        let value = self.rename_input.read(cx).value();
        let value = value.trim();
        value.is_empty()
            || self
                .playlists_all
                .iter()
                .any(|p| p.id == id && p.name == value)
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.renaming_id else {
            return;
        };
        let name = self.rename_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        if !self.rename_is_noop(cx) {
            cx.global::<Services>().library.rename_playlist(id, &name);
        }
        self.renaming_id = None;
        cx.notify();
    }
}

impl EventEmitter<PlaylistSelectedEvent> for PlaylistsView {}
impl EventEmitter<AllTracksSelectedEvent> for PlaylistsView {}

impl Render for PlaylistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let params = PlaylistRowParams {
            border: Colors::border(cx),
            list_hover: Colors::list_hover(cx),
            muted_fg: Colors::muted_foreground(cx),
            danger_fg: Colors::foreground(cx),
            icon_btn_hover: Colors::accent(cx),
        };
        let actions = actions_row(self.can_import, cx);

        if self.row_data.is_empty() {
            let show_empty_state = self.playlists_all.is_empty() && !self.creating;
            let new_row = (!show_empty_state).then(|| new_playlist_row(self, &params, cx));
            let all_tracks = (self.all_tracks_count > 0).then(|| all_tracks_row(self, &params, cx));
            let body = if show_empty_state {
                empty_state(params.muted_fg, cx).into_any_element()
            } else if self.playlists_all.is_empty() {
                div().into_any_element()
            } else {
                div()
                    .px_4()
                    .py_2()
                    .text_sm()
                    .text_color(params.muted_fg)
                    .child(tr().no_playlists_match.clone())
                    .into_any_element()
            };
            return v_flex()
                .size_full()
                .child(actions)
                .children(new_row)
                .children(all_tracks)
                .child(body);
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(actions).child(
            v_flex()
                .relative()
                .flex_1()
                .child(
                    v_virtual_list(
                        cx.entity().clone(),
                        "playlists_list",
                        item_sizes,
                        move |view, visible_range, _window, cx| {
                            visible_range
                                .map(|ix| match view.items[ix] {
                                    PlaylistItem::NewPlaylist => {
                                        new_playlist_row(view, &params, cx)
                                    }
                                    PlaylistItem::AllTracks => all_tracks_row(view, &params, cx),
                                    PlaylistItem::Gap => {
                                        div().w_full().h(px(PLAYLISTS_GAP)).into_any_element()
                                    }
                                    PlaylistItem::Playlist(row_ix) => {
                                        playlist_row(view, row_ix, &params, cx)
                                    }
                                })
                                .collect::<Vec<_>>()
                        },
                    )
                    .track_scroll(&self.scroll_handle)
                    .flex_1(),
                )
                .scrollbar(&self.scroll_handle, ScrollbarAxis::Vertical),
        )
    }
}

fn actions_row(can_import: bool, cx: &mut Context<PlaylistsView>) -> gpui::AnyElement {
    let strings = playlist_import_strings();
    h_flex()
        .w_full()
        .h(px(ACTIONS_HEIGHT))
        .flex_shrink_0()
        .px(px(ACTIONS_PADDING))
        .gap_1()
        .items_center()
        .justify_end()
        .when(can_import, |row| {
            row.child(
                Button::new("playlists-import")
                    .ghost()
                    .icon(Icon::default().path("icons/cloud-download.svg"))
                    .label(strings.import.clone())
                    .tooltip(strings.import_tooltip.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        playlist_import::open(this.import.clone(), window, cx);
                    })),
            )
        })
        .child(
            Button::new("playlists-ai")
                .ghost()
                .icon(Icon::default().path("icons/sparkles.svg"))
                .tooltip(strings.ai_tooltip.clone())
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::tools::OpenAiPlaylist), cx);
                }),
        )
        .into_any_element()
}

fn new_playlist_row(
    view: &PlaylistsView,
    p: &PlaylistRowParams,
    cx: &mut Context<PlaylistsView>,
) -> gpui::AnyElement {
    let row = h_flex()
        .id("playlists-new")
        .w_full()
        .h(px(PLAYLIST_ROW_HEIGHT))
        .flex_shrink_0()
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .child(
            svg()
                .path("icons/s1-plus.svg")
                .size(px(20.))
                .text_color(p.muted_fg),
        );

    if view.creating {
        return row
            .on_action(cx.listener(|this, _: &input::Escape, _, cx| this.cancel_create(cx)))
            .child(
                div().flex_1().child(
                    Input::new(&view.create_input)
                        .small()
                        .appearance(false)
                        .cleanable(false),
                ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        create_row_button(
                            "playlists-new-confirm",
                            "icons/check.svg",
                            tr().create.clone(),
                            if view.create_has_text {
                                Colors::primary(cx)
                            } else {
                                p.muted_fg.opacity(0.5)
                            },
                            p.icon_btn_hover,
                            view.create_has_text,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.commit_create(cx))),
                    )
                    .child(
                        create_row_button(
                            "playlists-new-cancel",
                            "icons/s1-x.svg",
                            tr().cancel.clone(),
                            p.muted_fg,
                            p.icon_btn_hover,
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_create(cx))),
                    ),
            )
            .into_any_element();
    }

    row.cursor_pointer()
        .hover(|s| s.bg(p.list_hover))
        .child(
            div()
                .flex_1()
                .text_color(p.muted_fg)
                .child(tr().new_playlist.clone()),
        )
        .on_click(cx.listener(|this, _, window, cx| this.start_create(window, cx)))
        .into_any_element()
}

fn renaming_row(
    view: &PlaylistsView,
    playlist_id: i64,
    p: &PlaylistRowParams,
    cx: &mut Context<PlaylistsView>,
) -> gpui::AnyElement {
    h_flex()
        .id(ElementId::NamedInteger(
            "pl-renaming".into(),
            playlist_id as u64,
        ))
        .w_full()
        .h(px(PLAYLIST_ROW_HEIGHT))
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .on_action(cx.listener(|this, _: &input::Escape, _, cx| this.cancel_rename(cx)))
        .child(
            svg()
                .path("icons/s1-playlists.svg")
                .size(px(20.))
                .text_color(p.danger_fg),
        )
        .child(
            div().flex_1().child(
                Input::new(&view.rename_input)
                    .small()
                    .appearance(false)
                    .cleanable(false),
            ),
        )
        .child(
            h_flex()
                .gap_1()
                .child(
                    create_row_button(
                        "pl-rename-confirm",
                        "icons/check.svg",
                        tr().rename.clone(),
                        if view.rename_has_text {
                            Colors::primary(cx)
                        } else {
                            p.muted_fg.opacity(0.5)
                        },
                        p.icon_btn_hover,
                        view.rename_has_text,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.commit_rename(cx))),
                )
                .child(
                    create_row_button(
                        "pl-rename-cancel",
                        "icons/s1-x.svg",
                        tr().cancel.clone(),
                        p.muted_fg,
                        p.icon_btn_hover,
                        true,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_rename(cx))),
                ),
        )
        .into_any_element()
}

fn create_row_button(
    id: &'static str,
    icon: &'static str,
    tooltip: SharedString,
    icon_color: Hsla,
    hover_bg: Hsla,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(ROW_ACTION_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(enabled, |d| {
            d.cursor_pointer().hover(move |s| s.bg(hover_bg))
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(svg().path(icon).size(px(16.)).text_color(icon_color))
}

fn empty_state(muted_fg: Hsla, cx: &mut Context<PlaylistsView>) -> gpui::Div {
    v_flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_3()
        .px_8()
        .pb_16()
        .child(
            svg()
                .path("icons/s1-playlists.svg")
                .size(px(40.))
                .text_color(muted_fg),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(tr().no_playlists_yet.clone()),
        )
        .child(
            div()
                .max_w(px(360.))
                .text_sm()
                .text_center()
                .text_color(muted_fg)
                .child(tr().playlists_empty_hint.clone()),
        )
        .child(
            div().pt_2().child(
                Button::new("playlists-empty-create")
                    .primary()
                    .small()
                    .icon(Icon::default().path("icons/s1-plus.svg"))
                    .label(tr().new_playlist.clone())
                    .on_click(cx.listener(|this, _, window, cx| this.start_create(window, cx))),
            ),
        )
}

fn all_tracks_row(
    view: &PlaylistsView,
    p: &PlaylistRowParams,
    cx: &mut Context<PlaylistsView>,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .h(px(PLAYLIST_ROW_HEIGHT))
        .flex_shrink_0()
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .cursor_pointer()
        .hover(|s| s.bg(p.list_hover))
        .child(
            svg()
                .path("icons/placeholder-notes.svg")
                .size(px(20.))
                .text_color(p.danger_fg),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .child(tr().all_tracks.clone()),
        )
        .child(
            div()
                .text_sm()
                .text_color(p.muted_fg)
                .child(view.all_tracks_count_label.clone()),
        )
        .child(div().size(px(ROW_ACTION_SIZE)))
        .id("playlists-all-tracks")
        .on_click(cx.listener(|_, _, _, cx| {
            cx.emit(AllTracksSelectedEvent);
        }))
        .into_any_element()
}

fn playlist_row(
    view: &mut PlaylistsView,
    row_ix: usize,
    p: &PlaylistRowParams,
    cx: &mut Context<PlaylistsView>,
) -> gpui::AnyElement {
    let row = &view.row_data[row_ix];
    let playlist_id = row.id;
    let count_label = row.count_label.clone();
    let pending_delete = view.pending_delete_id == Some(playlist_id);

    if view.renaming_id == Some(playlist_id) {
        return renaming_row(view, playlist_id, p, cx);
    }

    let pencil_button = div()
        .id(ElementId::NamedInteger(
            "pl-rename".into(),
            playlist_id as u64,
        ))
        .size(px(ROW_ACTION_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .opacity(0.)
        .group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        .hover(|s| s.bg(p.icon_btn_hover))
        .tooltip(|window, cx| Tooltip::new(tr().rename.clone()).build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, window, cx| {
            this.start_rename(playlist_id, window, cx);
        }))
        .child(
            svg()
                .path("icons/s1-pencil.svg")
                .size(px(15.))
                .text_color(p.danger_fg),
        );

    let trash_button = div()
        .id(ElementId::NamedInteger(
            "pl-trash".into(),
            playlist_id as u64,
        ))
        .size(px(ROW_ACTION_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .when(!pending_delete, |d| {
            d.opacity(0.).group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        })
        .hover(|s| s.bg(p.icon_btn_hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pending_delete_id = Some(playlist_id);
            cx.notify();
        }))
        .child(
            svg()
                .path("icons/s1-trash.svg")
                .size(px(15.))
                .text_color(p.danger_fg),
        );

    h_flex()
        .group(LIKE_ROW_GROUP)
        .w_full()
        .h(px(PLAYLIST_ROW_HEIGHT))
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .hover(|s| s.bg(p.list_hover))
        .child(
            svg()
                .path("icons/s1-playlists.svg")
                .size(px(20.))
                .text_color(p.danger_fg),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .child(row.name.clone()),
        )
        .child(div().text_sm().text_color(p.muted_fg).child(count_label))
        .when(pending_delete, |row| {
            let pid = playlist_id;
            row.child(
                h_flex()
                    .gap_2()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Button::new(ElementId::NamedInteger("pl-del-confirm".into(), pid as u64))
                            .danger()
                            .compact()
                            .label(tr().delete.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.global::<Services>().library.delete_playlist(pid);
                                this.pending_delete_id = None;
                            })),
                    )
                    .child(
                        Button::new(ElementId::NamedInteger("pl-del-cancel".into(), pid as u64))
                            .ghost()
                            .compact()
                            .label(tr().cancel.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pending_delete_id = None;
                                cx.notify();
                            })),
                    ),
            )
        })
        .when(!pending_delete, |row| {
            row.child(pencil_button).child(trash_button)
        })
        .id(ElementId::Integer(playlist_id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.renaming_id.is_some() {
                this.cancel_rename(cx);
                return;
            }
            if this.pending_delete_id.is_some() {
                return;
            }
            if let Some(playlist) = this
                .playlists_all
                .iter()
                .find(|p| p.id == playlist_id)
                .cloned()
            {
                cx.emit(PlaylistSelectedEvent { playlist });
            }
        }))
        .into_any_element()
}
