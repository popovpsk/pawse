use std::sync::LazyLock;
use std::time::{Duration, Instant};

pub const FAST: Duration = Duration::from_nanos(16_666_667);
pub const SLOW: Duration = Duration::from_nanos(2 * 16_666_667);
pub const SLOWEST: Duration = Duration::from_nanos(4 * 16_666_667);

static ANCHOR: LazyLock<Instant> = LazyLock::new(Instant::now);

pub fn next_tick(period: Duration, at_least: Duration) -> Instant {
    let anchor = *ANCHOR;
    next_tick_from(anchor, Instant::now(), period, at_least)
}

pub fn wait(period: Duration, at_least: Duration) -> Duration {
    next_tick(period, at_least).saturating_duration_since(Instant::now())
}

fn next_tick_from(anchor: Instant, now: Instant, period: Duration, at_least: Duration) -> Instant {
    let period_ns = period.as_nanos().max(1);
    let current = now.saturating_duration_since(anchor).as_nanos() / period_ns * period_ns;
    let earliest = current + at_least.max(period / 4).as_nanos();
    let tick = earliest.div_ceil(period_ns) * period_ns;
    anchor + Duration::from_nanos(u64::try_from(tick).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset(anchor: Instant, at: Instant) -> u128 {
        at.duration_since(anchor).as_nanos()
    }

    #[test]
    fn every_tick_is_on_the_grid_and_ahead() {
        let anchor = Instant::now();
        for offset_us in [0, 1, 4_000, 16_000, 16_667, 33_000, 1_000_123] {
            let now = anchor + Duration::from_micros(offset_us);
            for period in [FAST, SLOW, SLOWEST] {
                let tick = next_tick_from(anchor, now, period, Duration::ZERO);
                assert_eq!(offset(anchor, tick) % period.as_nanos(), 0);
                assert!(tick > now && tick <= now + period);
            }
        }
    }

    #[test]
    fn slow_ticks_are_fast_ticks() {
        let anchor = Instant::now();
        let now = anchor + Duration::from_millis(41);
        let slow = next_tick_from(anchor, now, SLOW, Duration::ZERO);
        assert_eq!(offset(anchor, slow) % FAST.as_nanos(), 0);
        let slowest = next_tick_from(anchor, now, SLOWEST, Duration::ZERO);
        assert_eq!(offset(anchor, slowest) % SLOW.as_nanos(), 0);
    }

    #[test]
    fn a_frame_drawn_late_still_takes_the_next_tick() {
        let anchor = Instant::now();
        let woke = anchor + SLOW * 3;
        let drawn = woke + Duration::from_millis(14);
        let tick = next_tick_from(anchor, drawn, SLOW, Duration::from_millis(24));
        assert_eq!(tick, woke + SLOW);
    }

    #[test]
    fn a_long_wait_lands_on_the_first_tick_after_it() {
        let anchor = Instant::now();
        let now = anchor + SLOW * 2;
        let tick = next_tick_from(anchor, now, SLOW, Duration::from_millis(200));
        assert!(tick >= now + Duration::from_millis(200));
        assert!(tick < now + Duration::from_millis(200) + SLOW);
        assert_eq!(offset(anchor, tick) % SLOW.as_nanos(), 0);
    }
}
