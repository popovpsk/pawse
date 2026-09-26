use std::rc::Rc;

use gpui::{Anchor, App, Entity, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Disableable, Sizable,
    button::Button,
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};

use crate::library_sources::LibrarySources;
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::{
    NETWORK_CACHE_CHOICES_GB, SettingsStore, UNLIMITED_CACHE_GB, notify_save_error,
};
use crate::theme_colors::Colors;

pub fn apply_cache_limit(cx: &App) {
    let bytes = cx.global::<SettingsStore>().network_cache_bytes();
    cx.global::<Services>()
        .remote_media
        .set_cache_limit(bytes, cx.background_executor());
}

fn set_limit(gb: u32, cx: &mut App) {
    if let Err(e) = cx.global_mut::<SettingsStore>().set_network_cache_gb(gb) {
        notify_save_error(cx, e);
    }
    apply_cache_limit(cx);
    cx.refresh_windows();
}

pub fn cache_group(sources: Entity<LibrarySources>) -> SettingGroup {
    let limit_labels: Rc<[SharedString]> = NETWORK_CACHE_CHOICES_GB
        .iter()
        .map(|&gb| {
            if gb == UNLIMITED_CACHE_GB {
                tr().cache_unlimited.clone()
            } else {
                tr().size(u64::from(gb) * 1024 * 1024 * 1024).into()
            }
        })
        .collect();
    SettingGroup::new()
        .title(tr().network_cache.clone())
        .item(SettingItem::new(
            tr().cache_used.clone(),
            SettingField::render(move |_window, cx: &mut App| {
                let state = sources.read(cx);
                let clearing = state.clearing_cache();
                let used = state.cache_bytes();
                let label = state.cache_label();
                let sources = sources.clone();
                h_flex()
                    .gap_3()
                    .items_center()
                    .children(label.map(|label| {
                        div()
                            .text_sm()
                            .text_color(Colors::muted_foreground(cx))
                            .child(label)
                    }))
                    .child(
                        Button::new("network-cache-clear")
                            .small()
                            .label(tr().cache_clear.clone())
                            .loading(clearing)
                            .disabled(clearing || used == Some(0))
                            .on_click(move |_, _, cx| {
                                sources.update(cx, |sources, cx| sources.clear_cache(cx));
                            }),
                    )
            }),
        ))
        .item(
            SettingItem::new(
                tr().cache_limit.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().network_cache_gb();
                    let label = NETWORK_CACHE_CHOICES_GB
                        .iter()
                        .position(|&gb| gb == current)
                        .and_then(|ix| limit_labels.get(ix))
                        .cloned()
                        .unwrap_or_default();
                    let labels = limit_labels.clone();
                    Button::new("network-cache-limit")
                        .small()
                        .label(label)
                        .dropdown_caret(true)
                        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
                            let current = cx.global::<SettingsStore>().network_cache_gb();
                            NETWORK_CACHE_CHOICES_GB.iter().zip(labels.iter()).fold(
                                menu,
                                |menu, (&gb, label)| {
                                    menu.item(
                                        PopupMenuItem::new(label.clone())
                                            .checked(current == gb)
                                            .on_click(move |_, _, cx| set_limit(gb, cx)),
                                    )
                                },
                            )
                        })
                        .into_any_element()
                }),
            )
            .description(tr().cache_limit_desc.clone()),
        )
}
