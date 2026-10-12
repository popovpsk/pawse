use std::collections::HashSet;
use std::sync::Arc;

use gpui::{App, RenderImage};

use crate::cover_art_cache::{LARGE_COVER_MIN_CAPACITY, TileLru};

pub struct PhotoTiles {
    side: u32,
    tiles: TileLru<String>,
    in_flight: HashSet<String>,
    missing: HashSet<String>,
    stale: HashSet<String>,
    generation: u64,
}

impl PhotoTiles {
    pub fn new(side: u32) -> Self {
        Self {
            side,
            tiles: TileLru::new(LARGE_COVER_MIN_CAPACITY),
            in_flight: HashSet::new(),
            missing: HashSet::new(),
            stale: HashSet::new(),
            generation: 0,
        }
    }

    pub fn side(&self) -> u32 {
        self.side
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, name: &str) -> Option<Arc<RenderImage>> {
        self.tiles.get(name)
    }

    pub fn wanted<'a>(
        &mut self,
        visible: usize,
        names: impl Iterator<Item = &'a str>,
        cx: &mut App,
    ) -> Vec<String> {
        self.tiles.fit_capacity(visible, cx);
        let mut wanted = Vec::new();
        for name in names {
            if self.tiles.touch(name).is_none() && self.claim(name) {
                wanted.push(name.to_string());
            }
        }
        wanted
    }

    pub fn finish(&mut self, name: String, photo: Option<Arc<RenderImage>>, cx: &mut App) -> bool {
        let changed = self.settle(&name, photo.is_some());
        if let Some(photo) = photo {
            self.tiles.insert(name, photo, cx);
        }
        changed
    }

    pub fn forget(&mut self, name: &str) -> bool {
        if self.in_flight.contains(name) {
            self.stale.insert(name.to_string());
        }
        self.missing.remove(name)
    }

    pub fn clear(&mut self, cx: &mut App) {
        self.tiles.clear(cx);
        self.in_flight.clear();
        self.missing.clear();
        self.stale.clear();
        self.generation += 1;
    }

    fn claim(&mut self, name: &str) -> bool {
        if self.in_flight.contains(name) || self.missing.contains(name) {
            return false;
        }
        self.in_flight.insert(name.to_string());
        true
    }

    fn settle(&mut self, name: &str, found: bool) -> bool {
        self.in_flight.remove(name);
        let stale = self.stale.remove(name);
        if found || stale {
            return true;
        }
        self.missing.insert(name.to_string());
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_in_flight_is_not_asked_for_twice() {
        let mut tiles = PhotoTiles::new(80);
        assert!(tiles.claim("Muse"));
        assert!(!tiles.claim("Muse"));
    }

    #[test]
    fn an_artist_without_a_photo_is_not_read_again_every_frame() {
        let mut tiles = PhotoTiles::new(80);
        tiles.claim("Muse");
        assert!(!tiles.settle("Muse", false));
        assert!(!tiles.claim("Muse"));
    }

    #[test]
    fn a_save_after_a_miss_lets_the_photo_be_read_again() {
        let mut tiles = PhotoTiles::new(80);
        tiles.claim("Muse");
        tiles.settle("Muse", false);
        assert!(tiles.forget("Muse"));
        assert!(tiles.claim("Muse"));
    }

    #[test]
    fn a_save_landing_during_the_read_is_not_lost() {
        let mut tiles = PhotoTiles::new(80);
        tiles.claim("Muse");
        assert!(!tiles.forget("Muse"), "nothing is missing yet");
        assert!(
            tiles.settle("Muse", false),
            "the stale miss must repaint so the next frame asks again"
        );
        assert!(
            tiles.claim("Muse"),
            "a miss read before the save must not mark the artist missing"
        );
    }

    #[test]
    fn a_found_photo_always_repaints() {
        let mut tiles = PhotoTiles::new(80);
        tiles.claim("Muse");
        assert!(tiles.settle("Muse", true));
        assert!(tiles.claim("Muse"), "settled, so no longer in flight");
    }
}
