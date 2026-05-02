use gpui::{
    AppContext, Bounds, Context, DispatchPhase, DragMoveEvent, Empty, EntityId, EventEmitter,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Pixels, Point, Render, StatefulInteractiveElement, Styled, Window, canvas, div, px, relative,
};
use gpui_component::ActiveTheme;

#[derive(Clone)]
struct DragSlider(EntityId);

impl Render for DragSlider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Event emitted by the slider when its value changes.
pub enum SliderEvent {
    Change(f32),
}

/// A custom horizontal slider component.
pub struct Slider {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    track_bounds: Bounds<Pixels>,
    live_update: bool,
    interacting: bool,
    disabled: bool,
    hovered: bool,
}

impl Slider {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.01,
            track_bounds: Bounds::default(),
            live_update: true,
            interacting: false,
            disabled: false,
            hovered: false,
        }
    }

    pub fn default_value(mut self, value: f32) -> Self {
        self.value = value.clamp(self.min, self.max);
        self
    }

    pub fn min(mut self, min: f32) -> Self {
        self.min = min;
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = max;
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn live_update(mut self, live_update: bool) -> Self {
        self.live_update = live_update;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        let new_value = value.clamp(self.min, self.max);
        if new_value != self.value {
            self.value = new_value;
            cx.emit(SliderEvent::Change(self.value));
            cx.notify();
        }
    }

    pub fn set_value_silent(&mut self, value: f32, cx: &mut Context<Self>) {
        let new_value = value.clamp(self.min, self.max);
        if new_value != self.value {
            self.value = new_value;
            cx.notify();
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn is_interacting(&self) -> bool {
        self.interacting
    }

    fn update_value_by_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let width = self.track_bounds.size.width;
        if width <= px(0.) {
            return;
        }

        let offset_x = position.x - self.track_bounds.left();
        let percentage = (offset_x / width).clamp(0.0, 1.0);
        let raw_value = self.min + percentage * (self.max - self.min);
        let stepped = (raw_value / self.step).round() * self.step;
        let new_value = stepped.clamp(self.min, self.max);

        if new_value != self.value {
            self.value = new_value;
            cx.emit(SliderEvent::Change(self.value));
            cx.notify();
        }
    }

    fn update_value_visual_by_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let width = self.track_bounds.size.width;
        if width <= px(0.) {
            return;
        }

        let offset_x = position.x - self.track_bounds.left();
        let percentage = (offset_x / width).clamp(0.0, 1.0);
        let raw_value = self.min + percentage * (self.max - self.min);
        let stepped = (raw_value / self.step).round() * self.step;
        let new_value = stepped.clamp(self.min, self.max);

        if new_value != self.value {
            self.value = new_value;
            cx.notify();
        }
    }
}

impl EventEmitter<SliderEvent> for Slider {}

impl Render for Slider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let pct = if self.max > self.min {
            (self.value - self.min) / (self.max - self.min)
        } else {
            0.0
        };

        let show_thumb = !self.disabled && (self.hovered || self.interacting);
        let disabled_opacity = if self.disabled { 0.4 } else { 1.0 };

        div()
            .id(("slider-track", entity_id))
            .relative()
            .w_full()
            .h(px(16.))
            .flex()
            .items_center()
            .opacity(disabled_opacity)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    if this.disabled {
                        return;
                    }
                    this.interacting = true;
                    if this.live_update {
                        this.update_value_by_position(event.position, cx);
                    } else {
                        this.update_value_visual_by_position(event.position, cx);
                    }
                }),
            )
            .on_drag(
                DragSlider(entity_id),
                move |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .on_drag_move(cx.listener(
                move |this, e: &DragMoveEvent<DragSlider>, _window, cx| {
                    if e.drag(cx).0 != entity_id {
                        return;
                    }
                    if this.live_update {
                        this.update_value_by_position(e.event.position, cx);
                    } else {
                        this.update_value_visual_by_position(e.event.position, cx);
                    }
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.disabled {
                        return;
                    }
                    if this.interacting {
                        this.interacting = false;
                        if !this.live_update {
                            cx.emit(SliderEvent::Change(this.value));
                        }
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.disabled {
                        return;
                    }
                    if this.interacting {
                        this.interacting = false;
                        if !this.live_update {
                            cx.emit(SliderEvent::Change(this.value));
                        }
                        cx.notify();
                    }
                }),
            )
            .on_drop::<DragSlider>({
                let entity = cx.entity();
                move |_, _, cx| {
                    entity.update(cx, |this, cx| {
                        if this.interacting {
                            this.interacting = false;
                            if !this.live_update {
                                cx.emit(SliderEvent::Change(this.value));
                            }
                            cx.notify();
                        }
                    });
                }
            })
            .on_hover({
                let entity = cx.entity();
                move |&hovered, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.hovered = hovered;
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .absolute()
                            .h_full()
                            .left(px(0.))
                            .w(relative(pct))
                            .rounded_full()
                            .bg(cx.theme().foreground),
                    )
                    .child(
                        div()
                            .absolute()
                            .size(px(12.))
                            .rounded_full()
                            .bg(cx.theme().foreground)
                            .left(relative(pct))
                            .ml(-px(6.))
                            .top(px(-4.))
                            .opacity(if show_thumb { 1.0 } else { 0.0 }),
                    ),
            )
            .child({
                let entity = cx.entity();
                let entity_for_paint = entity.clone();
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, _| {
                            this.track_bounds = bounds;
                        });
                    },
                    move |_bounds, _prepaint_result, window, _cx| {
                        window.on_mouse_event({
                            move |event: &MouseUpEvent, phase: DispatchPhase, _window, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                if event.button != MouseButton::Left {
                                    return;
                                }
                                entity_for_paint.update(cx, |this, cx| {
                                    if !this.interacting {
                                        return;
                                    }
                                    this.interacting = false;
                                    if !this.live_update {
                                        cx.emit(SliderEvent::Change(this.value));
                                    }
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full()
            })
    }
}