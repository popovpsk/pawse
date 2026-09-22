use gpui::Hsla;
use gpui_component::theme::ThemeColor;

use super::color::{Oklch, to_hsla, to_oklch};

pub const LIGHT_COVER: f32 = 0.62;
pub const MIN_TINT_STRENGTH: f32 = 0.15;
const NEUTRAL_CHROMA: f32 = 0.02;
const DARK_BACKGROUND: (f32, f32) = (0.12, 0.27);
const LIGHT_BACKGROUND: (f32, f32) = (0.93, 0.99);
const MIN_SPAN: f32 = 0.5;
const MIN_BASE_SPAN: f32 = 0.05;
const FOREGROUND_LIMIT: (f32, f32) = (0.08, 0.97);
const LIGHTNESS_LIMIT: (f32, f32) = (0.02, 0.995);
const SURFACE_CHROMA_DARK: f32 = 0.022;
const SURFACE_CHROMA_LIGHT: f32 = 0.012;
const ACCENT_DARK: (f32, f32) = (0.78, 0.13);
const ACCENT_LIGHT: (f32, f32) = (0.52, 0.15);
const ACCENT_LIMIT: (f32, f32) = (0.25, 0.92);
const MAX_ACCENT_CHROMA: f32 = 0.2;
const ON_ACCENT_SPLIT: f32 = 0.55;
const ON_ACCENT_DARK: f32 = 0.22;
const ON_ACCENT_LIGHT: f32 = 0.98;
const ON_ACCENT_CHROMA: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub hue: f32,
    pub strength: f32,
}

impl Tint {
    pub fn none() -> Self {
        Self {
            hue: 0.,
            strength: 0.,
        }
    }

    pub fn faded(self) -> Self {
        Self {
            hue: self.hue,
            strength: 0.,
        }
    }
}

fn map_colors(colors: &ThemeColor, transform: impl Fn(Hsla) -> Hsla) -> ThemeColor {
    let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(colors) else {
        return *colors;
    };
    let mapped = fields
        .into_iter()
        .map(|(key, value)| {
            let color = value
                .as_str()
                .map(serde_json::Value::from)
                .and_then(|hex| serde_json::from_value::<Hsla>(hex).ok())
                .and_then(|color| serde_json::to_value(transform(color)).ok());
            match color {
                Some(color) => (key, color),
                None => (key, value),
            }
        })
        .collect();
    serde_json::from_value(serde_json::Value::Object(mapped)).unwrap_or(*colors)
}

pub fn anchors(base: &ThemeColor, cover_l: f32) -> (f32, f32) {
    let background = to_oklch(base.background).l;
    let foreground = to_oklch(base.foreground).l;
    let span = (foreground - background).abs().max(MIN_SPAN);
    if cover_l < LIGHT_COVER {
        let t = (cover_l / LIGHT_COVER).clamp(0., 1.);
        let bg = DARK_BACKGROUND.0 + (DARK_BACKGROUND.1 - DARK_BACKGROUND.0) * t;
        (bg, (bg + span).min(FOREGROUND_LIMIT.1))
    } else {
        let t = ((cover_l - LIGHT_COVER) / (1. - LIGHT_COVER)).clamp(0., 1.);
        let bg = LIGHT_BACKGROUND.0 + (LIGHT_BACKGROUND.1 - LIGHT_BACKGROUND.0) * t;
        (bg, (bg - span).max(FOREGROUND_LIMIT.0))
    }
}

pub fn relit(base: &ThemeColor, (bg, fg): (f32, f32)) -> ThemeColor {
    let from_bg = to_oklch(base.background).l;
    let span = to_oklch(base.foreground).l - from_bg;
    if span.abs() < MIN_BASE_SPAN {
        return *base;
    }
    map_colors(base, |color| {
        let source = to_oklch(color);
        let t = (source.l - from_bg) / span;
        to_hsla(
            Oklch {
                l: (bg + (fg - bg) * t).clamp(LIGHTNESS_LIMIT.0, LIGHTNESS_LIMIT.1),
                c: source.c,
                h: source.h,
            },
            color.a,
        )
    })
}

