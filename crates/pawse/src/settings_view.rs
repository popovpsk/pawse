use std::path::PathBuf;

use gpui::{
    AnyElement, App, Axis, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Window, anchored, deferred, div, point, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    switch::Switch,
    theme::ThemeRegistry,
    v_flex,
};

use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use ui_resources::i18n::Lang;

use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::{
    FontScale, LangChoice, SettingsStore, ThemeChoice, apply_font_scale, apply_theme,
    notify_save_error,
};
use crate::theme_colors::Colors;

fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(path).spawn();
}

/// State for the custom theme picker dropdown.
pub struct ThemePickerState {
    pub open: bool,
    /// (key, display_label) pairs; "system" → "System", theme_name → theme_name.
    pub options: Vec<(SharedString, SharedString)>,
    /// Theme that was active when the popup opened — restored on Escape / outside-click.
    pub snapshot: Option<ThemeChoice>,
    /// Index of the item currently highlighted by mouse or keyboard.
    pub highlight_index: Option<usize>,
    pub focus_handle: FocusHandle,
    pub scroll_handle: ScrollHandle,
}

impl ThemePickerState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            options: Self::build_options(cx),
            snapshot: None,
            highlight_index: None,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn build_options(cx: &App) -> Vec<(SharedString, SharedString)> {
        let mut opts: Vec<(SharedString, SharedString)> = vec![("system".into(), "System".into())];
        for cfg in ThemeRegistry::global(cx).sorted_themes() {
            let name: SharedString = cfg.name.clone();
            opts.push((name.clone(), name));
        }
        opts
    }

    fn current_index(&self, cx: &App) -> Option<usize> {
        let key = cx.global::<SettingsStore>().theme().as_key();
        self.options.iter().position(|(k, _)| k.as_ref() == key)
    }

    fn current_label(&self, cx: &App) -> SharedString {
        let key = cx.global::<SettingsStore>().theme().as_key();
        self.options
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| tr().unknown.clone())
    }
}

/// Revert to the snapshot theme and close the popup. Does NOT call cx.notify();
/// the caller must do so inside a Context<ThemePickerState> update closure.
fn close_with_revert(state: &mut ThemePickerState, cx: &mut App) {
    if !state.open {
        return;
    }
    if let Some(snapshot) = state.snapshot.take() {
        apply_theme(&snapshot, cx);
    }
    state.open = false;
    state.highlight_index = None;
}

/// Save the selected theme and close the popup. Does NOT call cx.notify();
/// the caller must do so inside a Context<ThemePickerState> update closure.
fn confirm_save(key: &SharedString, state: &mut ThemePickerState, cx: &mut App) {
    let choice = ThemeChoice::from_key(key.as_ref());
    let save_result = cx.global_mut::<SettingsStore>().set_theme(choice.clone());
    if let Err(e) = save_result {
        notify_save_error(cx, e);
    }
    apply_theme(&choice, cx);
    state.open = false;
    state.snapshot = None;
    state.highlight_index = None;
}

/// State for the custom language picker dropdown. Mirrors [`ThemePickerState`]
/// but without live preview: the active language is read from `SettingsStore`
/// on every render, so a selection only takes effect once it is saved.
pub struct LangPickerState {
    pub open: bool,
    /// (key, display_label) pairs; "system" → localized "System", else
    /// language code → endonym.
    pub options: Vec<(SharedString, SharedString)>,
    pub highlight_index: Option<usize>,
    pub focus_handle: FocusHandle,
    pub scroll_handle: ScrollHandle,
}

impl LangPickerState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            options: Self::build_options(),
            highlight_index: None,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn build_options() -> Vec<(SharedString, SharedString)> {
        let mut opts: Vec<(SharedString, SharedString)> =
            vec![("system".into(), tr().system.clone())];
        for &lang in Lang::all() {
            opts.push((lang.code().into(), lang.display_name().into()));
        }
        opts
    }

    fn current_index(&self, cx: &App) -> Option<usize> {
        let key = cx.global::<SettingsStore>().language().as_key();
        self.options.iter().position(|(k, _)| k.as_ref() == key)
    }

    fn current_label(&self, cx: &App) -> SharedString {
        let key = cx.global::<SettingsStore>().language().as_key();
        self.options
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| tr().unknown.clone())
    }
}

