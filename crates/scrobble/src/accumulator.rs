use std::time::{Duration, Instant};

#[derive(Default)]
pub struct PlayAccumulator {
    accumulated: Duration,
    playing_since: Option<Instant>,
}

impl PlayAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_play(&mut self, now: Instant) {
        if self.playing_since.is_none() {
            self.playing_since = Some(now);
        }
    }

    pub fn on_pause(&mut self, now: Instant) {
        if let Some(since) = self.playing_since.take() {
            self.accumulated += now.saturating_duration_since(since);
        }
    }

    pub fn played(&self, now: Instant) -> Duration {
        self.accumulated
            + self
                .playing_since
                .map(|since| now.saturating_duration_since(since))
                .unwrap_or_default()
    }

    pub fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
        self.playing_since = None;
    }
}

pub fn should_scrobble(played: Duration, duration: Duration) -> bool {
    let duration = duration.as_secs();
    if duration < 30 {
        return false;
    }
    let played = played.as_secs();
    played >= duration / 2 || played >= 240
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn short_track_never_scrobbles() {
        assert!(!should_scrobble(secs(29), secs(29)));
        assert!(!should_scrobble(secs(1000), secs(20)));
    }

    #[test]
    fn half_listened_scrobbles() {
        assert!(should_scrobble(secs(60), secs(120)));
        assert!(!should_scrobble(secs(59), secs(120)));
    }

    #[test]
    fn four_minute_rule_scrobbles_long_track() {
        assert!(should_scrobble(secs(240), secs(600)));
        assert!(!should_scrobble(secs(239), secs(600)));
    }

    #[test]
    fn seeking_does_not_inflate_played_time() {
        let t0 = Instant::now();
        let mut acc = PlayAccumulator::new();
        acc.on_play(t0);
        acc.on_pause(t0 + secs(30));
        acc.on_play(t0 + secs(40));
        assert_eq!(acc.played(t0 + secs(50)), secs(40));
    }

    #[test]
    fn paused_time_is_excluded() {
        let t0 = Instant::now();
        let mut acc = PlayAccumulator::new();
        acc.on_play(t0);
        acc.on_pause(t0 + secs(30));
        assert_eq!(acc.played(t0 + secs(100)), secs(30));
    }

    #[test]
    fn played_counts_ongoing_span() {
        let t0 = Instant::now();
        let mut acc = PlayAccumulator::new();
        acc.on_play(t0);
        assert_eq!(acc.played(t0 + secs(75)), secs(75));
    }
}
