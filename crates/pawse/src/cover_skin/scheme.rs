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
const TEXT_CHROMA_DARK: f32 = 0.032;
const TEXT_CHROMA_LIGHT: f32 = 0.022;
const RATIO_LIMIT: (f32, f32) = (0.6, 1.8);
const FLAT_CHROMA: f32 = 0.004;
const CHROMA_DIP: f32 = 0.5;
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

fn as_color(value: &serde_json::Value) -> Option<Hsla> {
    let hex = serde_json::Value::from(value.as_str()?);
    serde_json::from_value::<Hsla>(hex).ok()
}

fn map_colors(colors: &ThemeColor, transform: impl Fn(Hsla) -> Hsla) -> ThemeColor {
    let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(colors) else {
        return *colors;
    };
    let mapped = fields
        .into_iter()
        .map(|(key, value)| {
            let color =
                as_color(&value).and_then(|color| serde_json::to_value(transform(color)).ok());
            match color {
                Some(color) => (key, color),
                None => (key, value),
            }
        })
        .collect();
    serde_json::from_value(serde_json::Value::Object(mapped)).unwrap_or(*colors)
}

fn mixed(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (start, end) = (to_oklch(from), to_oklch(to));
    let (start_radians, end_radians) = (start.h.to_radians(), end.h.to_radians());
    let (start_a, start_b) = (start.c * start_radians.cos(), start.c * start_radians.sin());
    let (end_a, end_b) = (end.c * end_radians.cos(), end.c * end_radians.sin());
    let a = start_a + (end_a - start_a) * t;
    let b = start_b + (end_b - start_b) * t;
    to_hsla(
        Oklch {
            l: start.l + (end.l - start.l) * t,
            c: (a * a + b * b).sqrt(),
            h: b.atan2(a).to_degrees().rem_euclid(360.),
        },
        from.a + (to.a - from.a) * t,
    )
}

