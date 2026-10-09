use gpui::{
    Anchor, App, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
    rems,
};
use gpui_component::{
    Icon, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    popover::{Popover, PopoverState},
    v_flex,
};
use ui_resources::i18n::similar_strings;

use super::Familiarity;
use super::actions::{OnApplied, mix_queue, start_radio};
use crate::library_views::view_menu::{
    MenuColors, ROW_HEIGHT, menu_surface, save, section_label, separator,
};
use crate::panel_header::{BUTTON_SIZE, ICON_SIZE};
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

pub fn queue_menu(on_applied: OnApplied, scale: f32, cx: &App) -> impl IntoElement {
    Popover::new("similar-queue-menu")
        .anchor(Anchor::TopRight)
        .appearance(false)
        .trigger(
            Button::new("similar-queue-menu-trigger")
                .ghost()
                .compact()
                .rounded_full()
                .w(rems(BUTTON_SIZE / 16.))
                .h(rems(BUTTON_SIZE / 16.))
                .icon(
                    Icon::default()
                        .path("icons/s1-similar.svg")
                        .size(rems(ICON_SIZE / 16.))
                        .text_color(Colors::muted_foreground(cx)),
                )
                .tooltip(similar_strings().menu.clone()),
        )
        .content(move |_, _, cx| content(&on_applied, scale, cx))
}

fn content(
    on_applied: &OnApplied,
    scale: f32,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<> {
    let s = similar_strings();
    let colors = MenuColors::from_cx(cx);
    let has_current = cx
        .global::<Services>()
        .playback_queue
        .borrow()
        .current_track()
        .is_some();
    let familiarity = cx.global::<SettingsStore>().similar_familiarity();
    let radio = on_applied.clone();
    let mix = on_applied.clone();
    menu_surface("similar-queue-menu-content", scale, cx)
        .child(action_row(
            "similar-radio",
            s.radio.clone(),
            s.radio_hint.clone(),
            has_current,
            colors,
            cx.listener(move |state, _: &ClickEvent, window, cx| {
                state.dismiss(window, cx);
                start_radio(radio.clone(), cx);
            }),
        ))
        .child(action_row(
            "similar-mix",
            s.mix.clone(),
            s.mix_hint.clone(),
            true,
            colors,
            cx.listener(move |state, _: &ClickEvent, window, cx| {
                state.dismiss(window, cx);
                mix_queue(mix.clone(), cx);
            }),
        ))
        .child(separator(colors))
        .child(section_label(s.tracks.clone(), colors))
        .child(familiarity_switch(familiarity))
}

fn action_row(
    id: &'static str,
    label: SharedString,
    hint: SharedString,
    enabled: bool,
    colors: MenuColors,
    run: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover = colors.hover;
    v_flex()
        .id(id)
        .min_h(px(ROW_HEIGHT))
        .px_2p5()
        .py_1()
        .rounded(px(6.))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(label),
        )
        .child(div().text_xs().text_color(colors.muted_fg).child(hint))
        .map(|row| {
            if enabled {
                row.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(run)
            } else {
                row.opacity(0.5)
            }
        })
}

fn familiarity_switch(current: Familiarity) -> impl IntoElement {
    const ORDER: [Familiarity; 3] = [Familiarity::Familiar, Familiarity::Any, Familiarity::New];
    let s = similar_strings();
    h_flex().w_full().px_1().pb_1().child(
        ButtonGroup::new("similar-familiarity")
            .small()
            .w_full()
            .child(
                Button::new("similar-familiar")
                    .flex_1()
                    .label(s.familiar.clone())
                    .selected(current == Familiarity::Familiar),
            )
            .child(
                Button::new("similar-any")
                    .flex_1()
                    .label(s.any.clone())
                    .selected(current == Familiarity::Any),
            )
            .child(
                Button::new("similar-new")
                    .flex_1()
                    .label(s.unheard.clone())
                    .selected(current == Familiarity::New),
            )
            .on_click(|clicks: &Vec<usize>, _, cx| {
                let Some(&familiarity) = clicks.first().and_then(|&ix| ORDER.get(ix)) else {
                    return;
                };
                save(cx, |s| s.set_similar_familiarity(familiarity));
            }),
    )
}
