use std::rc::Rc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, ElementId, Entity, EventEmitter, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, SharedString, Size, StatefulInteractiveElement,
    Styled, Subscription, Window, div, px, size, svg,
};
use gpui_component::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::theme_colors::Colors;
use crate::track_list::{
    LIKE_ROW_GROUP, RowButtonColors, TrackRowBase, add_to_playlist_button, add_to_queue_button,
    fmt_track_num, like_button, track_duration,
};
use nucleo_matcher::{Config, Matcher};

use crate::library_service::LibraryEvent;
use crate::library_views::album_info::AlbumInfo;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::localization::{LangChanged, tr};
use crate::now_playing::NavigateToArtistRequested;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const TOP_PADDING: f32 = 12.;
const TRACK_ROW_HEIGHT: f32 = 36.;
const DISC_HEADER_HEIGHT: f32 = 32.;
const DISC_HEADER_GAP: f32 = 24.;
const ALBUM_INFO_HEIGHT: f32 = 170.;

#[derive(Clone)]
enum TrackItem {
    TopPadding,
    AlbumInfo,
    DiscHeader(SharedString, bool),
    Track(usize),
}

struct TrackRow {
    base: TrackRowBase,
    track_all_ix: usize,
    track_num_str: SharedString,
    disc_number: i32,
}

impl TrackRow {
    fn from_track(track: &music_library::Track, track_all_ix: usize) -> Self {
        Self {
            base: TrackRowBase::from_track(track),
            track_all_ix,
            track_num_str: fmt_track_num(track.track_number),
            disc_number: track.disc_number,
        }
    }
}

pub struct TracksView {
    tracks_all: Vec<Rc<music_library::Track>>,
    row_data: Vec<TrackRow>,
    filter: String,
    matcher: Matcher,
    items: Vec<TrackItem>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    album_info: Entity<AlbumInfo>,
    current_track_id: Option<i64>,
    is_playing: bool,
    _subscription: Subscription,
    _library_subscription: Subscription,
    _album_info_subscription: Subscription,
    _lang_subscription: Subscription,
}

impl TracksView {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let tracks_all: Vec<Rc<_>> = services
            .library
            .tracks_for_album(album.id)
            .into_iter()
            .map(Rc::new)
            .collect();
        let row_data: Vec<_> = tracks_all
            .iter()
            .enumerate()
            .map(|(ix, t)| TrackRow::from_track(t, ix))
            .collect();
        let (items, item_sizes_vec) = Self::build_items(&row_data, tr());

