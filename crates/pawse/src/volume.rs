use gpui::{
    AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollDelta, ScrollWheelEvent, StatefulInteractiveElement, Styled, TouchPhase, Window,
    div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip};

use crate::theme_colors::Colors;
use ui_components::slider::{Slider, SliderEvent};

use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub const VOLUME_STEP: f32 = 0.05;
const WHEEL_PIXELS_PER_STEP: f32 = 24.;
const WHEEL_LINES_PER_STEP: f32 = 3.;
const ICON_SIZE: f32 = 22.;
const SLIDER_W: f32 = 100.;

pub fn full_width(rem: f32) -> f32 {
    ICON_SIZE + rem * 0.5 + SLIDER_W
}

#[derive(Default)]
struct WheelSteps {
    pending: f32,
    in_gesture: bool,
}

impl WheelSteps {
    fn volume_delta(&mut self, event: &ScrollWheelEvent) -> f32 {
        let delta = self.step(event, cfg!(target_os = "macos"));
        if natural_scrolling() { -delta } else { delta }
    }

    fn step(&mut self, event: &ScrollWheelEvent, macos: bool) -> f32 {
        let pixels = match event.delta {
            ScrollDelta::Lines(lines) if macos => {
                self.pending = 0.;
                return if lines.y == 0. {
                    0.
                } else {
                    lines.y.signum() * VOLUME_STEP
                };
            }
            ScrollDelta::Lines(lines) => {
                lines.y.clamp(-WHEEL_LINES_PER_STEP, WHEEL_LINES_PER_STEP) * WHEEL_PIXELS_PER_STEP
                    / WHEEL_LINES_PER_STEP
            }
            ScrollDelta::Pixels(pixels) => {
                if macos {
                    match event.touch_phase {
                        TouchPhase::Started => self.in_gesture = true,
                        TouchPhase::Ended | TouchPhase::Cancelled => self.in_gesture = false,
                        TouchPhase::Moved => {}
                    }
                    if !self.in_gesture {
                        self.pending = 0.;
                        return 0.;
                    }
                }
                f32::from(pixels.y)
            }
        };
        if pixels * self.pending < 0. {
            self.pending = 0.;
        }
        self.pending += pixels;
        let steps = (self.pending / WHEEL_PIXELS_PER_STEP).trunc();
        self.pending -= steps * WHEEL_PIXELS_PER_STEP;
        steps * VOLUME_STEP
    }
}

#[cfg(target_os = "macos")]
fn natural_scrolling() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("com.apple.swipescrolldirection");
    defaults.objectForKey(&key).is_none() || defaults.boolForKey(&key)
}

#[cfg(not(target_os = "macos"))]
fn natural_scrolling() -> bool {
    false
}

pub fn volume_icon(is_exclusive: bool, muted: bool, value: f32) -> &'static str {
    if is_exclusive {
        "icons/volume_unmute.svg"
    } else if muted || value <= 0. {
        "icons/volume_mute.svg"
    } else if value < 0.5 {
        "icons/volume_low.svg"
    } else {
        "icons/volume_unmute.svg"
    }
}

pub struct Volume {
    slider: Entity<Slider>,
    volume: f32,
    is_muted: bool,
    volume_before_mute: f32,
    /// Exclusive mode takes the app gain out of the signal path entirely, so the
    /// (disabled) slider is parked at unity while it lasts. `volume` keeps the
    /// user's real level and comes back on the way out.
    slider_pinned_to_unity: bool,
    wheel: WheelSteps,
    fill: bool,
}

impl Render for Volume {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let services = cx.global::<Services>();
        let is_exclusive = services.volume_locked();

        let icon_path = volume_icon(is_exclusive, self.is_muted, self.volume);

        let container = h_flex()
            .id("volume_control")
            .gap_2()
            .justify_end()
            .items_center()
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child({
                let tooltip_text = self.tooltip_text(is_exclusive);

                div()
                    .id("volume_icon")
                    .when(!is_exclusive, |d| d.cursor_pointer())
                    .opacity(if is_exclusive { 0.4 } else { 1.0 })
                    .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                    .on_click(cx.listener(Self::on_icon_click))
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(ICON_SIZE))
                            .text_color(Colors::foreground(cx)),
                    )
            });

        self.slider
            .update(cx, |slider, cx| slider.set_disabled(is_exclusive, cx));
        if self.slider_pinned_to_unity != is_exclusive {
            self.slider_pinned_to_unity = is_exclusive;
            let shown = if is_exclusive { 1.0 } else { self.volume };
            self.slider
                .update(cx, |slider, cx| slider.set_value_silent(shown, cx));
        }
        if self.fill {
            container
                .w_full()
                .child(div().flex_1().min_w(px(0.)).child(self.slider.clone()))
                .h_6()
        } else {
            container
                .child(div().w(px(SLIDER_W)).child(self.slider.clone()))
                .flex_shrink_0()
                .h_6()
        }
    }
}

impl Volume {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = cx.global::<crate::settings_store::SettingsStore>().volume();
        let slider = cx.new(|cx| {
            Slider::new(cx)
                .default_value(initial)
                .min(0.0)
                .max(1.0)
                .step(0.01)
        });

        cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => {
                let volume = *value;
                this.volume = volume;
                this.is_muted = volume <= 0.0;
                crate::services::set_volume(cx, volume);
                cx.notify();
            }
        })
        .detach();

        cx.observe_global::<SettingsStore>(Self::sync_volume)
            .detach();
        cx.observe_global::<crate::cast::CastState>(Self::sync_volume)
            .detach();

        Self {
            slider,
            volume: initial,
            is_muted: initial <= 0.0,
            volume_before_mute: if initial > 0.0 { initial } else { 1.0 },
            slider_pinned_to_unity: false,
            wheel: WheelSteps::default(),
            fill: false,
        }
    }

    pub fn fill_width(mut self) -> Self {
        self.fill = true;
        self
    }

    fn sync_volume(&mut self, cx: &mut Context<Self>) {
        let volume = crate::services::effective_volume(cx);
        if volume == self.volume {
            return;
        }
        self.volume = volume;
        self.is_muted = volume <= 0.0;
        if volume > 0.0 {
            self.volume_before_mute = volume;
        }
        self.slider
            .update(cx, |slider, cx| slider.set_value_silent(volume, cx));
        cx.notify();
    }

    fn tooltip_text(&self, is_exclusive: bool) -> gpui::SharedString {
        let s = tr();
        if is_exclusive {
            s.volume.clone()
        } else if self.is_muted || self.volume <= 0. {
            s.unmute.clone()
        } else {
            s.mute.clone()
        }
    }

    fn on_icon_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<Services>().volume_locked() {
            return;
        }

        let new_volume = if self.is_muted {
            self.is_muted = false;
            self.volume = self.volume_before_mute;
            self.volume_before_mute
        } else {
            self.is_muted = true;
            self.volume_before_mute = self.volume;
            self.volume = 0.0;
            0.0
        };

        self.slider
            .update(cx, |slider, cx| slider.set_value_silent(new_volume, cx));
        crate::services::set_volume(cx, new_volume);
        cx.notify();
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.wheel(e, cx);
    }

    pub fn wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = self.wheel.volume_delta(e);
        if delta != 0. {
            self.nudge(delta, cx);
        }
    }

    pub fn nudge(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set(self.volume + delta, cx);
    }

    pub fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        if cx.global::<Services>().volume_locked() {
            return;
        }
        let new = value.clamp(0.0, 1.0);
        self.volume = new;
        self.is_muted = new <= 0.0;
        if new > 0.0 {
            self.volume_before_mute = new;
        }
        self.slider
            .update(cx, |slider, cx| slider.set_value_silent(new, cx));
        crate::services::set_volume(cx, new);
        cx.notify();
    }

    pub fn value(&self) -> f32 {
        self.volume
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;
    use rstest::rstest;

    fn lines(y: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0., y)),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        }
    }

    fn pixels(y: f32, touch_phase: TouchPhase) -> ScrollWheelEvent {
        ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.), px(y))),
            touch_phase,
            ..Default::default()
        }
    }

    fn gesture(moves: Vec<ScrollWheelEvent>) -> Vec<ScrollWheelEvent> {
        [
            vec![pixels(0., TouchPhase::Started)],
            moves,
            vec![pixels(0., TouchPhase::Ended)],
        ]
        .concat()
    }

    #[rstest]
    #[case::wheel_notch(vec![lines(3.)], 1.)]
    #[case::high_res_notch(vec![lines(0.375); 8], 1.)]
    #[case::page_scroll_setting(vec![lines(1e9)], 1.)]
    #[case::touchpad_flick(vec![lines(1.5); 20], 10.)]
    #[case::one_line_setting(vec![lines(1.); 3], 1.)]
    #[case::wayland_pixels(vec![pixels(2., TouchPhase::Moved); 24], 2.)]
    #[case::reversal_drops_pending(
        vec![pixels(20., TouchPhase::Moved), pixels(-6., TouchPhase::Moved), pixels(-20., TouchPhase::Moved)],
        -1.
    )]
    fn wheel_steps_windows_linux(#[case] events: Vec<ScrollWheelEvent>, #[case] steps: f32) {
        assert_steps(&events, false, steps);
    }

    #[rstest]
    #[case::wheel_notch(vec![lines(1.)], 1.)]
    #[case::accelerated_notch(vec![lines(-7.)], -1.)]
    #[case::slow_notch(vec![lines(0.1)], 1.)]
    #[case::trackpad_gesture(gesture(vec![pixels(2., TouchPhase::Moved); 24]), 2.)]
    #[case::small_gesture_holds_back(gesture(vec![pixels(5., TouchPhase::Moved); 4]), 0.)]
    #[case::momentum_ignored(
        [gesture(vec![pixels(24., TouchPhase::Moved)]), vec![pixels(24., TouchPhase::Moved); 3]].concat(),
        1.
    )]
    #[case::gesture_started_elsewhere(vec![pixels(24., TouchPhase::Moved); 3], 0.)]
    fn wheel_steps_macos(#[case] events: Vec<ScrollWheelEvent>, #[case] steps: f32) {
        assert_steps(&events, true, steps);
    }

    fn assert_steps(events: &[ScrollWheelEvent], macos: bool, steps: f32) {
        let mut wheel = WheelSteps::default();
        let total: f32 = events.iter().map(|e| wheel.step(e, macos)).sum();
        assert!((total - steps * VOLUME_STEP).abs() < 1e-6, "{total}");
    }
}