fn accent_tint(color: Hsla, hue: f32, anchor: Oklch, target: (f32, f32)) -> Hsla {
    let base = to_oklch(color);
    if base.c < NEUTRAL_CHROMA {
        return color;
    }
    let chroma = if anchor.c > NEUTRAL_CHROMA {
        target.1 * (base.c / anchor.c)
    } else {
        target.1
    };
    to_hsla(
        Oklch {
            l: (target.0 + base.l - anchor.l).clamp(ACCENT_LIMIT.0, ACCENT_LIMIT.1),
            c: chroma.min(MAX_ACCENT_CHROMA),
            h: hue,
        },
        color.a,
    )
}

fn on_accent(color: Hsla, hue: f32, accent_l: f32) -> Hsla {
    to_hsla(
        Oklch {
            l: if accent_l > ON_ACCENT_SPLIT {
                ON_ACCENT_DARK
            } else {
                ON_ACCENT_LIGHT
            },
            c: ON_ACCENT_CHROMA,
            h: hue,
        },
        color.a,
    )
}

fn surface_tint(color: Hsla, hue: f32, strength: f32, target: f32) -> Hsla {
    let base = to_oklch(color);
    let (from, to) = (base.h.to_radians(), hue.to_radians());
    let a = base.c * from.cos() + (target * to.cos() - base.c * from.cos()) * strength;
    let b = base.c * from.sin() + (target * to.sin() - base.c * from.sin()) * strength;
    to_hsla(
        Oklch {
            l: base.l,
            c: (a * a + b * b).sqrt(),
            h: b.atan2(a).to_degrees().rem_euclid(360.),
        },
        color.a,
    )
}

macro_rules! tint_accents {
    ($colors:expr, $hue:expr, $anchor:expr, $target:expr, $($field:ident),+ $(,)?) => {
        $($colors.$field = accent_tint($colors.$field, $hue, $anchor, $target);)+
    };
}

macro_rules! tint_on_accents {
    ($colors:expr, $hue:expr, $accent_l:expr, $($field:ident),+ $(,)?) => {
        $($colors.$field = on_accent($colors.$field, $hue, $accent_l);)+
    };
}

macro_rules! tint_surfaces {
    ($colors:expr, $hue:expr, $strength:expr, $target:expr, $($field:ident),+ $(,)?) => {
        $($colors.$field = surface_tint($colors.$field, $hue, $strength, $target);)+
    };
}

pub fn tinted(base: &ThemeColor, tint: Tint) -> ThemeColor {
    let mut colors = *base;
    if tint.strength <= 0. {
        return colors;
    }
    let light = to_oklch(base.background).l > LIGHT_COVER;
    let surface = if light {
        SURFACE_CHROMA_LIGHT
    } else {
        SURFACE_CHROMA_DARK
    };
    let target = if light { ACCENT_LIGHT } else { ACCENT_DARK };
    let anchor = to_oklch(base.primary);
    tint_on_accents!(
        colors,
        tint.hue,
        target.0,
        primary_foreground,
        button_primary_foreground,
        sidebar_primary_foreground,
    );
    tint_accents!(
        colors,
        tint.hue,
        anchor,
        target,
        primary,
        primary_active,
        primary_hover,
        button_primary,
        button_primary_active,
        button_primary_hover,
        ring,
        link,
        link_active,
        link_hover,
        caret,
        progress_bar,
        slider_bar,
        slider_thumb,
        switch,
        selection,
        list_active_border,
        table_active_border,
        drag_border,
        sidebar_primary,
    );
    tint_surfaces!(
        colors,
        tint.hue,
        tint.strength,
        surface,
        background,
        title_bar,
        title_bar_border,
        status_bar,
        status_bar_border,
        popover,
        secondary,
        secondary_active,
        secondary_hover,
        muted,
        accent,
        accordion,
        group_box,
        sidebar,
        sidebar_accent,
        sidebar_border,
        tab,
        tab_active,
        tab_bar,
        tab_bar_segmented,
        list,
        list_active,
        list_even,
        list_head,
        list_hover,
        table,
        table_active,
        table_even,
        table_head,
        table_hover,
        table_row_border,
        input,
        border,
        button,
        button_hover,
        button_active,
        button_secondary,
        button_secondary_active,
        button_secondary_hover,
        scrollbar,
        scrollbar_thumb,
        scrollbar_thumb_hover,
        skeleton,
        drop_target,
    );
    colors
}

