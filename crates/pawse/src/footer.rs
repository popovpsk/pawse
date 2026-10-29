use gpui::{
    AnyElement, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::theme_colors::Colors;

use crate::localization::tr;
use crate::settings_store::SettingsStore;
use crate::{
    cover_volume::CoverVolume,
    next_button::NextButton,
    now_playing::{
        NavigateToAlbumRequested, NavigateToArtistRequested, NowPlaying, ToggleCoverModeRequested,
    },
    play_button::PlayButton,
    prev_button::PrevButton,
    repeat_button::RepeatButton,
    shuffle_button::ShuffleButton,
    track_progress_slider::TrackProgressSlider,
    volume::Volume,
};

#[derive(Clone, Debug)]
pub struct ToggleQueueEvent {
    pub show: bool,
}

#[derive(Clone, Debug)]
pub struct ToggleLyricsEvent {
    pub show: bool,
}

#[derive(Clone, Debug)]
pub struct OpenSleepTimerEvent;

pub const SIDE_COLUMN_W: f32 = 200.;
const BADGE_PAD: f32 = 6.;
const BADGE_ICON: f32 = 12.;

pub struct Footer {
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    pub shuffle_button: Entity<ShuffleButton>,
    repeat_button: Entity<RepeatButton>,
    volume_slider: Entity<Volume>,
    compact_volume: Entity<CoverVolume>,
    volume_compact: bool,
    badge_label_width: Option<(SharedString, Pixels, f32)>,
    track_progress_slider: Entity<TrackProgressSlider>,
    now_playing: Entity<NowPlaying>,
    show_queue: bool,
    show_lyrics: bool,
    show_repeat_shuffle: bool,
    sleep_timer: Option<Entity<crate::sleep_timer::SleepTimer>>,
    _sleep_timer_observe: Option<Subscription>,
    _settings_subscription: Subscription,
    _np_album_subscription: Subscription,
    _np_artist_subscription: Subscription,
    _np_cover_subscription: Subscription,
    _app_events: crate::view_cache::AppEvents,
}

impl Footer {
    pub fn volume(&self) -> &Entity<Volume> {
        &self.volume_slider
    }

    pub fn progress(&self) -> &Entity<TrackProgressSlider> {
        &self.track_progress_slider
    }

    pub fn set_show_queue(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_queue == show {
            return;
        }
        self.show_queue = show;
        cx.notify();
    }

    pub fn set_show_lyrics(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_lyrics == show {
            return;
        }
        self.show_lyrics = show;
        cx.notify();
    }
}

impl EventEmitter<ToggleQueueEvent> for Footer {}
impl EventEmitter<ToggleLyricsEvent> for Footer {}
impl EventEmitter<OpenSleepTimerEvent> for Footer {}
impl EventEmitter<NavigateToAlbumRequested> for Footer {}
impl EventEmitter<NavigateToArtistRequested> for Footer {}
impl EventEmitter<ToggleCoverModeRequested> for Footer {}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let show_repeat_shuffle = cx.global::<SettingsStore>().show_repeat_shuffle();
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let new_val = cx.global::<SettingsStore>().show_repeat_shuffle();
            if new_val != this.show_repeat_shuffle {
                this.show_repeat_shuffle = new_val;
                cx.notify();
            }
        });

        let sleep_timer = crate::sleep_timer::timer(cx);
        let sleep_timer_observe = sleep_timer
            .as_ref()
            .map(|timer| cx.observe(timer, |_, _, cx| cx.notify()));

        let now_playing = cx.new(|cx| NowPlaying::new(window, cx));
        let volume_slider = cx.new(|cx| Volume::new(window, cx));
        let compact_volume = cx.new(|cx| CoverVolume::new(volume_slider.clone(), window, cx));

        let np_album_subscription = cx.subscribe(
            &now_playing,
            |_, _, event: &NavigateToAlbumRequested, cx| {
                cx.emit(NavigateToAlbumRequested {
                    album_id: event.album_id,
                });
            },
        );
        let np_artist_subscription = cx.subscribe(
            &now_playing,
            |_, _, event: &NavigateToArtistRequested, cx| {
                cx.emit(event.clone());
            },
        );
        let np_cover_subscription = cx.subscribe(
            &now_playing,
            |_, _, event: &ToggleCoverModeRequested, cx| {
                cx.emit(event.clone());
            },
        );

        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
            shuffle_button: cx.new(|cx| ShuffleButton::new(window, cx)),
            repeat_button: cx.new(|cx| RepeatButton::new(window, cx)),
            volume_slider,
            compact_volume,
            volume_compact: false,
            badge_label_width: None,
            track_progress_slider: cx.new(|cx| TrackProgressSlider::new(window, cx)),
            now_playing,
            show_queue: false,
            show_lyrics: false,
            show_repeat_shuffle,
            sleep_timer,
            _sleep_timer_observe: sleep_timer_observe,
            _settings_subscription: settings_subscription,
            _np_album_subscription: np_album_subscription,
            _np_artist_subscription: np_artist_subscription,
            _np_cover_subscription: np_cover_subscription,
            _app_events: crate::view_cache::follow_app_events(cx),
        }
    }
}

