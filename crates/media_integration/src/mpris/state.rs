use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{MediaCommand, MediaPlaybackState, NowPlayingInfo};

const SEEK_TOLERANCE_SECS: f64 = 1.0;
const VOLUME_STEPS: f64 = 100.0;
const VOLUME_SIGNAL_INTERVAL: Duration = Duration::from_millis(200);
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const TRACK_PATH_PREFIX: &str = "/io/github/popovpsk/pawse/track/";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Track {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) art_url: Option<String>,
    pub(crate) length_us: i64,
}

impl Track {
    fn from_info(info: &NowPlayingInfo) -> Self {
        Self {
            title: info.title.clone(),
            artist: info.artist.clone(),
            album: info.album.clone(),
            art_url: info.artwork_path.as_deref().map(file_url),
            length_us: valid_secs(info.duration_secs).map_or(0, secs_to_us),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Changes {
    pub(crate) playback_status: bool,
    pub(crate) metadata: bool,
    pub(crate) can_play_pause: bool,
    pub(crate) can_seek: bool,
    pub(crate) seeked: bool,
    pub(crate) volume: bool,
    pub(crate) volume_restated: bool,
}

impl Changes {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn merge(&mut self, other: Changes) {
        self.playback_status |= other.playback_status;
        self.metadata |= other.metadata;
        self.can_play_pause |= other.can_play_pause;
        self.can_seek |= other.can_seek;
        self.seeked |= other.seeked;
        self.volume |= other.volume;
        self.volume_restated |= other.volume_restated;
    }
}

#[derive(Debug)]
pub(crate) struct VolumePacer {
    sent: Option<f64>,
    next_at: Option<Instant>,
    held: bool,
}

impl VolumePacer {
    pub(crate) fn new(known_to_clients: f64) -> Self {
        Self {
            sent: Some(known_to_clients),
            next_at: None,
            held: false,
        }
    }

    pub(crate) fn pace(
        &mut self,
        changes: &mut Changes,
        volume: f64,
        now: Instant,
    ) -> Option<Instant> {
        let changed = std::mem::take(&mut changes.volume);
        let held = std::mem::take(&mut self.held);
        if std::mem::take(&mut changes.volume_restated) {
            self.sent = None;
        }
        if !(changed || held || self.sent.is_none()) || self.sent == Some(volume) {
            return None;
        }
        if let Some(next_at) = self.next_at.filter(|next_at| now < *next_at) {
            self.held = true;
            return Some(next_at);
        }
        self.sent = Some(volume);
        self.next_at = Some(now + VOLUME_SIGNAL_INTERVAL);
        changes.volume = true;
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct Anchor {
    secs: f64,
    at: Instant,
}

impl Anchor {
    fn new(secs: f64, at: Instant) -> Self {
        Self { secs, at }
    }

    fn position(&self, status: MediaPlaybackState, now: Instant) -> f64 {
        match status {
            MediaPlaybackState::Playing => {
                self.secs + now.saturating_duration_since(self.at).as_secs_f64()
            }
            MediaPlaybackState::Paused | MediaPlaybackState::Stopped => self.secs,
        }
    }
}

pub(crate) struct State {
    status: MediaPlaybackState,
    track: Option<Track>,
    track_number: u64,
    live: Anchor,
    announced: Anchor,
    volume: f64,
}

impl State {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            status: MediaPlaybackState::Stopped,
            track: None,
            track_number: 0,
            volume: 1.0,
            live: Anchor::new(0.0, now),
            announced: Anchor::new(0.0, now),
        }
    }

    pub(crate) fn status(&self) -> MediaPlaybackState {
        self.status
    }

    pub(crate) fn track(&self) -> Option<&Track> {
        self.track.as_ref()
    }

    pub(crate) fn has_track(&self) -> bool {
        self.track.is_some()
    }

    pub(crate) fn can_seek(&self) -> bool {
        self.track.as_ref().is_some_and(|track| track.length_us > 0)
    }

    pub(crate) fn track_id(&self) -> String {
        if self.track.is_some() {
            format!("{TRACK_PATH_PREFIX}{}", self.track_number)
        } else {
            NO_TRACK.to_owned()
        }
    }

    pub(crate) fn position_us(&self, now: Instant) -> i64 {
        self.reported_us(self.live, now)
    }

    pub(crate) fn announced_position_us(&self, now: Instant) -> i64 {
        self.reported_us(self.announced, now)
    }

    pub(crate) fn set_track(
        &mut self,
        info: &NowPlayingInfo,
        status: MediaPlaybackState,
        now: Instant,
    ) -> Changes {
        let track = Track::from_info(info);
        if self.track.as_ref() == Some(&track) {
            return match info.elapsed_secs {
                Some(elapsed) => self.set_position(elapsed, status, now),
                None => self.set_status(status, now),
            };
        }
        let could_seek = self.can_seek();
        let had_track = self.has_track();
        self.track = Some(track);
        self.track_number += 1;
        let mut changes = self.set_status(status, now);
        changes.metadata = true;
        changes.can_play_pause = !had_track;
        changes.can_seek = could_seek != self.can_seek();
        let elapsed = info.elapsed_secs.and_then(valid_secs).unwrap_or(0.0);
        self.rebase(elapsed, now);
        changes
    }

    pub(crate) fn set_status(&mut self, status: MediaPlaybackState, now: Instant) -> Changes {
        if status == self.status {
            return Changes::default();
        }
        self.rebase(self.live.position(self.status, now), now);
        self.status = status;
        Changes {
            playback_status: true,
            ..Changes::default()
        }
    }

    pub(crate) fn set_position(
        &mut self,
        elapsed_secs: f64,
        status: MediaPlaybackState,
        now: Instant,
    ) -> Changes {
        let mut changes = self.set_status(status, now);
        let Some(elapsed) = valid_secs(elapsed_secs) else {
            return changes;
        };
        self.live = Anchor::new(elapsed, now);
        let drift = elapsed - self.announced.position(self.status, now);
        if drift.abs() > SEEK_TOLERANCE_SECS
            && self.has_track()
            && self.status != MediaPlaybackState::Stopped
        {
            self.announced = self.live;
            changes.seeked = true;
        }
        changes
    }

    pub(crate) fn volume(&self) -> f64 {
        self.volume
    }

    pub(crate) fn set_volume(&mut self, volume: f64) -> Changes {
        let Some(volume) = volume_level(volume) else {
            return Changes::default();
        };
        if volume == self.volume {
            return Changes::default();
        }
        self.volume = volume;
        Changes {
            volume: true,
            ..Changes::default()
        }
    }

    pub(crate) fn restate_volume(&mut self, volume: f64) -> Changes {
        let mut changes = self.set_volume(volume);
        changes.volume_restated = true;
        changes
    }

    pub(crate) fn seek_by(&self, offset_us: i64, now: Instant) -> Option<MediaCommand> {
        let length_us = self
            .track
            .as_ref()
            .filter(|track| track.length_us > 0)?
            .length_us;
        let target = self.position_us(now).saturating_add(offset_us).max(0);
        if target > length_us {
            Some(MediaCommand::Next)
        } else {
            Some(MediaCommand::Seek(us_to_secs(target)))
        }
    }

    pub(crate) fn seek_to(&self, track_id: &str, position_us: i64) -> Option<MediaCommand> {
        let track = self.track.as_ref().filter(|track| track.length_us > 0)?;
        if track_id != self.track_id() || !(0..=track.length_us).contains(&position_us) {
            return None;
        }
        Some(MediaCommand::Seek(us_to_secs(position_us)))
    }

    fn rebase(&mut self, secs: f64, now: Instant) {
        self.live = Anchor::new(secs, now);
        self.announced = self.live;
    }

    fn reported_us(&self, anchor: Anchor, now: Instant) -> i64 {
        let Some(track) = &self.track else {
            return 0;
        };
        if self.status == MediaPlaybackState::Stopped {
            return 0;
        }
        let position = secs_to_us(anchor.position(self.status, now).max(0.0));
        if track.length_us > 0 {
            position.min(track.length_us)
        } else {
            position
        }
    }
}

pub(crate) fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

pub(crate) fn volume_level(volume: f64) -> Option<f64> {
    volume
        .is_finite()
        .then(|| (volume.clamp(0.0, 1.0) * VOLUME_STEPS).round() / VOLUME_STEPS + 0.0)
}

fn valid_secs(secs: f64) -> Option<f64> {
    (secs.is_finite() && secs >= 0.0).then_some(secs)
}

fn secs_to_us(secs: f64) -> i64 {
    (secs * 1_000_000.0).round() as i64
}

fn us_to_secs(us: i64) -> f64 {
    us as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    const PLAYING: MediaPlaybackState = MediaPlaybackState::Playing;
    const PAUSED: MediaPlaybackState = MediaPlaybackState::Paused;
    const STOPPED: MediaPlaybackState = MediaPlaybackState::Stopped;

    fn info(title: &str, duration_secs: f64) -> NowPlayingInfo {
        NowPlayingInfo {
            title: title.to_owned(),
            artist: "Artist".to_owned(),
            album: "Album".to_owned(),
            artwork_path: None,
            duration_secs,
            elapsed_secs: Some(0.0),
        }
    }

    fn at(start: Instant, secs: f64) -> Instant {
        start + Duration::from_secs_f64(secs)
    }

    fn playing(title: &str, duration_secs: f64) -> (State, Instant) {
        let start = Instant::now();
        let mut state = State::new(start);
        state.set_track(&info(title, duration_secs), PLAYING, start);
        (state, start)
    }

    #[test]
    fn first_track_announces_everything_but_seeked() {
        let start = Instant::now();
        let mut state = State::new(start);
        let changes = state.set_track(&info("A", 200.0), PLAYING, start);
        assert_eq!(
            changes,
            Changes {
                playback_status: true,
                metadata: true,
                can_play_pause: true,
                can_seek: true,
                seeked: false,
                volume: false,
                volume_restated: false,
            }
        );
        assert_eq!(state.track_id(), format!("{TRACK_PATH_PREFIX}1"));
    }

    #[test]
    fn steady_ticks_change_nothing() {
        let (mut state, start) = playing("A", 200.0);
        for tick in 1..=50 {
            let secs = f64::from(tick) * 0.2;
            let changes = state.set_position(secs, PLAYING, at(start, secs));
            assert!(changes.is_empty(), "tick {tick}: {changes:?}");
        }
    }

    #[test]
    fn ticks_with_jitter_below_tolerance_change_nothing() {
        let (mut state, start) = playing("A", 200.0);
        for (reported, wall) in [(0.328, 0.2), (0.4, 0.45), (1.1, 0.6), (1.2, 1.4)] {
            assert!(
                state
                    .set_position(reported, PLAYING, at(start, wall))
                    .is_empty()
            );
        }
    }

    #[test]
    fn pause_reports_status_only_and_freezes_position() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PLAYING, at(start, 10.0));
        let changes = state.set_status(PAUSED, at(start, 10.1));
        assert_eq!(
            changes,
            Changes {
                playback_status: true,
                ..Changes::default()
            }
        );
        assert!(state.set_position(10.0, PAUSED, at(start, 10.1)).is_empty());
        assert_eq!(state.position_us(at(start, 30.0)), 10_000_000);
    }

    #[test]
    fn resume_extrapolates_from_the_paused_position() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PAUSED, at(start, 10.0));
        let changes = state.set_status(PLAYING, at(start, 60.0));
        assert!(changes.playback_status && !changes.seeked);
        assert_eq!(state.position_us(at(start, 62.0)), 12_000_000);
    }

    #[rstest]
    #[case::forward(40.0, true)]
    #[case::backward(2.0, true)]
    #[case::just_over(11.3, true)]
    #[case::within_tolerance(10.9, false)]
    fn jumps_beyond_tolerance_are_seeks(#[case] reported: f64, #[case] seeked: bool) {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PLAYING, at(start, 10.0));
        let changes = state.set_position(reported, PLAYING, at(start, 10.2));
        assert_eq!(changes.seeked, seeked);
        assert!(!changes.playback_status && !changes.metadata);
    }

    #[test]
    fn stalled_stream_is_corrected_by_a_seek() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PLAYING, at(start, 10.0));
        let changes = state.set_position(10.2, PLAYING, at(start, 15.0));
        assert!(changes.seeked);
        assert_eq!(state.position_us(at(start, 15.0)), 10_200_000);
    }

    #[test]
    fn slow_drift_is_measured_from_the_last_announcement() {
        let (mut state, start) = playing("A", 200.0);
        for tick in 1..=60 {
            let wall = f64::from(tick) * 0.2;
            let reported = wall * 0.9;
            let changes = state.set_position(reported, PLAYING, at(start, wall));
            assert_eq!(changes.seeked, tick == 51, "tick {tick}");
        }
        assert_eq!(state.announced_position_us(at(start, 10.2)), 9_180_000);
    }

    #[test]
    fn burst_after_a_stall_keeps_the_announced_position_within_tolerance() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PLAYING, at(start, 10.0));
        let now = at(start, 12.0);
        let mut seeks = 0;
        for step in 1..=10 {
            let reported = 10.0 + f64::from(step) * 0.2;
            if state.set_position(reported, PLAYING, now).seeked {
                seeks += 1;
            }
        }
        assert_eq!(seeks, 2);
        assert_eq!(state.position_us(now), 12_000_000);
        let lag = state.position_us(now) - state.announced_position_us(now);
        assert!(lag <= 1_000_000, "announced lags by {lag} us");
    }

    #[test]
    fn new_track_changes_metadata_without_seeked() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(150.0, PLAYING, at(start, 150.0));
        let changes = state.set_track(&info("B", 180.0), PLAYING, at(start, 150.5));
        assert_eq!(
            changes,
            Changes {
                metadata: true,
                ..Changes::default()
            }
        );
        assert_eq!(state.track_id(), format!("{TRACK_PATH_PREFIX}2"));
        assert_eq!(state.position_us(at(start, 151.5)), 1_000_000);
    }

    #[test]
    fn track_without_length_flips_can_seek() {
        let (mut state, start) = playing("A", 200.0);
        let changes = state.set_track(&info("Stream", 0.0), PLAYING, at(start, 1.0));
        assert!(changes.metadata && changes.can_seek);
        assert!(!state.can_seek());
    }

    #[test]
    fn same_track_again_keeps_metadata_and_id() {
        let (mut state, start) = playing("A", 200.0);
        let mut again = info("A", 200.0);
        again.elapsed_secs = Some(0.3);
        assert!(state.set_track(&again, PLAYING, at(start, 0.2)).is_empty());
        assert_eq!(state.track_id(), format!("{TRACK_PATH_PREFIX}1"));
    }

    #[test]
    fn replaying_the_same_track_from_the_end_is_a_seek() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(199.9, PLAYING, at(start, 199.9));
        let changes = state.set_track(&info("A", 200.0), PLAYING, at(start, 200.0));
        assert_eq!(
            changes,
            Changes {
                seeked: true,
                ..Changes::default()
            }
        );
    }

    #[test]
    fn position_is_clamped_to_length() {
        let (state, start) = playing("A", 200.0);
        assert_eq!(state.position_us(at(start, 500.0)), 200_000_000);
    }

    #[test]
    fn position_past_a_short_length_does_not_flood_seeked() {
        let (mut state, start) = playing("A", 10.0);
        for tick in 50..=100 {
            let secs = f64::from(tick) * 0.2;
            assert!(!state.set_position(secs, PLAYING, at(start, secs)).seeked);
        }
    }

    #[test]
    fn stopped_reports_zero_and_never_seeks() {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(50.0, PLAYING, at(start, 50.0));
        let changes = state.set_status(STOPPED, at(start, 51.0));
        assert!(changes.playback_status);
        assert!(!state.set_position(0.0, STOPPED, at(start, 51.0)).seeked);
        assert_eq!(state.position_us(at(start, 60.0)), 0);
    }

    #[rstest]
    #[case::negative(-1.0)]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    fn invalid_elapsed_is_ignored(#[case] elapsed: f64) {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PLAYING, at(start, 10.0));
        assert!(
            state
                .set_position(elapsed, PLAYING, at(start, 10.0))
                .is_empty()
        );
        assert_eq!(state.position_us(at(start, 10.0)), 10_000_000);
    }

    #[test]
    fn no_track_reports_the_no_track_id() {
        let state = State::new(Instant::now());
        assert_eq!(state.track_id(), NO_TRACK);
        assert!(!state.has_track() && !state.can_seek());
    }

    #[rstest]
    #[case::forward(5_000_000, Some(MediaCommand::Seek(15.0)))]
    #[case::before_start(-60_000_000, Some(MediaCommand::Seek(0.0)))]
    #[case::past_end(500_000_000, Some(MediaCommand::Next))]
    fn relative_seek(#[case] offset_us: i64, #[case] expected: Option<MediaCommand>) {
        let (mut state, start) = playing("A", 200.0);
        state.set_position(10.0, PAUSED, at(start, 10.0));
        assert_eq!(state.seek_by(offset_us, at(start, 10.0)), expected);
    }

    #[test]
    fn relative_seek_needs_a_length() {
        let (state, start) = playing("Stream", 0.0);
        assert_eq!(state.seek_by(5_000_000, start), None);
    }

    #[rstest]
    #[case::current(true, 30_000_000, Some(MediaCommand::Seek(30.0)))]
    #[case::stale_track(false, 30_000_000, None)]
    #[case::negative(true, -1, None)]
    #[case::past_end(true, 200_000_001, None)]
    fn absolute_seek(
        #[case] current_id: bool,
        #[case] position_us: i64,
        #[case] expected: Option<MediaCommand>,
    ) {
        let (mut state, start) = playing("A", 200.0);
        let stale_id = state.track_id();
        state.set_track(&info("B", 200.0), PLAYING, start);
        let track_id = if current_id {
            state.track_id()
        } else {
            stale_id
        };
        assert_eq!(state.seek_to(&track_id, position_us), expected);
    }

    #[test]
    fn changes_merge_as_a_union() {
        let mut changes = Changes {
            playback_status: true,
            ..Changes::default()
        };
        changes.merge(Changes {
            seeked: true,
            ..Changes::default()
        });
        assert_eq!(
            changes,
            Changes {
                playback_status: true,
                seeked: true,
                ..Changes::default()
            }
        );
    }

    #[rstest]
    #[case::same(1.0, false, 1.0)]
    #[case::lower(0.5, true, 0.5)]
    #[case::f32_noise(0.49000000953674316, true, 0.49)]
    #[case::below_a_step(0.999, false, 1.0)]
    #[case::negative(-0.3, true, 0.0)]
    #[case::zero(0.0, true, 0.0)]
    #[case::too_loud(1.7, false, 1.0)]
    #[case::nan(f64::NAN, false, 1.0)]
    fn volume_changes_by_whole_steps(#[case] set: f64, #[case] changed: bool, #[case] reads: f64) {
        let mut state = State::new(Instant::now());
        assert_eq!(state.set_volume(set).volume, changed);
        assert_eq!(state.volume(), reads);
    }

    #[test]
    fn volume_change_touches_nothing_else() {
        let (mut state, _) = playing("A", 200.0);
        assert_eq!(
            state.set_volume(0.3),
            Changes {
                volume: true,
                ..Changes::default()
            }
        );
    }

    fn volume_changed() -> Changes {
        Changes {
            volume: true,
            ..Changes::default()
        }
    }

    #[test]
    fn pacer_sends_the_first_change_and_holds_the_next() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(1.0);
        let mut first = volume_changed();
        assert_eq!(pacer.pace(&mut first, 0.9, start), None);
        assert!(first.volume);

        let mut second = volume_changed();
        let flush_at = pacer.pace(&mut second, 0.8, at(start, 0.05));
        assert_eq!(flush_at, Some(start + VOLUME_SIGNAL_INTERVAL));
        assert!(!second.volume);
    }

    #[test]
    fn pacer_flushes_the_held_change_with_the_latest_value() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(1.0);
        pacer.pace(&mut volume_changed(), 0.9, start);
        pacer.pace(&mut volume_changed(), 0.8, at(start, 0.05));
        pacer.pace(&mut volume_changed(), 0.7, at(start, 0.1));

        let mut timer_fired = Changes::default();
        assert_eq!(pacer.pace(&mut timer_fired, 0.7, at(start, 0.2)), None);
        assert!(timer_fired.volume);

        let mut idle = Changes::default();
        assert_eq!(pacer.pace(&mut idle, 0.7, at(start, 0.5)), None);
        assert!(!idle.volume);
    }

    #[test]
    fn pacer_drops_a_change_that_came_back_to_the_sent_value() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(1.0);
        pacer.pace(&mut volume_changed(), 0.9, start);
        pacer.pace(&mut volume_changed(), 0.8, at(start, 0.05));

        let mut back = volume_changed();
        assert_eq!(pacer.pace(&mut back, 0.9, at(start, 0.1)), None);
        assert!(!back.volume);
        let mut timer_fired = Changes::default();
        pacer.pace(&mut timer_fired, 0.9, at(start, 0.2));
        assert!(!timer_fired.volume);
    }

    #[test]
    fn pacer_says_nothing_about_the_volume_clients_already_read() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(0.4);
        let mut queued_before_connect = volume_changed();
        assert_eq!(pacer.pace(&mut queued_before_connect, 0.4, start), None);
        assert!(!queued_before_connect.volume);
    }

    #[test]
    fn pacer_restates_an_unchanged_volume_once() {
        let start = Instant::now();
        let mut state = State::new(start);
        let mut pacer = VolumePacer::new(state.volume());
        let mut refused = state.restate_volume(1.0);
        assert_eq!(pacer.pace(&mut refused, state.volume(), start), None);
        assert_eq!(refused, volume_changed());

        let mut idle = Changes::default();
        assert_eq!(pacer.pace(&mut idle, 1.0, at(start, 1.0)), None);
        assert!(idle.is_empty());
    }

    #[test]
    fn pacer_holds_a_burst_of_restatements() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(1.0);
        let restated = Changes {
            volume_restated: true,
            ..Changes::default()
        };
        let mut first = restated;
        pacer.pace(&mut first, 1.0, start);
        let mut second = restated;
        let flush_at = pacer.pace(&mut second, 1.0, at(start, 0.05));
        assert_eq!(flush_at, Some(start + VOLUME_SIGNAL_INTERVAL));
        assert!(second.is_empty());

        let mut timer_fired = Changes::default();
        assert_eq!(pacer.pace(&mut timer_fired, 1.0, at(start, 0.2)), None);
        assert_eq!(timer_fired, volume_changed());
    }

    #[test]
    fn negative_zero_volume_is_plain_zero() {
        assert_eq!(volume_level(-0.0).map(f64::to_bits), Some(0.0f64.to_bits()));
    }

    #[test]
    fn pacer_leaves_other_changes_alone() {
        let start = Instant::now();
        let mut pacer = VolumePacer::new(1.0);
        pacer.pace(&mut volume_changed(), 0.9, start);
        let mut mixed = Changes {
            playback_status: true,
            volume: true,
            ..Changes::default()
        };
        assert!(pacer.pace(&mut mixed, 0.8, at(start, 0.05)).is_some());
        assert_eq!(
            mixed,
            Changes {
                playback_status: true,
                ..Changes::default()
            }
        );
    }

    #[rstest]
    #[case::plain(
        "/home/u/.cache/pawse/artwork/12.jpg",
        "file:///home/u/.cache/pawse/artwork/12.jpg"
    )]
    #[case::space("/home/u/My Music/a b.jpg", "file:///home/u/My%20Music/a%20b.jpg")]
    #[case::reserved("/x/#?%&.jpg", "file:///x/%23%3F%25%26.jpg")]
    #[case::unicode("/x/ё.jpg", "file:///x/%D1%91.jpg")]
    fn art_url_is_percent_encoded(#[case] path: &str, #[case] url: &str) {
        assert_eq!(file_url(&PathBuf::from(path)), url);
    }

    #[test]
    fn art_url_reaches_the_track() {
        let start = Instant::now();
        let mut state = State::new(start);
        let mut with_art = info("A", 200.0);
        with_art.artwork_path = Some(PathBuf::from("/c/1.jpg"));
        state.set_track(&with_art, PLAYING, start);
        assert_eq!(
            state.track().and_then(|track| track.art_url.as_deref()),
            Some("file:///c/1.jpg")
        );
        assert_eq!(state.status(), PLAYING);
    }
}
