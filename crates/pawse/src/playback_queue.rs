use music_library::Track;
use rand::seq::SliceRandom;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

/// Where the current queue came from. Used by the UI to decide whether
/// per-track "remove from playlist" controls should be visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueueSource {
    #[default]
    Unknown,
    Playlist(i64),
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

pub struct PlaybackQueue {
    tracks: Vec<Track>,
    original_order: Option<Vec<Track>>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    source: QueueSource,
}

impl Default for PlaybackQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub enum PreviousAction<'a> {
    SeekToStart,
    PreviousTrack(&'a Track),
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            original_order: None,
            current_index: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            source: QueueSource::Unknown,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.set_tracks_with_source(tracks, QueueSource::Unknown);
    }

    pub fn set_tracks_with_source(&mut self, tracks: Vec<Track>, source: QueueSource) {
        self.tracks = tracks;
        self.original_order = None;
        self.current_index = None;
        self.source = source;
        if self.shuffle {
            self.apply_shuffle();
        }
    }

    /// Set tracks and immediately mark `index` as the current position,
    /// so that `apply_shuffle` anchors the clicked track to slot 0 when
    /// shuffle is enabled. Callers pass the index into the *natural* order.
    /// When `index` is in bounds, `current_track()` returns the intended
    /// track after this call; otherwise it returns `None`.
    pub fn set_tracks_and_play_at(
        &mut self,
        tracks: Vec<Track>,
        index: usize,
        source: QueueSource,
    ) -> Option<&Track> {
        self.original_order = None;
        self.source = source;
        self.current_index = if index < tracks.len() {
            Some(index)
        } else {
            None
        };
        self.tracks = tracks;
        if self.shuffle {
            self.apply_shuffle();
        }
        self.current_track()
    }

    pub fn source(&self) -> QueueSource {
        self.source
    }

    pub fn set_source(&mut self, source: QueueSource) {
        self.source = source;
    }

    /// Replace the queue's track list with a fresh snapshot, preserving the
    /// currently-playing track by id. Used when the playlist backing the
    /// queue has new tracks added/reordered. When shuffle is on, the new
    /// snapshot becomes the new `original_order` and is reshuffled so
    /// `set_shuffle(false)` can still restore a meaningful order later.
    pub fn refresh_keeping_current(&mut self, new_tracks: Vec<Track>) {
        let current_id = self.current_track().map(|t| t.id);
        self.tracks = new_tracks;
        self.current_index = current_id.and_then(|id| self.tracks.iter().position(|t| t.id == id));
        if self.shuffle {
            self.apply_shuffle();
        } else {
            self.original_order = None;
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn play_track_at(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            self.tracks.get(index)
        } else {
            None
        }
    }

    pub fn next_track(&mut self) -> Option<&Track> {
        let current = self.current_index?;

        if let RepeatMode::One = self.repeat {
            return self.tracks.get(current);
        }

        let next = current + 1;
        if next < self.tracks.len() {
            self.current_index = Some(next);
            return self.tracks.get(next);
        }

        match self.repeat {
            RepeatMode::All if !self.tracks.is_empty() => {
                self.current_index = Some(0);
                self.tracks.first()
            }
            _ => {
                self.current_index = None;
                None
            }
        }
    }

    pub fn previous(&mut self, position_secs: f32) -> PreviousAction<'_> {
        if position_secs > 3.0 {
            return PreviousAction::SeekToStart;
        }

        match self.current_index {
            Some(current) if current > 0 => {
                self.current_index = Some(current - 1);
                PreviousAction::PreviousTrack(self.tracks.get(current - 1).unwrap())
            }
            _ => PreviousAction::SeekToStart,
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    pub fn has_next(&self) -> bool {
        match self.current_index {
            Some(current) => {
                current + 1 < self.tracks.len()
                    || matches!(self.repeat, RepeatMode::All | RepeatMode::One)
                        && !self.tracks.is_empty()
            }
            None => false,
        }
    }

    pub fn has_previous(&self) -> bool {
        self.current_index.is_some()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn tracks_vec(&self) -> Vec<Track> {
        self.tracks.clone()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle == enabled {
            return;
        }
        self.shuffle = enabled;
        if enabled {
            self.apply_shuffle();
        } else {
            self.restore_original_order();
        }
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn original_order_vec(&self) -> Option<Vec<Track>> {
        self.original_order.clone()
    }

    pub fn restore(
        &mut self,
        tracks: Vec<Track>,
        original_order: Option<Vec<Track>>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat: RepeatMode,
        source: QueueSource,
    ) {
        self.tracks = tracks;
        self.original_order = original_order;
        self.current_index = current_index;
        self.shuffle = shuffle;
        self.repeat = repeat;
        self.source = source;
    }

    fn apply_shuffle(&mut self) {
        if self.tracks.len() <= 1 {
            self.original_order = Some(self.tracks.clone());
            return;
        }
        self.original_order = Some(self.tracks.clone());

        let current_id = self
            .current_index
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.id);

        let mut rng = rand::rng();
        self.tracks.shuffle(&mut rng);

        if let Some(id) = current_id
            && let Some(pos) = self.tracks.iter().position(|t| t.id == id)
        {
            self.tracks.swap(0, pos);
            self.current_index = Some(0);
        }
    }

    fn restore_original_order(&mut self) {
        let Some(original) = self.original_order.take() else {
            return;
        };
        let current_id = self
            .current_index
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.id);
        self.tracks = original;
        self.current_index =
            current_id.and_then(|id| self.tracks.iter().position(|t| t.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use music_library::Track;

    fn track(id: i64, path: &str) -> Track {
        Track {
            id,
            path: path.to_string(),
            title: format!("Track {}", id),
            album_id: None,
            track_number: Some(id as i32),
            disc_number: 0,
            duration_ms: Some(1000),
            year: None,
            cover_art_id: None,
            start_offset_ms: 0,
            liked: false,
        }
    }

    fn sample_tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| track(i as i64, &format!("/p/{}.flac", i)))
            .collect()
    }

    #[test]
    fn shuffle_on_off_restores_original_order_and_current_track() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(20));
        q.play_track_at(5);

        q.set_shuffle(true);
        assert!(q.shuffle());
        assert_eq!(q.current_track().map(|t| t.id), Some(5));
        // After shuffling, current track is moved to index 0.
        assert_eq!(q.current_index(), Some(0));

        q.set_shuffle(false);
        assert!(!q.shuffle());
        // Original order restored.
        let restored: Vec<i64> = q.tracks_vec().iter().map(|t| t.id).collect();
        assert_eq!(restored, (0..20).collect::<Vec<_>>());
        // Current track preserved by path.
        assert_eq!(q.current_track().map(|t| t.id), Some(5));
        assert_eq!(q.current_index(), Some(5));
    }

    #[test]
    fn repeat_all_wraps_from_last_to_first() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(3));
        q.play_track_at(2);
        q.set_repeat(RepeatMode::All);

        let next = q.next_track().cloned();
        assert_eq!(next.map(|t| t.id), Some(0));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn repeat_one_returns_same_track() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(3));
        q.play_track_at(1);
        q.set_repeat(RepeatMode::One);

        let next = q.next_track().cloned();
        assert_eq!(next.map(|t| t.id), Some(1));
        assert_eq!(q.current_index(), Some(1));
    }

    #[test]
    fn repeat_off_stops_at_end() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(2));
        q.play_track_at(1);

        assert!(q.next_track().is_none());
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn refresh_keeping_current_preserves_shuffle_and_seeds_new_original_order() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(5));
        q.play_track_at(2);
        q.set_shuffle(true);
        assert!(q.shuffle());

        // Pretend the backing playlist gained tracks and we refresh.
        let refreshed: Vec<Track> = (10..20)
            .map(|i| track(i, &format!("/r/{}.flac", i)))
            .collect();
        q.refresh_keeping_current(refreshed);

        // Shuffle stays on, original_order is set, set_shuffle(false) now
        // restores into the *new* set's order rather than no-opping.
        assert!(q.shuffle());
        assert!(q.original_order_vec().is_some());
        q.set_shuffle(false);
        let restored: Vec<i64> = q.tracks_vec().iter().map(|t| t.id).collect();
        assert_eq!(restored, (10..20).collect::<Vec<_>>());
    }

    #[test]
    fn set_tracks_while_shuffle_on_reshuffles_and_drops_prior_original() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(10));
        q.play_track_at(0);
        q.set_shuffle(true);

        // New tracks come in (e.g., user picks a new album with shuffle still on).
        let new_tracks: Vec<Track> = (100..110)
            .map(|i| track(i, &format!("/np/{}.flac", i)))
            .collect();
        q.set_tracks(new_tracks);

        // Still shuffled — calling set_shuffle(false) must restore the *new* set's order.
        assert!(q.shuffle());
        q.play_track_at(0);
        q.set_shuffle(false);
        let restored: Vec<i64> = q.tracks_vec().iter().map(|t| t.id).collect();
        assert_eq!(restored, (100..110).collect::<Vec<_>>());
    }

    #[test]
    fn set_tracks_and_play_at_with_shuffle_plays_clicked_track() {
        let mut q = PlaybackQueue::new();
        q.set_shuffle(true);
        let tracks = sample_tracks(20);
        let clicked_id = tracks[5].id;
        let result = q
            .set_tracks_and_play_at(tracks, 5, QueueSource::Unknown)
            .cloned();
        assert_eq!(result.map(|t| t.id), Some(clicked_id));
        assert_eq!(q.current_track().map(|t| t.id), Some(clicked_id));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn set_tracks_and_play_at_without_shuffle_uses_index() {
        let mut q = PlaybackQueue::new();
        let tracks = sample_tracks(10);
        let clicked_id = tracks[3].id;
        let result = q
            .set_tracks_and_play_at(tracks, 3, QueueSource::Unknown)
            .cloned();
        assert_eq!(result.map(|t| t.id), Some(clicked_id));
        assert_eq!(q.current_index(), Some(3));
    }

    #[test]
    fn shuffle_anchors_correct_track_when_paths_collide() {
        // Multi-track files (e.g. CUE sheets) share one path but have distinct ids.
        let tracks = vec![
            track(1, "/album.flac"),
            track(2, "/album.flac"),
            track(3, "/album.flac"),
        ];
        let mut q = PlaybackQueue::new();
        q.set_shuffle(true);
        let clicked = q.set_tracks_and_play_at(tracks, 2, QueueSource::Unknown).cloned();
        assert_eq!(clicked.map(|t| t.id), Some(3));
        assert_eq!(q.current_track().map(|t| t.id), Some(3));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn repeat_mode_cycle_order() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::Off);
    }

    #[test]
    fn has_next_respects_repeat_mode() {
        let mut q = PlaybackQueue::new();
        q.set_tracks(sample_tracks(2));
        q.play_track_at(1);
        assert!(!q.has_next());

        q.set_repeat(RepeatMode::All);
        assert!(q.has_next());

        q.set_repeat(RepeatMode::One);
        assert!(q.has_next());
    }
}
