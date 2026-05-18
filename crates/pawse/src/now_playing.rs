use audio_engine::EngineEvent;
use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, StyledImage, Subscription, Window, div,
    img, px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use gpui::prelude::FluentBuilder;

use crate::services::Services;

pub struct NowPlaying {
    track_title: String,
    artist_names: String,
    cover_art_id: Option<i64>,
    sample_rate: Option<u32>,
    bit_depth: Option<u8>,
    _subscription: Subscription,
}

fn format_specs(sample_rate: Option<u32>, bit_depth: Option<u8>) -> Option<String> {
    let (sr, bd) = (sample_rate?, bit_depth?);
    let khz = sr as f32 / 1000.0;
    let khz_str = if (khz.fract()).abs() < f32::EPSILON {
        format!("{} kHz", khz as u32)
    } else {
        format!("{:.1} kHz", khz)
    };
    Some(format!("{} \u{b7} {}-bit", khz_str, bd))
}

impl NowPlaying {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { params, .. } => {
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if let Some(track) = queue.current_track() {
                        let track_id = track.id;
                        let title = track.title.clone();
                        let cover = track.cover_art_id;
                        drop(queue);
                        this.track_title = title;
                        this.cover_art_id = cover;
                        this.artist_names = services.library.track_artists(track_id).join(", ");
                        this.sample_rate = Some(params.sample_rate);
                        this.bit_depth = Some(params.bit_depth);
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
            cover_art_id: None,
            sample_rate: None,
            bit_depth: None,
            _subscription: subscription,
        }
    }

    fn clear(&mut self) {
        self.track_title.clear();
        self.artist_names.clear();
        self.cover_art_id = None;
        self.sample_rate = None;
        self.bit_depth = None;
    }
}

impl Render for NowPlaying {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let specs = format_specs(self.sample_rate, self.bit_depth);

        h_flex()
            .gap_3()
            .items_center()
            .w(px(200.))
            .child({
                let cover_img = {
                    let services = cx.global::<Services>();
                    services.cover_art_cache.borrow_mut().get_small(self.cover_art_id, &services.library)
                };
                if let Some(cover_img) = cover_img {
                    img(cover_img)
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
                }
            })
            .child(
                v_flex()
                    .w(px(140.))
                    .items_start()
                    .child(
                        div()
                            .max_w(px(360.))
                            .truncate()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.track_title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(self.artist_names.clone()),
                    )
                    .when_some(specs, |this, specs| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(specs),
                        )
                    }),
            )
    }
}