impl Footer {
    fn update_volume_compact(
        &mut self,
        badge: Option<(&SharedString, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = cx.global::<SettingsStore>();
        let scale = settings.font_scale().ui_scale();
        let show_labels = settings.show_time_labels();
        let viewport_w = f32::from(window.viewport_size().width);
        let rem = f32::from(window.rem_size());
        let badge_w = badge.map_or(0., |(label, countdown)| {
            let label_w = if countdown {
                rem * 0.25 + self.badge_label_width(label, window)
            } else {
                0.
            };
            2. * BADGE_PAD + BADGE_ICON + label_w + rem * 0.5
        });
        let room =
            crate::track_progress_slider::right_column_room(viewport_w, rem, scale, show_labels);
        let compact = crate::volume::full_width(rem) + badge_w > room;
        if self.volume_compact && !compact {
            self.compact_volume.update(cx, |v, cx| v.collapse(cx));
        }
        self.volume_compact = compact;
    }

    fn badge_label_width(&mut self, label: &SharedString, window: &mut Window) -> f32 {
        let font_size = window.rem_size() * 0.75;
        if let Some((template, size, width)) = &self.badge_label_width
            && *size == font_size
            && same_shape(template, label)
        {
            return *width;
        }
        let template = SharedString::from(
            label
                .chars()
                .map(|c| if c.is_ascii_digit() { '0' } else { c })
                .collect::<String>(),
        );
        let width = f32::from(text_xs_width(&template, window));
        self.badge_label_width = Some((template, font_size, width));
        width
    }

    pub fn collapse_volume(&mut self, cx: &mut Context<Self>) {
        self.compact_volume.update(cx, |v, cx| v.collapse(cx));
    }

    fn compact_volume_slot(&self, rem: f32) -> AnyElement {
        let size = crate::cover_volume::ICON_SIZE;
        div()
            .relative()
            .flex_shrink_0()
            .w(px(size))
            .h_6()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom(px((rem * 1.5 - size) * 0.5))
                    .size(px(size))
                    .child(self.compact_volume.clone()),
            )
            .into_any_element()
    }
}

