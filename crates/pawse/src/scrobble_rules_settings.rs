use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext, Axis, Context, Entity, HighlightStyle, IntoElement, ParentElement,
    SharedString, Styled, StyledText, Subscription, Task, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};
use scrobble::rewrite::{self, CustomRule, Field, PRESETS, Preview, Rewriter, Sample};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};

use crate::library_service::LibraryEvent;
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::{SettingsStore, notify_save_error};
use crate::theme_colors::Colors;

const MAX_EXAMPLES: usize = 3;
const DRAFT_DEBOUNCE: Duration = Duration::from_millis(250);
const LCS_CELLS: usize = 250_000;
const HINT_PATTERN: &str = "^(.+), (.+)$";
const HINT_REPLACEMENT: &str = "$2 $1";
const HINT_BEFORE: &str = "Bach, Johann Sebastian";
const HINT_AFTER: &str = "Johann Sebastian Bach";

struct RuleRow {
    enabled: bool,
    summary: SharedString,
    flags: Option<SharedString>,
}

#[derive(Clone, Default)]
struct PreviewText {
    summary: SharedString,
    examples: Vec<ExampleLine>,
}

#[derive(Clone, Debug, PartialEq)]
struct ExampleLine {
    text: SharedString,
    removed: Vec<Range<usize>>,
    added: Vec<Range<usize>>,
}

pub struct ScrobbleRules {
    pattern: Entity<InputState>,
    replacement: Entity<InputState>,
    field: Field,
    regex: bool,
    ignore_case: bool,
    editing: Option<usize>,
    samples: Option<Arc<Vec<Sample>>>,
    load_task: Option<Task<()>>,
    preset_previews: Vec<Preview>,
    preset_texts: Vec<PreviewText>,
    draft_preview: Option<Preview>,
    draft_text: Option<PreviewText>,
    draft_error: Option<SharedString>,
    draft_task: Option<Task<()>>,
    rows: Vec<RuleRow>,
    _subscriptions: Vec<Subscription>,
}

