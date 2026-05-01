use std::path::Path;

use lofty::file::AudioFile;
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::tag::ItemKey;

use crate::types::ScannedTrack;

pub fn read_metadata(path: impl AsRef<Path>) -> anyhow::Result<ScannedTrack> {
    let path = path.as_ref();
    let tagged_file = lofty::read_from_path(path)?;

    let properties = tagged_file.properties();
    let duration_ms = properties.duration().as_millis() as u64;

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let mut title = None;
    let mut artist_names = Vec::new();
    let mut album_title = None;
    let mut track_number = None;
    let mut disc_number = None;
    let mut year = None;
    let mut cover_art = None;

    if let Some(tag) = tag {
        title = tag.title().map(|s| s.to_string());
        album_title = tag.album().map(|s| s.to_string());

        // Artists: prefer all artists, fall back to main artist
        let artists: Vec<String> = tag
            .get_strings(&ItemKey::TrackArtists)
            .map(|s| s.to_string())
            .collect();
        if !artists.is_empty() {
            artist_names = artists;
        } else if let Some(artist) = tag.artist() {
            artist_names.push(artist.to_string());
        }

        // Track number
        if let Some(item) = tag.get(&ItemKey::TrackNumber)
            && let Some(val) = item.value().text()
        {
            track_number = val.split('/').next().and_then(|s| s.parse().ok());
        }

        // Disc number
        if let Some(item) = tag.get(&ItemKey::DiscNumber)
            && let Some(val) = item.value().text()
        {
            disc_number = val.split('/').next().and_then(|s| s.parse().ok());
        }

        // Year
        if let Some(item) = tag.get(&ItemKey::Year)
            && let Some(val) = item.value().text()
        {
            year = val.parse().ok();
        }

        // Cover art
        if let Some(pic) = tag.pictures().iter().find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
        {
            cover_art = Some(pic.data().to_vec());
        }
    }

    Ok(ScannedTrack {
        path: path.to_path_buf(),
        title,
        artist_names,
        album_title,
        track_number,
        disc_number,
        year,
        duration_ms: Some(duration_ms),
        cover_art,
    })
}
