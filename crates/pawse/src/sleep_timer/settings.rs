use gpui::{Anchor, AnyElement, App, IntoElement, ParentElement, SharedString, Styled, px};
use gpui_component::{
    Disableable, Sizable,
    button::Button,
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    switch::Switch,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};
use ui_resources::i18n::tools_strings;

use crate::settings_store::{
    SLEEP_TIMER_DURATIONS, SLEEP_TIMER_STEP_MIN, SettingsStore, SleepTimerSettings,
    notify_save_error,
};

use super::schedule::MINUTES_PER_DAY;

const MENU_MAX_HEIGHT: f32 = 320.;
pub const SETTINGS_ANCHOR: &str = "sleep-timer";

fn update(cx: &mut App, change: impl FnOnce(&mut SleepTimerSettings)) {
    let mut settings = cx.global::<SettingsStore>().sleep_timer();
    change(&mut settings);
    if let Err(e) = cx.global_mut::<SettingsStore>().set_sleep_timer(settings) {
        notify_save_error(cx, e);
    }
}

pub fn fade_item(id: &'static str) -> SettingItem {
    SettingItem::new(
        tools_strings().timer_fade.clone(),
        SettingField::render(move |_window, cx: &mut App| {
            let enabled = cx.global::<SettingsStore>().sleep_timer().fade_out;
            h_flex().items_center().justify_end().child(
                Switch::new(id)
                    .checked(enabled)
                    .on_click(|on, _, cx| update(cx, |s| s.fade_out = *on)),
            )
        }),
    )
    .description(tools_strings().timer_fade_desc.clone())
}

fn time_menu(menu: PopupMenu, current: u16, set: fn(&mut SleepTimerSettings, u16)) -> PopupMenu {
    (0..MINUTES_PER_DAY)
        .step_by(SLEEP_TIMER_STEP_MIN as usize)
        .fold(
            menu.max_h(px(MENU_MAX_HEIGHT)).scrollable(true),
            |menu, minute| {
                menu.item(
                    PopupMenuItem::new(SharedString::from(super::clock_label(minute)))
                        .checked(minute == current)
                        .on_click(move |_, _, cx| update(cx, |s| set(s, minute))),
                )
            },
        )
}

fn time_dropdown(
    id: &'static str,
    current: u16,
    enabled: bool,
    set: fn(&mut SleepTimerSettings, u16),
) -> AnyElement {
    Button::new(id)
        .small()
        .label(SharedString::from(super::clock_label(current)))
        .dropdown_caret(true)
        .disabled(!enabled)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            time_menu(menu, current, set)
        })
        .into_any_element()
}

fn duration_dropdown(current: u32, enabled: bool) -> AnyElement {
    let s = tools_strings();
    Button::new("sleep-timer-auto-duration")
        .small()
        .label(SharedString::from(s.timer_minutes(current)))
        .dropdown_caret(true)
        .disabled(!enabled)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            SLEEP_TIMER_DURATIONS
                .into_iter()
                .fold(menu, |menu, minutes| {
                    menu.item(
                        PopupMenuItem::new(SharedString::from(
                            tools_strings().timer_minutes(minutes),
                        ))
                        .checked(minutes == current)
                        .on_click(move |_, _, cx| update(cx, |s| s.auto_duration_min = minutes)),
                    )
                })
        })
        .into_any_element()
}

pub fn settings_group() -> SettingGroup {
    let s = tools_strings();
    SettingGroup::new()
        .anchor(SETTINGS_ANCHOR)
        .title(s.tools_timer.clone())
        .item(
            SettingItem::new(
                s.timer_auto.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let enabled = cx.global::<SettingsStore>().sleep_timer().auto;
                    h_flex().items_center().justify_end().child(
                        Switch::new("sleep-timer-auto")
                            .checked(enabled)
                            .on_click(|on, _, cx| update(cx, |s| s.auto = *on)),
                    )
                }),
            )
            .description(s.timer_auto_desc.clone()),
        )
        .item(SettingItem::new(
            s.timer_auto_from.clone(),
            SettingField::render(|_window, cx: &mut App| {
                let settings = cx.global::<SettingsStore>().sleep_timer();
                time_dropdown(
                    "sleep-timer-auto-from",
                    settings.auto_from_min,
                    settings.auto,
                    |s, minute| s.auto_from_min = minute,
                )
            }),
        ))
        .item(SettingItem::new(
            s.timer_auto_until.clone(),
            SettingField::render(|_window, cx: &mut App| {
                let settings = cx.global::<SettingsStore>().sleep_timer();
                time_dropdown(
                    "sleep-timer-auto-until",
                    settings.auto_until_min,
                    settings.auto,
                    |s, minute| s.auto_until_min = minute,
                )
            }),
        ))
        .item(SettingItem::new(
            s.timer_auto_duration.clone(),
            SettingField::render(|_window, cx: &mut App| {
                let settings = cx.global::<SettingsStore>().sleep_timer();
                duration_dropdown(settings.auto_duration_min, settings.auto)
            }),
        ))
        .item(fade_item("sleep-timer-fade-general"))
}
