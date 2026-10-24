mod graph;
mod picker;
mod profiles;

use std::cell::RefCell;
use std::rc::Rc;

use equalizer::{Band, DEFAULT_Q, Kind, MAX_BANDS, MAX_FREQ, MAX_GAIN, MAX_Q, MIN_FREQ, MIN_Q};
use gpui::prelude::FluentBuilder;
use gpui::{
    Anchor, AnyElement, App, AppContext, Context, CursorStyle, DispatchPhase, Entity,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, canvas, div, px, relative,
};
use gpui_component::{
    Disableable, Icon, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{Cancel, Confirm, DialogFooter},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    notification::Notification,
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage};
use ui_resources::i18n::equalizer_strings;

use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::{SettingsStore, notify_save_error};
use crate::theme_colors::Colors;

pub use picker::HeadphonePicker;

const EXPORT_NAME: &str = "ParametricEQ.txt";
const Q_OCTAVES_PER_PIXEL: f32 = 1. / 60.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Freq,
    Gain,
    Q,
}

#[derive(Clone, Copy)]
struct Drag {
    ix: usize,
    anchor_y: Pixels,
    anchor_q: f64,
}

struct Row {
    label: SharedString,
    freq: Entity<InputState>,
    gain: Entity<InputState>,
    q: Entity<InputState>,
    _subscriptions: [Subscription; 3],
}

pub struct EqualizerView {
    bands: Vec<Band>,
    source: Option<SharedString>,
    profile: Option<SharedString>,
    saved: Option<Vec<Band>>,
    epoch: u64,
    dirty: bool,
    label: SharedString,
    status: Option<SharedString>,
    preamp: SharedString,
    selected: Option<usize>,
    dragging: Option<Drag>,
    rows: Vec<Row>,
    plot: graph::Plot,
}

