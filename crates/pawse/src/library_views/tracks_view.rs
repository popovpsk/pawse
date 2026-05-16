use std::rc::Rc;

use gpui::{
    AppContext, Context, ElementId, Entity, Hsla, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, Size, StatefulInteractiveElement, Styled, Window, div, px, size,
};
use gpui_component::{ActiveTheme, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};

use crate::library_views::album_info::AlbumInfo;
use crate::services::Services;

const TRACK_ROW_HEIGHT: f32 = 36.;
const DISC_HEADER_HEIGHT: f32 = 32.;
const ALBUM_INFO_HEIGHT: f32 = 170.;

#[derive(Clone, Copy)]
enum TrackItem {
    AlbumInfo,
    DiscHeader(i32),
    Track(usize),
}

pub struct TracksView {
    tracks: Vec<music_library::Track>,
    items: Vec<TrackItem>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
    album_info: Entity<AlbumInfo>,
}

impl TracksView {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let tracks = services.library.tracks_for_album(album.id);
        let max_disc = tracks.iter().map(|t| t.disc_number).max().unwrap_or(1);
        let multi_disc = max_disc > 1;

        let mut items = vec![TrackItem::AlbumInfo];
        let mut item_sizes_vec = vec![size(px(300.), px(ALBUM_INFO_HEIGHT + 1.))];

        if multi_disc {
            let mut current_disc = 0i32;
            for (ix, track) in tracks.iter().enumerate() {
                if track.disc_number != current_disc {
                    current_disc = track.disc_number;
                    items.push(TrackItem::DiscHeader(current_disc));
                    item_sizes_vec.push(size(px(300.), px(DISC_HEADER_HEIGHT + 1.)));
                }
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        } else {
            for ix in 0..tracks.len() {
                items.push(TrackItem::Track(ix));
                item_sizes_vec.push(size(px(300.), px(TRACK_ROW_HEIGHT + 1.)));
            }
        }

        let item_sizes = Rc::new(item_sizes_vec);
        let album_info = cx.new(|_cx| AlbumInfo::new(album));
        Self {
            tracks,
            items,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
            album_info,
        }
    }
}

impl Render for TracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tracks.is_empty() {
            return v_flex()
                .size_full()
                .child(self.album_info.clone())
                .child(div().px_4().child("No tracks found for this album."));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex().size_full().child(
            v_virtual_list(
                cx.entity().clone(),
                "tracks_list",
                item_sizes,
                |view, visible_range, _window, cx| {
                    visible_range
                        .map(|ix| match view.items[ix] {
                            TrackItem::AlbumInfo => view.album_info.clone().into_any_element(),
                            TrackItem::DiscHeader(disc) => h_flex()
                                .w_full()
                                .h(px(DISC_HEADER_HEIGHT))
                                .px_4()
                                .items_center()
                                .border_b(px(1.))
                                .border_color(Hsla {
                                    h: 0.,
                                    s: 0.,
                                    l: 1.,
                                    a: 0.1,
                                })
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("Disc {}", disc)),
                                )
                                .into_any_element(),
                            TrackItem::Track(track_ix) => {
                                let track = &view.tracks[track_ix];
                                let track_id = track.id;
                                let track_num_str = track
                                    .track_number
                                    .map(|n| format!("{}.", n))
                                    .unwrap_or_default();
                                let duration_str = track
                                    .duration_ms
                                    .map(|ms| {
                                        let secs = (ms / 1000) as u32;
                                        format!("{:02}:{:02}", secs / 60, secs % 60)
                                    })
                                    .unwrap_or_default();

                                h_flex()
                                    .w_full()
                                    .h(px(TRACK_ROW_HEIGHT))
                                    .px_4()
                                    .gap_2()
                                    .border_b(px(1.))
                                    .border_color(Hsla {
                                        h: 0.,
                                        s: 0.,
                                        l: 1.,
                                        a: 0.1,
                                    })
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|style| style.bg(cx.theme().secondary))
                                    .child(div().w_8().child(track_num_str))
                                    .child(div().flex_1().child(track.title.clone()))
                                    .child(div().w_16().child(duration_str))
                                    .id(ElementId::Integer(track_id as u64))
                                    .on_click(cx.listener(move |this, _, _, _cx| {
                                        let services = _cx.global::<Services>();
                                        let mut queue = services.playback_queue.borrow_mut();
                                        queue.set_tracks(this.tracks.clone());
                                        if let Some(track) = queue.play_track_at(track_ix).cloned() {
                                            services.play_track(&track);
                                        }
                                    }))
                                    .into_any_element()
                            }
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&self.scroll_handle)
            .flex_1(),
        )
    }
}