/// Close the language popup without saving. Does NOT call cx.notify().
fn close_lang(state: &mut LangPickerState) {
    state.open = false;
    state.highlight_index = None;
}

/// Save the selected language, rebuild the menus in the new language, and
/// trigger a global redraw so every `tr()` call re-reads the new table.
/// Does NOT call cx.notify().
fn confirm_lang(key: &SharedString, state: &mut LangPickerState, cx: &mut App) {
    let choice = LangChoice::from_key(key.as_ref());
    if let Err(e) = cx.global_mut::<SettingsStore>().set_language(choice) {
        notify_save_error(cx, e);
    }
    crate::localization::notify_lang_changed(cx);
    state.open = false;
    state.highlight_index = None;
    crate::app_menu::set_menus(cx);
    cx.refresh_windows();
}

/// Build the list of `SettingPage`s for the Settings widget.
///
/// Built once and cached on `MainView`. `SettingPage` is `Clone` so the cache
/// is cloned into a fresh `Settings::new(...).pages(...)` shell on each render.
pub fn build_settings_pages(
    theme_picker: Entity<ThemePickerState>,
    lang_picker: Entity<LangPickerState>,
) -> Vec<SettingPage> {
    vec![
        SettingPage::new(tr().settings_interface.clone())
            .group(interface_group(theme_picker, lang_picker))
            .group(queue_group()),
        SettingPage::new(tr().settings_library.clone()).group(library_group()),
    ]
}

/// Wrap pre-built pages into the `Settings` element for inline rendering.
pub fn settings_widget(pages: Vec<SettingPage>) -> Settings {
    Settings::new("pawse-settings").pages(pages)
}

/// Open a native folder picker (async, on the main thread), then add the
/// chosen folder to `SettingsStore` and trigger a full clear+rescan over the
/// whole list. Shared between the Settings view and the app menu's "Rescan"
/// action.
pub fn pick_and_add_folder(cx: &mut App) {
    cx.spawn(async move |cx| {
        if let Some(handle) = rfd::AsyncFileDialog::new().pick_folder().await {
            let path = handle.path().to_path_buf();
            cx.update(|cx| add_folder_and_rescan(path, cx)).ok();
        }
    })
    .detach();
}

/// Add a folder to settings (idempotent) and kick off a full rescan of all
/// configured folders. Surfaces save errors via `notify_save_error`.
pub fn add_folder_and_rescan(path: PathBuf, cx: &mut App) {
    let save_result = cx.global_mut::<SettingsStore>().add_music_folder(path);
    if let Err(e) = save_result {
        notify_save_error(cx, e);
    }
    let folders = cx.global::<SettingsStore>().music_folders().to_vec();
    cx.global::<Services>().library.clear_and_rescan(folders);
}

/// Remove a folder from settings and rescan whatever remains (a clear+rescan
/// is the simplest way to keep the DB consistent with the configured list).
pub fn remove_folder_and_rescan(path: PathBuf, cx: &mut App) {
    let save_result = cx.global_mut::<SettingsStore>().remove_music_folder(&path);
    if let Err(e) = save_result {
        notify_save_error(cx, e);
    }
    let folders = cx.global::<SettingsStore>().music_folders().to_vec();
    cx.global::<Services>().library.clear_and_rescan(folders);
}