impl EqualizerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.global::<SettingsStore>().equalizer().clone();
        let mut view = Self {
            bands: settings.bands,
            source: settings.source.map(SharedString::from),
            profile: settings.profile.map(SharedString::from),
            saved: None,
            epoch: 0,
            dirty: false,
            label: SharedString::default(),
            status: None,
            preamp: SharedString::default(),
            selected: None,
            dragging: None,
            rows: Vec::new(),
            plot: graph::Plot::default(),
        };
        view.rebuild_rows(window, cx);
        view.refresh();
        view.fetch_saved(cx);
        view
    }

    pub fn relabel(&mut self) {
        self.refresh();
    }

    pub fn load(
        &mut self,
        bands: Vec<Band>,
        source: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bands = bands;
        self.source = source.map(SharedString::from);
        self.profile = None;
        self.saved = None;
        self.epoch += 1;
        self.selected = None;
        self.dragging = None;
        self.rebuild_rows(window, cx);
        self.commit(Some(true), cx);
    }

    fn fetch_saved(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(name)) = (profiles::folder(), self.profile.clone()) else {
            self.profile = None;
            return;
        };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn({
                    let name = name.clone();
                    async move { profiles::read(&folder, &name) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.profile.as_ref() != Some(&name) {
                    return;
                }
                match read {
                    Ok(bands) => this.saved = Some(bands),
                    Err(e) => {
                        log::warn!("equalizer: profile {name} unreadable: {e}");
                        this.profile = None;
                    }
                }
                this.refresh();
                cx.notify();
            });
        })
        .detach();
    }

    fn select_profile(&mut self, name: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = profiles::folder() else {
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_spawn({
                    let name = name.clone();
                    async move { profiles::read(&folder, &name) }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(_) if this.epoch != epoch => {}
                Ok(bands) => {
                    this.bands = bands.clone();
                    this.saved = Some(bands);
                    this.profile = Some(name);
                    this.source = None;
                    this.selected = None;
                    this.dragging = None;
                    this.rebuild_rows(window, cx);
                    this.commit(Some(true), cx);
                }
                Err(e) => {
                    let text = equalizer_strings().import_failed(&name, &e);
                    log::warn!("equalizer: {text}");
                    window.push_notification(Notification::error(text), cx);
                }
            });
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.profile.clone() {
            self.store(name, window, cx);
        }
    }

    fn store(&mut self, name: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = profiles::folder() else {
            return;
        };
        if self.bands.is_empty() {
            return;
        }
        self.epoch += 1;
        let epoch = self.epoch;
        let original = self.bands.clone();
        let bands = profiles::normalized(&original);
        cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn({
                    let (name, bands) = (name.clone(), bands.clone());
                    async move { profiles::write(&folder, &name, &bands) }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match written {
                Ok(()) if this.epoch != epoch => {}
                Ok(()) => {
                    if this.bands == original && original != bands {
                        this.bands = bands.clone();
                        for ix in 0..this.bands.len() {
                            this.sync_row(ix, window, cx);
                        }
                    }
                    this.saved = Some(bands);
                    this.profile = Some(name);
                    this.commit(None, cx);
                }
                Err(e) => {
                    let text = equalizer_strings().export_failed(&name, &e.to_string());
                    log::warn!("equalizer: {text}");
                    window.push_notification(Notification::error(text), cx);
                }
            });
        })
        .detach();
    }

    fn delete_profile(&mut self, name: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = profiles::folder() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let removed = cx
                .background_spawn({
                    let name = name.clone();
                    async move { profiles::delete(&folder, &name) }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match removed {
                Ok(()) => {
                    if this.profile.as_ref() == Some(&name) {
                        this.profile = None;
                        this.saved = None;
                        this.commit(None, cx);
                    }
                }
                Err(e) => {
                    let text = equalizer_strings().delete_failed(&name, &e.to_string());
                    log::warn!("equalizer: {text}");
                    window.push_notification(Notification::error(text), cx);
                }
            });
        })
        .detach();
    }

    fn suggested_name(&self) -> String {
        let base = match (&self.profile, &self.source) {
            (Some(profile), _) => profile.to_string(),
            (None, Some(source)) => {
                let model = source.split(" · ").next().unwrap_or(source.as_ref());
                model.strip_suffix(".txt").unwrap_or(model).to_string()
            }
            (None, None) => String::new(),
        };
        profiles::clean_name(&base).unwrap_or_default()
    }

    fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.commit(Some(enabled), cx);
    }

    fn refresh(&mut self) {
        let s = equalizer_strings();
        let preamp = equalizer::preamp_db(&self.bands, 48_000.) + 0.;
        self.preamp = s.preamp(&format_signed(preamp)).into();
        self.dirty = self.profile.is_some()
            && self
                .saved
                .as_ref()
                .is_some_and(|saved| *saved != self.bands);
        self.label = match (&self.profile, &self.source) {
            (Some(name), _) | (None, Some(name)) => name.clone(),
            (None, None) => s.no_profile.clone(),
        };
        self.status = if self.dirty {
            Some(s.edited.clone())
        } else if self.profile.is_none() && !self.bands.is_empty() {
            Some(s.not_saved.clone())
        } else {
            None
        };
        self.plot.set_curve(graph::curve(&self.bands));
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.refresh();
        let enabled = cx.global::<SettingsStore>().equalizer().enabled;
        let services = cx.global::<Services>();
        services.equalizer.set(enabled.then(|| self.bands.clone()));
        services
            .output
            .set_equalized(enabled && self.bands.iter().any(|band| !band.is_flat()));
        cx.notify();
    }

    fn commit(&mut self, enabled: Option<bool>, cx: &mut Context<Self>) {
        let mut settings = cx.global::<SettingsStore>().equalizer().clone();
        if let Some(enabled) = enabled {
            settings.enabled = enabled;
        }
        settings.bands.clone_from(&self.bands);
        settings.source = self.source.as_ref().map(ToString::to_string);
        settings.profile = self.profile.as_ref().map(ToString::to_string);
        if let Err(e) = cx.global_mut::<SettingsStore>().set_equalizer(settings) {
            notify_save_error(cx, e);
        }
        self.apply(cx);
    }

    fn rebuild_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = (0..self.bands.len())
            .map(|ix| {
                let band = self.bands[ix];
                let (freq, freq_sub) = input(ix, Field::Freq, freq_text(band.freq), window, cx);
                let (gain, gain_sub) = input(ix, Field::Gain, gain_text(band.gain), window, cx);
                let (q, q_sub) = input(ix, Field::Q, q_text(band.q), window, cx);
                Row {
                    label: SharedString::from((ix + 1).to_string()),
                    freq,
                    gain,
                    q,
                    _subscriptions: [freq_sub, gain_sub, q_sub],
                }
            })
            .collect();
        self.rows = rows;
    }

    fn sync_row(&self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(row), Some(band)) = (self.rows.get(ix), self.bands.get(ix)) else {
            return;
        };
        for (state, text) in [
            (&row.freq, freq_text(band.freq)),
            (&row.gain, gain_text(band.gain)),
            (&row.q, q_text(band.q)),
        ] {
            state.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    fn typed(&mut self, ix: usize, field: Field, text: &str, cx: &mut Context<Self>) {
        let Some(value) = parse_number(text) else {
            return;
        };
        let Some(band) = self.bands.get_mut(ix) else {
            return;
        };
        match field {
            Field::Freq => band.freq = value.clamp(MIN_FREQ, MAX_FREQ),
            Field::Gain => band.gain = value.clamp(-MAX_GAIN, MAX_GAIN),
            Field::Q => band.q = value.clamp(MIN_Q, MAX_Q),
        }
        self.selected = Some(ix);
        self.commit(None, cx);
    }

    fn add_band(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bands.len() >= MAX_BANDS {
            return;
        }
        self.bands.push(Band::new(Kind::Peak, 1_000., 0., 1.));
        self.selected = Some(self.bands.len() - 1);
        self.rebuild_rows(window, cx);
        self.commit(None, cx);
    }

    fn remove_band(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.bands.len() {
            return;
        }
        self.bands.remove(ix);
        self.selected = None;
        self.dragging = None;
        self.rebuild_rows(window, cx);
        self.commit(None, cx);
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bands.clear();
        self.source = None;
        self.selected = None;
        self.dragging = None;
        self.rebuild_rows(window, cx);
        self.commit(None, cx);
    }

    fn set_kind(&mut self, ix: usize, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(band) = self.bands.get_mut(ix) else {
            return;
        };
        if band.kind != kind && !band.kind.has_gain() && kind.has_gain() {
            band.q = band.q.max(DEFAULT_Q);
        }
        band.kind = kind;
        self.selected = Some(ix);
        self.sync_row(ix, window, cx);
        self.commit(None, cx);
    }

    fn toggle_band(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(band) = self.bands.get_mut(ix) {
            band.enabled = !band.enabled;
            self.commit(None, cx);
        }
    }

    fn start_drag(&mut self, ix: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(band) = self.bands.get(ix) else {
            return;
        };
        self.selected = Some(ix);
        self.dragging = Some(Drag {
            ix,
            anchor_y: event.position.y,
            anchor_q: band.q,
        });
        cx.notify();
    }

    fn drag_to(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.dragging.as_mut() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
            return;
        }
        let ix = drag.ix;
        let (Some((x, y)), Some(band)) =
            (self.plot.fraction(event.position), self.bands.get_mut(ix))
        else {
            return;
        };
        if event.modifiers.shift {
            let octaves = f32::from(drag.anchor_y - event.position.y) * Q_OCTAVES_PER_PIXEL;
            let q = drag.anchor_q * 2f64.powf(f64::from(octaves));
            band.q = round_to(q.clamp(MIN_Q, MAX_Q), 0.01);
        } else {
            band.freq = round_freq(graph::freq_at(x));
            if band.kind.has_gain() {
                band.gain = round_to(graph::db_at(y), 0.1);
            }
            drag.anchor_y = event.position.y;
            drag.anchor_q = band.q;
        }
        self.sync_row(ix, window, cx);
        self.apply(cx);
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging.take().is_some() {
            self.commit(None, cx);
        }
    }

    fn graph(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = cx.global::<SettingsStore>().equalizer().enabled;
        let colors = graph::Colors {
            grid: Colors::border(cx).opacity(0.5),
            zero: Colors::border(cx),
            curve: if enabled {
                Colors::primary(cx)
            } else {
                Colors::muted_foreground(cx)
            },
        };
        let muted = Colors::muted_foreground(cx);
        let node_bg = if enabled { Colors::primary(cx) } else { muted };
        let node_fg = Colors::primary_foreground(cx);
        let ring = Colors::foreground(cx);
        let measure = cx.entity();
        let painter = cx.entity();
        let mover = cx.entity();
        let releaser = cx.entity();
        let presser = cx.entity();
        let mut plot = div()
            .absolute()
            .left(px(graph::LEFT))
            .right(px(graph::RIGHT))
            .top(px(graph::TOP))
            .bottom(px(graph::BOTTOM))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        measure.update(cx, |this, _| this.plot.bounds = bounds);
                    },
                    move |bounds, (), window, cx| {
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                presser.update(cx, |this, cx| this.end_drag(cx));
                            }
                        });
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble {
                                mover.update(cx, |this, cx| this.drag_to(e, window, cx));
                            }
                        });
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                                releaser.update(cx, |this, cx| this.end_drag(cx));
                            }
                        });
                        painter.update(cx, |this, _| this.plot.paint(bounds, colors, window));
                    },
                )
                .size_full(),
            );
        for (freq, label) in graph::FREQ_TICKS {
            plot = plot.child(
                div()
                    .absolute()
                    .top(relative(1.))
                    .left(relative(graph::x_of(freq)))
                    .ml(px(-20.))
                    .mt(px(4.))
                    .w(px(40.))
                    .text_center()
                    .text_xs()
                    .text_color(muted)
                    .child(label),
            );
        }
        for (db, label) in graph::DB_TICKS {
            plot = plot.child(
                div()
                    .absolute()
                    .right(relative(1.))
                    .top(relative(graph::y_of(db)))
                    .mr(px(6.))
                    .mt(px(-8.))
                    .text_xs()
                    .text_color(muted)
                    .child(label),
            );
        }
        for (ix, band) in self.bands.iter().enumerate() {
            let Some(row) = self.rows.get(ix) else {
                break;
            };
            let gain = if band.kind.has_gain() { band.gain } else { 0. };
            let selected = self.selected == Some(ix);
            plot = plot.child(
                div()
                    .id(("eq-node", ix))
                    .absolute()
                    .left(relative(graph::x_of(band.freq)))
                    .top(relative(graph::y_of(gain)))
                    .ml(px(-graph::NODE / 2.))
                    .mt(px(-graph::NODE / 2.))
                    .size(px(graph::NODE))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(node_bg)
                    .text_color(node_fg)
                    .text_xs()
                    .cursor(CursorStyle::OpenHand)
                    .when(!band.enabled, |d| d.opacity(0.45))
                    .when(selected, |d| d.border_2().border_color(ring))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.start_drag(ix, e, cx);
                        }),
                    )
                    .child(row.label.clone()),
            );
        }
        div()
            .relative()
            .w_full()
            .h(px(graph::HEIGHT))
            .rounded_md()
            .border_1()
            .border_color(Colors::border(cx))
            .bg(Colors::background(cx))
            .child(plot)
    }

    fn band_table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = equalizer_strings();
        let muted = Colors::muted_foreground(cx);
        let header = h_flex()
            .gap_2()
            .px_1()
            .text_xs()
            .text_color(muted)
            .child(div().w(px(20.)))
            .child(div().w(px(20.)))
            .child(div().w(px(132.)).child(s.column_type.clone()))
            .child(div().w(px(96.)).child(s.column_freq.clone()))
            .child(div().w(px(80.)).child(s.column_gain.clone()))
            .child(div().w(px(72.)).child("Q"));
        let mut rows = Vec::with_capacity(self.bands.len());
        for (ix, (band, row)) in self.bands.iter().zip(&self.rows).enumerate() {
            rows.push(self.band_row(ix, band, row, cx));
        }
        v_flex()
            .gap_1()
            .when(!self.bands.is_empty(), |d| d.child(header))
            .children(rows)
            .when(self.bands.is_empty(), |d| {
                d.child(div().text_sm().text_color(muted).child(s.no_bands.clone()))
            })
    }

    fn band_row(&self, ix: usize, band: &Band, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let s = equalizer_strings();
        let entity = cx.entity();
        let current = band.kind;
        h_flex()
            .id(("eq-band", ix))
            .gap_2()
            .px_1()
            .py_0p5()
            .rounded_md()
            .items_center()
            .when(self.selected == Some(ix), |d| d.bg(Colors::list_active(cx)))
            .child(
                div()
                    .w(px(20.))
                    .text_xs()
                    .text_color(Colors::muted_foreground(cx))
                    .child(row.label.clone()),
            )
            .child(
                div().w(px(20.)).child(
                    Checkbox::new(("eq-band-on", ix))
                        .checked(band.enabled)
                        .on_click(
                            cx.listener(move |this, _: &bool, _, cx| this.toggle_band(ix, cx)),
                        ),
                ),
            )
            .child(
                div().w(px(132.)).child(
                    Button::new(("eq-band-kind", ix))
                        .small()
                        .w_full()
                        .label(kind_label(current))
                        .dropdown_caret(true)
                        .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                            Kind::ALL.iter().fold(menu, |menu, &kind| {
                                let entity = entity.clone();
                                menu.item(
                                    PopupMenuItem::new(kind_label(kind))
                                        .checked(kind == current)
                                        .on_click(move |_, window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.set_kind(ix, kind, window, cx)
                                            })
                                        }),
                                )
                            })
                        }),
                ),
            )
            .child(div().w(px(96.)).child(Input::new(&row.freq).small()))
            .child(
                div().w(px(80.)).child(
                    Input::new(&row.gain)
                        .small()
                        .disabled(!band.kind.has_gain()),
                ),
            )
            .child(div().w(px(72.)).child(Input::new(&row.q).small()))
            .child(
                Button::new(("eq-band-remove", ix))
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/s1-x.svg"))
                    .tooltip(s.remove_band.clone())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.remove_band(ix, window, cx)),
                    ),
            )
            .into_any_element()
    }
}

