use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    Disableable, Sizable, h_flex,
    input::{InputState, NumberInput},
    slider::Slider,
    switch::Switch,
    time_field::TimeField,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};
use ui_resources::i18n::tools_strings;

use crate::settings_store::{SettingsStore, SleepTimerSettings, notify_save_error};
use crate::theme_colors::Colors;

use super::controls::SleepTimerControls;

pub const SETTINGS_ANCHOR: &str = "sleep-timer";
const DURATION_INPUT_WIDTH: f32 = 112.;
const FADE_SLIDER_WIDTH: f32 = 160.;
const FADE_LABEL_MIN_WIDTH: f32 = 48.;

pub(super) fn update(cx: &mut App, change: impl FnOnce(&mut SleepTimerSettings)) {
    let mut settings = cx.global::<SettingsStore>().sleep_timer();
    change(&mut settings);
    if let Err(e) = cx.global_mut::<SettingsStore>().set_sleep_timer(settings) {
        notify_save_error(cx, e);
    }
}

pub fn duration_field(input: &Entity<InputState>, disabled: bool, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .w(px(DURATION_INPUT_WIDTH))
                .child(NumberInput::new(input).small().disabled(disabled)),
        )
        .child(
            div()
                .text_sm()
                .text_color(Colors::muted_foreground(cx))
                .child(tools_strings().timer_minutes_unit.clone()),
        )
        .into_any_element()
}

pub fn fade_item(id: &'static str, controls: Entity<SleepTimerControls>) -> SettingItem {
    SettingItem::new(
        tools_strings().timer_fade.clone(),
        SettingField::render(move |_window, cx: &mut App| {
            let enabled = cx.global::<SettingsStore>().sleep_timer().fade_out;
            let (slider, label) = {
                let controls = controls.read(cx);
                (controls.fade.clone(), controls.fade_label())
            };
            h_flex()
                .gap_3()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .min_w(px(FADE_LABEL_MIN_WIDTH))
                        .text_right()
                        .text_sm()
                        .text_color(Colors::muted_foreground(cx))
                        .child(label),
                )
                .child(
                    div()
                        .w(px(FADE_SLIDER_WIDTH))
                        .child(Slider::new(&slider).disabled(!enabled)),
                )
                .child(
                    Switch::new(id)
                        .checked(enabled)
                        .on_click(|on, _, cx| update(cx, |s| s.fade_out = *on)),
                )
        }),
    )
    .description(tools_strings().timer_fade_desc.clone())
}

fn auto_item(controls: Entity<SleepTimerControls>) -> SettingItem {
    SettingItem::new(
        tools_strings().timer_auto.clone(),
        SettingField::render(move |_window, cx: &mut App| {
            let enabled = cx.global::<SettingsStore>().sleep_timer().auto;
            let (from, until) = {
                let controls = controls.read(cx);
                (controls.from.clone(), controls.until.clone())
            };
            h_flex()
                .gap_2()
                .items_center()
                .justify_end()
                .child(TimeField::new(&from).small().disabled(!enabled))
                .child(
                    div()
                        .text_sm()
                        .text_color(Colors::muted_foreground(cx))
                        .child("–"),
                )
                .child(TimeField::new(&until).small().disabled(!enabled))
                .child(
                    div().pl_2().child(
                        Switch::new("sleep-timer-auto")
                            .checked(enabled)
                            .on_click(|on, _, cx| update(cx, |s| s.auto = *on)),
                    ),
                )
        }),
    )
    .description(tools_strings().timer_auto_desc.clone())
}

fn duration_item(controls: Entity<SleepTimerControls>) -> SettingItem {
    SettingItem::new(
        tools_strings().timer_auto_duration.clone(),
        SettingField::render(move |_window, cx: &mut App| {
            let enabled = cx.global::<SettingsStore>().sleep_timer().auto;
            let input = controls.read(cx).auto_duration.clone();
            duration_field(&input, !enabled, cx)
        }),
    )
}

pub fn settings_group(controls: Entity<SleepTimerControls>) -> SettingGroup {
    SettingGroup::new()
        .anchor(SETTINGS_ANCHOR)
        .title(tools_strings().tools_timer.clone())
        .item(auto_item(controls.clone()))
        .item(duration_item(controls.clone()))
        .item(fade_item("sleep-timer-fade-general", controls))
}
