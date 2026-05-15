use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Image, ImageFormat};

use crate::library_service::LibraryService;

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