impl Render for EqualizerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = equalizer_strings();
        let muted = Colors::muted_foreground(cx);
        let empty = self.bands.is_empty();
        v_flex()
            .gap_3()
            .child(self.graph(cx))
            .when(!empty, |d| {
                d.child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .child(div().id("eq-preamp").child(self.preamp.clone()).tooltip(
                            |window, cx| {
                                Tooltip::new(equalizer_strings().preamp_tip.clone())
                                    .build(window, cx)
                            },
                        ))
                        .child("·")
                        .child(s.hint.clone()),
                )
            })
            .child(self.band_table(cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("eq-add-band")
                            .small()
                            .icon(Icon::default().path("icons/plus.svg"))
                            .label(s.add_band.clone())
                            .disabled(self.bands.len() >= MAX_BANDS)
                            .on_click(cx.listener(|this, _, window, cx| this.add_band(window, cx))),
                    )
                    .child(
                        Button::new("eq-clear")
                            .small()
                            .label(s.clear.clone())
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    ),
            )
    }
}

fn input(
    ix: usize,
    field: Field,
    text: String,
    window: &mut Window,
    cx: &mut Context<EqualizerView>,
) -> (Entity<InputState>, Subscription) {
    let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
    let subscription =
        cx.subscribe_in(
            &state,
            window,
            move |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value();
                    this.typed(ix, field, &text, cx);
                }
                InputEvent::Blur => this.sync_row(ix, window, cx),
                InputEvent::Focus if this.selected != Some(ix) => {
                    this.selected = Some(ix);
                    cx.notify();
                }
                _ => {}
            },
        );
    (state, subscription)
}

