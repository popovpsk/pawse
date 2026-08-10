pub mod dsf_id3;

use std::fs::File;
use std::path::Path;

use crate::container::{dff, dsf};
use crate::error::DsdError;
use crate::source::{DsdKind, sniff};

#[derive(Debug, Clone, Default)]
pub struct DsdTags {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub cover_art: Option<Vec<u8>>,
    pub duration_ms: Option<u64>,
}

impl From<dsf_id3::Id3Tag> for DsdTags {
    fn from(t: dsf_id3::Id3Tag) -> Self {
        Self {
            title: t.title,
            artists: t.artists,
            album: t.album,
            album_artists: t.album_artists,
            track_number: t.track_number,
            disc_number: t.disc_number,
            year: t.year,
            genres: t.genres,
            cover_art: t.cover_art,
            duration_ms: None,
        }
    }
}

pub fn read_tags(path: &Path) -> Result<DsdTags, DsdError> {
    let kind = sniff(path).ok_or(DsdError::NotDsd)?;
    let mut file = File::open(path)?;

    match kind {
        DsdKind::Dsf => {
            let info = dsf::parse_header(&mut file)?;
            let duration_ms =
                Some((info.bytes_per_channel() as f64 * 8000.0 / info.dsd_rate as f64) as u64);

            let mut tags = match info.id3_offset {
                Some(offset) => dsf_id3::read_from(&mut file, offset)?
                    .map(DsdTags::from)
                    .unwrap_or_default(),
                None => DsdTags::default(),
            };
            tags.duration_ms = duration_ms;
            Ok(tags)
        }
        DsdKind::Dff => {
            let info = dff::parse_header(&mut file)?;
            let bytes_per_channel = info.data_len / info.channels as u64;
            let duration_ms =
                Some((bytes_per_channel as f64 * 8000.0 / info.dsd_rate as f64) as u64);
            Ok(DsdTags {
                duration_ms,
                ..Default::default()
            })
        }
    }
}
