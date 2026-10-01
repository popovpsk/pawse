use gpui::{
    AnyElement, App, Axis, Entity, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder,
};
use gpui_component::{
    Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage};
use ui_resources::i18n::tools_strings;

use crate::localization::tr;
use crate::settings_store::{SLEEP_TIMER_DURATIONS, SettingsStore};
use crate::settings_view::OpenSleepTimerSettings;
use crate::sleep_timer::controls::SleepTimerControls;
use crate::sleep_timer::settings::duration_field;
use crate::sleep_timer::{Armed, EXTEND_MIN, SleepTimer, clock_label};
use crate::theme_colors::Colors;

fn status_field(timer: Entity<SleepTimer>, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let (armed, status) = {
        let t = timer.read(cx);
        (t.armed(), t.status())
    };
    let mut row = h_flex().gap_2().items_center().child(
        div()
            .flex_1()
            .text_sm()
            .text_color(if armed.is_some() {
                Colors::foreground(cx)
            } else {
                Colors::muted_foreground(cx)
            })
            .child(status),
    );
    if matches!(armed, Some(Armed::Until { .. })) {
        let timer = timer.clone();
        row = row.child(
            Button::new("sleep-timer-extend")
                .small()
                .label(SharedString::from(s.timer_extend(EXTEND_MIN)))
                .on_click(move |_, _, cx| timer.update(cx, |t, cx| t.extend(EXTEND_MIN, cx))),
        );
    }
    if armed.is_some() {
        row = row.child(
            Button::new("sleep-timer-cancel")
                .small()
                .label(s.timer_cancel.clone())
                .on_click(move |_, _, cx| timer.update(cx, |t, cx| t.cancel(cx))),
        );
    }
    row.into_any_element()
}

fn presets_field(
    timer: Entity<SleepTimer>,
    controls: Entity<SleepTimerControls>,
    cx: &mut App,
) -> AnyElement {
    let s = tools_strings();
    let end_of_track = timer.read(cx).armed() == Some(Armed::EndOfTrack);
    let mut row = h_flex().gap_2().flex_wrap();
    for (ix, minutes) in SLEEP_TIMER_DURATIONS.into_iter().enumerate() {
        let timer = timer.clone();
        row = row.child(
            Button::new(("sleep-timer-preset", ix))
                .small()
                .label(SharedString::from(s.timer_minutes(minutes)))
                .on_click(move |_, _, cx| timer.update(cx, |t, cx| t.start(minutes, cx))),
        );
    }
    let end_timer = timer.clone();
    row = row.child(
        Button::new("sleep-timer-end-of-track")
            .small()
            .label(s.timer_end_of_track.clone())
            .when(end_of_track, |b| b.primary())
            .on_click(move |_, _, cx| end_timer.update(cx, |t, cx| t.start_end_of_track(cx))),
    );
    let input = controls.read(cx).manual_duration.clone();
    let custom = h_flex()
        .gap_2()
        .items_center()
        .child(duration_field(&input, false, cx))
        .child(
            Button::new("sleep-timer-custom-start")
                .small()
                .label(s.timer_run.clone())
                .on_click(move |_, _, cx| {
                    let minutes = controls.read(cx).manual_minutes();
                    timer.update(cx, |t, cx| t.start(minutes, cx));
                }),
        );
    v_flex().gap_2().child(row).child(custom).into_any_element()
}

fn auto_field(cx: &mut App) -> AnyElement {
    let settings = cx.global::<SettingsStore>().sleep_timer();
    let state = if settings.auto {
        SharedString::from(format!(
            "{}–{}",
            clock_label(settings.auto_from_min),
            clock_label(settings.auto_until_min)
        ))
    } else {
        tools_strings().timer_off.clone()
    };
    h_flex()
        .gap_2()
        .items_center()
        .justify_end()
        .child(
            div()
                .text_sm()
                .text_color(Colors::muted_foreground(cx))
                .child(state),
        )
        .child(
            Button::new("sleep-timer-auto-settings")
                .small()
                .label(tr().settings.clone())
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(OpenSleepTimerSettings), cx);
                }),
        )
        .into_any_element()
}

pub fn page(timer: Entity<SleepTimer>, controls: Entity<SleepTimerControls>) -> SettingPage {
    let s = tools_strings();
    let status_timer = timer.clone();
    let presets_controls = controls.clone();
    SettingPage::new(s.tools_timer.clone()).group(
        SettingGroup::new()
            .title(s.tools_timer.clone())
            .description(s.timer_intro.clone())
            .item(SettingItem::unlabeled(SettingField::render(
                move |_window, cx: &mut App| status_field(status_timer.clone(), cx),
            )))
            .item(
                SettingItem::new(
                    s.timer_start.clone(),
                    SettingField::render(move |_window, cx: &mut App| {
                        presets_field(timer.clone(), presets_controls.clone(), cx)
                    }),
                )
                .layout(Axis::Vertical),
            )
            .item(crate::sleep_timer::settings::fade_item(
                "sleep-timer-fade-tools",
                controls,
            ))
            .item(SettingItem::new(
                s.timer_auto.clone(),
                SettingField::render(|_window, cx: &mut App| auto_field(cx)),
            )),
    )
}
