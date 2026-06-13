use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, ElementId, Entity, EventEmitter, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Render, SharedString, Size, StatefulInteractiveElement,
    Styled, Subscription, Window, div, px, size, svg,
};
use gpui_component::{
    Sizable, VirtualListScrollHandle,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::{ScrollableElement, ScrollbarAxis},
    v_flex, v_virtual_list,
};
use nucleo_matcher::{Config, Matcher};

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;
use crate::track_list::LIKE_ROW_GROUP;

#[derive(Clone, Debug)]
pub struct PlaylistSelectedEvent {
    pub playlist: music_library::PlaylistSummary,
}

#[derive(Clone, Debug)]
pub struct AllTracksSelectedEvent;

enum PlaylistItem {
    TopPadding,
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

const TOP_PADDING: f32 = 12.;
const PLAYLIST_ROW_HEIGHT: f32 = 48.;
const ROW_ACTION_SIZE: f32 = 28.;

pub struct PlaylistsView {
    playlists_all: Vec<music_library::PlaylistSummary>,
    all_tracks_count: i64,
    all_tracks_count_label: SharedString,
    row_data: Vec<PlaylistRowData>,
    items: Vec<PlaylistItem>,
    filter: String,
    matcher: Matcher,
    creating: bool,
    create_input: Entity<InputState>,
    pending_delete_id: Option<i64>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    _subscription: Subscription,
    _create_subscription: Subscription,
}

impl PlaylistsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_event_bus = cx.global::<Services>().library_event_bus.clone();

        let create_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr().new_playlist_name.clone())
                .clean_on_escape()
        });

        let create_subscription = cx.subscribe(&create_input, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.commit_create(cx);
            }
        });

        let playlists_all = cx.global::<Services>().library.playlists();
        let all_tracks_count = cx.global::<Services>().library.track_count();
        let all_tracks_count_label: SharedString = tr().n_tracks(all_tracks_count).into();
        let row_data: Vec<PlaylistRowData> =
            playlists_all.iter().map(PlaylistRowData::new).collect();
        let (items, item_sizes) = Self::build_items(row_data.len());

        let subscription = cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
            let refresh = match event {
                LibraryEvent::PlaylistsChanged => true,
                LibraryEvent::ScanComplete { changed } => *changed,
                _ => false,
            };
            if refresh {
                let services = cx.global::<Services>();
                this.playlists_all = services.library.playlists();
                this.all_tracks_count = services.library.track_count();
                this.all_tracks_count_label = tr().n_tracks(this.all_tracks_count).into();
                this.recompute_visible();
                cx.notify();
            }
        });

        Self {
            playlists_all,
            all_tracks_count,
            all_tracks_count_label,
            row_data,
            items,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            creating: false,
            create_input,
            pending_delete_id: None,
            item_sizes: Rc::new(item_sizes),
            scroll_handle: VirtualListScrollHandle::new(),
            _subscription: subscription,
            _create_subscription: create_subscription,
        }
    }

    fn build_items(count: usize) -> (Vec<PlaylistItem>, Vec<Size<Pixels>>) {
        let mut items = vec![PlaylistItem::TopPadding];
        let mut sizes = vec![size(px(0.), px(TOP_PADDING))];
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
        let (items, sizes) = Self::build_items(self.row_data.len());
        self.items = items;
        self.item_sizes = Rc::new(sizes);
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
}

impl EventEmitter<PlaylistSelectedEvent> for PlaylistsView {}
impl EventEmitter<AllTracksSelectedEvent> for PlaylistsView {}

impl Render for PlaylistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::border(cx);
        let list_hover = Colors::list_hover(cx);
        let muted_fg = Colors::muted_foreground(cx);
        let danger_fg = Colors::foreground(cx);
        let icon_btn_hover = Colors::accent(cx);

        let create_section = if self.creating {
            v_flex().px_4().py_3().child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(240.))
                            .child(Input::new(&self.create_input).small().cleanable(false)),
                    )
                    .child(
                        Button::new("playlists-confirm-create")
                            .primary()
                            .compact()
                            .label(tr().create.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.commit_create(cx);
                            })),
                    )
                    .child(
                        Button::new("playlists-cancel-create")
                            .ghost()
                            .compact()
                            .label(tr().cancel.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.creating = false;
                                cx.notify();
                            })),
                    ),
            )
        } else {
            v_flex().px_4().py_3().child(
                h_flex().child(
                    Button::new("playlists-new")
                        .outline()
                        .compact()
                        .label(tr().new_playlist.clone())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.creating = true;
                            this.create_input.update(cx, |s, cx| {
                                s.set_value("", window, cx);
                                s.focus(window, cx);
                            });
                            cx.notify();
                        })),
                ),
            )
        };

        let all_tracks = (self.all_tracks_count > 0).then(|| {
            all_tracks_row(
                self.all_tracks_count_label.clone(),
                border,
                list_hover,
                muted_fg,
                danger_fg,
                cx,
            )
        });

        if self.row_data.is_empty() {
            let message = if self.playlists_all.is_empty() {
                tr().no_playlists_yet.clone()
            } else {
                tr().no_playlists_match.clone()
            };
            return v_flex()
                .size_full()
                .child(create_section)
                .children(all_tracks)
                .child(
                    div()
                        .px_4()
                        .py_2()
                        .text_sm()
                        .text_color(muted_fg)
                        .child(message),
                );
        }

        let params = PlaylistRowParams {
            border,
            list_hover,
            muted_fg,
            danger_fg,
            icon_btn_hover,
        };
        let item_sizes = self.item_sizes.clone();
        v_flex()
            .size_full()
            .child(create_section)
            .children(all_tracks)
            .child(
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
                                        PlaylistItem::TopPadding => {
                                            div().w_full().h(px(TOP_PADDING)).into_any_element()
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

fn all_tracks_row(
    count_label: SharedString,
    border: Hsla,
    list_hover: Hsla,
    muted_fg: Hsla,
    icon_fg: Hsla,
    cx: &mut Context<PlaylistsView>,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .h(px(PLAYLIST_ROW_HEIGHT))
        .px_4()
        .gap_3()
        .items_center()
        .border_b(px(1.))
        .border_color(border)
        .cursor_pointer()
        .hover(|s| s.bg(list_hover))
        .child(
            svg()
                .path("icons/placeholder-notes.svg")
                .size(px(20.))
                .text_color(icon_fg),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .truncate()
                .child(tr().all_tracks.clone()),
        )
        .child(div().text_sm().text_color(muted_fg).child(count_label))
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
                .truncate()
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
        .when(!pending_delete, |row| row.child(trash_button))
        .id(ElementId::Integer(playlist_id as u64))
        .on_click(cx.listener(move |this, _, _, cx| {
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
