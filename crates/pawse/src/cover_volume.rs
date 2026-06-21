use gpui::{
    AppContext, BoxShadow, ClickEvent, Context, DispatchPhase, Entity, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, ScrollDelta, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, point,
    prelude::FluentBuilder, px, svg,
};

use gpui_component::tooltip::Tooltip;

use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;
use crate::volume::{VOLUME_STEP, Volume, volume_icon};
use ui_components::slider::{Slider, SliderEvent};

const ICON_SIZE: f32 = 36.;
const PANEL_H: f32 = 132.;

pub struct CoverVolume {
    volume: Entity<Volume>,
    slider: Entity<Slider>,
    expanded: bool,
    _slider_subscription: Subscription,
    _volume_subscription: Subscription,
}

impl CoverVolume {
    pub fn new(volume: Entity<Volume>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = volume.read(cx).value();
        let slider = cx.new(|cx| {
            Slider::new(cx)
                .vertical(true)
                .default_value(initial)
                .min(0.0)
                .max(1.0)
                .step(0.01)
        });

        let slider_subscription = cx.subscribe(&slider, {
            let volume = volume.clone();
            move |_, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(v) = event;
                let v = *v;
                volume.update(cx, |vol, cx| vol.set(v, cx));
            }
        });

        let volume_subscription = cx.observe(&volume, {
            let slider = slider.clone();
            move |_, volume, cx| {
                let v = volume.read(cx).value();
                slider.update(cx, |s, cx| s.set_value_silent(v, cx));
                cx.notify();
            }
        });

        Self {
            volume,
            slider,
            expanded: false,
            _slider_subscription: slider_subscription,
            _volume_subscription: volume_subscription,
        }
    }

    pub fn collapse(&mut self, cx: &mut Context<Self>) {
        if self.expanded {
            self.expanded = false;
            cx.notify();
        }
    }

    fn on_icon_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<Services>().output.is_exclusive() {
            return;
        }
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(p) => p.y,
        };
        if dy == 0. {
            return;
        }
        let delta = if dy > 0. { VOLUME_STEP } else { -VOLUME_STEP };
        self.volume.update(cx, |vol, cx| vol.nudge(delta, cx));
    }
}

impl Render for CoverVolume {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_exclusive = cx.global::<Services>().output.is_exclusive();
        let (value, muted) = {
            let vol = self.volume.read(cx);
            (vol.value(), vol.is_muted())
        };

        let icon = volume_icon(is_exclusive, muted, value);

        let fg = Colors::foreground(cx);
        let hover_bg = Colors::muted(cx);
        let expanded = self.expanded && !is_exclusive;

        self.slider
            .update(cx, |s, cx| s.set_disabled(is_exclusive, cx));

        div()
            .id("cover_volume")
            .absolute()
            .bottom(px(12.))
            .right(px(100.))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .when(expanded, |d| {
                let entity = cx.entity();
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture || e.button != MouseButton::Left
                                {
                                    return;
                                }
                                if bounds.contains(&e.position) {
                                    return;
                                }
                                entity.update(cx, |this, cx| this.collapse(cx));
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .h(px(PANEL_H))
                        .w(px(ICON_SIZE))
                        .flex()
                        .justify_center()
                        .py(px(16.))
                        .rounded_full()
                        .bg(Colors::popover(cx))
                        .border_1()
                        .border_color(Colors::border(cx))
                        .shadow(vec![BoxShadow {
                            color: gpui::black().opacity(0.35),
                            offset: point(px(0.), px(6.)),
                            blur_radius: px(16.),
                            spread_radius: px(0.),
                        }])
                        .child(self.slider.clone()),
                )
            })
            .child(
                div()
                    .id("cover_volume_icon")
                    .size(px(ICON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .when(!is_exclusive, |d| d.cursor_pointer())
                    .when(expanded, |d| d.bg(hover_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .tooltip(|window, cx| Tooltip::new(tr().volume.clone()).build(window, cx))
                    .on_click(cx.listener(Self::on_icon_click))
                    .child(svg().path(icon).size(px(20.)).text_color(fg)),
            )
    }
}