fn kind_label(kind: Kind) -> SharedString {
    let s = equalizer_strings();
    match kind {
        Kind::Peak => s.peak.clone(),
        Kind::LowShelf => s.low_shelf.clone(),
        Kind::HighShelf => s.high_shelf.clone(),
        Kind::LowPass => s.low_pass.clone(),
        Kind::HighPass => s.high_pass.clone(),
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .replace('−', "-")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn round_to(value: f64, step: f64) -> f64 {
    (value / step).round() * step + 0.
}

fn round_freq(freq: f64) -> f64 {
    if freq >= 100. {
        freq.round()
    } else {
        round_to(freq, 0.1)
    }
}

fn freq_text(freq: f64) -> String {
    if freq >= 100. {
        format!("{freq:.0}")
    } else {
        let text = format!("{freq:.1}");
        text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
    }
}

fn gain_text(gain: f64) -> String {
    format!("{:.1}", round_to(gain, 0.1))
}

fn q_text(q: f64) -> String {
    format!("{q:.2}")
}

fn format_signed(db: f64) -> String {
    let rounded = round_to(db, 0.1);
    if rounded < 0. {
        format!("−{:.1}", -rounded)
    } else {
        format!("{rounded:.1}")
    }
}

fn enable_field(editor: &Entity<EqualizerView>, cx: &App) -> impl IntoElement + use<> {
    let enabled = cx.global::<SettingsStore>().equalizer().enabled;
    let editor = editor.clone();
    h_flex().items_center().justify_end().child(
        Switch::new("eq-enabled")
            .checked(enabled)
            .on_click(move |on, _, cx| {
                editor.update(cx, |this, cx| this.set_enabled(*on, cx));
            }),
    )
}

fn toolbar(
    editor: &Entity<EqualizerView>,
    picker: &Entity<HeadphonePicker>,
    cx: &App,
) -> impl IntoElement + use<> {
    let s = equalizer_strings();
    let view = editor.read(cx);
    let can_save = !view.bands.is_empty() && (view.profile.is_none() || view.dirty);
    let menu_editor = editor.clone();
    let save_editor = editor.clone();
    let picker = picker.clone();
    h_flex()
        .flex_wrap()
        .gap_2()
        .items_center()
        .child(
            Button::new("eq-profile")
                .small()
                .label(view.label.clone())
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, cx| {
                    profile_menu(menu, &menu_editor, cx)
                }),
        )
        .when_some(view.status.clone(), |d, status| {
            d.child(
                div()
                    .text_xs()
                    .text_color(Colors::muted_foreground(cx))
                    .child(status),
            )
        })
        .child(
            Button::new("eq-profile-save")
                .small()
                .label(s.save.clone())
                .disabled(!can_save)
                .on_click(move |_, window, cx| save(save_editor.clone(), window, cx)),
        )
        .child(div().flex_1())
        .child(
            Button::new("eq-headphones")
                .small()
                .icon(Icon::default().path("icons/headphones.svg"))
                .label(s.pick_headphones.clone())
                .on_click(move |_, window, cx| picker::open(picker.clone(), window, cx)),
        )
}

fn save(editor: Entity<EqualizerView>, window: &mut Window, cx: &mut App) {
    if editor.read(cx).profile.is_some() {
        editor.update(cx, |this, cx| this.save(window, cx));
    } else {
        save_as(editor, window, cx);
    }
}

fn profile_menu(menu: PopupMenu, editor: &Entity<EqualizerView>, cx: &App) -> PopupMenu {
    let s = equalizer_strings();
    let view = editor.read(cx);
    let current = view.profile.clone();
    let empty = view.bands.is_empty();
    let folder = profiles::folder();
    let names = folder.as_deref().map(profiles::list).unwrap_or_default();
    let mut menu = if names.is_empty() {
        menu.label(s.no_profiles.clone())
    } else {
        names.into_iter().fold(menu, |menu, name| {
            let name = SharedString::from(name);
            let editor = editor.clone();
            menu.item(
                PopupMenuItem::new(name.clone())
                    .checked(current.as_ref() == Some(&name))
                    .on_click(move |_, window, cx| {
                        let name = name.clone();
                        editor.update(cx, |this, cx| this.select_profile(name, window, cx));
                    }),
            )
        })
    };
    let (save_as_editor, import_editor, export_editor) =
        (editor.clone(), editor.clone(), editor.clone());
    menu = menu
        .separator()
        .item(
            PopupMenuItem::new(s.save_as.clone())
                .disabled(empty)
                .on_click(move |_, window, cx| save_as(save_as_editor.clone(), window, cx)),
        )
        .item(
            PopupMenuItem::new(s.import.clone())
                .on_click(move |_, window, cx| import(import_editor.clone(), window, cx)),
        )
        .item(
            PopupMenuItem::new(s.export.clone())
                .disabled(empty)
                .on_click(move |_, window, cx| export(&export_editor, window, cx)),
        );
    if let Some(folder) = folder {
        menu = menu
            .separator()
            .item(
                PopupMenuItem::new(tr().reveal_folder.clone()).on_click(move |_, _, _| {
                    if let Err(e) = std::fs::create_dir_all(&folder) {
                        log::warn!("equalizer: {}: {e}", folder.display());
                    }
                    crate::settings_view::reveal_in_file_manager(&folder);
                }),
            );
    }
    if let Some(name) = current {
        let editor = editor.clone();
        menu = menu.item(PopupMenuItem::new(s.delete_profile(&name)).on_click(
            move |_, window, cx| {
                confirm_delete(editor.clone(), name.clone(), window, cx);
            },
        ));
    }
    menu
}

fn save_as(editor: Entity<EqualizerView>, window: &mut Window, cx: &mut App) {
    let s = equalizer_strings();
    let suggested = editor.read(cx).suggested_name();
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(s.name_placeholder.clone())
            .default_value(suggested)
    });
    let focus = input.clone();
    let existing = Rc::new(
        profiles::folder()
            .map(|folder| profiles::list(&folder))
            .unwrap_or_default(),
    );
    let warned: Rc<RefCell<Option<String>>> = Rc::default();
    window.open_dialog(cx, move |dialog, _, cx| {
        let name = profiles::clean_name(&input.read(cx).value());
        let warn = name.is_some() && *warned.borrow() == name;
        let (editor, input, existing, warned) = (
            editor.clone(),
            input.clone(),
            existing.clone(),
            warned.clone(),
        );
        dialog
            .overlay_closable(false)
            .close_button(false)
            .w(px(420.))
            .title(s.save_title.clone())
            .child(v_flex().gap_2().child(Input::new(&input)).when(warn, |d| {
                d.child(
                    div()
                        .text_sm()
                        .text_color(Colors::muted_foreground(cx))
                        .child(s.name_exists.clone()),
                )
            }))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label(tr().cancel.clone())
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    )
                    .child(Button::new("ok").label(s.save.clone()).primary().on_click(
                        |_, window, cx| {
                            window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                        },
                    )),
            )
            .on_ok(move |_, window, cx| {
                let Some(name) = profiles::clean_name(&input.read(cx).value()) else {
                    return false;
                };
                let current = editor.read(cx).profile.clone();
                let replacing = profiles::contains(&existing, &name)
                    && !current.is_some_and(|current| profiles::same_name(&current, &name));
                if replacing && warned.borrow().as_deref() != Some(name.as_str()) {
                    *warned.borrow_mut() = Some(name);
                    window.refresh();
                    return false;
                }
                editor.update(cx, |this, cx| this.store(name.into(), window, cx));
                true
            })
    });
    focus.update(cx, |state, cx| state.focus(window, cx));
}

