use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::playback_status::{Phase, StatusChanged};
use gpui::{
    AnyElement, Context, EventEmitter, Image, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window,
    div, img, px,
};
use gpui_component::{h_flex, v_flex};

use crate::library_service::LibraryEvent;
use crate::settings_store::{NowPlayingDetails, SettingsStore};
use crate::theme_colors::Colors;
use ui_components::cover_placeholder::cover_placeholder;

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
    has_track: bool,
    track_title: SharedString,
    artists: Vec<(i64, SharedString)>,
    album_id: Option<i64>,
    album_title: SharedString,
    year: Option<SharedString>,
    cover_art_id: Option<i64>,
    cover_image: Option<Arc<Image>>,
    specs: SharedString,
    _subscription: Subscription,
    _library_subscription: Subscription,
}

fn format_specs(
    sample_rate: Option<u32>,
    bit_depth: Option<u8>,
    bitrate: Option<u32>,
    dsd_rate: Option<u32>,
) -> String {
    use std::fmt::Write;
    let mut specs = String::new();
    if let Some(dsd) = dsd_rate {
        let _ = write!(specs, "DSD{}", dsd / 44_100);
    }
    if let (Some(sr), Some(bd)) = (sample_rate, bit_depth) {
        if !specs.is_empty() {
            specs.push('\u{2192}');
        }
        let khz = sr as f32 / 1000.0;
        if khz.fract().abs() < f32::EPSILON {
            let _ = write!(specs, "{} kHz \u{b7} {}-bit", khz as u32, bd);
        } else {
            let _ = write!(specs, "{:.1} kHz \u{b7} {}-bit", khz, bd);
        }
    }
    // DSD has no meaningful compressed bitrate — the source is already fully
    // disclosed by the DSD{n}→ prefix above.
    if dsd_rate.is_none()
        && let Some(kbps) = bitrate
    {
        if !specs.is_empty() {
            specs.push_str(" \u{b7} ");
        }
        let _ = write!(specs, "{} kbps", kbps);
    }
    specs
}

impl NowPlaying {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let playback_status = cx.global::<Services>().playback_status.clone();
        let subscription = cx.subscribe(
            &playback_status,
            |this, status, event: &StatusChanged, cx| {
                let (track_id, phase) = {
                    let status = status.read(cx);
                    (status.track_id(), status.phase())
                };
                match (track_id, phase) {
                    (None, _) => {
                        this.clear();
                        cx.notify();
                    }
                    (
                        Some(_),
                        Phase::Ready {
                            sample_rate,
                            bit_depth,
                            dsd_rate,
                        },
                    ) => this.populate_current(Some(sample_rate), Some(bit_depth), dsd_rate, cx),
                    (Some(_), Phase::Preparing) => this.populate_current(None, None, None, cx),
                    (Some(_), Phase::Idle) if event.track_changed => {
                        this.populate_current(None, None, None, cx)
                    }
                    (Some(_), Phase::Idle) => {}
                }
            },
        );

