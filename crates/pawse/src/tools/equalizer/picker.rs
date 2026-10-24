use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use autoeq::{Client, Entry};
use equalizer::Band;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollStrategy, SharedString, StatefulInteractiveElement, Styled,
    Subscription, UniformListScrollHandle, Window, canvas, div, px, relative, uniform_list,
};
use gpui_component::{
    Disableable, Icon, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    h_flex,
    input::{Input, InputEvent, InputState, MoveDown, MoveUp},
    spinner::Spinner,
    v_flex,
};
use nucleo_matcher::{Config, Matcher};
use ui_resources::i18n::equalizer_strings;

use crate::fuzzy::fuzzy_sorted;
use crate::localization::tr;
use crate::theme_colors::Colors;

use super::{EqualizerView, graph};

const DIALOG_WIDTH: f32 = 600.;
const ROW_HEIGHT: f32 = 32.;
const LIST_INSET: f32 = 4.;
const LIST_ROWS: usize = 8;
const MARK: f32 = 14.;
const DETAILS_HEIGHT: f32 = 28.;
const PREVIEW_HEIGHT: f32 = 140.;
const PREVIEW_INSET: f32 = 8.;
const PREVIEW_LABELS: f32 = 20.;
const PREVIEW_TICKS: [(f64, &str); 3] = [(100., "100"), (1_000., "1k"), (10_000., "10k")];
const PREVIEW_DELAY: Duration = Duration::from_millis(150);
const SOURCE_ORDER: [&str; 2] = ["oratory1990", "crinacle"];
const CACHE_FOLDER: &str = "autoeq";

struct Measurement {
    entry: Entry,
    label: SharedString,
    source: SharedString,
}

struct Model {
    name: SharedString,
    summary: SharedString,
    measurements: Vec<Measurement>,
}

enum Index {
    Idle,
    Loading,
    Ready(Arc<[Model]>),
    Failed(SharedString),
}

enum Preview {
    Ready(Arc<[Band]>),
    Failed(SharedString),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Pick {
    model: usize,
    measurement: usize,
}

pub struct HeadphonePicker {
    editor: Entity<EqualizerView>,
    client: Client,
    search: Entity<InputState>,
    index: Index,
    matches: Vec<usize>,
    query: u64,
    searching: bool,
    selected: Option<Pick>,
    loaded: Option<Pick>,
    previews: HashMap<Pick, Preview>,
    fetching: HashSet<Pick>,
    preview_request: u64,
    confirming: bool,
    plot: graph::Plot,
    scroll: UniformListScrollHandle,
    _search_subscription: Subscription,
}

pub fn open(picker: Entity<HeadphonePicker>, window: &mut Window, cx: &mut App) {
    picker.update(cx, |this, cx| this.prepare(window, cx));
    let search = picker.read(cx).search.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let s = equalizer_strings();
        let this = picker.read(cx);
        let failed = this
            .selected
            .is_some_and(|pick| matches!(this.previews.get(&pick), Some(Preview::Failed(_))));
        let ready = this.selected.is_some() && !failed;
        let busy = this.confirming;
        let (confirm, closed) = (picker.clone(), picker.clone());
        dialog
            .w(px(DIALOG_WIDTH))
            .close_button(false)
            .title(s.headphones.clone())
            .child(picker.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(Colors::muted_foreground(cx))
                            .child(s.credit.clone()),
                    )
                    .child(
                        Button::new("cancel")
                            .label(tr().cancel.clone())
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    )
                    .child(
                        Button::new("ok")
                            .label(s.load.clone())
                            .primary()
                            .disabled(!ready)
                            .loading(busy)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| confirm.update(cx, |this, cx| this.confirm(window, cx)))
            .on_close(move |_, _, cx| closed.update(cx, |this, _| this.confirming = false))
    });
    search.update(cx, |state, cx| state.focus(window, cx));
}

