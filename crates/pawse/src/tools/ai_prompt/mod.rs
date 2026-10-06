mod answer;
mod builder;
mod options;
mod taste;
mod template;

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, App, AppContext, Axis, ClipboardItem, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage};
use ui_resources::i18n::{ToolsStrings, tools_strings};

use crate::services::Services;
use crate::theme_colors::Colors;

pub use builder::build_prompt;
pub use options::{
    DEFAULT_COUNT, Detail, Mode, Period, PromptOptions, RELEASE_PERIODS, language_name,
};

const PREVIEW_CHARS: usize = 4000;
const PREVIEW_HEIGHT: f32 = 320.;
const PLAYLIST_NAME_MAX_CHARS: usize = 40;

#[derive(Clone)]
pub struct AiPromptInputs {
    pub wishes: Entity<TextareaState>,
    pub answer: Entity<TextareaState>,
    pub playlist_name: Entity<InputState>,
}

pub struct AiPromptState {
    mode: Mode,
    period: Period,
    release_window: Period,
    count: u32,
    detail: Detail,
    busy: bool,
    generation: u64,
    prompt: Option<SharedString>,
    preview: SharedString,
    size: SharedString,
    no_history: bool,
    status: Option<SharedString>,
    import_busy: bool,
    import_status: Option<SharedString>,
    import_missing: Option<SharedString>,
}

impl Default for AiPromptState {
    fn default() -> Self {
        Self {
            mode: Mode::NewMusic,
            period: Period::HalfYear,
            release_window: Period::HalfYear,
            count: DEFAULT_COUNT,
            detail: Detail::Low,
            busy: false,
            generation: 0,
            prompt: None,
            preview: SharedString::default(),
            size: SharedString::default(),
            no_history: false,
            status: None,
            import_busy: false,
            import_status: None,
            import_missing: None,
        }
    }
}