impl ScrobbleRules {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pattern = cx.new(|cx| InputState::new(window, cx).placeholder(tr().rule_find.clone()));
        let replacement =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().rule_replace.clone()));
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let subscriptions = vec![
            cx.subscribe(&pattern, |this, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.refresh_draft(cx);
                }
            }),
            cx.subscribe(&replacement, |this, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.refresh_draft(cx);
                }
            }),
            cx.subscribe(
                &library_event_bus,
                |this, _, event: &LibraryEvent, cx| match event {
                    LibraryEvent::CatalogChanged | LibraryEvent::TagsSaved => {
                        this.invalidate_samples(cx)
                    }
                    _ => {}
                },
            ),
            cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
                this.rebuild_texts();
                this.sync_rows(cx);
                cx.notify();
            }),
        ];
        let mut this = Self {
            pattern,
            replacement,
            field: Field::Title,
            regex: false,
            ignore_case: false,
            editing: None,
            samples: None,
            load_task: None,
            preset_previews: Vec::new(),
            preset_texts: Vec::new(),
            draft_preview: None,
            draft_text: None,
            draft_error: None,
            draft_task: None,
            rows: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.rebuild_texts();
        this.sync_rows(cx);
        this
    }

    fn invalidate_samples(&mut self, cx: &mut Context<Self>) {
        self.samples = None;
        self.load_task = None;
        cx.notify();
    }

    fn needs_samples(&self) -> bool {
        self.samples.is_none() && self.load_task.is_none()
    }

    fn load_samples(&mut self, cx: &mut Context<Self>) {
        if !self.needs_samples() {
            return;
        }
        let repo = cx.global::<Services>().library.repo();
        let first_only = cx.global::<SettingsStore>().scrobble().first_artist_only;
        let executor = cx.background_executor().clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let (samples, previews) = executor
                .spawn(async move {
                    let samples = Arc::new(library_samples(&*repo, first_only));
                    let previews: Vec<Preview> = PRESETS
                        .iter()
                        .map(|preset| {
                            rewrite::preview(&Rewriter::for_preset(preset), &samples, MAX_EXAMPLES)
                        })
                        .collect();
                    (samples, previews)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.samples = Some(samples);
                this.preset_previews = previews;
                this.rebuild_texts();
                this.refresh_draft(cx);
                cx.notify();
            });
        }));
    }

    fn draft(&self, cx: &App) -> CustomRule {
        CustomRule {
            enabled: true,
            field: self.field,
            pattern: self.pattern.read(cx).value().to_string(),
            replacement: self.replacement.read(cx).value().to_string(),
            regex: self.regex,
            ignore_case: self.ignore_case,
        }
    }

    fn refresh_draft(&mut self, cx: &mut Context<Self>) {
        let rule = self.draft(cx);
        self.draft_task = None;
        if rule.pattern.is_empty() {
            self.draft_error = None;
            self.draft_preview = None;
            self.draft_text = None;
            cx.notify();
            return;
        }
        let rewriter = match Rewriter::for_rule(&rule) {
            Ok(rewriter) => rewriter,
            Err(message) => {
                self.draft_error = Some(tr().rule_invalid(&message).into());
                self.draft_preview = None;
                self.draft_text = None;
                cx.notify();
                return;
            }
        };
        self.draft_error = None;
        let Some(samples) = self.samples.clone() else {
            cx.notify();
            return;
        };
        let executor = cx.background_executor().clone();
        self.draft_task = Some(cx.spawn(async move |this, cx| {
            executor.timer(DRAFT_DEBOUNCE).await;
            let preview = executor
                .spawn(async move { rewrite::preview(&rewriter, &samples, MAX_EXAMPLES) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.draft_preview = Some(preview);
                this.rebuild_texts();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn rebuild_texts(&mut self) {
        self.preset_texts = PRESETS
            .iter()
            .enumerate()
            .map(|(ix, preset)| match self.preset_previews.get(ix) {
                Some(preview) if preview.tracks > 0 => preview_text(preview),
                Some(_) => PreviewText {
                    summary: tr().rule_no_matches.clone(),
                    examples: vec![example_line(preset.example.0, preset.example.1)],
                },
                None => PreviewText {
                    summary: tr().rule_example.clone(),
                    examples: vec![example_line(preset.example.0, preset.example.1)],
                },
            })
            .collect();
        self.draft_text = self.draft_preview.as_ref().map(|preview| {
            if preview.tracks > 0 {
                preview_text(preview)
            } else {
                PreviewText {
                    summary: tr().rule_no_matches.clone(),
                    examples: Vec::new(),
                }
            }
        });
    }

    fn sync_rows(&mut self, cx: &App) {
        self.rows = cx
            .global::<SettingsStore>()
            .scrobble()
            .rewrite
            .rules
            .iter()
            .map(|rule| {
                let mut flags = Vec::new();
                if rule.regex {
                    flags.push(tr().rule_regex.clone());
                }
                if rule.ignore_case {
                    flags.push(tr().rule_ignore_case.clone());
                }
                let after = if rule.replacement.is_empty() {
                    "∅"
                } else {
                    rule.replacement.as_str()
                };
                RuleRow {
                    enabled: rule.enabled,
                    summary: format!(
                        "{}: {}  →  {}",
                        field_label(rule.field),
                        rule.pattern,
                        after
                    )
                    .into(),
                    flags: (!flags.is_empty()).then(|| flags.join(" · ").into()),
                }
            })
            .collect();
    }

    fn edit_config(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut scrobble::RewriteConfig),
    ) {
        update_rewrite(cx, edit);
        self.sync_rows(cx);
        cx.notify();
    }

    fn set_form(&mut self, rule: CustomRule, window: &mut Window, cx: &mut Context<Self>) {
        self.field = rule.field;
        self.regex = rule.regex;
        self.ignore_case = rule.ignore_case;
        self.pattern
            .update(cx, |s, cx| s.set_value(rule.pattern, window, cx));
        self.replacement
            .update(cx, |s, cx| s.set_value(rule.replacement, window, cx));
        self.refresh_draft(cx);
    }

    fn start_edit(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rule) = cx
            .global::<SettingsStore>()
            .scrobble()
            .rewrite
            .rules
            .get(ix)
            .cloned()
        else {
            return;
        };
        self.editing = Some(ix);
        self.set_form(rule, window, cx);
    }

    fn reset_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.set_form(CustomRule::default(), window, cx);
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut rule = self.draft(cx);
        if rule.pattern.is_empty() || Rewriter::for_rule(&rule).is_err() {
            return;
        }
        let editing = self.editing;
        self.edit_config(cx, move |config| match editing {
            Some(ix) if ix < config.rules.len() => {
                rule.enabled = config.rules[ix].enabled;
                config.rules[ix] = rule;
            }
            _ => config.rules.push(rule),
        });
        self.reset_form(window, cx);
    }

    fn remove(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_config(cx, move |config| {
            if ix < config.rules.len() {
                config.rules.remove(ix);
            }
        });
        match self.editing {
            Some(editing) if editing == ix => self.reset_form(window, cx),
            Some(editing) if editing > ix => self.editing = Some(editing - 1),
            _ => {}
        }
        cx.notify();
    }
}

pub(crate) fn library_samples(
    repo: &dyn music_library::LibraryRepository,
    first_only: bool,
) -> Vec<Sample> {
    let tracks = repo.all_tracks().unwrap_or_else(|e| {
        log::warn!("scrobble rules: could not read tracks for the preview: {e}");
        Vec::new()
    });
    let albums: std::collections::HashMap<i64, (String, String)> = repo
        .albums()
        .unwrap_or_else(|e| {
            log::warn!("scrobble rules: could not read albums for the preview: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|a| (a.id, (a.title, a.artist_name)))
        .collect();
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let artists = repo.track_artists_map(&ids).unwrap_or_else(|e| {
        log::warn!("scrobble rules: could not read artists for the preview: {e}");
        Default::default()
    });
    tracks
        .into_iter()
        .map(|track| {
            let album = track.album_id.and_then(|id| albums.get(&id));
            Sample {
                artist: artists
                    .get(&track.id)
                    .and_then(|names| scrobble::primary_artist(names, first_only))
                    .unwrap_or_default(),
                title: track.title,
                album: album.map(|(title, _)| title.clone()),
                album_artist: album
                    .map(|(_, artist)| artist.clone())
                    .filter(|a| !a.is_empty()),
            }
        })
        .collect()
}

fn example_line(before: &str, after: &str) -> ExampleLine {
    let before: Vec<char> = before.chars().collect();
    let after: Vec<char> = after.chars().collect();
    let (keep_before, keep_after) = common_chars(&before, &after);
    let mut text = String::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    push_marked(&mut text, &before, &keep_before, &mut removed);
    text.push_str("  →  ");
    if after.is_empty() {
        text.push('∅');
    } else {
        push_marked(&mut text, &after, &keep_after, &mut added);
    }
    ExampleLine {
        text: text.into(),
        removed,
        added,
    }
}

fn visible(c: char) -> char {
    match c {
        '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{00A0}' => '·',
        c => c,
    }
}

fn push_marked(out: &mut String, chars: &[char], keep: &[bool], ranges: &mut Vec<Range<usize>>) {
    for (&c, &kept) in chars.iter().zip(keep) {
        let start = out.len();
        out.push(if kept { c } else { visible(c) });
        if kept {
            continue;
        }
        match ranges.last_mut() {
            Some(range) if range.end == start => range.end = out.len(),
            _ => ranges.push(start..out.len()),
        }
    }
}

fn common_chars(before: &[char], after: &[char]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (before.len(), after.len());
    let mut keep_before = vec![false; n];
    let mut keep_after = vec![false; m];
    if n.saturating_mul(m) > LCS_CELLS {
        let prefix = before.iter().zip(after).take_while(|(x, y)| x == y).count();
        let suffix = before[prefix..]
            .iter()
            .rev()
            .zip(after[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        keep_before[..prefix].fill(true);
        keep_after[..prefix].fill(true);
        keep_before[n - suffix..].fill(true);
        keep_after[m - suffix..].fill(true);
        return (keep_before, keep_after);
    }
    let width = m + 1;
    let mut table = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * width + j] = if before[i] == after[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if before[i] == after[j] {
            keep_before[i] = true;
            keep_after[j] = true;
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (keep_before, keep_after)
}

fn preview_text(preview: &Preview) -> PreviewText {
    let count = i64::try_from(preview.tracks).unwrap_or(i64::MAX);
    PreviewText {
        summary: tr().rule_affects(&tr().n_tracks(count)).into(),
        examples: preview
            .examples
            .iter()
            .map(|(before, after)| example_line(before, after))
            .collect(),
    }
}

fn update_rewrite(cx: &mut App, edit: impl FnOnce(&mut scrobble::RewriteConfig)) {
    if let Err(e) = cx
        .global_mut::<SettingsStore>()
        .update_scrobble(|s| edit(&mut s.rewrite))
    {
        notify_save_error(cx, e);
    }
    crate::scrobble_bridge::apply_rewrite(cx);
}

fn field_label(field: Field) -> SharedString {
    match field {
        Field::Artist => tr().rule_field_artist.clone(),
        Field::Title => tr().rule_field_title.clone(),
        Field::Album => tr().rule_field_album.clone(),
        Field::AlbumArtist => tr().rule_field_album_artist.clone(),
    }
}

fn preset_name(id: &str) -> (SharedString, SharedString) {
    let t = tr();
    match id {
        "remastered" => (
            t.preset_remastered.clone(),
            t.preset_remastered_desc.clone(),
        ),
        "explicit" => (t.preset_explicit.clone(), t.preset_explicit_desc.clone()),
        "single_ep" => (t.preset_single_ep.clone(), t.preset_single_ep_desc.clone()),
        "version" => (t.preset_version.clone(), t.preset_version_desc.clone()),
        "edition" => (t.preset_edition.clone(), t.preset_edition_desc.clone()),
        "remix_suffix" => (
            t.preset_remix_suffix.clone(),
            t.preset_remix_suffix_desc.clone(),
        ),
        "feat" => (t.preset_feat.clone(), t.preset_feat_desc.clone()),
        "live" => (t.preset_live.clone(), t.preset_live_desc.clone()),
        other => (
            SharedString::from(other.to_string()),
            SharedString::default(),
        ),
    }
}

fn ensure_samples(rules: &Entity<ScrobbleRules>, window: &mut Window, cx: &App) {
    if !rules.read(cx).needs_samples() {
        return;
    }
    let rules = rules.clone();
    window.on_next_frame(move |_, cx| {
        rules.update(cx, |rules, cx| rules.load_samples(cx));
    });
}

fn preview_block(text: &PreviewText, cx: &App) -> AnyElement {
    v_flex()
        .gap_0p5()
        .text_xs()
        .text_color(Colors::muted_foreground(cx))
        .child(div().child(text.summary.clone()))
        .children(text.examples.iter().map(|line| {
            let removed = HighlightStyle {
                background_color: Some(cx.theme().danger.opacity(0.25)),
                color: Some(cx.theme().foreground),
                ..Default::default()
            };
            let added = HighlightStyle {
                background_color: Some(cx.theme().success.opacity(0.25)),
                color: Some(cx.theme().foreground),
                ..Default::default()
            };
            let highlights = line
                .removed
                .iter()
                .map(move |range| (range.clone(), removed))
                .chain(line.added.iter().map(move |range| (range.clone(), added)));
            div()
                .pl_2()
                .child(StyledText::new(line.text.clone()).with_highlights(highlights))
        }))
        .into_any_element()
}

pub fn presets_group(rules: Entity<ScrobbleRules>) -> SettingGroup {
    let first_artist_rules = rules.clone();
    let mut group = SettingGroup::new()
        .separated()
        .title(tr().scrobble_rules.clone())
        .description(tr().scrobble_rules_desc.clone())
        .item(
            SettingItem::new(
                tr().scrobble_first_artist.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    let enabled = cx.global::<SettingsStore>().scrobble().first_artist_only;
                    let rules = first_artist_rules.clone();
                    h_flex().items_center().justify_end().child(
                        Switch::new("scrobble-first-artist-toggle")
                            .checked(enabled)
                            .on_click(move |new_val, _, cx| {
                                let value = *new_val;
                                crate::scrobble_settings::update_scrobble(cx, move |s| {
                                    s.first_artist_only = value
                                });
                                rules.update(cx, |rules, cx| rules.invalidate_samples(cx));
                            }),
                    )
                }),
            )
            .description(tr().scrobble_first_artist_desc.clone()),
        );
    for (ix, preset) in PRESETS.iter().enumerate() {
        let (name, desc) = preset_name(preset.id);
        let rules = rules.clone();
        let id = preset.id;
        group = group.item(
            SettingItem::new(
                name,
                SettingField::render(move |window, cx: &mut App| {
                    ensure_samples(&rules, window, cx);
                    let enabled = cx
                        .global::<SettingsStore>()
                        .scrobble()
                        .rewrite
                        .presets
                        .contains(id);
                    let preview = rules
                        .read(cx)
                        .preset_texts
                        .get(ix)
                        .map(|text| preview_block(text, cx));
                    h_flex()
                        .w_full()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .child(div().flex_1().min_w_0().children(preview))
                        .child(
                            Switch::new(("scrobble-preset", ix))
                                .checked(enabled)
                                .on_click(move |new_val, _, cx| {
                                    let value = *new_val;
                                    update_rewrite(cx, move |config| {
                                        if value {
                                            config.presets.insert(id.to_string());
                                        } else {
                                            config.presets.remove(id);
                                        }
                                    });
                                }),
                        )
                }),
            )
            .layout(Axis::Vertical)
            .description(desc),
        );
    }
    group
}

pub fn custom_rules_group(rules: Entity<ScrobbleRules>) -> SettingGroup {
    let list_rules = rules.clone();
    SettingGroup::new()
        .separated()
        .title(tr().scrobble_custom_rules.clone())
        .description(tr().scrobble_custom_rules_desc.clone())
        .item(SettingItem::unlabeled(SettingField::render(
            move |_window, cx: &mut App| rules_list(list_rules.clone(), cx),
        )))
        .item(SettingItem::unlabeled(SettingField::render(
            move |window, cx: &mut App| {
                ensure_samples(&rules, window, cx);
                rule_form(rules.clone(), cx)
            },
        )))
}

fn rules_list(rules: Entity<ScrobbleRules>, cx: &mut App) -> AnyElement {
    let state = rules.read(cx);
    if state.rows.is_empty() {
        return div()
            .text_sm()
            .text_color(Colors::muted_foreground(cx))
            .child(tr().rule_none.clone())
            .into_any_element();
    }
    let editing = state.editing;
    let mut list = v_flex().gap_2().w_full();
    for (ix, row) in state.rows.iter().enumerate() {
        let toggle_rules = rules.clone();
        let edit_rules = rules.clone();
        let remove_rules = rules.clone();
        list = list.child(
            h_flex()
                .w_full()
                .items_center()
                .gap_3()
                .child(
                    Switch::new(("scrobble-rule-enabled", ix))
                        .checked(row.enabled)
                        .on_click(move |new_val, _, cx| {
                            let value = *new_val;
                            toggle_rules.update(cx, |rules, cx| {
                                rules.edit_config(cx, move |config| {
                                    if let Some(rule) = config.rules.get_mut(ix) {
                                        rule.enabled = value;
                                    }
                                });
                            });
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(if row.enabled {
                                    Colors::foreground(cx)
                                } else {
                                    Colors::muted_foreground(cx)
                                })
                                .child(row.summary.clone()),
                        )
                        .when_some(row.flags.clone(), |this, flags| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(Colors::muted_foreground(cx))
                                    .child(flags),
                            )
                        }),
                )
                .child(
                    Button::new(("scrobble-rule-edit", ix))
                        .small()
                        .icon(Icon::default().path("icons/s1-pencil.svg"))
                        .tooltip(tr().rule_edit.clone())
                        .selected(editing == Some(ix))
                        .on_click(move |_, window, cx| {
                            edit_rules.update(cx, |rules, cx| rules.start_edit(ix, window, cx));
                        }),
                )
                .child(
                    Button::new(("scrobble-rule-remove", ix))
                        .small()
                        .icon(Icon::default().path("icons/s1-trash.svg"))
                        .tooltip(tr().delete.clone())
                        .on_click(move |_, window, cx| {
                            remove_rules.update(cx, |rules, cx| rules.remove(ix, window, cx));
                        }),
                ),
        );
    }
    list.into_any_element()
}

fn regex_hint(cx: &App) -> AnyElement {
    let arrow = || {
        Icon::default()
            .path("icons/s1-arrow-right.svg")
            .size(px(12.))
            .text_color(Colors::muted_foreground(cx))
    };
    v_flex()
        .gap_1()
        .text_xs()
        .text_color(Colors::muted_foreground(cx))
        .child(div().child(tr().rule_regex_hint.clone()))
        .child(div().pt_1().child(tr().rule_example.clone()))
        .child(
            h_flex()
                .pl_2()
                .gap_2()
                .items_center()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(Colors::foreground(cx))
                .child(HINT_PATTERN)
                .child(arrow())
                .child(HINT_REPLACEMENT),
        )
        .child(
            h_flex()
                .pl_2()
                .gap_2()
                .items_center()
                .child(HINT_BEFORE)
                .child(arrow())
                .child(HINT_AFTER),
        )
        .into_any_element()
}

fn rule_form(rules: Entity<ScrobbleRules>, cx: &mut App) -> AnyElement {
    let state = rules.read(cx);
    let field = state.field;
    let regex = state.regex;
    let ignore_case = state.ignore_case;
    let editing = state.editing.is_some();
    let can_commit = !state.pattern.read(cx).value().is_empty() && state.draft_error.is_none();
    let pattern = state.pattern.clone();
    let replacement = state.replacement.clone();
    let error = state.draft_error.clone();
    let preview = state
        .draft_text
        .as_ref()
        .map(|text| preview_block(text, cx));

    let field_rules = rules.clone();
    let regex_rules = rules.clone();
    let case_rules = rules.clone();
    let commit_rules = rules.clone();
    let cancel_rules = rules;

    let mut fields = ButtonGroup::new("scrobble-rule-field").small();
    for (ix, f) in Field::ALL.into_iter().enumerate() {
        fields = fields.child(
            Button::new(("scrobble-rule-field", ix))
                .label(field_label(f))
                .selected(field == f),
        );
    }

    let fields = fields.on_click(move |clicks: &Vec<usize>, _, cx| {
        let Some(field) = clicks.first().and_then(|&ix| Field::ALL.get(ix).copied()) else {
            return;
        };
        field_rules.update(cx, |rules, cx| {
            rules.field = field;
            rules.refresh_draft(cx);
        });
    });

    let inputs = h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(Input::new(&pattern).small()))
        .child(
            Icon::default()
                .path("icons/s1-arrow-right.svg")
                .size(px(14.))
                .text_color(Colors::muted_foreground(cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&replacement).small()),
        );

    let options = h_flex()
        .gap_4()
        .child(
            Switch::new("scrobble-rule-regex")
                .small()
                .checked(regex)
                .label(tr().rule_regex.clone())
                .on_click(move |new_val, _, cx| {
                    let value = *new_val;
                    regex_rules.update(cx, |rules, cx| {
                        rules.regex = value;
                        rules.refresh_draft(cx);
                    });
                }),
        )
        .child(
            Switch::new("scrobble-rule-case")
                .small()
                .checked(ignore_case)
                .label(tr().rule_ignore_case.clone())
                .on_click(move |new_val, _, cx| {
                    let value = *new_val;
                    case_rules.update(cx, |rules, cx| {
                        rules.ignore_case = value;
                        rules.refresh_draft(cx);
                    });
                }),
        );

    let actions = h_flex()
        .gap_2()
        .when(editing, |this| {
            this.child(
                Button::new("scrobble-rule-cancel")
                    .small()
                    .label(tr().cancel.clone())
                    .on_click(move |_, window, cx| {
                        cancel_rules.update(cx, |rules, cx| rules.reset_form(window, cx));
                    }),
            )
        })
        .child(
            Button::new("scrobble-rule-commit")
                .small()
                .primary()
                .label(if editing {
                    tr().rule_save.clone()
                } else {
                    tr().rule_add.clone()
                })
                .disabled(!can_commit)
                .on_click(move |_, window, cx| {
                    commit_rules.update(cx, |rules, cx| rules.commit(window, cx));
                }),
        );

    v_flex()
        .w_full()
        .gap_4()
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .child(fields)
                .child(inputs)
                .when(regex, |this| this.child(regex_hint(cx)))
                .when_some(error, |this, error| {
                    this.child(div().text_xs().text_color(Colors::danger(cx)).child(error))
                })
                .children(preview),
        )
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_3()
                .child(options)
                .child(actions),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked<'a>(line: &'a ExampleLine, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &line.text[r.clone()]).collect()
    }

    #[test]
    fn the_regex_hint_example_is_what_the_engine_does() {
        let rule = CustomRule {
            field: Field::Artist,
            pattern: HINT_PATTERN.to_string(),
            replacement: HINT_REPLACEMENT.to_string(),
            regex: true,
            ..CustomRule::default()
        };
        let rewriter = Rewriter::for_rule(&rule).unwrap();
        assert_eq!(
            rewriter.rewrite(Field::Artist, HINT_BEFORE).as_deref(),
            Some(HINT_AFTER)
        );
    }

    #[test]
    fn a_removed_suffix_is_marked_and_nothing_is_added() {
        let line = example_line("Let It Be (Remastered 2009)", "Let It Be");
        assert_eq!(line.text, "Let It Be (Remastered 2009)  →  Let It Be");
        assert_eq!(marked(&line, &line.removed), vec![" (Remastered 2009)"]);
        assert!(line.added.is_empty());
    }

    #[test]
    fn one_character_changes_are_marked_one_by_one() {
        let line = example_line("Don\u{2019}t Stop  Me Now", "Don't Stop Me Now");
        assert_eq!(marked(&line, &line.removed), vec!["\u{2019}", " "]);
        assert_eq!(marked(&line, &line.added), vec!["'"]);
    }

    #[test]
    fn invisible_characters_are_shown_as_dots() {
        let line = example_line("Sig\u{200B}ur", "Sigur");
        assert_eq!(line.text, "Sig·ur  →  Sigur");
        assert_eq!(marked(&line, &line.removed), vec!["·"]);
    }

    #[test]
    fn moved_punctuation_marks_both_sides() {
        let line = example_line("Track - X Remix", "Track (X Remix)");
        assert_eq!(marked(&line, &line.removed), vec!["- "]);
        assert_eq!(marked(&line, &line.added), vec!["(", ")"]);
    }

    #[test]
    fn an_emptied_value_shows_the_empty_sign() {
        let line = example_line("Unknown", "");
        assert_eq!(line.text, "Unknown  →  ∅");
        assert!(line.added.is_empty());
    }

    #[test]
    fn long_values_fall_back_to_prefix_and_suffix() {
        let before = format!("{} (Remastered)", "a".repeat(600));
        let after = "a".repeat(600);
        let line = example_line(&before, &after);
        assert_eq!(marked(&line, &line.removed), vec![" (Remastered)"]);
    }
}
