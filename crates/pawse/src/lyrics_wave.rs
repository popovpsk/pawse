use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use gpui::{Pixels, px};

const RESPONSE: f32 = 0.62;
const DAMPING: f32 = 0.84;
const ROW_PITCH_EM: f32 = 2.;
const ROW_DELAY: f32 = 0.05;
const MAX_DELAY: f32 = 0.4;
const REST: f32 = 0.005;
const MAX_ROWS: f32 = 4.;

pub fn fits(step: Pixels, em: Pixels) -> bool {
    step.abs() <= em * ROW_PITCH_EM * MAX_ROWS
}

#[derive(Clone, Copy)]
struct Shift {
    start: Instant,
    from: Pixels,
    amount: Pixels,
}

#[derive(Default)]
pub struct Wave {
    shifts: Vec<Shift>,
}

pub struct Frame<'a> {
    shifts: &'a [Shift],
    now: Instant,
    anchor: Pixels,
    pitch: Pixels,
}

impl Wave {
    pub fn push(&mut self, from: Pixels, to: Pixels, now: Instant) {
        self.settle(now);
        self.shifts.push(Shift {
            start: now,
            from,
            amount: from - to,
        });
    }

    pub fn clear(&mut self) {
        self.shifts.clear();
    }

    pub fn settle(&mut self, now: Instant) -> bool {
        if self.shifts.is_empty() {
            return false;
        }
        let span = Duration::from_secs_f32(MAX_DELAY + settle_secs());
        self.shifts
            .retain(|shift| now.saturating_duration_since(shift.start) < span);
        !self.shifts.is_empty()
    }

    pub fn reach(&self) -> Pixels {
        self.shifts
            .iter()
            .fold(px(0.), |total, shift| total + shift.amount.abs())
    }

    pub fn frame(&self, now: Instant, anchor: Pixels, em: Pixels) -> Option<Frame<'_>> {
        (!self.shifts.is_empty()).then(|| Frame {
            shifts: &self.shifts,
            now,
            anchor,
            pitch: (em * ROW_PITCH_EM).max(px(1.)),
        })
    }
}

impl Frame<'_> {
    pub fn offset(&self, row_top: Pixels) -> Pixels {
        self.shifts.iter().fold(px(0.), |total, shift| {
            let rows = ((row_top + shift.from - self.anchor) / self.pitch).max(0.);
            let delay = (rows * ROW_DELAY).min(MAX_DELAY);
            let elapsed = self
                .now
                .saturating_duration_since(shift.start)
                .as_secs_f32();
            total + shift.amount * (1. - progress(elapsed - delay))
        })
    }
}

fn progress(t: f32) -> f32 {
    if t <= 0. {
        return 0.;
    }
    let omega = TAU / RESPONSE;
    let damped = omega * (1. - DAMPING * DAMPING).sqrt();
    let decay = (-DAMPING * omega * t).exp();
    1. - decay * ((damped * t).cos() + DAMPING * omega / damped * (damped * t).sin())
}

fn settle_secs() -> f32 {
    let omega = TAU / RESPONSE;
    (1. / (REST * (1. - DAMPING * DAMPING).sqrt())).ln() / (DAMPING * omega)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: f32) -> Instant {
        start + Duration::from_secs_f32(secs)
    }

    #[test]
    fn spring_starts_at_rest_and_settles_within_rest() {
        assert_eq!(progress(-0.1), 0.);
        assert_eq!(progress(0.), 0.);
        assert!(progress(0.15) > 0.3 && progress(0.15) < 0.7);
        let t = settle_secs();
        assert!((1. - progress(t)).abs() <= REST + 1e-4);
        assert!((1. - progress(t * 2.)).abs() <= REST);
    }

    #[test]
    fn row_starts_where_it_was_drawn_and_lands_in_place() {
        let start = Instant::now();
        let mut wave = Wave::default();
        wave.push(px(0.), px(-40.), start);
        let frame = wave.frame(start, px(300.), px(16.)).unwrap();
        assert_eq!(frame.offset(px(100.)), px(40.));

        let late = at(start, MAX_DELAY + settle_secs() + 0.01);
        assert!(!wave.settle(late));
        assert!(wave.frame(late, px(300.), px(16.)).is_none());
    }

    #[test]
    fn rows_below_the_anchor_start_later_up_to_the_cap() {
        let start = Instant::now();
        let mut wave = Wave::default();
        wave.push(px(0.), px(-40.), start);
        let frame = wave.frame(at(start, 0.1), px(300.), px(16.)).unwrap();
        let above = frame.offset(px(200.));
        let below = frame.offset(px(400.));
        let far = frame.offset(px(5_000.));
        assert!(above < below);
        assert_eq!(below, px(40.));
        assert_eq!(far, px(40.));

        let capped = wave
            .frame(at(start, MAX_DELAY + 0.1), px(300.), px(16.))
            .unwrap();
        assert!(capped.offset(px(5_000.)) < px(40.));
    }

    #[test]
    fn only_short_steps_wave() {
        assert!(fits(px(-60.), px(16.)));
        assert!(fits(px(128.), px(16.)));
        assert!(!fits(px(-129.), px(16.)));
        assert!(!fits(px(1.), px(0.)));
    }

    #[test]
    fn shifts_add_up_and_reach_covers_them() {
        let start = Instant::now();
        let mut wave = Wave::default();
        wave.push(px(0.), px(-40.), start);
        wave.push(px(-40.), px(-70.), start);
        assert_eq!(wave.reach(), px(70.));
        let frame = wave.frame(start, px(300.), px(16.)).unwrap();
        assert_eq!(frame.offset(px(0.)), px(70.));
    }
}