fn interface_group(
    picker: Entity<ThemePickerState>,
    lang_picker: Entity<LangPickerState>,
) -> SettingGroup {
    let mut group = SettingGroup::new().item(
        SettingItem::new(
            tr().theme.clone(),
            SettingField::render({
                let picker = picker.clone();
                move |window, cx: &mut App| theme_picker_dropdown(picker.clone(), window, cx)
            }),
        )
        .description(tr().theme_desc.clone()),
    );

    group = group.item(language_field(lang_picker));

    group = group.item(
        SettingItem::new(
            tr().font_size.clone(),
            SettingField::render(|_window, cx: &mut App| {
                let current = cx.global::<SettingsStore>().font_scale();
                h_flex().items_center().justify_end().child(
                    ButtonGroup::new("font-size-group")
                        .small()
                        .child(
                            Button::new("font-small")
                                .label(tr().font_size_small.clone())
                                .selected(current == FontScale::Small),
                        )
                        .child(
                            Button::new("font-medium")
                                .label(tr().font_size_medium.clone())
                                .selected(current == FontScale::Medium),
                        )
                        .child(
                            Button::new("font-large")
                                .label(tr().font_size_large.clone())
                                .selected(current == FontScale::Large),
                        )
                        .on_click(|clicks: &Vec<usize>, _, cx| {
                            let Some(&ix) = clicks.first() else {
                                return;
                            };
                            let scale = match ix {
                                0 => FontScale::Small,
                                2 => FontScale::Large,
                                _ => FontScale::Medium,
                            };
                            if let Err(e) = cx.global_mut::<SettingsStore>().set_font_scale(scale) {
                                notify_save_error(cx, e);
                            }
                            apply_font_scale(scale, cx);
                            cx.refresh_windows();
                        }),
                )
            }),
        )
        .description(tr().font_size_desc.clone()),
    );

    // Exclusive (hog) mode is macOS/Windows-only; hide the toggle on Linux so it
    // can't be enabled there (Linux exclusive output is unimplemented for now).
    #[cfg(not(target_os = "linux"))]
    {
        group = group.item(
            SettingItem::new(
                tr().exclusive_mode_button.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_hog_button();
                    h_flex().items_center().justify_end().child(
                        Switch::new("exclusive-mode-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_hog_button(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().exclusive_mode_button_desc.clone()),
        );
    }

    group
        .item(
            SettingItem::new(
                tr().repeat_shuffle.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_repeat_shuffle();
                    h_flex().items_center().justify_end().child(
                        Switch::new("repeat-shuffle-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_repeat_shuffle(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().repeat_shuffle_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().time_labels.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_time_labels();
                    h_flex().items_center().justify_end().child(
                        Switch::new("time-labels-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_time_labels(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().time_labels_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().liked_tracks.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let enabled = cx.global::<SettingsStore>().liked_enabled();
                    h_flex().items_center().justify_end().child(
                        Switch::new("liked-enabled-toggle")
                            .checked(enabled)
                            .on_click(|new_val, _, cx| {
                                if let Err(e) =
                                    cx.global_mut::<SettingsStore>().set_liked_enabled(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            }),
                    )
                }),
            )
            .description(tr().liked_tracks_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().tab_playlists.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let enabled = cx.global::<SettingsStore>().playlists_enabled();
                    h_flex().items_center().justify_end().child(
                        Switch::new("playlists-enabled-toggle")
                            .checked(enabled)
                            .on_click(|new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_playlists_enabled(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            }),
                    )
                }),
            )
            .description(tr().playlists_desc.clone()),
        )
}

fn queue_group() -> SettingGroup {
    SettingGroup::new()
        .title(tr().queue.clone())
        .item(
            SettingItem::new(
                tr().track_duration.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_track_duration();
                    h_flex().items_center().justify_end().child(
                        Switch::new("track-duration-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_track_duration(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().track_duration_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().action_buttons.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_queue_actions();
                    h_flex().items_center().justify_end().child(
                        Switch::new("queue-actions-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_queue_actions(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().action_buttons_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().artist_name.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let show = cx.global::<SettingsStore>().show_queue_artist();
                    h_flex().items_center().justify_end().child(
                        Switch::new("queue-artist-toggle").checked(show).on_click(
                            |new_val, _, cx| {
                                if let Err(e) = cx
                                    .global_mut::<SettingsStore>()
                                    .set_show_queue_artist(*new_val)
                                {
                                    notify_save_error(cx, e);
                                }
                            },
                        ),
                    )
                }),
            )
            .description(tr().artist_name_desc.clone()),
        )
}

fn library_group() -> SettingGroup {
    SettingGroup::new().item(
        SettingItem::new(
            tr().music_folders.clone(),
            SettingField::render(|_window, cx: &mut App| {
                let folders = cx.global::<SettingsStore>().music_folders().to_vec();

                let mut list = v_flex().gap_2().w_full();

                if folders.is_empty() {
                    list = list.child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(Colors::muted_foreground(cx))
                            .child(tr().no_folders_added.clone()),
                    );
                } else {
                    for path in &folders {
                        let path_text: SharedString = path.to_string_lossy().into_owned().into();
                        let path_for_finder = path.clone();
                        let path_for_remove = path.clone();
                        let finder_id = format!("show-{}", path.display());
                        let remove_id = format!("remove-{}", path.display());

                        list = list.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .px_3()
                                .py_2()
                                .rounded(px(6.))
                                .bg(Colors::muted(cx))
                                .child(
                                    Icon::new(IconName::Folder)
                                        .text_color(Colors::muted_foreground(cx)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_sm()
                                        .truncate()
                                        .text_color(Colors::foreground(cx))
                                        .child(path_text),
                                )
                                .child(
                                    Button::new(SharedString::from(finder_id))
                                        .ghost()
                                        .label(tr().reveal_folder.clone())
                                        .on_click(move |_, _, _| {
                                            reveal_in_file_manager(&path_for_finder);
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(remove_id))
                                        .ghost()
                                        .label(tr().remove.clone())
                                        .on_click(move |_, _, cx| {
                                            remove_folder_and_rescan(path_for_remove.clone(), cx);
                                        }),
                                ),
                        );
                    }
                }

                v_flex().gap_3().w_full().child(list).child(
                    h_flex().gap_2().child(
                        Button::new("add-folder")
                            .label(tr().add_folder.clone())
                            .on_click(|_, _, cx| pick_and_add_folder(cx)),
                    ),
                )
            }),
        )
        .layout(Axis::Vertical)
        .description(tr().music_folders_desc.clone()),
    )
}

/// The "Language" setting row: a custom dropdown mirroring the theme picker.
/// Unlike the theme picker there is no live preview — selecting commits the
/// language (save + menu rebuild + global redraw) immediately.
fn language_field(picker: Entity<LangPickerState>) -> SettingItem {
    SettingItem::new(
        tr().language.clone(),
        SettingField::render({
            let picker = picker.clone();
            move |window, cx: &mut App| lang_picker_dropdown(picker.clone(), window, cx)
        }),
    )
    .description(tr().language_desc.clone())
}

pub fn theme_picker_dropdown(
    picker: Entity<ThemePickerState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = picker.read(cx);
    let open = state.open;
    let highlight_index = state.highlight_index;
    let options = state.options.clone();
    let current_label = state.current_label(cx);
    let focus_handle = state.focus_handle.clone();
    let scroll_handle = state.scroll_handle.clone();
    let _ = state;

    // Trigger button that opens/closes the popup
    let trigger = {
        let picker_t = picker.clone();
        div()
            .id("theme-picker-trigger")
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_1p5()
            .rounded(px(6.))
            .bg(Colors::background(cx))
            .border_1()
            .border_color(Colors::border(cx))
            .cursor_pointer()
            .hover(|s| s.bg(Colors::muted(cx)))
            .child(
                div()
                    .text_sm()
                    .text_color(Colors::foreground(cx))
                    .child(current_label),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .xsmall()
                    .text_color(Colors::muted_foreground(cx)),
            )
            .on_click(move |_, window, cx| {
                // When popup is open, a backdrop (priority 0) sits above
                // the trigger. Clicking the trigger while open routes
                // through the backdrop, which closes the popup and
                // occludes the event — this on_click won't fire in that
                // case. So here open is always false when this fires.
                let focus_handle = picker_t.read(cx).focus_handle.clone();
                picker_t.update(cx, |state, cx| {
                    let saved = cx.global::<SettingsStore>().theme();
                    let current_ix = state.current_index(cx);
                    state.open = true;
                    state.snapshot = Some(saved);
                    state.highlight_index = current_ix;
                    if let Some(ix) = current_ix {
                        state.scroll_handle.scroll_to_item(ix);
                    }
                    cx.notify();
                });
                focus_handle.focus(window);
            })
    };

    // Overlay elements: backdrop (priority 0) + popup (priority 1).
    // Rendered only while the popup is open.
    let mut overlay: Vec<AnyElement> = Vec::new();

    if open {
        let viewport = window.viewport_size();

        // Full-window backdrop at priority 0. Sits above normal content
        // (including the trigger) but below the popup. Clicking anywhere
        // — including on the trigger — routes through here and closes the
        // popup; occlude() prevents the event from reaching the trigger,
        // so on_click on the trigger does NOT re-open the popup.
        let picker_b = picker.clone();
        overlay.push(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .occlude()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            picker_b.update(cx, |state, cx| {
                                close_with_revert(state, cx);
                                cx.notify();
                            });
                        }),
                ),
            )
            .with_priority(0)
            .into_any_element(),
        );

        // Popup panel at priority 1 (above backdrop).
        let items = options
            .iter()
            .enumerate()
            .map(|(i, (key, label))| {
                let is_highlighted = highlight_index == Some(i);
                let key_c = key.clone();
                let label_c = label.clone();

                div()
                    .id(("theme-item", i))
                    .px_2()
                    .py_1p5()
                    .rounded(px(4.))
                    .text_sm()
                    .cursor_pointer()
                    .when(is_highlighted, |d| {
                        d.bg(Colors::accent(cx))
                            .text_color(Colors::accent_foreground(cx))
                    })
                    .when(!is_highlighted, |d| {
                        d.hover(|s| s.bg(Colors::secondary(cx)))
                    })
                    .child(label_c)
                    .on_mouse_move({
                        let picker_m = picker.clone();
                        let key_m = key_c.clone();
                        move |_, _, cx| {
                            picker_m.update(cx, |state, cx| {
                                if state.highlight_index == Some(i) {
                                    return;
                                }
                                state.highlight_index = Some(i);
                                let choice = ThemeChoice::from_key(key_m.as_ref());
                                apply_theme(&choice, cx);
                                cx.notify();
                            });
                        }
                    })
                    .on_click({
                        let picker_c = picker.clone();
                        let key_click = key_c.clone();
                        move |_, _, cx| {
                            picker_c.update(cx, |state, cx| {
                                confirm_save(&key_click, state, cx);
                                cx.notify();
                            });
                        }
                    })
            })
            .collect::<Vec<_>>();

        let popup_content = v_flex()
            .id("theme-picker-popup")
            .bg(Colors::popover(cx))
            .border_1()
            .border_color(Colors::border(cx))
            .rounded(px(6.))
            .shadow_md()
            .w(px(220.))
            .occlude()
            .relative()
            .track_focus(&focus_handle)
            .on_key_down({
                let picker_k = picker.clone();
                move |ev: &KeyDownEvent, _, cx| {
                    let key = ev.keystroke.key.as_str();
                    picker_k.update(cx, |state, cx| {
                        let len = state.options.len();
                        if len == 0 {
                            return;
                        }
                        match key {
                            "up" => {
                                let new_ix = state
                                    .highlight_index
                                    .map_or(len - 1, |i| if i == 0 { len - 1 } else { i - 1 });
                                state.highlight_index = Some(new_ix);
                                state.scroll_handle.scroll_to_item(new_ix);
                                let choice =
                                    ThemeChoice::from_key(state.options[new_ix].0.as_ref());
                                apply_theme(&choice, cx);
                                cx.notify();
                            }
                            "down" => {
                                let new_ix = state.highlight_index.map_or(0, |i| (i + 1) % len);
                                state.highlight_index = Some(new_ix);
                                state.scroll_handle.scroll_to_item(new_ix);
                                let choice =
                                    ThemeChoice::from_key(state.options[new_ix].0.as_ref());
                                apply_theme(&choice, cx);
                                cx.notify();
                            }
                            "escape" => {
                                close_with_revert(state, cx);
                                cx.notify();
                            }
                            "enter" => {
                                if let Some(ix) = state.highlight_index {
                                    let k = state.options[ix].0.clone();
                                    confirm_save(&k, state, cx);
                                } else {
                                    close_with_revert(state, cx);
                                }
                                cx.notify();
                            }
                            _ => {}
                        }
                    });
                }
            })
            .child(
                div()
                    .id("theme-picker-list")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .track_scroll(&scroll_handle)
                    .p_1()
                    .children(items),
            )
            .vertical_scrollbar(&scroll_handle);

        overlay.push(
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.))
                    .child(div().mt_1().occlude().child(popup_content)),
            )
            .with_priority(1)
            .into_any_element(),
        );
    }

    div()
        .relative()
        .child(trigger)
        .children(overlay)
        .into_any_element()
}