impl AiPromptState {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            let counts = mode.counts();
            if !counts.contains(&self.count) {
                self.count = counts
                    .iter()
                    .copied()
                    .filter(|&c| c <= self.count)
                    .max()
                    .unwrap_or(DEFAULT_COUNT);
            }
            self.clear_result();
        }
    }

    pub fn prefer_playlist_mode(&mut self) -> bool {
        if self.mode.builds_playlist() {
            return false;
        }
        self.set_mode(Mode::FromLibrary);
        true
    }

    fn set_period(&mut self, period: Period) {
        if self.period != period {
            self.period = period;
            self.clear_result();
        }
    }

    fn set_release_window(&mut self, window: Period) {
        if self.release_window != window {
            self.release_window = window;
            self.clear_result();
        }
    }

    fn set_count(&mut self, count: u32) {
        if self.count != count {
            self.count = count;
            self.clear_result();
        }
    }

    fn set_detail(&mut self, detail: Detail) {
        if self.detail != detail {
            self.detail = detail;
            self.clear_result();
        }
    }

    fn clear_result(&mut self) {
        self.generation += 1;
        self.prompt = None;
        self.preview = SharedString::default();
        self.size = SharedString::default();
        self.status = None;
    }

    fn options(&self, wishes: String) -> PromptOptions {
        PromptOptions {
            mode: self.mode,
            period: self.period,
            release_window: self.release_window,
            count: self.count,
            detail: self.detail,
            wishes,
            answer_language: language_name(ui_resources::i18n::active().code()),
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn preview_of(prompt: &str) -> SharedString {
    match prompt.char_indices().nth(PREVIEW_CHARS) {
        Some((cut, _)) => SharedString::from(format!("{}\n…", &prompt[..cut])),
        None => SharedString::from(prompt.to_string()),
    }
}

fn generate(state: Entity<AiPromptState>, wishes: Entity<TextareaState>, cx: &mut App) {
    let repo = cx.global::<Services>().library.repo();
    let wishes = wishes.read(cx).value().to_string();
    let (options, generation) = state.update(cx, |s, cx| {
        s.busy = true;
        s.status = None;
        cx.notify();
        (s.options(wishes), s.generation)
    });
    cx.spawn(async move |cx| {
        let outcome = cx
            .background_spawn(async move {
                let now = unix_now();
                let snapshot = taste::gather(repo.as_ref(), options.period.cutoff(now))?;
                let prompt = build_prompt(&snapshot, &options, now);
                music_library::Result::Ok((prompt, snapshot.has_history()))
            })
            .await;
        cx.update(|cx| {
            state.update(cx, |s, cx| {
                s.busy = false;
                if s.generation != generation {
                    cx.notify();
                    return;
                }
                match outcome {
                    Ok((prompt, has_history)) => {
                        s.preview = preview_of(&prompt);
                        s.size = SharedString::from(
                            tools_strings().ai_prompt_size(prompt.chars().count()),
                        );
                        s.no_history = !has_history;
                        s.prompt = Some(SharedString::from(prompt));
                    }
                    Err(e) => {
                        log::warn!("AI prompt: failed to read the library: {e}");
                        s.clear_result();
                        s.status = Some(SharedString::from(e.to_string()));
                    }
                }
                cx.notify();
            })
        });
    })
    .detach();
}

fn copy(state: Entity<AiPromptState>, cx: &mut App) {
    let Some(prompt) = state.read(cx).prompt.clone() else {
        return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(prompt.to_string()));
    state.update(cx, |s, cx| {
        s.status = Some(tools_strings().ai_prompt_copied.clone());
        cx.notify();
    });
}

fn playlist_name(typed: &str, wishes: &str) -> String {
    let typed = typed.trim();
    if !typed.is_empty() {
        return typed.to_string();
    }
    let wish = wishes.lines().map(str::trim).find(|line| !line.is_empty());
    match wish {
        Some(wish) if wish.chars().count() > PLAYLIST_NAME_MAX_CHARS => {
            let cut: String = wish.chars().take(PLAYLIST_NAME_MAX_CHARS).collect();
            format!("{}…", cut.trim_end())
        }
        Some(wish) => wish.to_string(),
        None => tools_strings().ai_answer_default_name.to_string(),
    }
}

fn import_answer(state: Entity<AiPromptState>, inputs: AiPromptInputs, cx: &mut App) {
    let text = inputs.answer.read(cx).value().to_string();
    if text.trim().is_empty() || state.read(cx).import_busy {
        return;
    }
    let name = playlist_name(
        &inputs.playlist_name.read(cx).value(),
        &inputs.wishes.read(cx).value(),
    );
    let library = cx.global::<Services>().library.clone();
    let repo = library.repo();
    state.update(cx, |s, cx| {
        s.import_busy = true;
        s.import_status = None;
        s.import_missing = None;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let parsed = cx
            .background_spawn(async move {
                let listings = repo.track_listings()?;
                let index = answer::TrackIndex::new(&listings);
                music_library::Result::Ok(answer::parse_answer(&text, &index))
            })
            .await;
        cx.update(|cx| {
            let s = tools_strings();
            let (status, missing) = match parsed {
                Ok(parsed) if parsed.track_ids.is_empty() => {
                    (s.ai_answer_nothing_found.clone(), parsed.missing)
                }
                Ok(parsed) => {
                    let total = parsed.track_ids.len() + parsed.missing.len();
                    match library.create_playlist(&name) {
                        Some(playlist_id) => {
                            library.add_tracks_to_playlist(playlist_id, &parsed.track_ids);
                            let status = s.ai_answer_created(&name, parsed.track_ids.len(), total);
                            (SharedString::from(status), parsed.missing)
                        }
                        None => (s.ai_answer_failed.clone(), Vec::new()),
                    }
                }
                Err(e) => {
                    log::warn!("AI answer: failed to read the library: {e}");
                    (SharedString::from(e.to_string()), Vec::new())
                }
            };
            state.update(cx, |ui, cx| {
                ui.import_busy = false;
                ui.import_status = Some(status);
                ui.import_missing =
                    (!missing.is_empty()).then(|| SharedString::from(missing.join("\n")));
                cx.notify();
            });
        });
    })
    .detach();
}

fn mode_label(s: &ToolsStrings, mode: Mode) -> SharedString {
    match mode {
        Mode::NewMusic => s.ai_prompt_mode_new.clone(),
        Mode::NewReleases => s.ai_prompt_mode_releases.clone(),
        Mode::FromLibrary => s.ai_prompt_mode_library.clone(),
        Mode::Forgotten => s.ai_prompt_mode_forgotten.clone(),
    }
}

fn mode_description(s: &ToolsStrings, mode: Mode) -> SharedString {
    match mode {
        Mode::NewMusic => s.ai_prompt_mode_new_desc.clone(),
        Mode::NewReleases => s.ai_prompt_mode_releases_desc.clone(),
        Mode::FromLibrary => s.ai_prompt_mode_library_desc.clone(),
        Mode::Forgotten => s.ai_prompt_mode_forgotten_desc.clone(),
    }
}

fn period_label(s: &ToolsStrings, period: Period) -> SharedString {
    match period {
        Period::Week => s.ai_prompt_period_week.clone(),
        Period::Month => s.ai_prompt_period_month.clone(),
        Period::HalfYear => s.ai_prompt_period_half_year.clone(),
        Period::AllTime => s.ai_prompt_period_all.clone(),
    }
}

fn muted_text(text: SharedString, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(Colors::muted_foreground(cx))
        .child(text)
}

fn mode_field(state: Entity<AiPromptState>, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let current = state.read(cx).mode;
    let mut group = ButtonGroup::new("ai-prompt-mode").small();
    for (ix, mode) in Mode::ALL.into_iter().enumerate() {
        group = group.child(
            Button::new(("ai-prompt-mode", ix))
                .label(mode_label(s, mode))
                .selected(current == mode),
        );
    }
    v_flex()
        .gap_2()
        .child(
            h_flex().child(group.on_click(move |clicks: &Vec<usize>, _, cx| {
                let Some(mode) = clicks.first().and_then(|&ix| Mode::ALL.get(ix)) else {
                    return;
                };
                state.update(cx, |s, cx| {
                    s.set_mode(*mode);
                    cx.notify();
                });
            })),
        )
        .child(muted_text(mode_description(s, current), cx))
        .into_any_element()
}

fn period_group(
    id: &'static str,
    current: Period,
    periods: &'static [Period],
    on_pick: impl Fn(Period, &mut App) + 'static,
) -> AnyElement {
    let s = tools_strings();
    let mut group = ButtonGroup::new(id).small();
    for (ix, &period) in periods.iter().enumerate() {
        group = group.child(
            Button::new((id, ix))
                .label(period_label(s, period))
                .selected(current == period),
        );
    }
    h_flex()
        .justify_end()
        .child(group.on_click(move |clicks: &Vec<usize>, _, cx| {
            let Some(&period) = clicks.first().and_then(|&ix| periods.get(ix)) else {
                return;
            };
            on_pick(period, cx);
        }))
        .into_any_element()
}

fn period_field(state: Entity<AiPromptState>, cx: &mut App) -> AnyElement {
    let current = state.read(cx).period;
    period_group(
        "ai-prompt-period",
        current,
        &Period::ALL,
        move |period, cx| {
            state.update(cx, |s, cx| {
                s.set_period(period);
                cx.notify();
            });
        },
    )
}

fn release_window_field(state: Entity<AiPromptState>, cx: &mut App) -> AnyElement {
    let current = state.read(cx).release_window;
    period_group(
        "ai-prompt-window",
        current,
        &RELEASE_PERIODS,
        move |window, cx| {
            state.update(cx, |s, cx| {
                s.set_release_window(window);
                cx.notify();
            });
        },
    )
}

fn count_field(state: Entity<AiPromptState>, cx: &mut App) -> AnyElement {
    let (current, counts) = {
        let s = state.read(cx);
        (s.count, s.mode.counts())
    };
    let mut group = ButtonGroup::new("ai-prompt-count").small();
    for (ix, &count) in counts.iter().enumerate() {
        group = group.child(
            Button::new(("ai-prompt-count", ix))
                .label(SharedString::from(count.to_string()))
                .selected(current == count),
        );
    }
    h_flex()
        .justify_end()
        .child(group.on_click(move |clicks: &Vec<usize>, _, cx| {
            let Some(count) = clicks.first().and_then(|&ix| counts.get(ix)) else {
                return;
            };
            state.update(cx, |s, cx| {
                s.set_count(*count);
                cx.notify();
            });
        }))
        .into_any_element()
}

fn detail_label(s: &ToolsStrings, detail: Detail) -> SharedString {
    match detail {
        Detail::Low => s.ai_prompt_detail_low.clone(),
        Detail::Medium => s.ai_prompt_detail_medium.clone(),
        Detail::High => s.ai_prompt_detail_high.clone(),
    }
}

fn detail_field(state: Entity<AiPromptState>, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let current = state.read(cx).detail;
    let mut group = ButtonGroup::new("ai-prompt-detail").small();
    for (ix, detail) in Detail::ALL.into_iter().enumerate() {
        group = group.child(
            Button::new(("ai-prompt-detail", ix))
                .label(detail_label(s, detail))
                .selected(current == detail),
        );
    }
    h_flex()
        .justify_end()
        .child(group.on_click(move |clicks: &Vec<usize>, _, cx| {
            let Some(detail) = clicks.first().and_then(|&ix| Detail::ALL.get(ix)) else {
                return;
            };
            state.update(cx, |s, cx| {
                s.set_detail(*detail);
                cx.notify();
            });
        }))
        .into_any_element()
}

fn result_field(
    state: Entity<AiPromptState>,
    wishes: Entity<TextareaState>,
    cx: &mut App,
) -> AnyElement {
    let s = tools_strings();
    let ui = state.read(cx);
    let busy = ui.busy;
    let ready = ui.prompt.is_some();
    let preview = ui.preview.clone();
    let size = ui.size.clone();
    let no_history = ui.no_history && ready;
    let status = ui.status.clone();

    let buttons = h_flex()
        .gap_2()
        .child(
            Button::new("ai-prompt-build")
                .small()
                .primary()
                .label(s.ai_prompt_build.clone())
                .loading(busy)
                .disabled(busy)
                .on_click({
                    let state = state.clone();
                    move |_, _, cx| generate(state.clone(), wishes.clone(), cx)
                }),
        )
        .child(
            Button::new("ai-prompt-copy")
                .small()
                .label(crate::localization::tr().copy.clone())
                .disabled(!ready || busy)
                .on_click(move |_, _, cx| copy(state.clone(), cx)),
        );

    let mut column = v_flex().gap_2().child(buttons);
    if let Some(status) = status {
        column = column.child(muted_text(status, cx));
    }
    if ready {
        column = column.child(muted_text(size, cx));
        if no_history {
            column = column.child(muted_text(s.ai_prompt_no_history.clone(), cx));
        }
        column = column.child(
            div()
                .id("ai-prompt-preview")
                .max_h(px(PREVIEW_HEIGHT))
                .overflow_y_scroll()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(Colors::border(cx))
                .bg(Colors::muted(cx))
                .text_xs()
                .text_color(Colors::foreground(cx))
                .child(preview),
        );
    }
    column.into_any_element()
}

fn import_field(state: Entity<AiPromptState>, inputs: AiPromptInputs, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let ui = state.read(cx);
    let busy = ui.import_busy;
    let status = ui.import_status.clone();
    let missing = ui.import_missing.clone();
    let mut column = v_flex().gap_2().child(
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&inputs.playlist_name).small()),
            )
            .child(
                Button::new("ai-answer-create")
                    .small()
                    .primary()
                    .label(s.ai_answer_create.clone())
                    .loading(busy)
                    .disabled(busy)
                    .on_click(move |_, _, cx| import_answer(state.clone(), inputs.clone(), cx)),
            ),
    );
    if let Some(status) = status {
        column = column.child(muted_text(status, cx));
    }
    if let Some(missing) = missing {
        column = column
            .child(muted_text(s.ai_answer_missing.clone(), cx))
            .child(
                div()
                    .text_xs()
                    .text_color(Colors::muted_foreground(cx))
                    .child(missing),
            );
    }
    column.into_any_element()
}

