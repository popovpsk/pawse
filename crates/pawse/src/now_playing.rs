use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img, px, rems,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use ui_components::cover_placeholder::cover_placeholder;
use ui_components::fade::{FadeEdge, fade_overlay};

use crate::services::Services;

#[derive(Clone, Debug)]
pub struct NavigateToAlbumRequested {
    pub album_id: i64,
}

#[derive(Clone, Debug)]
pub struct NavigateToArtistRequested {
    pub artist_id: i64,
}

pub struct NowPlaying {
    track_title: String,
    artists: Vec<(i64, String)>,
    album_id: Option<i64>,
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

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { params, .. } => {
                        let services = cx.global::<Services>();
                        let queue = services.playback_queue.borrow();
                        if let Some(track) = queue.current_track() {
                            let track_id = track.id;
                            let title = track.title.clone();
                            let cover = track.cover_art_id;
                            let album_id = track.album_id;
                            drop(queue);
                            this.track_title = title;
                            this.cover_art_id = cover;
                            this.album_id = album_id;
                            let mut seen = std::collections::HashSet::new();
                            this.artists = services
                                .library
                                .track_artists_with_ids(track_id)
                                .into_iter()
                                .filter(|(id, _)| seen.insert(*id))
                                .collect();
                            this.sample_rate = Some(params.sample_rate);
                            this.bit_depth = Some(params.bit_depth);
                        } else {
                            drop(queue);
                            this.clear();
                        }
                        cx.notify();
                    }
                    EngineEvent::TrackEnded | EngineEvent::Stopped => {
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
            artists: Vec::new(),
            album_id: None,
            cover_art_id: None,
            sample_rate: None,
            bit_depth: None,
            _subscription: subscription,
        }
    }

    fn clear(&mut self) {
        self.track_title.clear();
        self.artists.clear();
        self.album_id = None;
        self.cover_art_id = None;
        self.sample_rate = None;
        self.bit_depth = None;
    }
}

impl EventEmitter<NavigateToAlbumRequested> for NowPlaying {}
impl EventEmitter<NavigateToArtistRequested> for NowPlaying {}

impl Render for NowPlaying {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let specs = format_specs(self.sample_rate, self.bit_depth);
        let viewport_w = f32::from(window.viewport_size().width);
        let title_max_w = ((viewport_w - 800.0) * 0.5 + 220.0).clamp(220.0, 460.0);

        let title_overflows = if !self.track_title.is_empty() {
            let font_size = rems(0.875).to_pixels(window.rem_size());
            let mut text_style = window.text_style();
            text_style.font_weight = gpui::FontWeight::SEMIBOLD;
            let run = text_style.to_run(self.track_title.len());
            let shaped = window.text_system().shape_line(
                self.track_title.clone().into(),
                font_size,
                &[run],
                None,
            );
            shaped.width > px(title_max_w)
        } else {
            false
        };

        let album_id = self.album_id;
        let track_title = self.track_title.clone();
        let artists = self.artists.clone();
        let foreground = cx.theme().foreground;

        h_flex()
            .gap_3()
            .items_center()
            .w(px(200.))
            .child({
                let cover_img = {
                    let services = cx.global::<Services>();
                    services
                        .cover_art_cache
                        .borrow_mut()
                        .get_small(self.cover_art_id, &services.library)
                };
                if let Some(cover_img) = cover_img {
                    img(cover_img)
                        .w(px(48.))
                        .h(px(48.))
                        .rounded(px(4.))
                        .object_fit(gpui::ObjectFit::Cover)
                        .with_fallback({
                            let bg = cx.theme().secondary;
                            let fg = cx.theme().muted_foreground;
                            move || cover_placeholder(48., 4., bg, fg).into_any_element()
                        })
                        .into_any_element()
                } else {
                    cover_placeholder(48., 4., cx.theme().secondary, cx.theme().muted_foreground)
                        .into_any_element()
                }
            })
            .child(
                v_flex()
                    .w(px(140.))
                    .items_start()
                    .child({
                        let title_inner = div()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(track_title);

                        let title_container = div()
                            .relative()
                            .max_w(px(title_max_w))
                            .overflow_hidden()
                            .child(title_inner)
                            .when(title_overflows, |this| {
                                this.child(fade_overlay(
                                    FadeEdge::Right,
                                    cx.theme().background,
                                    20.0,
                                    0.0,
                                ))
                            });

                        if let Some(aid) = album_id {
                            div()
                                .id("np_title")
                                .cursor_pointer()
                                .border_b(px(1.))
                                .hover(|s| s.border_color(foreground))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(NavigateToAlbumRequested { album_id: aid });
                                }))
                                .child(title_container)
                                .into_any_element()
                        } else {
                            title_container.into_any_element()
                        }
                    })
                    .child({
                        let muted_fg = cx.theme().muted_foreground;
                        if artists.is_empty() {
                            div()
                                .text_xs()
                                .text_color(muted_fg)
                                .truncate()
                                .into_any_element()
                        } else {
                            h_flex()
                                .overflow_hidden()
                                .flex_wrap()
                                .children(
                                    artists
                                        .into_iter()
                                        .enumerate()
                                        .flat_map(|(i, (artist_id, name))| {
                                            let separator = if i > 0 {
                                                Some(
                                                    div()
                                                        .text_xs()
                                                        .text_color(muted_fg)
                                                        .child(", ")
                                                        .into_any_element(),
                                                )
                                            } else {
                                                None
                                            };
                                            let link = div()
                                                .id(("np_artist", artist_id as u64))
                                                .text_xs()
                                                .text_color(muted_fg)
                                                .cursor_pointer()
                                                .border_b(px(1.))
                                                .hover(|s| s.border_color(muted_fg))
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.emit(NavigateToArtistRequested {
                                                        artist_id,
                                                    });
                                                }))
                                                .child(name)
                                                .into_any_element();
                                            [separator, Some(link)]
                                        })
                                        .flatten(),
                                )
                                .into_any_element()
                        }
                    })
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
