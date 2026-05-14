use std::path::PathBuf;

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, StyledImage, Window, div, img, px,
};
use gpui_component::{h_flex, v_flex, ActiveTheme};

pub struct AlbumInfo {
    title: String,
    artist_name: String,
    year: Option<i32>,
    cover_art_path: Option<String>,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary) -> Self {
        Self {
            title: album.title.clone(),
            artist_name: album.artist_name.clone(),
            year: album.year,
            cover_art_path: album.cover_art_path.clone(),
        }
    }
}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_4()
            .items_start()
            .child(
                div()
                    .w(px(150.))
                    .h(px(150.))
                    .rounded(px(6.))
                    .child(if let Some(ref path) = self.cover_art_path {
                        img(PathBuf::from(path))
                            .w(px(150.))
                            .h(px(150.))
                            .rounded(px(6.))
                            .object_fit(gpui::ObjectFit::Cover)
                            .with_fallback({
                                let bg = cx.theme().secondary;
                                move || {
                                    div()
                                        .w(px(150.))
                                        .h(px(150.))
                                        .rounded(px(6.))
                                        .bg(bg)
                                        .into_any_element()
                                }
                            })
                            .into_any_element()
                    } else {
                        div()
                            .w(px(150.))
                            .h(px(150.))
                            .rounded(px(6.))
                            .bg(cx.theme().secondary)
                            .into_any_element()
                    }),
            )
            .child(
                v_flex()
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
                            .text_color(cx.theme().muted_foreground)
                            .child(self.artist_name.clone()),
                    )
                    .child(if let Some(year) = self.year {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(year.to_string())
                    } else {
                        div()
                    }),
            )
    }
}
