use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Anchor, App, AppContext, Bounds, BoxShadow, ClickEvent, Context, DispatchPhase, Div, Entity,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Render,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, Window, anchored, canvas,
    deferred, div, point, prelude::FluentBuilder, px, svg,
};

use gpui_component::tooltip::Tooltip;

use crate::cover_backdrop;
use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;
use crate::volume::{Volume, volume_icon};
use ui_components::slider::{Slider, SliderEvent};

pub const ICON_SIZE: f32 = 34.;
pub const PANEL_H: f32 = 132.;
const FIT_MARGIN: f32 = 6.;
const FIT_GAP: f32 = 8.;

pub struct CoverVolume {
    volume: Entity<Volume>,
    slider: Entity<Slider>,
    expanded: bool,
    fit_window: bool,
    icon_bounds: Rc<Cell<Bounds<Pixels>>>,
    panel_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
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
            fit_window: false,
            icon_bounds: Rc::default(),
            panel_bounds: Rc::default(),
            _slider_subscription: slider_subscription,
            _volume_subscription: volume_subscription,
        }
    }

    pub fn fit_window(mut self) -> Self {
        self.fit_window = true;
        self
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn collapse(&mut self, cx: &mut Context<Self>) {
        if self.expanded {
            self.expanded = false;
            cx.notify();
        }
    }

    fn on_icon_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<Services>().volume_locked() {
            return;
        }
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.volume.update(cx, |vol, cx| vol.wheel(e, cx));
        cx.stop_propagation();
    }

    fn panel(&self, height: f32, cx: &App) -> Div {
        div()
            .h(px(height))
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
                inset: false,
            }])
            .child(self.slider.clone())
    }

    fn fitted_panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let insets = gpui_component::window_paddings(window);
        let room =
            f32::from(window.viewport_size().height - insets.top - insets.bottom) - FIT_MARGIN * 2.;
        let icon = self.icon_bounds.get();
        let panel_bounds = self.panel_bounds.clone();
        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .position(point(icon.origin.x, icon.origin.y - px(FIT_GAP)))
                .snap_to_window_with_margin(px(FIT_MARGIN))
                .child(
                    self.panel(PANEL_H.min(room), cx)
                        .occlude()
                        .on_scroll_wheel(cx.listener(Self::on_scroll))
                        .child(
                            canvas(
                                move |bounds, _, _| panel_bounds.set(Some(bounds)),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        ),
                ),
        )
    }
}

impl Render for CoverVolume {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_exclusive = cx.global::<Services>().volume_locked();
        let (value, muted) = {
            let vol = self.volume.read(cx);
            (vol.value(), vol.is_muted())
        };

        let icon = volume_icon(is_exclusive, muted, value);

        let fg = Colors::foreground(cx);
        let hover_bg = cover_backdrop::inset_bg(Colors::muted(cx), cover_backdrop::veil_factor(cx));
        let expanded = self.expanded && !is_exclusive;

        self.slider
            .update(cx, |s, cx| s.set_disabled(is_exclusive, cx));

        div()
            .id("cover_volume")
            .absolute()
            .bottom_0()
            .left_0()
            .w(px(ICON_SIZE))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .when(self.fit_window, |d| {
                let icon_bounds = self.icon_bounds.clone();
                d.child(
                    canvas(move |bounds, _, _| icon_bounds.set(bounds), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
            })
            .when(expanded, |d| {
                let entity = cx.entity();
                let panel_bounds = self.panel_bounds.clone();
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture || e.button != MouseButton::Left
                                {
                                    return;
                                }
                                if bounds.contains(&e.position)
                                    || panel_bounds.get().is_some_and(|b| b.contains(&e.position))
                                {
                                    return;
                                }
                                entity.update(cx, |this, cx| this.collapse(cx));
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .map(|d| {
                    if self.fit_window {
                        d.child(self.fitted_panel(window, cx))
                    } else {
                        d.child(self.panel(PANEL_H, cx).block_mouse_except_scroll())
                    }
                })
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
                    .child(
                        svg()
                            .path(icon)
                            .size(px(22.))
                            .text_color(fg)
                            .opacity(if is_exclusive { 0.4 } else { 1.0 }),
                    ),
            )
    }
}
