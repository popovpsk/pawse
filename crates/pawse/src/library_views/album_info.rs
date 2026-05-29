use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, px,
};
use gpui_component::{h_flex, v_flex};

use crate::theme_colors::Colors;
use ui_components::cover_placeholder::cover_placeholder;

use crate::now_playing::NavigateToArtistRequested;
use crate::services::Services;
use crate::track_list::add_album_to_queue_button;

pub struct AlbumInfo {
    album_id: i64,
    title: String,
    artist_name: SharedString,
    artist_id: Option<i64>,
    year: Option<i32>,
    cover_art_id: Option<i64>,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary) -> Self {
        Self {
            album_id: album.id,
            title: album.title.clone(),
            artist_name: album.artist_name.clone().into(),
            artist_id: album.artist_id,
            year: album.year,
            cover_art_id: album.cover_art_id,
        }
    }
}

impl EventEmitter<NavigateToArtistRequested> for AlbumInfo {}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = Colors::text_secondary(cx);
        let album_id = self.album_id;
        let artist_id = self.artist_id;

        h_flex()
            .w_full()
            .px_4()
            .gap_4()
            .items_start()
            .child({
                let fallback_bg = Colors::cover_fallback_bg(cx);
                let fallback_fg = muted_fg;
                let services = cx.global::<Services>();
                let cover_img = services
                    .cover_art_cache
                    .borrow_mut()
                    .get_large(self.cover_art_id, &services.library);
                if let Some(cover_img) = cover_img {
                    img(cover_img)
                        .w(px(150.))
                        .h(px(150.))
                        .rounded(px(6.))
                        .object_fit(gpui::ObjectFit::Cover)
                        .with_fallback({
                            let bg = fallback_bg;
                            let fg = fallback_fg;
                            move || cover_placeholder(150., 6., bg, fg).into_any_element()
                        })
                        .into_any_element()
                } else {
                    cover_placeholder(150., 6., fallback_bg, fallback_fg).into_any_element()
                }
            })
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
                            .child(self.title.clone()),
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
                    }),
            )
            .child(add_album_to_queue_button(album_id, 42., 26., cx))
    }
}
