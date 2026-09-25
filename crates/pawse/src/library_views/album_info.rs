use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, RenderImage,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::theme_colors::Colors;
use ui_components::cover_thumb::cover_tile;

use crate::cache_fill::FillTarget;
use crate::now_playing::NavigateToArtistRequested;
use crate::services::Services;
use crate::track_list::{add_album_to_queue_button, save_to_cache_button};

pub struct AlbumInfo {
    album_id: i64,
    title: String,
    artist_name: SharedString,
    artist_id: Option<i64>,
    year: Option<i32>,
    cover: Option<Arc<RenderImage>>,
    genres_inline: SharedString,
    genres_tooltip: Option<SharedString>,
    missing_in_cache: bool,
    fills_seen: u64,
    _fill_subscription: gpui::Subscription,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let (cache, library, fill) = {
            let services = cx.global::<Services>();
            (
                services.cover_art_cache.clone(),
                services.library.clone(),
                services.cache_fill.clone(),
            )
        };
        let fills_seen = fill.read(cx).revision();
        let fill_subscription = cx.observe(&fill, |this, fill, cx| {
            let finished = fill.read(cx).revision();
            if finished != this.fills_seen {
                this.fills_seen = finished;
                this.missing_in_cache = album_missing_in_cache(this.album_id, cx);
            }
            cx.notify();
        });
        let cover = cache
            .borrow_mut()
            .get_large(album.cover_art_id, &library, cx);
        let all_genres = library.album_genres(album.id);
        let shown = all_genres
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let (genres_inline, genres_tooltip): (SharedString, Option<SharedString>) =
            if all_genres.len() > 3 {
                (
                    format!("{shown} …").into(),
                    Some(all_genres.join(" · ").into()),
                )
            } else {
                (shown.into(), None)
            };
        Self {
            album_id: album.id,
            title: album.title.clone(),
            artist_name: album.artist_name.clone().into(),
            artist_id: album.artist_id,
            year: album.year,
            cover,
            genres_inline,
            genres_tooltip,
            missing_in_cache: album_missing_in_cache(album.id, cx),
            fills_seen,
            _fill_subscription: fill_subscription,
        }
    }
}

fn album_missing_in_cache(album_id: i64, cx: &gpui::App) -> bool {
    let services = cx.global::<Services>();
    let tracks = services.library.tracks_for_album(album_id);
    crate::cache_fill::has_missing(&tracks, &services.remote_media)
}

impl EventEmitter<NavigateToArtistRequested> for AlbumInfo {}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = Colors::muted_foreground(cx);
        let album_id = self.album_id;
        let target = FillTarget::Album(album_id);
        let progress = cx.global::<Services>().cache_fill.read(cx).progress(target);
        let save_button = (self.missing_in_cache || progress.is_some()).then(|| {
            save_to_cache_button(
                gpui::ElementId::NamedInteger("save-album-to-cache".into(), album_id as u64),
                progress,
                42.,
                22.,
                cx,
                move |window, cx| {
                    let tracks = cx.global::<Services>().library.tracks_for_album(album_id);
                    crate::cache_fill::request(target, tracks, window, cx);
                },
            )
        });
        let artist_id = self.artist_id;
        let title: SharedString = if self.album_id == music_library::NO_METADATA_ALBUM_ID {
            crate::localization::tr().no_metadata.clone()
        } else {
            self.title.clone().into()
        };

        h_flex()
            .w_full()
            .px_4()
            .gap_4()
            .items_start()
            .child(cover_tile(
                self.cover.as_ref(),
                150.,
                6.,
                Colors::secondary(cx),
                muted_fg,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .gap_1()
                    .pt_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(if let Some(aid) = artist_id {
                        h_flex()
                            .child(
                                div()
                                    .id(("al_artist", aid as u64))
                                    .text_sm()
                                    .text_color(muted_fg)
                                    .cursor_pointer()
                                    .border_b(px(1.))
                                    .hover(|s| s.border_color(muted_fg))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(NavigateToArtistRequested { artist_id: aid });
                                    }))
                                    .child(self.artist_name.clone()),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.artist_name.clone())
                            .into_any_element()
                    })
                    .child(if let Some(year) = self.year {
                        div().text_sm().text_color(muted_fg).child(year.to_string())
                    } else {
                        div()
                    })
                    .child(if self.genres_inline.is_empty() {
                        div().into_any_element()
                    } else if let Some(tooltip) = self.genres_tooltip.clone() {
                        div()
                            .id(("album_genres", album_id as u64))
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.genres_inline.clone())
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .into_any_element()
                    } else {
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.genres_inline.clone())
                            .into_any_element()
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .when(
                        cx.global::<crate::settings_store::SettingsStore>()
                            .tag_editor_enabled(),
                        |el| {
                            el.child(crate::track_list::edit_album_tags_button(
                                album_id, 42., 22., cx,
                            ))
                        },
                    )
                    .when_some(save_button, |el, button| el.child(button))
                    .child(add_album_to_queue_button(album_id, 42., 26., cx)),
            )
    }
}
