use gpui::{
    App, AppContext, Context, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    Disableable, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex,
    theme::ThemeRegistry,
    v_flex,
};

use crate::localization::tr;
use crate::settings_store::{SettingsStore, notify_save_error};
use crate::settings_view::{
    LangPickerState, ThemePickerState, lang_picker_dropdown, pick_and_add_folder,
    remove_folder_and_rescan, theme_picker_dropdown,
};
use crate::theme_colors::Colors;

pub struct OnboardingView {
    theme_picker: Entity<ThemePickerState>,
    lang_picker: Entity<LangPickerState>,
    focus_handle: FocusHandle,
    _theme_picker_subscription: Subscription,
    _lang_picker_subscription: Subscription,
    _theme_registry_subscription: Subscription,
    _settings_observer: Subscription,
}

impl OnboardingView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_picker: Entity<ThemePickerState> = cx.new(|cx| ThemePickerState::new(cx));
        let lang_picker: Entity<LangPickerState> = cx.new(|cx| LangPickerState::new(cx));
        let focus_handle = cx.focus_handle();

        let theme_picker_subscription = cx.observe(&theme_picker, |_, _, cx| cx.notify());
        let lang_picker_subscription = cx.observe(&lang_picker, |_, _, cx| cx.notify());

        let theme_registry_subscription = cx.observe_global::<ThemeRegistry>({
            let theme_picker = theme_picker.clone();
            move |_, cx| {
                theme_picker.update(cx, |state, cx| {
                    state.options = ThemePickerState::build_options(cx);
                    cx.notify();
                });
            }
        });

        let settings_observer = cx.observe_global::<SettingsStore>(|_, cx| cx.notify());

        Self {
            theme_picker,
            lang_picker,
            focus_handle,
            _theme_picker_subscription: theme_picker_subscription,
            _lang_picker_subscription: lang_picker_subscription,
            _theme_registry_subscription: theme_registry_subscription,
            _settings_observer: settings_observer,
        }
    }
}

fn finish_onboarding(window: &mut Window, cx: &mut App) {
    if let Err(e) = cx
        .global_mut::<SettingsStore>()
        .set_onboarding_complete(true)
    {
        notify_save_error(cx, e);
        return;
    }
    crate::open_main_window(cx, false);
    window.remove_window();
}

fn section(label: SharedString, field: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Colors::foreground(cx))
                .child(label),
        )
        .child(field)
}

impl Render for OnboardingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = tr();
        let folders = cx.global::<SettingsStore>().music_folders().to_vec();
        let can_finish = !folders.is_empty();

        let mut folder_list = v_flex().gap_2().w_full();
        if folders.is_empty() {
            folder_list = folder_list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(Colors::muted_foreground(cx))
                    .child(s.no_folders_added.clone()),
            );
        } else {
            for path in &folders {
                let path_text: SharedString = path.to_string_lossy().into_owned().into();
                let path_for_remove = path.clone();
                let remove_id = format!("ob-remove-{}", path.display());
                folder_list = folder_list.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .bg(Colors::muted(cx))
                        .child(Icon::new(IconName::Folder).text_color(Colors::muted_foreground(cx)))
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .truncate()
                                .text_color(Colors::foreground(cx))
                                .child(path_text),
                        )
                        .child(
                            Button::new(SharedString::from(remove_id))
                                .ghost()
                                .label(s.remove.clone())
                                .on_click(move |_, _, cx| {
                                    remove_folder_and_rescan(path_for_remove.clone(), cx)
                                }),
                        ),
                );
            }
        }

        let folder_field = v_flex().gap_3().w_full().child(folder_list).child(
            Button::new("ob-add-folder")
                .label(s.add_folder.clone())
                .on_click(|_, _, cx| pick_and_add_folder(cx)),
        );

        div()
            .id("onboarding")
            .size_full()
            .overflow_hidden()
            .bg(Colors::title_bar(cx))
            .flex()
            .items_center()
            .justify_center()
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .w(px(460.))
                    .gap_5()
                    .p_8()
                    .rounded(px(12.))
                    .bg(Colors::background(cx))
                    .border_1()
                    .border_color(Colors::border(cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Colors::foreground(cx))
                                    .child(s.onboarding_title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(Colors::muted_foreground(cx))
                                    .child(s.onboarding_subtitle.clone()),
                            ),
                    )
                    .child(section(
                        s.onboarding_theme_prompt.clone(),
                        theme_picker_dropdown(self.theme_picker.clone(), window, cx),
                        cx,
                    ))
                    .child(section(
                        s.onboarding_language_prompt.clone(),
                        lang_picker_dropdown(self.lang_picker.clone(), window, cx),
                        cx,
                    ))
                    .child(section(
                        s.onboarding_folder_prompt.clone(),
                        folder_field,
                        cx,
                    ))
                    .child(
                        h_flex().justify_end().child(
                            Button::new("ob-finish")
                                .primary()
                                .label(s.onboarding_finish.clone())
                                .disabled(!can_finish)
                                .on_click(|_, window, cx| finish_onboarding(window, cx)),
                        ),
                    ),
            )
    }
}
