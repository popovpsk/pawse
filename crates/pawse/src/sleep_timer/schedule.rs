use std::time::Duration;

use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike};

pub fn time_of_day(minute_of_day: u16) -> NaiveTime {
    NaiveTime::from_hms_opt(
        u32::from(minute_of_day / 60),
        u32::from(minute_of_day % 60),
        0,
    )
    .unwrap_or(NaiveTime::MIN)
}

pub fn minute_of_day(time: NaiveTime) -> u16 {
    (time.hour() * 60 + time.minute()) as u16
}

pub fn fade_window(enabled: bool, fade_secs: u32, span: Duration) -> Duration {
    if !enabled {
        return Duration::ZERO;
    }
    Duration::from_secs(u64::from(fade_secs)).min(span)
}

const FADE_UPDATES: u32 = 120;
const FADE_TICK_MIN: Duration = Duration::from_millis(250);
const FADE_TICK_MAX: Duration = Duration::from_secs(1);

pub fn fade_tick(fade: Duration) -> Duration {
    (fade / FADE_UPDATES).clamp(FADE_TICK_MIN, FADE_TICK_MAX)
}

pub fn settled_fade(current: Duration, wanted: Duration, remaining: Duration) -> Duration {
    if wanted.is_zero() || (remaining > current && remaining > wanted) {
        wanted
    } else {
        current
    }
}

pub fn window_opened_on(now: NaiveDateTime, from_min: u16, until_min: u16) -> Option<NaiveDate> {
    let minute = minute_of_day(now.time());
    let today = now.date();
    let yesterday = || today.checked_sub_days(Days::new(1));
    if from_min == until_min {
        return if minute >= from_min {
            Some(today)
        } else {
            yesterday()
        };
    }
    if from_min < until_min {
        return (from_min..until_min).contains(&minute).then_some(today);
    }
    if minute >= from_min {
        Some(today)
    } else if minute < until_min {
        yesterday()
    } else {
        None
    }
}

pub fn clock_label(minute_of_day: u16) -> String {
    format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
}

pub fn end_clock_label(now: NaiveDateTime, remaining: Duration) -> String {
    let end = TimeDelta::from_std(remaining)
        .ok()
        .and_then(|delta| now.checked_add_signed(delta))
        .unwrap_or(now);
    end.format("%H:%M").to_string()
}

