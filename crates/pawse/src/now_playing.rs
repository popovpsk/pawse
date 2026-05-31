use std::sync::Arc;

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, Image, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img,
    px, rems,
};
use gpui_component::{h_flex, v_flex};

use crate::library_service::LibraryEvent;
use crate::theme_colors::Colors;
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
    track_title: SharedString,
    artists: Vec<(i64, SharedString)>,
    album_id: Option<i64>,
    cover_art_id: Option<i64>,
    cover_image: Option<Arc<Image>>,
    shaped_title_w: Option<Pixels>,
    shaped_at_rem: Pixels,
    specs: SharedString,
    _subscription: Subscription,
    _library_subscription: Subscription,
}

fn format_specs(sample_rate: Option<u32>, bit_depth: Option<u8>, bitrate: Option<u32>) -> String {
    use std::fmt::Write;
    let mut specs = String::new();
    if let (Some(sr), Some(bd)) = (sample_rate, bit_depth) {
        let khz = sr as f32 / 1000.0;
        if khz.fract().abs() < f32::EPSILON {
            let _ = write!(specs, "{} kHz \u{b7} {}-bit", khz as u32, bd);
        } else {
            let _ = write!(specs, "{:.1} kHz \u{b7} {}-bit", khz, bd);
        }
    }
    if let Some(kbps) = bitrate {
        if !specs.is_empty() {
            specs.push_str(" \u{b7} ");
        }
        let _ = write!(specs, "{} kbps", kbps);
    }
    specs
}

impl NowPlaying {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Loaded { params, .. } => {
                        this.populate_current(Some(params.sample_rate), Some(params.bit_depth), cx);
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

        let library_event_bus = cx.global::<Services>().library_event_bus.clone();
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::ScanComplete { changed: true } = event {
                    let (sample_rate, bit_depth) = cx
                        .global::<Services>()
                        .output
                        .source_format()
                        .map_or((None, None), |(sr, bd)| (Some(sr), Some(bd)));
                    this.populate_current(sample_rate, bit_depth, cx);
                }
            });

        let mut this = Self {
            track_title: SharedString::default(),
            artists: Vec::new(),
            album_id: None,
            cover_art_id: None,
            cover_image: None,
            shaped_title_w: None,
            shaped_at_rem: px(0.),
            specs: SharedString::default(),
            _subscription: subscription,
            _library_subscription: library_subscription,
        };

        let (sample_rate, bit_depth) = cx
            .global::<Services>()
            .output
            .source_format()
            .map_or((None, None), |(sr, bd)| (Some(sr), Some(bd)));
        this.populate_current(sample_rate, bit_depth, cx);
        this
    }

    fn populate_current(
        &mut self,
        sample_rate: Option<u32>,
        bit_depth: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        let services = cx.global::<Services>();
        let queue = services.playback_queue.borrow();
        if let Some(track) = queue.current_track() {
            let track_id = track.id;
            let title = track.title.clone();
            let cover = track.cover_art_id;
            let album_id = track.album_id;
            let bitrate = track.bitrate;
            drop(queue);
            self.track_title = title.into();
            self.shaped_title_w = None;
            self.cover_art_id = cover;
            self.album_id = album_id;
            self.specs = SharedString::from(format_specs(sample_rate, bit_depth, bitrate));
            let mut seen = std::collections::HashSet::new();
            self.artists = services
                .library
                .track_artists_with_ids(track_id)
                .into_iter()
                .filter(|(id, _)| seen.insert(*id))
                .map(|(id, name)| (id, SharedString::from(name)))
                .collect();
            self.cover_image = services
                .cover_art_cache
                .borrow_mut()
                .get_small(cover, &services.library);
        } else {
            drop(queue);
            self.clear();
        }
        cx.notify();
    }

    fn clear(&mut self) {
        self.track_title = SharedString::default();
        self.shaped_title_w = None;
        self.artists.clear();
        self.album_id = None;
        self.cover_art_id = None;
        self.cover_image = None;
        self.specs = SharedString::default();
    }
}

impl EventEmitter<NavigateToAlbumRequested> for NowPlaying {}
impl EventEmitter<NavigateToArtistRequested> for NowPlaying {}

