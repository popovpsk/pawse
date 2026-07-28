use gpui::{
    AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip};

use crate::theme_colors::Colors;
use ui_components::slider::{Slider, SliderEvent};

use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub const VOLUME_STEP: f32 = 0.05;

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
}

impl Render for Volume {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let services = cx.global::<Services>();
        let is_exclusive = services.output.is_exclusive();

        let icon_path = volume_icon(is_exclusive, self.is_muted, self.volume);

        let mut container = h_flex()
            .id("volume_control")
            .gap_2()
            .justify_end()
            .items_center()
            .child({
                let tooltip_text = self.tooltip_text(is_exclusive);

                div()
                    .id("volume_icon")
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                    .on_click(cx.listener(Self::on_icon_click))
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(22.))
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
        container = container.child(div().w(px(100.)).child(self.slider.clone()));

        container.w_full().h_6()
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

        cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let volume = cx.global::<SettingsStore>().volume();
            if volume == this.volume {
                return;
            }
            this.volume = volume;
            this.is_muted = volume <= 0.0;
            if volume > 0.0 {
                this.volume_before_mute = volume;
            }
            this.slider
                .update(cx, |slider, cx| slider.set_value_silent(volume, cx));
            cx.notify();
        })
        .detach();

        Self {
            slider,
            volume: initial,
            is_muted: initial <= 0.0,
            volume_before_mute: if initial > 0.0 { initial } else { 1.0 },
            slider_pinned_to_unity: false,
        }
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
        if cx.global::<Services>().output.is_exclusive() {
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

    pub fn nudge(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set(self.volume + delta, cx);
    }

    pub fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        if cx.global::<Services>().output.is_exclusive() {
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