fn confirm_delete(
    editor: Entity<EqualizerView>,
    name: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let title = SharedString::from(equalizer_strings().delete_title(&name));
    window.open_dialog(cx, move |dialog, _, _| {
        let (editor, name) = (editor.clone(), name.clone());
        dialog
            .overlay_closable(false)
            .close_button(false)
            .title(title.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label(tr().cancel.clone())
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    )
                    .child(
                        Button::new("ok")
                            .label(tr().delete.clone())
                            .primary()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| {
                let name = name.clone();
                editor.update(cx, |this, cx| this.delete_profile(name, window, cx));
                true
            })
    });
}

fn import(editor: Entity<EqualizerView>, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Some(picked) = rfd::AsyncFileDialog::new()
            .add_filter("ParametricEQ", &["txt"])
            .pick_file()
            .await
        else {
            return;
        };
        let name = picked.file_name();
        let bytes = picked.read().await;
        let parsed = equalizer::parse(&String::from_utf8_lossy(&bytes));
        let _ = handle.update(cx, |_, window, cx| match parsed {
            Ok(bands) => {
                editor.update(cx, |this, cx| this.load(bands, Some(name), window, cx));
            }
            Err(e) => {
                let text = equalizer_strings().import_failed(&name, &e.to_string());
                log::warn!("equalizer: {text}");
                window.push_notification(Notification::error(text), cx);
            }
        });
    })
    .detach();
}

