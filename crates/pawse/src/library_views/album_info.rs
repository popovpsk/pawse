use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, StyledImage, Window, div, img, px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use ui_components::cover_placeholder::cover_placeholder;

use crate::queue_button::add_album_to_queue_button;
use crate::services::Services;

pub struct AlbumInfo {
    album_id: i64,
    title: String,
    artist_name: String,
    year: Option<i32>,
    cover_art_id: Option<i64>,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary) -> Self {
        Self {
            album_id: album.id,
            title: album.title.clone(),
            artist_name: album.artist_name.clone(),
            year: album.year,
            cover_art_id: album.cover_art_id,
        }
    }
}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = cx.theme().muted_foreground;
        let album_id = self.album_id;

        h_flex()
            .w_full()
            .px_4()
            .gap_4()
            .items_start()
            .child({
                let fallback_bg = cx.theme().secondary;
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
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.artist_name.clone()),
                    )
                    .child(if let Some(year) = self.year {
                        div().text_sm().text_color(muted_fg).child(year.to_string())
                    } else {
                        div()
                    }),
            )
            .child(add_album_to_queue_button(album_id, 42., 26., cx))
    }
}