#[cfg(test)]
mod tests {
    use super::super::color::{Oklch, to_hsla, to_oklch};
    use super::{ACCENT_DARK, MIN_SPAN, Tint, anchors, map_colors, relit, tinted};
    use gpui_component::theme::ThemeColor;

    fn dark_theme() -> ThemeColor {
        ThemeColor {
            background: to_hsla(
                Oklch {
                    l: 0.22,
                    c: 0.02,
                    h: 264.,
                },
                1.,
            ),
            foreground: to_hsla(
                Oklch {
                    l: 0.9,
                    c: 0.01,
                    h: 264.,
                },
                1.,
            ),
            primary: to_hsla(
                Oklch {
                    l: 0.66,
                    c: 0.13,
                    h: 264.,
                },
                1.,
            ),
            primary_foreground: to_hsla(
                Oklch {
                    l: 0.98,
                    c: 0.,
                    h: 0.,
                },
                1.,
            ),
            ..Default::default()
        }
    }

    fn light_theme() -> ThemeColor {
        let dark = dark_theme();
        relit(&dark, anchors(&dark, 0.95))
    }

    fn full(hue: f32) -> Tint {
        Tint { hue, strength: 1. }
    }

    #[test]
    fn every_token_survives_the_serde_walk() {
        let base = dark_theme();
        let walked = map_colors(&base, |color| color);
        let before = serde_json::to_value(base).expect("serialize");
        assert_eq!(before.as_object().expect("object").len(), 138);
        assert_eq!(before, serde_json::to_value(walked).expect("serialize"));
    }

    #[test]
    fn a_light_cover_turns_a_dark_theme_into_a_light_one() {
        let base = dark_theme();
        let light = relit(&base, anchors(&base, 0.9));
        assert!(to_oklch(light.background).l > 0.9);
        assert!(to_oklch(light.foreground).l < 0.45);
        assert!(to_oklch(base.background).l < 0.3);
    }

    #[test]
    fn a_generated_theme_keeps_the_contrast_it_started_with() {
        let base = dark_theme();
        let span = (to_oklch(base.foreground).l - to_oklch(base.background).l).abs();
        for cover_l in [0.05, 0.3, 0.55, 0.7, 0.99] {
            let generated = relit(&base, anchors(&base, cover_l));
            let contrast =
                (to_oklch(generated.foreground).l - to_oklch(generated.background).l).abs();
            assert!(
                contrast >= span.min(MIN_SPAN) - 0.02,
                "cover {cover_l}: contrast {contrast} vs {span}"
            );
        }
    }

    #[test]
    fn a_darker_cover_yields_a_darker_background() {
        let base = dark_theme();
        let deep = to_oklch(relit(&base, anchors(&base, 0.05)).background).l;
        let shallow = to_oklch(relit(&base, anchors(&base, 0.55)).background).l;
        assert!(deep < shallow, "{deep} should be darker than {shallow}");
    }

    #[test]
    fn relighting_keeps_the_relative_order_of_surfaces() {
        let base = dark_theme();
        let generated = relit(&base, anchors(&base, 0.9));
        let base_gap = to_oklch(base.muted).l - to_oklch(base.background).l;
        let generated_gap = to_oklch(generated.muted).l - to_oklch(generated.background).l;
        assert!(
            base_gap.signum() != generated_gap.signum(),
            "a flipped theme lifts surfaces the other way"
        );
    }

    #[test]
    fn relighting_leaves_a_theme_with_no_contrast_alone() {
        let flat = ThemeColor {
            background: to_hsla(
                Oklch {
                    l: 0.5,
                    c: 0.,
                    h: 0.,
                },
                1.,
            ),
            foreground: to_hsla(
                Oklch {
                    l: 0.52,
                    c: 0.,
                    h: 0.,
                },
                1.,
            ),
            ..Default::default()
        };
        let generated = relit(&flat, anchors(&flat, 0.9));
        assert_eq!(generated.background, flat.background);
    }

