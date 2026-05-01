use music_library::Track;

pub struct PlaybackQueue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
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
            current_index: None,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = tracks;
        self.current_index = None;
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
        let next = current + 1;
        if next < self.tracks.len() {
            self.current_index = Some(next);
            self.tracks.get(next)
        } else {
            None
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
            Some(current) => current + 1 < self.tracks.len(),
            None => false,
        }
    }

    pub fn has_previous(&self) -> bool {
        self.current_index.is_some()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }
}