        let library_event_bus = cx.global::<Services>().library_event_bus.clone();
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::CatalogChanged = event {
                    let (sample_rate, bit_depth) = cx
                        .global::<Services>()
                        .output
                        .source_format()
                        .map_or((None, None), |(sr, bd)| (Some(sr), Some(bd)));
                    let dsd_rate = match cx
                        .global::<Services>()
                        .current_dsd_rate
                        .load(Ordering::Relaxed)
                    {
                        0 => None,
                        v => Some(v),
                    };
                    this.populate_current(sample_rate, bit_depth, dsd_rate, cx);
                }
            });

        let mut this = Self {
            has_track: false,
            track_title: SharedString::default(),
            artists: Vec::new(),
            album_id: None,
            album_title: SharedString::default(),
            year: None,
            cover_art_id: None,
            cover_image: None,
            specs: SharedString::default(),
            _subscription: subscription,
            _library_subscription: library_subscription,
        };

        let (sample_rate, bit_depth) = cx
            .global::<Services>()
            .output
            .source_format()
            .map_or((None, None), |(sr, bd)| (Some(sr), Some(bd)));
        // `output.source_format()` reflects live audio-callback state, but the
        // engine only emits `dsd_rate` once per track load (`EngineEvent::Loaded`)
        // — on macOS, closing the window and reopening it via the dock icon
        // rebuilds this view without reloading the track, so that event never
        // refires. Read the last-loaded value cached on `Services` instead of
        // hardcoding `None`, or the DSD{n}→ label silently drops after reopen.
        let dsd_rate = match cx
            .global::<Services>()
            .current_dsd_rate
            .load(Ordering::Relaxed)
        {
            0 => None,
            v => Some(v),
        };
        this.populate_current(sample_rate, bit_depth, dsd_rate, cx);
        this
    }

    fn populate_current(
        &mut self,
        sample_rate: Option<u32>,
        bit_depth: Option<u8>,
        dsd_rate: Option<u32>,
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
            let year = track.year;
            drop(queue);
            self.has_track = true;
            self.track_title = title.into();
            self.cover_art_id = cover;
            self.album_id = album_id;
            self.album_title = album_id
                .and_then(|id| services.library.album_title(id))
                .map(SharedString::from)
                .unwrap_or_default();
            self.year = year.map(|y| SharedString::from(y.to_string()));
            self.specs =
                SharedString::from(format_specs(sample_rate, bit_depth, bitrate, dsd_rate));
            self.artists = services
                .library
                .unique_track_artists(track_id)
                .into_iter()
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

    fn details_line(
        &self,
        details: NowPlayingDetails,
        max_w: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let muted_fg = Colors::muted_foreground(cx);
        let line = |text: SharedString| {
            div()
                .max_w(px(max_w))
                .text_xs()
                .text_color(muted_fg)
                .truncate()
                .child(text)
        };
        match details {
            NowPlayingDetails::Specs => (!self.specs.is_empty()).then(|| {
                div()
                    .max_w(px(max_w))
                    .text_xs()
                    .text_color(muted_fg)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(self.specs.clone())
                    .into_any_element()
            }),
            NowPlayingDetails::Year => self.year.clone().map(|year| line(year).into_any_element()),
            NowPlayingDetails::Album => {
                let album_id = self.album_id?;
                if self.album_title.is_empty() {
                    return None;
                }
                Some(
                    div()
                        .id("np_album")
                        .max_w(px(max_w))
                        .text_xs()
                        .text_color(muted_fg)
                        .truncate()
                        .cursor_pointer()
                        .border_b(px(1.))
                        .hover(|s| s.border_color(muted_fg))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(NavigateToAlbumRequested { album_id });
                        }))
                        .child(self.album_title.clone())
                        .into_any_element(),
                )
            }
            NowPlayingDetails::Hidden => None,
        }
    }

    fn clear(&mut self) {
        self.has_track = false;
        self.track_title = SharedString::default();
        self.artists.clear();
        self.album_id = None;
        self.album_title = SharedString::default();
        self.year = None;
        self.cover_art_id = None;
        self.cover_image = None;
        self.specs = SharedString::default();
    }
}

impl EventEmitter<NavigateToAlbumRequested> for NowPlaying {}
impl EventEmitter<NavigateToArtistRequested> for NowPlaying {}

impl Render for NowPlaying {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let album_id = self.album_id;
        let track_title = self.track_title.clone();
        let foreground = Colors::foreground(cx);
        let settings = cx.global::<SettingsStore>();
        let scale = settings.font_scale().ui_scale();
        if !self.has_track {
            return h_flex().w(px(200. * scale)).into_any_element();
        }
        let show_time_labels = settings.show_time_labels();
        let transport_buttons = if settings.show_repeat_shuffle() {
            5.
        } else {
            3.
        };
        let details = settings.now_playing_details();
        let viewport_w = f32::from(window.viewport_size().width);
        let rem = f32::from(window.rem_size());
        let cover_size = 56. * scale;
        let left_of = |x: f32, margin: f32| {
            (x - rem - cover_size - rem * 0.75 - margin).clamp(96. * scale, 460.)
        };
        let progress_left = (viewport_w
            - crate::track_progress_slider::row_content_width(viewport_w, rem, show_time_labels))
            * 0.5;
        let buttons_left =
            (viewport_w - (transport_buttons * 36. + (transport_buttons - 1.) * rem * 0.5)) * 0.5;
        let text_w = left_of(buttons_left, rem * 0.5);
        let line_w = left_of(progress_left, rem * 0.75);
        let details_line = self.details_line(details, line_w, cx);
        let artists_w = if details_line.is_some() {
            text_w
        } else {
            line_w
        };
        let cover_radius = 6. * scale;