    #[test]
    fn a_full_tint_moves_the_accent_to_the_cover_hue() {
        let primary = to_oklch(tinted(&dark_theme(), full(30.)).primary);
        assert!((primary.h - 30.).abs() < 2.);
        assert!((primary.l - ACCENT_DARK.0).abs() < 0.02, "{}", primary.l);
        assert!(primary.c > 0.1, "a generated accent is never washed out");
    }

    #[test]
    fn a_weak_tint_still_uses_the_cover_hue_and_never_invents_one() {
        for strength in [0.2, 0.5, 0.8, 1.] {
            let tinted = tinted(&dark_theme(), Tint { hue: 85., strength });
            let primary = to_oklch(tinted.primary);
            assert!(
                (primary.h - 85.).abs() < 2.,
                "strength {strength}: hue {} drifted off the cover",
                primary.h
            );
        }
    }

    #[test]
    fn a_pale_theme_still_gets_a_punchy_accent() {
        let pale = ThemeColor {
            primary: to_hsla(
                Oklch {
                    l: 0.4,
                    c: 0.03,
                    h: 264.,
                },
                1.,
            ),
            ..dark_theme()
        };
        let primary = to_oklch(tinted(&pale, full(30.)).primary);
        assert!(
            primary.c > 0.09,
            "chroma {} should be lifted to the target",
            primary.c
        );
        assert!((primary.l - ACCENT_DARK.0).abs() < 0.02);
    }

    #[test]
    fn what_sits_on_the_accent_stays_readable_against_it() {
        for base in [dark_theme(), light_theme()] {
            let tinted = tinted(&base, full(85.));
            let primary = to_oklch(tinted.primary);
            let on_primary = to_oklch(tinted.primary_foreground);
            assert!(
                (primary.l - on_primary.l).abs() > 0.4,
                "accent {:.2} vs its text {:.2}",
                primary.l,
                on_primary.l
            );
        }
    }

    #[test]
    fn a_weak_tint_barely_moves_an_already_colourful_surface() {
        let saturated = ThemeColor {
            background: to_hsla(
                Oklch {
                    l: 0.22,
                    c: 0.073,
                    h: 290.,
                },
                1.,
            ),
            ..dark_theme()
        };
        let nudged = to_oklch(
            tinted(
                &saturated,
                Tint {
                    hue: 140.,
                    strength: 0.16,
                },
            )
            .background,
        );
        assert!(
            (nudged.h - 290.).abs() < 20.,
            "hue {} jumped off the theme on a 16% tint",
            nudged.h
        );
        let full = to_oklch(tinted(&saturated, full(140.)).background);
        assert!((full.h - 140.).abs() < 5., "hue {}", full.h);
    }

    #[test]
    fn strength_only_governs_how_far_the_surfaces_go() {
        let base = dark_theme();
        let weak = to_oklch(
            tinted(
                &base,
                Tint {
                    hue: 85.,
                    strength: 0.2,
                },
            )
            .background,
        );
        let strong = to_oklch(tinted(&base, full(85.)).background);
        assert!(
            weak.c < strong.c,
            "{} should be paler than {}",
            weak.c,
            strong.c
        );
        assert!((strong.h - 85.).abs() < 5., "hue {}", strong.h);
    }

    #[test]
    fn tinting_never_touches_body_text() {
        let base = dark_theme();
        let tinted = tinted(&base, full(30.));
        assert_eq!(tinted.foreground, base.foreground);
        assert_eq!(tinted.muted_foreground, base.muted_foreground);
        assert_eq!(tinted.danger, base.danger);
    }

    #[test]
    fn zero_strength_leaves_the_theme_alone() {
        let base = dark_theme();
        let tinted = tinted(&base, Tint::none());
        assert_eq!(tinted.primary, base.primary);
        assert_eq!(tinted.background, base.background);
    }
}