pub fn lang_picker_dropdown(
    picker: Entity<LangPickerState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = picker.read(cx);
    let open = state.open;
    let highlight_index = state.highlight_index;
    let options = state.options.clone();
    let current_label = state.current_label(cx);
    let focus_handle = state.focus_handle.clone();
    let scroll_handle = state.scroll_handle.clone();

    let trigger = {
        let picker_t = picker.clone();
        div()
            .id("lang-picker-trigger")
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_1p5()
            .rounded(px(6.))
            .bg(Colors::background(cx))
            .border_1()
            .border_color(Colors::border(cx))
            .cursor_pointer()
            .hover(|s| s.bg(Colors::muted(cx)))
            .child(
                div()
                    .text_sm()
                    .text_color(Colors::foreground(cx))
                    .child(current_label),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .xsmall()
                    .text_color(Colors::muted_foreground(cx)),
            )
            .on_click(move |_, window, cx| {
                let focus_handle = picker_t.read(cx).focus_handle.clone();
                picker_t.update(cx, |state, cx| {
                    let current_ix = state.current_index(cx);
                    state.open = true;
                    state.highlight_index = current_ix;
                    if let Some(ix) = current_ix {
                        state.scroll_handle.scroll_to_item(ix);
                    }
                    cx.notify();
                });
                focus_handle.focus(window);
            })
    };

    let mut overlay: Vec<AnyElement> = Vec::new();

    if open {
        let viewport = window.viewport_size();

        let picker_b = picker.clone();
        overlay.push(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .occlude()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            picker_b.update(cx, |state, cx| {
                                close_lang(state);
                                cx.notify();
                            });
                        }),
                ),
            )
            .with_priority(0)
            .into_any_element(),
        );

        let items = options
            .iter()
            .enumerate()
            .map(|(i, (key, label))| {
                let is_highlighted = highlight_index == Some(i);
                let key_c = key.clone();
                let label_c = label.clone();

                div()
                    .id(("lang-item", i))
                    .px_2()
                    .py_1p5()
                    .rounded(px(4.))
                    .text_sm()
                    .cursor_pointer()
                    .when(is_highlighted, |d| {
                        d.bg(Colors::accent(cx))
                            .text_color(Colors::accent_foreground(cx))
                    })
                    .when(!is_highlighted, |d| {
                        d.hover(|s| s.bg(Colors::secondary(cx)))
                    })
                    .child(label_c)
                    .on_mouse_move({
                        let picker_m = picker.clone();
                        move |_, _, cx| {
                            picker_m.update(cx, |state, cx| {
                                if state.highlight_index == Some(i) {
                                    return;
                                }
                                state.highlight_index = Some(i);
                                cx.notify();
                            });
                        }
                    })
                    .on_click({
                        let picker_c = picker.clone();
                        let key_click = key_c.clone();
                        move |_, _, cx| {
                            picker_c.update(cx, |state, cx| {
                                confirm_lang(&key_click, state, cx);
                                cx.notify();
                            });
                        }
                    })
            })
            .collect::<Vec<_>>();

        let popup_content = v_flex()
            .id("lang-picker-popup")
            .bg(Colors::popover(cx))
            .border_1()
            .border_color(Colors::border(cx))
            .rounded(px(6.))
            .shadow_md()
            .w(px(220.))
            .occlude()
            .relative()
            .track_focus(&focus_handle)
            .on_key_down({
                let picker_k = picker.clone();
                move |ev: &KeyDownEvent, _, cx| {
                    let key = ev.keystroke.key.as_str();
                    picker_k.update(cx, |state, cx| {
                        let len = state.options.len();
                        if len == 0 {
                            return;
                        }
                        match key {
                            "up" => {
                                let new_ix = state
                                    .highlight_index
                                    .map_or(len - 1, |i| if i == 0 { len - 1 } else { i - 1 });
                                state.highlight_index = Some(new_ix);
                                state.scroll_handle.scroll_to_item(new_ix);
                                cx.notify();
                            }
                            "down" => {
                                let new_ix = state.highlight_index.map_or(0, |i| (i + 1) % len);
                                state.highlight_index = Some(new_ix);
                                state.scroll_handle.scroll_to_item(new_ix);
                                cx.notify();
                            }
                            "escape" => {
                                close_lang(state);
                                cx.notify();
                            }
                            "enter" => {
                                if let Some(ix) = state.highlight_index {
                                    let k = state.options[ix].0.clone();
                                    confirm_lang(&k, state, cx);
                                } else {
                                    close_lang(state);
                                }
                                cx.notify();
                            }
                            _ => {}
                        }
                    });
                }
            })
            .child(
                div()
                    .id("lang-picker-list")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .track_scroll(&scroll_handle)
                    .p_1()
                    .children(items),
            )
            .vertical_scrollbar(&scroll_handle);

        overlay.push(
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.))
                    .child(div().mt_1().occlude().child(popup_content)),
            )
            .with_priority(1)
            .into_any_element(),
        );
    }

    div()
        .relative()
        .child(trigger)
        .children(overlay)
        .into_any_element()
}