impl Render for NowPlaying {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport_w = f32::from(window.viewport_size().width);
        let title_max_w = ((viewport_w - 800.0) * 0.5 + 220.0).clamp(220.0, 460.0);

        let title_overflows = if self.track_title.is_empty() {
            false
        } else {
            let rem_size = window.rem_size();
            let shaped_w = match self.shaped_title_w {
                Some(w) if self.shaped_at_rem == rem_size => w,
                _ => {
                    let font_size = rems(0.875).to_pixels(rem_size);
                    let mut text_style = window.text_style();
                    text_style.font_weight = gpui::FontWeight::SEMIBOLD;
                    let run = text_style.to_run(self.track_title.len());
                    let shaped = window.text_system().shape_line(
                        self.track_title.clone(),
                        font_size,
                        &[run],
                        None,
                    );
                    self.shaped_title_w = Some(shaped.width);
                    self.shaped_at_rem = rem_size;
                    shaped.width
                }
            };
            shaped_w > px(title_max_w)
        };

        let album_id = self.album_id;
        let track_title = self.track_title.clone();
        let foreground = Colors::text_primary(cx);

        h_flex()
            .gap_3()
            .items_center()
            .w(px(200.))
            .child({
                if let Some(cover_img) = self.cover_image.clone() {
                    img(cover_img)
                        .w(px(56.))
                        .h(px(56.))
                        .rounded(px(6.))
                        .object_fit(gpui::ObjectFit::Cover)
                        .with_fallback({
                            let bg = Colors::cover_fallback_bg(cx);
                            let fg = Colors::text_secondary(cx);
                            move || cover_placeholder(56., 6., bg, fg).into_any_element()
                        })
                        .into_any_element()
                } else {
                    cover_placeholder(
                        56.,
                        6.,
                        Colors::cover_fallback_bg(cx),
                        Colors::text_secondary(cx),
                    )
                    .into_any_element()
                }
            })
            .child(
                v_flex()
                    .w(px(132.))
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
                                    Colors::app_background(cx),
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
                        let muted_fg = Colors::text_secondary(cx);
                        if self.artists.is_empty() {
                            div()
                                .text_xs()
                                .text_color(muted_fg)
                                .truncate()
                                .into_any_element()
                        } else {
                            let mut row = h_flex().overflow_hidden().flex_wrap();
                            for (i, (artist_id, name)) in self.artists.iter().enumerate() {
                                if i > 0 {
                                    row =
                                        row.child(div().text_xs().text_color(muted_fg).child(", "));
                                }
                                let artist_id = *artist_id;
                                row = row.child(
                                    div()
                                        .id(("np_artist", artist_id as u64))
                                        .text_xs()
                                        .text_color(muted_fg)
                                        .cursor_pointer()
                                        .border_b(px(1.))
                                        .hover(|s| s.border_color(muted_fg))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(NavigateToArtistRequested { artist_id });
                                        }))
                                        .child(name.clone()),
                                );
                            }
                            row.into_any_element()
                        }
                    })
                    .when(!self.specs.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(Colors::text_secondary(cx))
                                .truncate()
                                .child(self.specs.clone()),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::format_specs;

    #[test]
    fn integer_khz() {
        assert_eq!(
            format_specs(Some(48000), Some(24), None),
            "48 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn fractional_khz() {
        assert_eq!(
            format_specs(Some(44100), Some(16), None),
            "44.1 kHz \u{b7} 16-bit"
        );
    }

    #[test]
    fn bitrate_only() {
        assert_eq!(format_specs(None, None, Some(320)), "320 kbps");
    }

    #[test]
    fn sample_rate_and_bitrate_combined() {
        assert_eq!(
            format_specs(Some(96000), Some(24), Some(1411)),
            "96 kHz \u{b7} 24-bit \u{b7} 1411 kbps"
        );
    }

    #[test]
    fn empty_when_nothing_known() {
        assert_eq!(format_specs(None, None, None), "");
    }

    #[test]
    fn needs_both_rate_and_depth() {
        assert_eq!(format_specs(Some(48000), None, None), "");
        assert_eq!(format_specs(None, Some(24), None), "");
    }
}