        let item_sizes = Rc::new(item_sizes_vec);
        let engine_event_bus = services.engine_event_bus.clone();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let current_track_id = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|t| t.id);
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);
        let album_info = cx.new(|cx| AlbumInfo::new(album, cx));

        let scroll_handle = VirtualListScrollHandle::new();
        if let Some(track_id) = current_track_id
            && let Some(item_ix) = items.iter().position(|item| {
                matches!(item, TrackItem::Track(ix) if row_data[*ix].base.id == track_id)
            })
        {
            scroll_handle.scroll_to_item(item_ix, gpui::ScrollStrategy::Center);
        }

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { .. } => {
                        let id = cx
                            .global::<Services>()
                            .playback_queue
                            .borrow()
                            .current_track()
                            .map(|t| t.id);
                        if this.current_track_id != id {
                            this.current_track_id = id;
                            cx.notify();
                        }
                    }
                    EngineEvent::Playing if !this.is_playing => {
                        this.is_playing = true;
                        cx.notify();
                    }
                    EngineEvent::Paused if this.is_playing => {
                        this.is_playing = false;
                        cx.notify();
                    }
                    EngineEvent::TrackEnded => {
                        if this.is_playing {
                            this.is_playing = false;
                            cx.notify();
                        }
                        let queue_empty = cx
                            .global::<Services>()
                            .playback_queue
                            .borrow()
                            .current_track()
                            .is_none();
                        if queue_empty && this.current_track_id.is_some() {
                            this.current_track_id = None;
                            cx.notify();
                        }
                    }
                    _ => {}
                },
            );

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::TrackLikedChanged { track_id, liked } = event {
                    let mut changed = false;
                    for t in this.tracks_all.iter_mut() {
                        if t.id == *track_id && t.liked != *liked {
                            Rc::make_mut(t).liked = *liked;
                            changed = true;
                        }
                    }
                    for r in this.row_data.iter_mut() {
                        if r.base.id == *track_id && r.base.liked != *liked {
                            r.base.liked = *liked;
                            changed = true;
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                }
            });

        let album_info_subscription = cx.subscribe(
            &album_info,
            |_, _, event: &NavigateToArtistRequested, cx| {
                cx.emit(NavigateToArtistRequested {
                    artist_id: event.artist_id,
                });
            },
        );

        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            let (items, item_sizes_vec) = Self::build_items(&this.row_data, tr());
            this.items = items;
            this.item_sizes = Rc::new(item_sizes_vec);
            cx.notify();
        });

        Self {
            tracks_all,
            row_data,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            items,
            item_sizes,
            scroll_handle,
            album_info,
            current_track_id,
            is_playing,
            _subscription: subscription,
            _library_subscription: library_subscription,
            _album_info_subscription: album_info_subscription,
            _lang_subscription: lang_subscription,
        }
    }

    fn build_items(
        rows: &[TrackRow],
        strings: &ui_resources::i18n::Strings,
    ) -> (Vec<TrackItem>, Vec<Size<Pixels>>) {
        let max_disc = rows.iter().map(|r| r.disc_number).max().unwrap_or(1);
        let multi_disc = max_disc > 1;

        let mut items = vec![TrackItem::TopPadding, TrackItem::AlbumInfo];
        let mut item_sizes_vec = vec![
            size(px(0.), px(TOP_PADDING)),
            size(px(0.), px(ALBUM_INFO_HEIGHT + 1.)),
        ];

        if multi_disc {
            let mut current_disc = 0i32;
            for (ix, row) in rows.iter().enumerate() {
                if row.disc_number != current_disc {
                    let gap = current_disc != 0;
                    current_disc = row.disc_number;
                    items.push(TrackItem::DiscHeader(
                        strings.disc(current_disc as u32).into(),
                        gap,
                    ));
                    let extra = if gap { DISC_HEADER_GAP } else { 0. };
                    item_sizes_vec.push(size(px(0.), px(DISC_HEADER_HEIGHT + extra + 1.)));
                }
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(0.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        } else {
            for ix in 0..rows.len() {
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(0.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        }

        (items, item_sizes_vec)
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible(cx);
        self.scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    fn recompute_visible(&mut self, _cx: &mut Context<Self>) {
        if self.filter.is_empty() {
            self.row_data = self
                .tracks_all
                .iter()
                .enumerate()
                .map(|(ix, t)| TrackRow::from_track(t, ix))
                .collect();
        } else {
            let indices = fuzzy_sorted(
                &mut self.matcher,
                &self.filter,
                self.tracks_all
                    .iter()
                    .enumerate()
                    .map(|(ix, t)| (ix, t.title.as_str())),
            );
            self.row_data = indices
                .into_iter()
                .map(|ix| TrackRow::from_track(&self.tracks_all[ix], ix))
                .collect();
        }
        let strings = tr();
        let (items, item_sizes_vec) = Self::build_items(&self.row_data, strings);
        self.items = items;
        self.item_sizes = Rc::new(item_sizes_vec);
    }
}

impl EventEmitter<NavigateToArtistRequested> for TracksView {}

impl Render for TracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.row_data.is_empty() {
            let message = if self.tracks_all.is_empty() {
                tr().no_tracks_for_album.clone()
            } else {
                tr().no_tracks_match.clone()
            };
            return v_flex()
                .size_full()
                .child(self.album_info.clone())
                .child(div().px_4().child(message));
        }

        let p = TrackRowParams {
            border: Colors::panel_border(cx),
            list_hover: Colors::list_row_hover_bg(cx),
            muted_fg: Colors::text_secondary(cx),
            foreground: Colors::text_primary(cx),
            liked_enabled: cx.global::<SettingsStore>().liked_enabled(),
            playlists_enabled: cx.global::<SettingsStore>().playlists_enabled(),
            buttons: RowButtonColors::from_cx(cx),
        };
        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "tracks_list",
                item_sizes,
                move |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match &view.items[ix] {
                            TrackItem::TopPadding => {
                                div().w_full().h(px(TOP_PADDING)).into_any_element()
                            }
                            TrackItem::AlbumInfo => view.album_info.clone().into_any_element(),
                            TrackItem::DiscHeader(disc, gap) => {
                                track_disc_header(disc.clone(), *gap, p.border, p.muted_fg)
                            }
                            TrackItem::Track(track_ix) => track_row(view, *track_ix, &p, cx),
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}

fn track_disc_header(
    disc: SharedString,
    gap: bool,
    border: gpui::Hsla,
    muted_fg: gpui::Hsla,
) -> gpui::AnyElement {
    let extra = if gap { DISC_HEADER_GAP } else { 0. };
    h_flex()
        .w_full()
        .h(px(DISC_HEADER_HEIGHT + extra))
        .px_4()
        .pb_2()
        .items_end()
        .border_b(px(1.))
        .border_color(border)
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(muted_fg)
                .child(disc),
        )
        .into_any_element()
}

struct TrackRowParams {
    border: gpui::Hsla,
    list_hover: gpui::Hsla,
    muted_fg: gpui::Hsla,
    foreground: gpui::Hsla,
    liked_enabled: bool,
    playlists_enabled: bool,
    buttons: RowButtonColors,
}

fn track_row(
    view: &mut TracksView,
    track_ix: usize,
    p: &TrackRowParams,
    cx: &mut Context<TracksView>,
) -> gpui::AnyElement {
    let row = &view.row_data[track_ix];
    let track_id = row.base.id;
    let track_all_ix = row.track_all_ix;
    let is_current = Some(track_id) == view.current_track_id;
    let track_for_queue = view.tracks_all[row.track_all_ix].clone();

    h_flex()
        .group(LIKE_ROW_GROUP)
        .w_full()
        .h(px(TRACK_ROW_HEIGHT))
        .pl_4()
        .pr_2()
        .gap_2()
        .items_center()
        .border_b(px(1.))
        .border_color(p.border)
        .when(is_current, |s| crate::track_list::current_row(s, cx))
        .hover(|style| style.bg(p.list_hover))
        .child(if is_current {
            let icon = if view.is_playing {
                "icons/play.svg"
            } else {
                "icons/pause.svg"
            };
            div()
                .w_8()
                .flex()
                .items_center()
                .child(svg().path(icon).size(px(12.)).text_color(p.foreground))
        } else {
            div()
                .w_8()
                .text_color(p.muted_fg)
                .child(row.track_num_str.clone())
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .truncate()
                .when(is_current, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(row.base.title.clone()),
        )
        .when(p.playlists_enabled, |el| {
            el.child(add_to_playlist_button(track_id, &p.buttons))
        })
        .when(p.liked_enabled, |el| {
            el.child(like_button(track_id, row.base.liked, &p.buttons))
        })
        .child(track_duration(cx, row.base.duration.clone()))
        .child(add_to_queue_button(track_for_queue, 26., 16., &p.buttons))
        .id(ElementId::Integer(track_id as u64))
        .on_click(cx.listener(move |this, _, _, _cx| {
            let services = _cx.global::<Services>();
            let mut queue = services.playback_queue.borrow_mut();
            let track = queue
                .set_tracks_and_play_at(
                    this.tracks_all.clone(),
                    track_all_ix,
                    crate::playback_queue::QueueSource::Unknown,
                )
                .cloned();
            drop(queue);
            if let Some(track) = track {
                services.play_track(&track);
                crate::services::save_playback(_cx);
            }
        }))
        .into_any_element()
}
