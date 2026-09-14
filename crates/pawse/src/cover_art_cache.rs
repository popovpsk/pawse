use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, Image, ImageFormat, RenderImage};

use crate::library_service::LibraryService;

/// Frees the sprite-atlas tile of a `RenderImage` that is no longer displayed.
///
/// Deferred on purpose: `App::drop_image` walks `App.windows`, and gpui takes the
/// current window *out* of that map for the whole duration of a window update
/// (`update_window_id`). Called straight from a click or key handler it would skip
/// the only window we have and the tile would stay in the atlas forever — the
/// atlas has no eviction of its own. Running it as a deferred effect puts the call
/// after the window is back in the map, which works from every context.
pub fn drop_atlas_tile(image: Arc<RenderImage>, cx: &mut App) {
    cx.defer(move |cx| cx.drop_image(image, None));
}

pub const LARGE_COVER_MIN_CAPACITY: usize = 32;
pub const LARGE_COVER_MAX_CAPACITY: usize = 512;
pub const LARGE_COVER_SCREENS: usize = 3;

fn capacity_for_visible(visible: usize) -> usize {
    let ceiling = LARGE_COVER_MAX_CAPACITY.max(visible.saturating_add(1));
    visible
        .saturating_mul(LARGE_COVER_SCREENS)
        .clamp(LARGE_COVER_MIN_CAPACITY, ceiling)
}

pub fn capacity_for_peak_visible(peak: &mut usize, visible: usize) -> usize {
    *peak = (*peak).max(visible);
    capacity_for_visible(*peak)
}

pub fn decode_cover_tile(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let decoded = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let mut raster = decoded.to_rgba8();
    for pixel in raster.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(raster)])))
}

struct LargeEntry {
    image: Arc<RenderImage>,
    used: u64,
}

pub struct CoverArtCache {
    small: HashMap<i64, Arc<Image>>,
    large: HashMap<i64, LargeEntry>,
    large_capacity: usize,
    tick: u64,
}

impl Default for CoverArtCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverArtCache {
    pub fn new() -> Self {
        Self::with_large_capacity(LARGE_COVER_MIN_CAPACITY)
    }

    pub fn with_large_capacity(large_capacity: usize) -> Self {
        Self {
            small: HashMap::new(),
            large: HashMap::new(),
            large_capacity: large_capacity.max(1),
            tick: 0,
        }
    }