pub fn mix(from: &ThemeColor, to: &ThemeColor, t: f32) -> ThemeColor {
    let (Ok(serde_json::Value::Object(source)), Ok(serde_json::Value::Object(target))) =
        (serde_json::to_value(from), serde_json::to_value(to))
    else {
        return *to;
    };
    let mapped = source
        .into_iter()
        .map(|(key, value)| {
            let color = as_color(&value)
                .zip(target.get(&key).and_then(as_color))
                .map(|(from, to)| mixed(from, to, t))
                .and_then(|color| serde_json::to_value(color).ok());
            match color {
                Some(color) => (key, color),
                None => (key, value),
            }
        })
        .collect();
    serde_json::from_value(serde_json::Value::Object(mapped)).unwrap_or(*to)
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

fn blend(color: Hsla, hue: f32, strength: f32, target: f32, anchor: f32) -> Hsla {
    let base = to_oklch(color);
    let goal = if anchor < NEUTRAL_CHROMA {
        target
    } else {
        target * (base.c / anchor).clamp(RATIO_LIMIT.0, RATIO_LIMIT.1)
    };
    let mut delta = (hue - base.h).rem_euclid(360.);
    if delta > 180. {
        delta -= 360.;
    }
    let carried = (base.c / FLAT_CHROMA).clamp(0., 1.);
    let rotation = strength + (1. - strength) * (1. - carried);
    let travel = delta.abs() / 180. * carried;
    let dip = 1. - CHROMA_DIP * travel * (std::f32::consts::PI * strength).sin().max(0.);
    to_hsla(
        Oklch {
            l: base.l,
            c: (base.c + (goal - base.c) * strength) * dip,
            h: (base.h + delta * rotation).rem_euclid(360.),
        },
        color.a,
    )
}

pub fn matches(a: &ThemeColor, b: &ThemeColor) -> bool {
    a.background == b.background && a.foreground == b.foreground && a.primary == b.primary
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

macro_rules! tint_blends {
    ($colors:expr, $hue:expr, $strength:expr, $target:expr, $anchor:expr, $($field:ident),+ $(,)?) => {
        $($colors.$field = blend($colors.$field, $hue, $strength, $target, $anchor);)+
    };
}

pub fn tinted(base: &ThemeColor, tint: Tint) -> ThemeColor {
    let mut colors = *base;
    if tint.strength <= 0. {
        return colors;
    }
    let light = to_oklch(base.background).l > LIGHT_COVER;
    let (surface, text) = if light {
        (SURFACE_CHROMA_LIGHT, TEXT_CHROMA_LIGHT)
    } else {
        (SURFACE_CHROMA_DARK, TEXT_CHROMA_DARK)
    };
    let surface_anchor = to_oklch(base.background).c;
    let text_anchor = to_oklch(base.foreground).c;
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
    tint_blends!(
        colors,
        tint.hue,
        tint.strength,
        surface,
        surface_anchor,
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
    tint_blends!(
        colors,
        tint.hue,
        tint.strength,
        text,
        text_anchor,
        foreground,
        muted_foreground,
        accent_foreground,
        popover_foreground,
        secondary_foreground,
        button_foreground,
        button_secondary_foreground,
        group_box_foreground,
        description_list_label_foreground,
        sidebar_foreground,
        sidebar_accent_foreground,
        tab_foreground,
        tab_active_foreground,
        table_head_foreground,
        table_foot_foreground,
    );
    colors
}

#[cfg(test)]
mod tests {
    use super::super::color::{Oklch, to_hsla, to_oklch};
    use super::{ACCENT_DARK, MIN_SPAN, Tint, anchors, map_colors, mix, relit, tinted};
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
            (nudged.h - 290.).abs() < 30.,
            "hue {} jumped off the theme on a 16% tint",
            nudged.h
        );
        assert!(
            nudged.c > 0.045,
            "chroma {} collapsed on a 16% tint",
            nudged.c
        );
        let full = to_oklch(tinted(&saturated, full(140.)).background);
        assert!((full.h - 140.).abs() < 5., "hue {}", full.h);
    }

    #[test]
    fn a_cover_opposite_the_theme_never_greys_the_surfaces_out() {
        let base = dark_theme();
        let opposite = to_oklch(base.background).h + 180.;
        for strength in [0.3, 0.5, 0.7] {
            let background = to_oklch(
                tinted(
                    &base,
                    Tint {
                        hue: opposite,
                        strength,
                    },
                )
                .background,
            );
            assert!(
                background.c > 0.01,
                "strength {strength}: chroma {} cancelled out",
                background.c
            );
        }
        let full = to_oklch(tinted(&base, full(opposite)).background);
        assert!(full.c > 0.02, "a full tint dipped, chroma {}", full.c);
        assert!(
            (full.h - opposite.rem_euclid(360.)).abs() < 3.,
            "hue {}",
            full.h
        );
    }

    #[test]
    fn two_near_neutral_surfaces_do_not_split_onto_different_hues() {
        let sample = |chroma: f32| {
            let base = ThemeColor {
                background: to_hsla(
                    Oklch {
                        l: 0.22,
                        c: chroma,
                        h: 264.,
                    },
                    1.,
                ),
                ..dark_theme()
            };
            to_oklch(
                tinted(
                    &base,
                    Tint {
                        hue: 120.,
                        strength: 0.5,
                    },
                )
                .background,
            )
        };
        let below = sample(0.0039);
        let above = sample(0.0045);
        assert!(
            (below.h - above.h).abs() < 15.,
            "hue {} vs {} across the flat-chroma threshold",
            below.h,
            above.h
        );
        assert!(
            (below.c - above.c).abs() < 0.003,
            "chroma {} vs {}",
            below.c,
            above.c
        );
    }

    #[test]
    fn surfaces_keep_their_relative_colourfulness() {
        let base = ThemeColor {
            secondary: to_hsla(
                Oklch {
                    l: 0.4,
                    c: 0.04,
                    h: 264.,
                },
                1.,
            ),
            ..dark_theme()
        };
        let tinted = tinted(&base, full(30.));
        let background = to_oklch(tinted.background);
        let secondary = to_oklch(tinted.secondary);
        assert!(
            secondary.c > background.c * 1.2,
            "secondary {} lost its lift over the background {}",
            secondary.c,
            background.c
        );
        assert!((secondary.h - 30.).abs() < 3., "hue {}", secondary.h);
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
    fn tinting_never_touches_semantic_colours() {
        let base = dark_theme();
        let tinted = tinted(&base, full(30.));
        assert_eq!(tinted.danger, base.danger);
        assert_eq!(tinted.success, base.success);
        assert_eq!(tinted.warning, base.warning);
        assert_eq!(tinted.info, base.info);
        assert_eq!(tinted.chart_bullish, base.chart_bullish);
    }

    #[test]
    fn body_text_follows_the_cover_without_moving_its_lightness() {
        let base = ThemeColor {
            muted_foreground: to_hsla(
                Oklch {
                    l: 0.7,
                    c: 0.03,
                    h: 264.,
                },
                1.,
            ),
            ..dark_theme()
        };
        let tinted = tinted(&base, full(30.));
        for (generated, original) in [
            (tinted.foreground, base.foreground),
            (tinted.muted_foreground, base.muted_foreground),
        ] {
            let (generated, original) = (to_oklch(generated), to_oklch(original));
            assert!((generated.h - 30.).abs() < 3., "hue {}", generated.h);
            assert!(
                (generated.l - original.l).abs() < 0.01,
                "lightness moved from {} to {}",
                original.l,
                generated.l
            );
        }
    }

    #[test]
    fn muted_text_stays_less_colourful_than_body_text() {
        let base = ThemeColor {
            muted_foreground: to_hsla(
                Oklch {
                    l: 0.7,
                    c: 0.02,
                    h: 264.,
                },
                1.,
            ),
            foreground: to_hsla(
                Oklch {
                    l: 0.9,
                    c: 0.05,
                    h: 264.,
                },
                1.,
            ),
            ..dark_theme()
        };
        let tinted = tinted(&base, full(30.));
        assert!(to_oklch(tinted.muted_foreground).c < to_oklch(tinted.foreground).c);
    }

    #[test]
    fn a_mix_walks_from_one_theme_to_the_other() {
        let base = dark_theme();
        let warm = tinted(&base, full(30.));
        let (from, to) = (to_oklch(base.background), to_oklch(warm.background));
        let start = to_oklch(mix(&base, &warm, 0.).background);
        let end = to_oklch(mix(&base, &warm, 1.).background);
        assert!((start.h - from.h).abs() < 2., "hue {}", start.h);
        assert!((end.h - to.h).abs() < 2., "hue {}", end.h);
        let half = to_oklch(mix(&base, &warm, 0.5).background);
        assert!(
            (half.l - (from.l + to.l) * 0.5).abs() < 0.01,
            "lightness {}",
            half.l
        );
        assert!(
            half.c < from.c.max(to.c),
            "a half mix should pass through less chroma, got {}",
            half.c
        );
    }

    #[test]
    fn zero_strength_leaves_the_theme_alone() {
        let base = dark_theme();
        let tinted = tinted(&base, Tint::none());
        assert_eq!(tinted.primary, base.primary);
        assert_eq!(tinted.background, base.background);
    }
}