impl Render for Footer {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let show_repeat_shuffle = self.show_repeat_shuffle;
        let scale = cx.global::<SettingsStore>().font_scale().ui_scale();
        let badge = self.sleep_timer.as_ref().and_then(|timer| {
            let timer = timer.read(cx);
            timer
                .badge()
                .map(|label| (label, timer.status(), timer.is_countdown()))
        });
        self.update_volume_compact(
            badge
                .as_ref()
                .map(|(label, _, countdown)| (label, *countdown)),
            window,
            cx,
        );
        let compact = self.volume_compact;
        let volume = if compact {
            self.compact_volume_slot(f32::from(window.rem_size()))
        } else {
            self.volume_slider.clone().into_any_element()
        };
        let sleep_badge =
            badge.map(|(label, status, countdown)| sleep_timer_badge(label, status, countdown, cx));
        h_flex()
            .gap_4()
            .w_full()
            .h_full()
            .items_center()
            .px_4()
            .child(self.now_playing.clone())
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .gap_1()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(show_repeat_shuffle, |b| {
                                b.child(self.shuffle_button.clone())
                            })
                            .child(self.prev_button.clone())
                            .child(self.play_button.clone())
                            .child(self.next_button.clone())
                            .when(show_repeat_shuffle, |b| b.child(self.repeat_button.clone())),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .px_4()
                            .child(self.track_progress_slider.clone()),
                    ),
            )
            .child({
                let queue_color = if self.show_queue {
                    Colors::primary(cx)
                } else {
                    Colors::muted_foreground(cx)
                };
                let lyrics_color = if self.show_lyrics {
                    Colors::primary(cx)
                } else {
                    Colors::muted_foreground(cx)
                };
                v_flex()
                    .w(px(SIDE_COLUMN_W * scale))
                    .items_end()
                    .justify_center()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .id("mini_player_toggle")
                                    .size(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_full()
                                    .hover(|s| s.bg(Colors::muted(cx)))
                                    .tooltip(|window, cx| {
                                        Tooltip::new(tr().mini_player.clone()).build(window, cx)
                                    })
                                    .on_click(|_, _, cx| crate::mini_player::enter(cx))
                                    .child(
                                        svg()
                                            .path("icons/s2-mini-player.svg")
                                            .size(px(22.))
                                            .text_color(Colors::muted_foreground(cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("lyrics_toggle")
                                    .size(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_full()
                                    .hover(|s| s.bg(Colors::muted(cx)))
                                    .tooltip(|window, cx| {
                                        Tooltip::new(tr().lyrics.clone()).build(window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_lyrics = !this.show_lyrics;
                                        cx.emit(ToggleLyricsEvent {
                                            show: this.show_lyrics,
                                        });
                                        cx.notify();
                                    }))
                                    .child(
                                        svg()
                                            .path("icons/s2-lyrics.svg")
                                            .size(px(22.))
                                            .text_color(lyrics_color),
                                    ),
                            )
                            .child(
                                div()
                                    .id("queue_toggle")
                                    .size(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_full()
                                    .hover(|s| s.bg(Colors::muted(cx)))
                                    .tooltip(|window, cx| {
                                        Tooltip::new(tr().queue.clone()).build(window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_queue = !this.show_queue;
                                        cx.emit(ToggleQueueEvent {
                                            show: this.show_queue,
                                        });
                                        cx.notify();
                                    }))
                                    .child(
                                        svg()
                                            .path("icons/s2-queue.svg")
                                            .size(px(22.))
                                            .text_color(queue_color),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_end()
                            .when(!compact, |d| d.gap_2())
                            .children(sleep_badge)
                            .child(volume),
                    )
            })
    }
}

pub(crate) fn text_xs_width(text: &SharedString, window: &mut Window) -> Pixels {
    let run = window.text_style().to_run(text.len());
    window
        .text_system()
        .shape_line(text.clone(), window.rem_size() * 0.75, &[run], None)
        .width
}

fn same_shape(template: &str, label: &str) -> bool {
    template.len() == label.len()
        && template
            .bytes()
            .zip(label.bytes())
            .all(|(t, l)| t == l || (t == b'0' && l.is_ascii_digit()))
}

fn sleep_timer_badge(
    label: SharedString,
    status: SharedString,
    countdown: bool,
    cx: &mut Context<Footer>,
) -> AnyElement {
    let color = Colors::muted_foreground(cx);
    let hover_bg = Colors::muted(cx);
    div()
        .id("sleep_timer_badge")
        .flex()
        .items_center()
        .gap_1()
        .h(px(22.))
        .px(px(BADGE_PAD))
        .rounded_full()
        .text_xs()
        .text_color(color)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .tooltip(move |window, cx| Tooltip::new(status.clone()).build(window, cx))
        .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenSleepTimerEvent)))
        .child(
            svg()
                .path("icons/moon.svg")
                .size(px(BADGE_ICON))
                .text_color(color),
        )
        .when(countdown, |d| d.child(label))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::same_shape;

    #[test]
    fn same_shape_ignores_digit_values() {
        assert!(same_shape("0:00:00", "1:59:21"));
        assert!(same_shape("00:00", "00:00"));
        assert!(!same_shape("0:00:00", "59:59"));
        assert!(!same_shape("00:00", "ab:cd"));
    }
}