    pub fn get_small(
        &mut self,
        cover_art_id: Option<i64>,
        library: &LibraryService,
    ) -> Option<Arc<Image>> {
        let id = cover_art_id?;
        if let Some(img) = self.small.get(&id) {
            return Some(img.clone());
        }
        let bytes = library.get_cover_art_small(id)?;
        let image = Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes));
        self.small.insert(id, image.clone());
        Some(image)
    }

    pub fn get_large(
        &mut self,
        cover_art_id: Option<i64>,
        library: &LibraryService,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let id = cover_art_id?;
        if let Some(hit) = self.touch_large(id) {
            return Some(hit);
        }
        let bytes = library.get_cover_art_large(id)?;
        let image = decode_cover_tile(&bytes)?;
        self.insert_large(id, image.clone(), cx);
        Some(image)
    }

    pub fn peek_large(&mut self, cover_art_id: Option<i64>) -> Option<Arc<RenderImage>> {
        self.touch_large(cover_art_id?)
    }

    pub fn holds_large(&self, id: i64) -> bool {
        self.large.contains_key(&id)
    }

    pub fn set_large_capacity(&mut self, capacity: usize, cx: &mut App) {
        let capacity = capacity.max(1);
        if capacity == self.large_capacity {
            return;
        }
        self.large_capacity = capacity;
        self.evict_large(cx);
    }

    pub fn insert_large(&mut self, id: i64, image: Arc<RenderImage>, cx: &mut App) {
        if self.touch_large(id).is_some() {
            return;
        }
        self.tick += 1;
        let used = self.tick;
        self.large.insert(id, LargeEntry { image, used });
        self.evict_large(cx);
    }

    fn evict_large(&mut self, cx: &mut App) {
        while self.large.len() > self.large_capacity {
            let Some(coldest) = self.coldest_evictable() else {
                break;
            };
            if let Some(entry) = self.large.remove(&coldest) {
                drop_atlas_tile(entry.image, cx);
            }
        }
    }

    pub fn clear(&mut self, cx: &mut App) {
        self.small.clear();
        for (_, entry) in self.large.drain() {
            drop_atlas_tile(entry.image, cx);
        }
    }

    fn touch_large(&mut self, id: i64) -> Option<Arc<RenderImage>> {
        self.tick += 1;
        let used = self.tick;
        let entry = self.large.get_mut(&id)?;
        entry.used = used;
        Some(entry.image.clone())
    }

    fn coldest_evictable(&self) -> Option<i64> {
        self.large
            .iter()
            .filter(|(_, entry)| Arc::strong_count(&entry.image) == 1)
            .min_by_key(|(_, entry)| entry.used)
            .map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(1, 1),
        )]))
    }

    fn seed(cache: &mut CoverArtCache, entries: &[(i64, u64)]) {
        for (id, used) in entries {
            cache.large.insert(
                *id,
                LargeEntry {
                    image: tile(),
                    used: *used,
                },
            );
        }
    }

    #[test]
    fn capacity_always_outgrows_what_is_on_screen() {
        for visible in [
            1usize,
            12,
            24,
            50,
            84,
            220,
            400,
            LARGE_COVER_MAX_CAPACITY,
            LARGE_COVER_MAX_CAPACITY * 2,
            usize::MAX - 1,
        ] {
            let capacity = capacity_for_visible(visible);
            assert!(
                capacity > visible,
                "{visible} visible tiles would thrash against a capacity of {capacity}"
            );
        }
    }

    #[test]
    fn capacity_stays_between_its_bounds() {
        assert_eq!(capacity_for_visible(0), LARGE_COVER_MIN_CAPACITY);
        assert_eq!(capacity_for_visible(100), LARGE_COVER_MAX_CAPACITY.min(300));
    }

    #[test]
    fn a_measuring_pass_cannot_shrink_the_capacity() {
        let mut peak = 0;
        let scrolling = capacity_for_peak_visible(&mut peak, 30);
        let measuring = capacity_for_peak_visible(&mut peak, 1);
        assert_eq!(
            measuring, scrolling,
            "the virtual list measures with a one-item range every frame; \
             letting that set the capacity evicts the visible tiles and flickers"
        );
    }

    #[test]
    fn capacity_still_grows_with_a_bigger_viewport() {
        let mut peak = 0;
        let small = capacity_for_peak_visible(&mut peak, 30);
        let large = capacity_for_peak_visible(&mut peak, 120);
        assert!(large > small);
    }

    #[test]
    fn a_racing_insert_keeps_the_copy_views_already_hold() {
        let mut cache = CoverArtCache::with_large_capacity(4);
        seed(&mut cache, &[(7, 1)]);
        let held = cache
            .large
            .get(&7)
            .map(|entry| entry.image.clone())
            .unwrap();
        let racer = tile();
        assert!(!Arc::ptr_eq(&held, &racer));

        cache.tick += 1;
        let used = cache.tick;
        if cache.touch_large(7).is_none() {
            cache.large.insert(7, LargeEntry { image: racer, used });
        }

        let kept = cache
            .large
            .get(&7)
            .map(|entry| entry.image.clone())
            .unwrap();
        assert!(
            Arc::ptr_eq(&held, &kept),
            "replacing an entry drops the atlas tile of a copy a view is still painting"
        );
    }

    #[test]
    fn eviction_picks_the_least_recently_used() {
        let mut cache = CoverArtCache::with_large_capacity(2);
        seed(&mut cache, &[(1, 30), (2, 10), (3, 20)]);
        assert_eq!(cache.coldest_evictable(), Some(2));
    }

    #[test]
    fn eviction_skips_covers_a_view_still_holds() {
        let mut cache = CoverArtCache::with_large_capacity(2);
        seed(&mut cache, &[(1, 30), (2, 10), (3, 20)]);
        let _held = cache.large.get(&2).map(|entry| entry.image.clone());
        assert_eq!(cache.coldest_evictable(), Some(3));
    }

    #[test]
    fn nothing_is_evicted_while_every_cover_is_in_use() {
        let mut cache = CoverArtCache::with_large_capacity(1);
        seed(&mut cache, &[(1, 10), (2, 20)]);
        let _held: Vec<_> = cache
            .large
            .values()
            .map(|entry| entry.image.clone())
            .collect();
        assert_eq!(cache.coldest_evictable(), None);
    }

    #[test]
    fn touching_a_cover_makes_it_the_newest() {
        let mut cache = CoverArtCache::with_large_capacity(2);
        seed(&mut cache, &[(1, 1), (2, 2), (3, 3)]);
        cache.tick = 3;
        let hit = cache.touch_large(1);
        assert!(hit.is_some());
        drop(hit);
        assert_eq!(cache.coldest_evictable(), Some(2));
    }
}
