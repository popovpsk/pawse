use std::rc::Rc;

use audio_output::{BitPerfectIssue, BitPerfectStatus};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, FontWeight, Hsla, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
};
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    v_flex,
};
use ui_resources::i18n::bit_perfect_strings;

use crate::audio_settings::NATIVE_RATE_WORDING;
use crate::services::Services;
use crate::theme_colors::Colors;

struct Explanation {
    intro: [SharedString; 2],
    status: SharedString,
    issues: Vec<IssueText>,
    footnote: Option<SharedString>,
}

struct IssueText {
    label: SharedString,
    details: Vec<SharedString>,
}

pub fn tooltip(status: &BitPerfectStatus) -> SharedString {
    let s = bit_perfect_strings();
    let mut text = String::new();
    if status.is_bit_perfect() {
        text.push_str(&s.bit_perfect);
    } else if nothing_loaded(status) {
        text.push_str(&s.no_source);
    } else {
        text.push_str(&s.not_bit_perfect);
        for issue in &status.issues {
            text.push_str("\n• ");
            text.push_str(&issue_label(issue));
        }
    }
    text.push('\n');
    text.push_str(&s.click_for_details);
    text.into()
}

pub fn open(window: &mut Window, cx: &mut App) {
    let status = cx.global::<Services>().output.bit_perfect_status();
    let explanation = Rc::new(explain(&status));
    window.open_dialog(cx, move |dialog, _window, cx| {
        let s = bit_perfect_strings();
        dialog
            .w(px(520.))
            .close_button(false)
            .title(s.about_title.clone())
            .child(body(&explanation, Colors::muted_foreground(cx)))
            .footer(
                DialogFooter::new().child(
                    Button::new("bit-perfect-ok")
                        .label(s.got_it.clone())
                        .primary()
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

fn body(explanation: &Explanation, muted: Hsla) -> impl IntoElement {
    v_flex()
        .gap_3()
        .children(
            explanation
                .intro
                .iter()
                .map(|text| div().child(text.clone())),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(explanation.status.clone()),
        )
        .children(explanation.issues.iter().map(|issue| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(issue.label.clone()),
                )
                .children(
                    issue
                        .details
                        .iter()
                        .map(|text| div().text_color(muted).child(text.clone())),
                )
        }))
        .when_some(explanation.footnote.clone(), |el, footnote| {
            el.child(div().text_sm().text_color(muted).child(footnote))
        })
}

fn explain(status: &BitPerfectStatus) -> Explanation {
    let s = bit_perfect_strings();
    let mode = if NATIVE_RATE_WORDING {
        &s.about_native_rate
    } else {
        &s.about_exclusive
    };
    let intro = [s.about.clone(), mode.clone()];
    if status.is_bit_perfect() || nothing_loaded(status) {
        let status = if status.is_bit_perfect() {
            s.status_ok.clone()
        } else {
            s.no_source_desc.clone()
        };
        return Explanation {
            intro,
            status,
            issues: Vec::new(),
            footnote: None,
        };
    }
    Explanation {
        intro,
        status: s.status_issues.clone(),
        issues: status
            .issues
            .iter()
            .map(|issue| IssueText {
                label: issue_label(issue),
                details: issue_details(issue),
            })
            .collect(),
        footnote: Some(s.not_an_error.clone()),
    }
}

fn nothing_loaded(status: &BitPerfectStatus) -> bool {
    matches!(status.issues.as_slice(), [BitPerfectIssue::NoSource])
}

fn issue_label(issue: &BitPerfectIssue) -> SharedString {
    let s = bit_perfect_strings();
    match issue {
        BitPerfectIssue::NotExclusive if NATIVE_RATE_WORDING => s.native_rate_off.clone(),
        BitPerfectIssue::NotExclusive => s.not_exclusive.clone(),
        BitPerfectIssue::SystemVolumeNotUnity { current } => {
            s.system_volume((current * 100.).round() as u32).into()
        }
        BitPerfectIssue::SystemMuted => s.system_muted.clone(),
        BitPerfectIssue::SampleRateMismatch { source, device } => {
            s.sample_rate(&khz(*source), &khz(*device)).into()
        }
        BitPerfectIssue::BitDepthExceedsContainer { source } => s.bit_depth(*source).into(),
        BitPerfectIssue::NoSource => s.no_source.clone(),
    }
}

fn issue_details(issue: &BitPerfectIssue) -> Vec<SharedString> {
    let s = bit_perfect_strings();
    match issue {
        BitPerfectIssue::NotExclusive => vec![s.not_exclusive_desc.clone()],
        BitPerfectIssue::SystemVolumeNotUnity { .. } => vec![
            s.system_volume_desc.clone(),
            s.system_volume_caution.clone(),
        ],
        BitPerfectIssue::SystemMuted => vec![s.system_muted_desc.clone()],
        BitPerfectIssue::SampleRateMismatch { .. } => vec![s.sample_rate_desc.clone()],
        BitPerfectIssue::BitDepthExceedsContainer { .. } => vec![s.bit_depth_desc.clone()],
        BitPerfectIssue::NoSource => vec![s.no_source_desc.clone()],
    }
}

fn khz(hz: u32) -> String {
    if hz.is_multiple_of(1000) {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f32 / 1000.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rates_read_as_khz() {
        assert_eq!(khz(48_000), "48 kHz");
        assert_eq!(khz(44_100), "44.1 kHz");
        assert_eq!(khz(352_800), "352.8 kHz");
    }

    #[test]
    fn tooltip_names_each_issue_and_points_to_details() {
        let status = BitPerfectStatus {
            issues: vec![
                BitPerfectIssue::SystemVolumeNotUnity { current: 0.5 },
                BitPerfectIssue::SampleRateMismatch {
                    source: 44_100,
                    device: 48_000,
                },
            ],
        };
        assert_eq!(
            tooltip(&status).to_string(),
            "Not bit-perfect\n\
             • System volume is at 50%, not 100%\n\
             • The track is 44.1 kHz, but the device runs at 48 kHz\n\
             Click for details"
        );
    }

    #[test]
    fn nothing_loaded_is_not_reported_as_a_problem() {
        let status = BitPerfectStatus {
            issues: vec![BitPerfectIssue::NoSource],
        };
        assert_eq!(
            tooltip(&status).to_string(),
            "Nothing is playing yet\nClick for details"
        );
        let explanation = explain(&status);
        assert!(explanation.issues.is_empty());
        assert!(explanation.footnote.is_none());
    }
}