pub fn page(state: Entity<AiPromptState>, inputs: AiPromptInputs, mode: Mode) -> SettingPage {
    let s = tools_strings();
    let mode_state = state.clone();
    let period_state = state.clone();
    let window_state = state.clone();
    let count_state = state.clone();
    let detail_state = state.clone();
    let wishes = inputs.wishes.clone();
    let wishes_input = inputs.wishes.clone();
    let answer_input = inputs.answer.clone();
    let import_state = state.clone();
    let mut group = SettingGroup::new()
        .title(s.tools_ai_prompt.clone())
        .description(s.ai_prompt_intro.clone())
        .item(
            SettingItem::new(
                s.ai_prompt_mode.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    mode_field(mode_state.clone(), cx)
                }),
            )
            .layout(Axis::Vertical),
        )
        .item(
            SettingItem::new(
                s.ai_prompt_period.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    period_field(period_state.clone(), cx)
                }),
            )
            .description(s.ai_prompt_period_desc.clone()),
        );
    if mode == Mode::NewReleases {
        group = group.item(
            SettingItem::new(
                s.ai_prompt_window.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    release_window_field(window_state.clone(), cx)
                }),
            )
            .description(s.ai_prompt_window_desc.clone()),
        );
    }
    let group = group
        .item(SettingItem::new(
            s.ai_prompt_count.clone(),
            SettingField::render(move |_window, cx: &mut App| count_field(count_state.clone(), cx)),
        ))
        .item(
            SettingItem::new(
                s.ai_prompt_detail.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    detail_field(detail_state.clone(), cx)
                }),
            )
            .description(s.ai_prompt_detail_desc.clone()),
        )
        .item(
            SettingItem::new(
                s.ai_prompt_wishes.clone(),
                SettingField::render(move |_window, _cx: &mut App| Textarea::new(&wishes_input)),
            )
            .layout(Axis::Vertical),
        )
        .item(
            SettingItem::new(
                s.ai_prompt_result.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    result_field(state.clone(), wishes.clone(), cx)
                }),
            )
            .layout(Axis::Vertical),
        );
    let page = SettingPage::new(s.tools_ai_prompt.clone()).group(group);
    if !mode.builds_playlist() {
        return page;
    }
    page.group(
        SettingGroup::new()
            .title(s.ai_answer.clone())
            .description(s.ai_answer_desc.clone())
            .item(
                SettingItem::new(
                    s.ai_answer_text.clone(),
                    SettingField::render(move |_window, _cx: &mut App| {
                        Textarea::new(&answer_input)
                    }),
                )
                .layout(Axis::Vertical),
            )
            .item(
                SettingItem::new(
                    s.ai_answer_name.clone(),
                    SettingField::render(move |_window, cx: &mut App| {
                        import_field(import_state.clone(), inputs.clone(), cx)
                    }),
                )
                .layout(Axis::Vertical),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_playlist_modes_offer_fifty() {
        assert!(!Mode::NewMusic.counts().contains(&50));
        assert!(Mode::FromLibrary.counts().contains(&50));
        assert!(Mode::Forgotten.counts().contains(&50));
    }

    #[test]
    fn only_playlist_modes_build_playlists() {
        assert!(!Mode::NewMusic.builds_playlist());
        assert!(!Mode::NewReleases.builds_playlist());
        assert!(Mode::FromLibrary.builds_playlist());
        assert!(Mode::Forgotten.builds_playlist());
    }

    #[test]
    fn release_window_is_independent_of_the_taste_period() {
        let mut state = AiPromptState::default();
        state.set_period(Period::AllTime);
        assert_eq!(state.release_window, Period::HalfYear);
        state.set_release_window(Period::Week);
        state.set_mode(Mode::NewReleases);
        assert_eq!(state.period, Period::AllTime);
        assert_eq!(state.release_window, Period::Week);
        let options = state.options(String::new());
        assert_eq!(options.period, Period::AllTime);
        assert_eq!(options.release_window, Period::Week);
    }

    #[test]
    fn changing_the_release_window_drops_the_prompt() {
        let mut state = AiPromptState {
            prompt: Some(SharedString::from("old")),
            ..Default::default()
        };
        state.set_release_window(Period::Month);
        assert!(state.prompt.is_none());
    }

    #[test]
    fn switching_to_new_music_clamps_the_count() {
        let mut state = AiPromptState::default();
        state.set_mode(Mode::Forgotten);
        state.set_count(50);
        state.set_mode(Mode::NewMusic);
        assert_eq!(state.count, 20);
        state.set_mode(Mode::FromLibrary);
        assert_eq!(state.count, 20);
    }
}