pub fn remaining_label(remaining: Duration) -> String {
    let secs = remaining.as_millis().div_ceil(1000) as u64;
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn fade_gain(remaining: Duration, fade: Duration) -> f32 {
    if fade.is_zero() || remaining >= fade {
        return 1.;
    }
    let t = remaining.as_secs_f32() / fade.as_secs_f32();
    t * t
}

pub fn next_tick(remaining: Duration, fade: Duration, fade_step: Duration) -> Duration {
    let to_next_second = Duration::from_millis((remaining.as_millis() % 1000) as u64 + 1);
    let mut delay = to_next_second;
    if remaining <= fade {
        delay = delay.min(fade_step);
    } else {
        delay = delay.min(remaining - fade);
    }
    delay.min(remaining).max(Duration::from_millis(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn day(d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, 9, d)
    }

    #[rstest]
    #[case::evening_inside(at(10, 23, 30), day(10))]
    #[case::at_start(at(10, 23, 0), day(10))]
    #[case::after_midnight(at(11, 2, 15), day(10))]
    #[case::at_end_is_outside(at(11, 6, 0), None)]
    #[case::daytime(at(11, 14, 0), None)]
    #[case::just_before_start(at(10, 22, 59), None)]
    fn wrapping_window(#[case] now: NaiveDateTime, #[case] expected: Option<NaiveDate>) {
        assert_eq!(window_opened_on(now, 23 * 60, 6 * 60), expected);
    }

    #[rstest]
    #[case::inside(at(10, 14, 0), day(10))]
    #[case::before(at(10, 12, 59), None)]
    #[case::end(at(10, 18, 0), None)]
    fn plain_window(#[case] now: NaiveDateTime, #[case] expected: Option<NaiveDate>) {
        assert_eq!(window_opened_on(now, 13 * 60, 18 * 60), expected);
    }

    #[test]
    fn equal_bounds_mean_all_day_opening_at_the_bound() {
        assert_eq!(window_opened_on(at(10, 21, 0), 20 * 60, 20 * 60), day(10));
        assert_eq!(window_opened_on(at(10, 19, 0), 20 * 60, 20 * 60), day(9));
    }

    #[test]
    fn window_across_month_start() {
        let now = NaiveDate::from_ymd_opt(2026, 10, 1)
            .unwrap()
            .and_hms_opt(1, 0, 0)
            .unwrap();
        assert_eq!(window_opened_on(now, 23 * 60, 6 * 60), day(30));
    }

    #[rstest]
    #[case::rounds_up(Duration::from_millis(59_001), "1:00")]
    #[case::exact(Duration::from_secs(59), "0:59")]
    #[case::minutes(Duration::from_secs(29 * 60 + 5), "29:05")]
    #[case::hours(Duration::from_secs(2 * 3600 + 61), "2:01:01")]
    #[case::zero(Duration::ZERO, "0:00")]
    fn remaining_labels(#[case] remaining: Duration, #[case] expected: &str) {
        assert_eq!(remaining_label(remaining), expected);
    }

    #[rstest]
    #[case::same_hour(at(10, 23, 17), Duration::from_secs(30 * 60), "23:47")]
    #[case::past_midnight(at(10, 23, 50), Duration::from_secs(20 * 60), "00:10")]
    #[case::seconds_are_dropped(
        at(10, 23, 17) + TimeDelta::seconds(59),
        Duration::from_secs(30 * 60),
        "23:47"
    )]
    fn end_clock_labels(
        #[case] now: NaiveDateTime,
        #[case] remaining: Duration,
        #[case] expected: &str,
    ) {
        assert_eq!(end_clock_label(now, remaining), expected);
    }

    #[test]
    fn clock() {
        assert_eq!(clock_label(0), "00:00");
        assert_eq!(clock_label(23 * 60 + 30), "23:30");
    }

    #[rstest]
    #[case::midnight(0)]
    #[case::odd_minute(23 * 60 + 47)]
    #[case::last_minute(24 * 60 - 1)]
    fn minute_of_day_round_trips(#[case] minute: u16) {
        assert_eq!(minute_of_day(time_of_day(minute)), minute);
    }

    #[test]
    fn seconds_are_ignored_by_minute_of_day() {
        let time = NaiveTime::from_hms_opt(6, 5, 59).unwrap();
        assert_eq!(minute_of_day(time), 6 * 60 + 5);
    }

    #[rstest]
    #[case::off(false, 30, Duration::from_secs(600), Duration::ZERO)]
    #[case::setting(true, 30, Duration::from_secs(600), Duration::from_secs(30))]
    #[case::capped_by_short_timer(true, 120, Duration::from_secs(60), Duration::from_secs(60))]
    fn fade_windows(
        #[case] enabled: bool,
        #[case] secs: u32,
        #[case] span: Duration,
        #[case] expected: Duration,
    ) {
        assert_eq!(fade_window(enabled, secs, span), expected);
    }

    #[rstest]
    #[case::before_any_fade(600, 30, 90, 90)]
    #[case::shortened_during_fade(20, 120, 5, 120)]
    #[case::lengthened_into_remaining(40, 30, 120, 30)]
    #[case::switched_off_during_fade(10, 30, 0, 0)]
    #[case::switched_on_during_window(10, 0, 30, 0)]
    fn fade_changes_never_jump_the_volume(
        #[case] remaining: u64,
        #[case] current: u64,
        #[case] wanted: u64,
        #[case] expected: u64,
    ) {
        assert_eq!(
            settled_fade(
                Duration::from_secs(current),
                Duration::from_secs(wanted),
                Duration::from_secs(remaining),
            ),
            Duration::from_secs(expected)
        );
    }

    #[rstest]
    #[case::short_fade(5, 250)]
    #[case::default_fade(30, 250)]
    #[case::one_minute(60, 500)]
    #[case::hour(3600, 1000)]
    fn fade_ticks_scale_with_the_fade(#[case] fade_secs: u64, #[case] tick_ms: u64) {
        assert_eq!(
            fade_tick(Duration::from_secs(fade_secs)),
            Duration::from_millis(tick_ms)
        );
    }

    #[test]
    fn a_capped_fade_starts_at_full_volume() {
        let span = Duration::from_secs(60);
        let fade = fade_window(true, 120, span);
        assert_eq!(fade_gain(span, fade), 1.);
    }

    #[test]
    fn fade_is_full_before_the_window_and_silent_at_the_end() {
        let fade = Duration::from_secs(30);
        assert_eq!(fade_gain(Duration::from_secs(31), fade), 1.);
        assert_eq!(fade_gain(Duration::from_secs(30), fade), 1.);
        assert_eq!(fade_gain(Duration::ZERO, fade), 0.);
        assert!((fade_gain(Duration::from_secs(15), fade) - 0.25).abs() < 1e-6);
        assert_eq!(fade_gain(Duration::ZERO, Duration::ZERO), 1.);
    }

    #[test]
    fn ticks_land_on_second_boundaries_and_on_the_fade_start() {
        let fade = Duration::from_secs(30);
        let step = Duration::from_millis(250);
        assert_eq!(
            next_tick(Duration::from_millis(100_400), fade, step),
            Duration::from_millis(401)
        );
        assert_eq!(
            next_tick(Duration::from_millis(30_200), fade, step),
            Duration::from_millis(200)
        );
        assert_eq!(
            next_tick(Duration::from_millis(10_900), fade, step),
            Duration::from_millis(250)
        );
        assert_eq!(
            next_tick(Duration::from_millis(5), fade, step),
            Duration::from_millis(5)
        );
        assert_eq!(
            next_tick(Duration::ZERO, fade, step),
            Duration::from_millis(1)
        );
    }
}
