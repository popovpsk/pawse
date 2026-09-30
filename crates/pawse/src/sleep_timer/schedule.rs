use std::time::Duration;

use chrono::{Days, NaiveDate, NaiveDateTime, TimeDelta, Timelike};

pub const MINUTES_PER_DAY: u16 = 24 * 60;

pub fn window_opened_on(now: NaiveDateTime, from_min: u16, until_min: u16) -> Option<NaiveDate> {
    let minute = (now.hour() * 60 + now.minute()) as u16;
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