fn export(editor: &Entity<EqualizerView>, window: &mut Window, cx: &mut App) {
    let text = equalizer::write(&editor.read(cx).bands);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Some(picked) = rfd::AsyncFileDialog::new()
            .add_filter("ParametricEQ", &["txt"])
            .set_file_name(EXPORT_NAME)
            .save_file()
            .await
        else {
            return;
        };
        if let Err(e) = picked.write(text.as_bytes()).await {
            let text = equalizer_strings().export_failed(&picked.file_name(), &e.to_string());
            log::warn!("equalizer: {text}");
            let _ = handle.update(cx, |_, window, cx| {
                window.push_notification(Notification::error(text), cx);
            });
        }
    })
    .detach();
}

pub fn page(editor: Entity<EqualizerView>, picker: Entity<HeadphonePicker>) -> SettingPage {
    let s = equalizer_strings();
    let switch_editor = editor.clone();
    let toolbar_editor = editor.clone();
    SettingPage::new(s.title.clone()).group(
        SettingGroup::new()
            .title(s.title.clone())
            .item(
                SettingItem::new(
                    s.enable.clone(),
                    SettingField::render(move |_, cx: &mut App| enable_field(&switch_editor, cx)),
                )
                .description(s.enable_desc.clone()),
            )
            .item(SettingItem::unlabeled(SettingField::render(
                move |_, cx: &mut App| toolbar(&toolbar_editor, &picker, cx),
            )))
            .item(SettingItem::unlabeled(SettingField::render(move |_, _| {
                editor.clone()
            }))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_accept_commas_and_typographic_minus() {
        assert_eq!(parse_number(" 2,5 "), Some(2.5));
        assert_eq!(parse_number("−3.1"), Some(-3.1));
        assert_eq!(parse_number("-"), None);
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn values_print_without_negative_zero() {
        assert_eq!(gain_text(-0.04), "0.0");
        assert_eq!(format_signed(-0.04), "0.0");
        assert_eq!(format_signed(-6.26), "−6.3");
        assert_eq!(freq_text(31.5), "31.5");
        assert_eq!(freq_text(40.0), "40");
        assert_eq!(freq_text(8445.4), "8445");
    }

    #[test]
    fn graph_axes_round_trip() {
        for freq in [20., 105., 1_000., 12_345., 20_000.] {
            assert!((graph::freq_at(graph::x_of(freq)) - freq).abs() / freq < 1e-4);
        }
        for db in [-15., -6., 0., 3.5, 15.] {
            assert!((graph::db_at(graph::y_of(db)) - db).abs() < 1e-4);
        }
    }
}