        h_flex()
            .gap_3()
            .items_center()
            .w(px(200. * scale))
            .child({
                if let Some(cover_img) = self.cover_image.clone() {
                    img(cover_img)
                        .flex_shrink_0()
                        .w(px(cover_size))
                        .h(px(cover_size))
                        .rounded(px(cover_radius))
                        .object_fit(gpui::ObjectFit::Cover)
                        .with_fallback({
                            let bg = Colors::secondary(cx);
                            let fg = Colors::muted_foreground(cx);
                            move || {
                                cover_placeholder(cover_size, cover_radius, bg, fg)
                                    .into_any_element()
                            }
                        })
                        .into_any_element()
                } else {
                    cover_placeholder(
                        cover_size,
                        cover_radius,
                        Colors::secondary(cx),
                        Colors::muted_foreground(cx),
                    )
                    .into_any_element()
                }
            })
            .child(
                v_flex()
                    .flex_shrink_0()
                    .w(px(text_w))
                    .items_start()
                    .child({
                        let title = div()
                            .max_w_full()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(track_title);

                        if let Some(aid) = album_id {
                            div()
                                .id("np_title")
                                .max_w_full()
                                .cursor_pointer()
                                .border_b(px(1.))
                                .hover(|s| s.border_color(foreground))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(NavigateToAlbumRequested { album_id: aid });
                                }))
                                .child(title)
                                .into_any_element()
                        } else {
                            title.into_any_element()
                        }
                    })
                    .child({
                        let muted_fg = Colors::muted_foreground(cx);
                        if self.artists.is_empty() {
                            div()
                                .text_xs()
                                .text_color(muted_fg)
                                .truncate()
                                .into_any_element()
                        } else {
                            let mut row = h_flex().overflow_hidden().max_w(px(artists_w));
                            let last = self.artists.len() - 1;
                            for (i, (artist_id, name)) in self.artists.iter().enumerate() {
                                if i > 0 {
                                    row = row.child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .child(", "),
                                    );
                                }
                                let artist_id = *artist_id;
                                let item = div()
                                    .id(("np_artist", artist_id as u64))
                                    .text_xs()
                                    .text_color(muted_fg)
                                    .cursor_pointer()
                                    .border_b(px(1.))
                                    .hover(|s| s.border_color(muted_fg))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(NavigateToArtistRequested { artist_id });
                                    }))
                                    .child(name.clone());
                                row = row.child(if i == last {
                                    item.min_w(px(0.)).overflow_hidden().text_ellipsis()
                                } else {
                                    item.flex_shrink_0()
                                });
                            }
                            row.into_any_element()
                        }
                    })
                    .children(details_line),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::format_specs;

    #[test]
    fn integer_khz() {
        assert_eq!(
            format_specs(Some(48000), Some(24), None, None),
            "48 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn fractional_khz() {
        assert_eq!(
            format_specs(Some(44100), Some(16), None, None),
            "44.1 kHz \u{b7} 16-bit"
        );
    }

    #[test]
    fn bitrate_only() {
        assert_eq!(format_specs(None, None, Some(320), None), "320 kbps");
    }

    #[test]
    fn sample_rate_and_bitrate_combined() {
        assert_eq!(
            format_specs(Some(96000), Some(24), Some(1411), None),
            "96 kHz \u{b7} 24-bit \u{b7} 1411 kbps"
        );
    }

    #[test]
    fn empty_when_nothing_known() {
        assert_eq!(format_specs(None, None, None, None), "");
    }

    #[test]
    fn needs_both_rate_and_depth() {
        assert_eq!(format_specs(Some(48000), None, None, None), "");
        assert_eq!(format_specs(None, Some(24), None, None), "");
    }

    #[test]
    fn dsd64_label_with_target_rate() {
        assert_eq!(
            format_specs(Some(352_800), Some(24), None, Some(2_822_400)),
            "DSD64\u{2192}352.8 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn dsd128_label_but_target_rate_stays_fixed() {
        // Higher DSD multiples are cascaded down to DSD64's own 352.8kHz
        // (see `dsd::source`) instead of handing out 705.6kHz+ PCM — the
        // label still reflects the *source* multiplier, only the rate after
        // the arrow stays constant across DSD64/128/256/512.
        assert_eq!(
            format_specs(Some(352_800), Some(24), None, Some(5_644_800)),
            "DSD128\u{2192}352.8 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn dsd256_label_but_target_rate_stays_fixed() {
        assert_eq!(
            format_specs(Some(352_800), Some(24), None, Some(11_289_600)),
            "DSD256\u{2192}352.8 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn dsd_suppresses_bitrate_even_if_present() {
        assert_eq!(
            format_specs(Some(352_800), Some(24), Some(1411), Some(2_822_400)),
            "DSD64\u{2192}352.8 kHz \u{b7} 24-bit"
        );
    }

    #[test]
    fn dsd_label_alone_without_target_rate() {
        assert_eq!(format_specs(None, None, None, Some(2_822_400)), "DSD64");
    }
}
