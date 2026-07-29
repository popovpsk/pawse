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

pub struct CoverArtCache {
    small: HashMap<i64, Arc<Image>>,
    large: HashMap<i64, Arc<Image>>,
}

impl Default for CoverArtCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverArtCache {
    pub fn new() -> Self {
        Self {
            small: HashMap::new(),
            large: HashMap::new(),
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
    ) -> Option<Arc<Image>> {
        let id = cover_art_id?;
        if let Some(img) = self.large.get(&id) {
            return Some(img.clone());
        }
        let bytes = library.get_cover_art_large(id)?;
        let image = Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes));
        self.large.insert(id, image.clone());
        Some(image)
    }

    pub fn clear(&mut self) {
        self.small.clear();
        self.large.clear();
    }
}
