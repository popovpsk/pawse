use gpui::{App, Entity, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonGroup},
    h_flex,
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
    cx.global::<Services>().remote_media.set_cache_limit(bytes);
}

pub fn cache_group(sources: Entity<LibrarySources>) -> SettingGroup {
    let limit_labels: Vec<SharedString> = NETWORK_CACHE_CHOICES_GB
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
        .description(tr().network_cache_desc.clone())
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
                    NETWORK_CACHE_CHOICES_GB
                        .iter()
                        .zip(&limit_labels)
                        .enumerate()
                        .fold(
                            ButtonGroup::new("network-cache-limit").small(),
                            |group, (ix, (&gb, label))| {
                                group.child(
                                    Button::new(("network-cache-limit", ix))
                                        .label(label.clone())
                                        .selected(current == gb),
                                )
                            },
                        )
                        .on_click(|clicks: &Vec<usize>, _, cx| {
                            let Some(&gb) = clicks
                                .first()
                                .and_then(|&ix| NETWORK_CACHE_CHOICES_GB.get(ix))
                            else {
                                return;
                            };
                            if let Err(e) =
                                cx.global_mut::<SettingsStore>().set_network_cache_gb(gb)
                            {
                                notify_save_error(cx, e);
                            }
                            apply_cache_limit(cx);
                            cx.refresh_windows();
                        })
                        .into_any_element()
                }),
            )
            .description(tr().cache_limit_desc.clone()),
        )
}
