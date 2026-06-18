use std::sync::Arc;

use gpui::{
    Context, EventEmitter, Image, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::theme_colors::Colors;
use ui_components::cover_thumb::cover_thumb;

use crate::now_playing::NavigateToArtistRequested;
use crate::services::Services;
use crate::track_list::add_album_to_queue_button;

pub struct AlbumInfo {
    album_id: i64,
    title: String,
    artist_name: SharedString,
    artist_id: Option<i64>,
    year: Option<i32>,
    cover: Option<Arc<Image>>,
    genres_inline: SharedString,
    genres_tooltip: Option<SharedString>,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let cover = services
            .cover_art_cache
            .borrow_mut()
            .get_large(album.cover_art_id, &services.library);
        let all_genres = services.library.album_genres(album.id);
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
        }
    }
}

impl EventEmitter<NavigateToArtistRequested> for AlbumInfo {}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = Colors::muted_foreground(cx);
        let album_id = self.album_id;
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
            .child(cover_thumb(
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
            .child(add_album_to_queue_button(album_id, 42., 26., cx))
    }
}