impl HeadphonePicker {
    pub fn new(editor: Entity<EqualizerView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(equalizer_strings().search_placeholder.clone())
        });
        let search_subscription = cx.subscribe_in(&search, window, |this, _, event, window, cx| {
            if let InputEvent::Change = event {
                this.refilter(true, window, cx);
            }
        });
        Self {
            editor,
            client: Client::new(),
            search,
            index: Index::Idle,
            matches: Vec::new(),
            query: 0,
            searching: false,
            selected: None,
            loaded: None,
            previews: HashMap::new(),
            fetching: HashSet::new(),
            preview_request: 0,
            confirming: false,
            plot: graph::Plot::default(),
            scroll: UniformListScrollHandle::new(),
            _search_subscription: search_subscription,
        }
    }

    pub fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = equalizer_strings().search_placeholder.clone();
        self.search.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
        if let Index::Failed(_) = self.index {
            self.index = Index::Idle;
        }
        self.previews
            .retain(|_, preview| matches!(preview, Preview::Ready(_)));
    }

    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirming = false;
        self.search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.fetch_index(window, cx);
        self.sync_with_editor(window, cx);
        self.show_preview(window, cx);
        self.refilter(false, window, cx);
    }

    fn fetch_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.index, Index::Loading | Index::Ready(_)) {
            return;
        }
        self.index = Index::Loading;
        cx.notify();
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = net_runtime::unblock(move || client.index().map(group)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.index = match result {
                    Ok(models) => Index::Ready(models),
                    Err(e) => {
                        log::warn!("equalizer: AutoEq list failed: {e}");
                        Index::Failed(equalizer_strings().index_failed(&e.to_string()).into())
                    }
                };
                this.sync_with_editor(window, cx);
                this.refilter(false, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn sync_with_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Index::Ready(models) = &self.index else {
            return;
        };
        let source = self.editor.read(cx).source.clone();
        self.loaded = source.and_then(|source| find(models, &source));
        if let Some(pick) = self.loaded {
            self.select(pick, window, cx);
        }
    }

    fn refilter(&mut self, typed: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.query += 1;
        let query = self.search.read(cx).value().trim().to_string();
        let Index::Ready(models) = &self.index else {
            self.matches.clear();
            self.searching = false;
            cx.notify();
            return;
        };
        if query.is_empty() {
            self.matches = (0..models.len()).collect();
            self.searching = false;
            self.reveal(ScrollStrategy::Center);
            cx.notify();
            return;
        }
        let models = models.clone();
        let generation = self.query;
        self.searching = true;
        cx.spawn_in(window, async move |this, cx| {
            let matches = cx
                .background_spawn(async move {
                    let mut matcher = Matcher::new(Config::DEFAULT);
                    let names = models
                        .iter()
                        .enumerate()
                        .map(|(ix, model)| (ix, model.name.as_ref()));
                    fuzzy_sorted(&mut matcher, &query, names)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.query != generation {
                    return;
                }
                this.matches = matches;
                this.searching = false;
                match this.matches.first() {
                    Some(&model) if typed => {
                        this.select(this.pick_for(model), window, cx);
                        this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    _ => this.reveal(ScrollStrategy::Center),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reveal(&self, strategy: ScrollStrategy) {
        let row = self
            .selected
            .and_then(|pick| self.matches.iter().position(|&model| model == pick.model));
        if let Some(row) = row {
            self.scroll.scroll_to_item(row, strategy);
        }
    }

    fn pick_for(&self, model: usize) -> Pick {
        [self.selected, self.loaded]
            .into_iter()
            .flatten()
            .find(|pick| pick.model == model)
            .unwrap_or(Pick {
                model,
                measurement: 0,
            })
    }

    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(last) = self.matches.len().checked_sub(1) else {
            return;
        };
        let row = self
            .selected
            .and_then(|pick| self.matches.iter().position(|&model| model == pick.model))
            .map_or(0, |row| row.saturating_add_signed(delta).min(last));
        self.select(self.pick_for(self.matches[row]), window, cx);
        self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
    }

    fn select(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected == Some(pick) {
            return;
        }
        self.selected = Some(pick);
        self.confirming = false;
        self.show_preview(window, cx);
        cx.notify();
    }

    fn measurement(&self, pick: Pick) -> Option<&Measurement> {
        let Index::Ready(models) = &self.index else {
            return None;
        };
        models.get(pick.model)?.measurements.get(pick.measurement)
    }

    fn show_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pick) = self.selected else {
            self.plot.set_curve(Vec::new());
            return;
        };
        match self.previews.get(&pick) {
            Some(Preview::Ready(bands)) => self.plot.set_curve(graph::curve(bands)),
            Some(Preview::Failed(_)) => self.plot.set_curve(Vec::new()),
            None => {
                self.plot.set_curve(Vec::new());
                self.fetch_preview(pick, window, cx);
            }
        }
    }

    fn fetch_preview(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        let Some(measurement) = self.measurement(pick) else {
            return;
        };
        let entry = measurement.entry.clone();
        let name = measurement.source.to_string();
        let client = self.client.clone();
        self.preview_request += 1;
        let request = self.preview_request;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let current = this
                .update(cx, |this, _| {
                    this.preview_request == request
                        && !this.previews.contains_key(&pick)
                        && this.fetching.insert(pick)
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let result = net_runtime::unblock(move || {
                load_profile(&client, &entry, cache_file(&entry).as_deref())
            })
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.fetching.remove(&pick);
                let preview = match result {
                    Ok(bands) => Preview::Ready(bands.into()),
                    Err(e) => {
                        log::warn!("equalizer: AutoEq profile {name} failed: {e}");
                        Preview::Failed(equalizer_strings().profile_failed(&name, &e).into())
                    }
                };
                this.previews.insert(pick, preview);
                if this.selected == Some(pick) {
                    this.show_preview(window, cx);
                    if this.confirming && this.confirm(window, cx) {
                        window.close_dialog(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pick) = self.selected {
            self.previews.remove(&pick);
            self.show_preview(window, cx);
            cx.notify();
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(pick) = self.selected else {
            return false;
        };
        let Some(source) = self.measurement(pick).map(|m| m.source.to_string()) else {
            return false;
        };
        match self.previews.get(&pick) {
            Some(Preview::Ready(bands)) => {
                let bands = bands.to_vec();
                self.confirming = false;
                self.loaded = Some(pick);
                self.editor.update(cx, |editor, cx| {
                    editor.load(bands, Some(source), window, cx)
                });
                true
            }
            Some(Preview::Failed(_)) => {
                self.confirming = false;
                false
            }
            None => {
                self.confirming = true;
                cx.notify();
                false
            }
        }
    }

    fn row(&self, model_ix: usize, row_ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Index::Ready(models) = &self.index else {
            return div().into_any_element();
        };
        let Some(model) = models.get(model_ix) else {
            return div().into_any_element();
        };
        let muted = Colors::muted_foreground(cx);
        let loaded = self.loaded.is_some_and(|pick| pick.model == model_ix);
        h_flex()
            .id(("eq-headphone", row_ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .when(
                self.selected.is_some_and(|pick| pick.model == model_ix),
                |d| d.bg(Colors::list_active(cx)),
            )
            .hover(|d| d.bg(Colors::list_hover(cx)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.select(this.pick_for(model_ix), window, cx);
                if event.click_count() > 1 && this.confirm(window, cx) {
                    window.close_dialog(cx);
                }
            }))
            .child(div().flex_shrink_0().size(px(MARK)).when(loaded, |d| {
                d.child(
                    Icon::default()
                        .path("icons/check.svg")
                        .size(px(MARK))
                        .text_color(Colors::primary(cx)),
                )
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_sm()
                    .child(model.name.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(muted)
                    .child(model.summary.clone()),
            )
            .into_any_element()
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = equalizer_strings();
        let muted = Colors::muted_foreground(cx);
        let count = self.matches.len();
        let message = |text: SharedString| {
            v_flex()
                .size_full()
                .px_4()
                .gap_2()
                .items_center()
                .justify_center()
                .text_sm()
                .text_center()
                .text_color(muted)
                .child(text)
        };
        let body = match &self.index {
            Index::Ready(_) if count > 0 => {
                let entity = cx.entity();
                uniform_list("eq-headphones", count, move |range, _, cx| {
                    entity.update(cx, |this, cx| {
                        range
                            .filter_map(|row| {
                                let model = *this.matches.get(row)?;
                                Some(this.row(model, row, cx))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .track_scroll(&self.scroll)
                .size_full()
                .into_any_element()
            }
            Index::Ready(_) if self.searching => div().into_any_element(),
            Index::Ready(_) => message(s.no_matches.clone()).into_any_element(),
            Index::Idle | Index::Loading => message(s.index_loading.clone()).into_any_element(),
            Index::Failed(error) => message(error.clone())
                .child(
                    Button::new("eq-index-retry")
                        .small()
                        .label(s.retry.clone())
                        .on_click(cx.listener(|this, _, window, cx| this.fetch_index(window, cx))),
                )
                .into_any_element(),
        };
        let height = ROW_HEIGHT * LIST_ROWS as f32 + 2. * LIST_INSET + 2.;
        div()
            .h(px(height))
            .flex_shrink_0()
            .rounded_md()
            .border_1()
            .border_color(Colors::border(cx))
            .bg(Colors::background(cx))
            .p(px(LIST_INSET))
            .on_scroll_wheel(crate::tools::contain_scroll(
                &self.scroll.0.borrow().base_handle,
            ))
            .child(body)
    }

    fn details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = equalizer_strings();
        let muted = Colors::muted_foreground(cx);
        let row = h_flex()
            .min_h(px(DETAILS_HEIGHT))
            .flex_wrap()
            .gap_1()
            .items_center();
        let models = match &self.index {
            Index::Ready(models) => Some(models),
            _ => None,
        };
        let Some((pick, model)) = self
            .selected
            .and_then(|pick| Some((pick, models?.get(pick.model)?)))
        else {
            return row.child(div().text_sm().text_color(muted).child(s.pick_hint.clone()));
        };
        let chips = model
            .measurements
            .iter()
            .enumerate()
            .map(|(ix, measurement)| {
                let chip = Pick {
                    model: pick.model,
                    measurement: ix,
                };
                Button::new(("eq-measurement", ix))
                    .small()
                    .label(measurement.label.clone())
                    .selected(chip == pick)
                    .on_click(cx.listener(move |this, _, window, cx| this.select(chip, window, cx)))
            })
            .collect::<Vec<_>>();
        row.child(div().mr_2().text_sm().child(model.name.clone()))
            .child(
                div()
                    .mr_1()
                    .text_xs()
                    .text_color(muted)
                    .child(s.measured_by.clone()),
            )
            .children(chips)
    }

    fn preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = equalizer_strings();
        let muted = Colors::muted_foreground(cx);
        let border = Colors::border(cx);
        let colors = graph::Colors {
            grid: border.opacity(0.5),
            zero: border,
            curve: Colors::primary(cx),
        };
        let painter = cx.entity();
        let mut plot = div()
            .absolute()
            .left(px(PREVIEW_INSET))
            .right(px(PREVIEW_INSET))
            .top(px(PREVIEW_INSET))
            .bottom(px(PREVIEW_LABELS))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, cx| {
                        painter.update(cx, |this, _| this.plot.paint(bounds, colors, window));
                    },
                )
                .size_full(),
            );
        for (freq, label) in PREVIEW_TICKS {
            plot = plot.child(
                div()
                    .absolute()
                    .top(relative(1.))
                    .left(relative(graph::x_of(freq)))
                    .ml(px(-20.))
                    .mt(px(2.))
                    .w(px(40.))
                    .text_center()
                    .text_xs()
                    .text_color(muted)
                    .child(label),
            );
        }
        let overlay = match self.selected.map(|pick| self.previews.get(&pick)) {
            Some(None) => Some(Spinner::new().color(muted).into_any_element()),
            Some(Some(Preview::Failed(error))) => Some(
                v_flex()
                    .px_4()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .text_center()
                    .text_color(muted)
                    .child(error.clone())
                    .child(
                        Button::new("eq-preview-retry")
                            .small()
                            .label(s.retry.clone())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.retry_preview(window, cx)),
                            ),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };
        div()
            .relative()
            .w_full()
            .h(px(PREVIEW_HEIGHT))
            .flex_shrink_0()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(Colors::background(cx))
            .child(plot)
            .when_some(overlay, |d, overlay| {
                d.child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(overlay),
                )
            })
    }
}

impl Render for HeadphonePicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                this.step(-1, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                this.step(1, window, cx);
                cx.stop_propagation();
            }))
            .child(Input::new(&self.search))
            .child(self.list(cx))
            .child(self.details(cx))
            .child(self.preview(cx))
    }
}

fn cache_file(entry: &Entry) -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("pawse")
            .join(CACHE_FOLDER)
            .join(entry.cache_name()?),
    )
}

fn load_profile(
    client: &Client,
    entry: &Entry,
    cached: Option<&Path>,
) -> Result<Vec<Band>, String> {
    let hit = cached
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| equalizer::parse(&text).ok());
    if let Some(bands) = hit {
        return Ok(bands);
    }
    let text = client.parametric_eq(entry).map_err(|e| e.to_string())?;
    let bands = equalizer::parse(&text).map_err(|e| e.to_string())?;
    if let Some(file) = cached
        && let Err(e) = store(file, &text)
    {
        log::warn!("equalizer: AutoEq cache {}: {e}", file.display());
    }
    Ok(bands)
}

fn store(file: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)
}

fn find(models: &[Model], source: &str) -> Option<Pick> {
    models.iter().enumerate().find_map(|(model, entry)| {
        let measurement = entry
            .measurements
            .iter()
            .position(|measurement| measurement.source.as_ref() == source)?;
        Some(Pick { model, measurement })
    })
}

fn rank(source: &str) -> usize {
    SOURCE_ORDER
        .iter()
        .position(|preferred| *preferred == source)
        .unwrap_or(SOURCE_ORDER.len())
}

fn group(entries: Vec<Entry>) -> Arc<[Model]> {
    let mut entries: Vec<(usize, Entry)> = entries.into_iter().enumerate().collect();
    entries.sort_by_cached_key(|(order, entry)| {
        (
            entry.name.to_lowercase(),
            entry.name.clone(),
            rank(&entry.source),
            *order,
        )
    });
    let mut models: Vec<Model> = Vec::new();
    for (_, entry) in entries {
        let label = match &entry.rig {
            Some(rig) => format!("{} · {rig}", entry.source),
            None => entry.source.clone(),
        };
        let measurement = Measurement {
            source: format!("{} · {label}", entry.name).into(),
            entry,
            label: label.into(),
        };
        match models.last_mut() {
            Some(model) if model.name.as_ref() == measurement.entry.name => {
                model.measurements.push(measurement);
            }
            _ => models.push(Model {
                name: measurement.entry.name.clone().into(),
                summary: SharedString::default(),
                measurements: vec![measurement],
            }),
        }
    }
    for model in &mut models {
        let first = &model.measurements[0].entry.source;
        model.summary = match model.measurements.len() {
            1 => first.clone().into(),
            n => format!("{first} +{}", n - 1).into(),
        };
    }
    models.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_model_per_name_with_preferred_sources_first() {
        let index = "- [HD 600](./Rtings/x/HD%20600) by Rtings on B&K\n\
                     - [HD 600](./crinacle/y/HD%20600) by crinacle on GRAS\n\
                     - [HD 600](./oratory1990/z/HD%20600) by oratory1990\n\
                     - [Aria](./crinacle/y/Aria) by crinacle\n";
        let models = group(autoeq::parse_index(index));
        let names: Vec<&str> = models.iter().map(|m| m.name.as_ref()).collect();
        assert_eq!(names, ["Aria", "HD 600"]);
        let labels: Vec<&str> = models[1]
            .measurements
            .iter()
            .map(|m| m.label.as_ref())
            .collect();
        assert_eq!(labels, ["oratory1990", "crinacle · GRAS", "Rtings · B&K"]);
        assert_eq!(models[1].summary.as_ref(), "oratory1990 +2");
        assert_eq!(models[0].summary.as_ref(), "crinacle");
        assert_eq!(
            find(&models, "HD 600 · crinacle · GRAS"),
            Some(Pick {
                model: 1,
                measurement: 1
            })
        );
        assert_eq!(find(&models, "HD 600"), None);
    }

    #[test]
    fn a_cached_profile_is_read_without_the_network() {
        let folder =
            std::env::temp_dir().join(format!("pawse-autoeq-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let entry = autoeq::parse_index("- [HD 600](./a/b/HD%20600) by a").remove(0);
        let file = folder.join(entry.cache_name().unwrap());
        store(&file, "Filter 1: ON PK Fc 1000 Hz Gain 3.0 dB Q 1.00\n").unwrap();
        let bands = load_profile(&Client::new(), &entry, Some(&file)).unwrap();
        assert_eq!(bands.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn fuzzy_search_forgives_a_missing_space_and_a_missing_letter() {
        let index = "- [Sennheiser HD 600](./a/b/HD) by oratory1990\n\
                     - [Sennheiser HD 650](./a/b/HD) by oratory1990\n\
                     - [Sony WH-1000XM5](./a/b/WH) by oratory1990\n";
        let models = group(autoeq::parse_index(index));
        let mut matcher = Matcher::new(Config::DEFAULT);
        let names = || {
            models
                .iter()
                .enumerate()
                .map(|(ix, m)| (ix, m.name.as_ref()))
        };
        assert_eq!(
            fuzzy_sorted(&mut matcher, "hd600", names()).first(),
            Some(&0)
        );
        assert_eq!(
            fuzzy_sorted(&mut matcher, "senheiser 650", names()).first(),
            Some(&1)
        );
    }
}
