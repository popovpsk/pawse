use gpui::{App, ParentElement, SharedString, Styled};
use gpui_component::{
    Selectable, Sizable,
    button::{Button, ButtonGroup},
    h_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};
use ui_resources::i18n::spectrum_strings;

use crate::settings_store::{
    SettingsStore, SpectrumFrameRate, SpectrumSmoothing, SpectrumStyle, SpectrumTilt,
    notify_save_error,
};

const STYLES: [SpectrumStyle; 4] = [
    SpectrumStyle::Cover,
    SpectrumStyle::Silhouette,
    SpectrumStyle::Zones,
    SpectrumStyle::Analyzer,
];
const TILTS: [SpectrumTilt; 3] = [
    SpectrumTilt::Off,
    SpectrumTilt::Gentle,
    SpectrumTilt::Standard,
];
const FRAME_RATES: [SpectrumFrameRate; 3] = [
    SpectrumFrameRate::HighRefresh,
    SpectrumFrameRate::Smooth,
    SpectrumFrameRate::Efficient,
];
const SMOOTHINGS: [SpectrumSmoothing; 4] = [
    SpectrumSmoothing::Auto,
    SpectrumSmoothing::Third,
    SpectrumSmoothing::Sixth,
    SpectrumSmoothing::Twelfth,
];

fn style_label(style: SpectrumStyle) -> SharedString {
    let s = spectrum_strings();
    match style {
        SpectrumStyle::Cover => s.style_cover.clone(),
        SpectrumStyle::Silhouette => s.style_silhouette.clone(),
        SpectrumStyle::Zones => s.style_zones.clone(),
        SpectrumStyle::Analyzer => s.style_analyzer.clone(),
    }
}

fn tilt_label(tilt: SpectrumTilt) -> SharedString {
    match tilt {
        SpectrumTilt::Off => spectrum_strings().tilt_off.clone(),
        SpectrumTilt::Gentle => SharedString::new_static("3 dB"),
        SpectrumTilt::Standard => SharedString::new_static("4.5 dB"),
    }
}

fn smoothing_label(smoothing: SpectrumSmoothing) -> SharedString {
    match smoothing {
        SpectrumSmoothing::Auto => spectrum_strings().smoothing_auto.clone(),
        SpectrumSmoothing::Third => SharedString::new_static("1/3"),
        SpectrumSmoothing::Sixth => SharedString::new_static("1/6"),
        SpectrumSmoothing::Twelfth => SharedString::new_static("1/12"),
    }
}

fn frame_rate_label(rate: SpectrumFrameRate) -> SharedString {
    match rate {
        SpectrumFrameRate::HighRefresh => SharedString::new_static("120 fps"),
        SpectrumFrameRate::Smooth => SharedString::new_static("60 fps"),
        SpectrumFrameRate::Efficient => SharedString::new_static("30 fps"),
    }
}

fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &'static [T],
    current: T,
    label: fn(T) -> SharedString,
    save: fn(&mut SettingsStore, T) -> anyhow::Result<()>,
) -> ButtonGroup {
    let mut group = ButtonGroup::new(id).small();
    for (ix, &option) in options.iter().enumerate() {
        group = group.child(
            Button::new((id, ix))
                .label(label(option))
                .selected(option == current),
        );
    }
    group.on_click(move |clicks: &Vec<usize>, _, cx| {
        let Some(&option) = clicks.first().and_then(|&ix| options.get(ix)) else {
            return;
        };
        if let Err(e) = save(cx.global_mut::<SettingsStore>(), option) {
            notify_save_error(cx, e);
        }
    })
}

pub fn settings_group() -> SettingGroup {
    let s = spectrum_strings();
    SettingGroup::new()
        .title(s.title.clone())
        .description(s.about.clone())
        .item(
            SettingItem::new(
                s.style.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().spectrum_style();
                    h_flex().items_center().justify_end().child(choice(
                        "spectrum-style",
                        &STYLES,
                        current,
                        style_label,
                        SettingsStore::set_spectrum_style,
                    ))
                }),
            )
            .description(s.style_desc.clone()),
        )
        .item(
            SettingItem::new(
                s.tilt.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().spectrum_tilt();
                    h_flex().items_center().justify_end().child(choice(
                        "spectrum-tilt",
                        &TILTS,
                        current,
                        tilt_label,
                        SettingsStore::set_spectrum_tilt,
                    ))
                }),
            )
            .description(s.tilt_desc.clone()),
        )
        .item(
            SettingItem::new(
                s.smoothing.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().spectrum_smoothing();
                    h_flex().items_center().justify_end().child(choice(
                        "spectrum-smoothing",
                        &SMOOTHINGS,
                        current,
                        smoothing_label,
                        SettingsStore::set_spectrum_smoothing,
                    ))
                }),
            )
            .description(s.smoothing_desc.clone()),
        )
        .item(
            SettingItem::new(
                s.frame_rate.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().spectrum_frame_rate();
                    h_flex().items_center().justify_end().child(choice(
                        "spectrum-frame-rate",
                        &FRAME_RATES,
                        current,
                        frame_rate_label,
                        SettingsStore::set_spectrum_frame_rate,
                    ))
                }),
            )
            .description(s.frame_rate_desc.clone()),
        )
}
