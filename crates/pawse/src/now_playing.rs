use std::path::PathBuf;

use audio_engine::EngineEvent;
use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, StyledImage, Subscription, Window, div,
    img, px,
};
use gpui_component::{ActiveTheme, h_flex};

use crate::services::Services;

pub struct NowPlaying {
    track_title: String,
    artist_names: String,
    cover_art_path: Option<String>,
    _subscription: Subscription,
}

impl NowPlaying {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => {
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if let Some(track) = queue.current_track() {
                        let track_id = track.id;
                        let title = track.title.clone();
                        let cover = track.cover_art_path.clone();
                        drop(queue);
                        this.track_title = title;
                        this.cover_art_path = cover;
                        this.artist_names = services.library.track_artists(track_id).join(", ");
                    } else {
                        drop(queue);
                        this.clear();
                    }
                    cx.notify();
                }
                EngineEvent::TrackEnded => {
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if queue.current_track().is_none() {
                        drop(queue);
                        this.clear();
                        cx.notify();
                    } else {
                        drop(queue);
                    }
                }
                _ => {}
            },
        );

        Self {
            track_title: String::new(),
            artist_names: String::new(),
            cover_art_path: None,
            _subscription: subscription,
        }
    }

    fn clear(&mut self) {
        self.track_title.clear();
        self.artist_names.clear();
        self.cover_art_path = None;
    }
}

impl Render for NowPlaying {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .items_center()
            .w(px(200.))
            .child(
                div()
                    .w(px(48.))
                    .h(px(48.))
                    .rounded(px(4.))
                    .child(if let Some(ref path) = self.cover_art_path {
                        img(PathBuf::from(path))
                            .w(px(48.))
                            .h(px(48.))
                            .rounded(px(4.))
                            .object_fit(gpui::ObjectFit::Cover)
                            .with_fallback({
                                let bg = cx.theme().secondary;
                                move || {
                                    div()
                                        .w(px(48.))
                                        .h(px(48.))
                                        .rounded(px(4.))
                                        .bg(bg)
                                        .into_any_element()
                                }
                            })
                            .into_any_element()
                    } else {
                        div()
                            .w(px(48.))
                            .h(px(48.))
                            .rounded(px(4.))
                            .bg(cx.theme().secondary)
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.track_title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(self.artist_names.clone()),
                    ),
            )
    }
}
