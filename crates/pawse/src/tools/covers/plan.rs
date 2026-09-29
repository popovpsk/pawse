use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use music_library::{AlbumSummary, NO_METADATA_ALBUM_ID, Track};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumRef {
    pub album_id: i64,
    pub artist: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    NoName,
    SharedFolder,
    Scattered,
    CoverExists(String),
    ImageExists(String),
    ReadOnly,
    NotFound,
    SearchFailed(String),
    DownloadFailed(String),
    BadImage,
    WriteFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub album: AlbumRef,
    pub folder: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub jobs: Vec<Job>,
    pub skipped: Vec<(AlbumRef, Skip)>,
}

pub fn plan(albums: &[AlbumSummary], tracks: &[Track]) -> Plan {
    let mut folders: HashMap<i64, BTreeSet<PathBuf>> = HashMap::new();
    let mut seen_from: HashMap<&Path, HashSet<Option<i64>>> = HashMap::new();
    for track in tracks {
        let Some(dir) = track.local_file().and_then(Path::parent) else {
            continue;
        };
        for seen in [Some(dir), dir.parent()].into_iter().flatten() {
            seen_from.entry(seen).or_default().insert(track.album_id);
        }
        if track.available
            && let Some(album_id) = track.album_id
        {
            folders
                .entry(album_id)
                .or_default()
                .insert(dir.to_path_buf());
        }
    }

    let mut plan = Plan::default();
    for album in albums {
        if album.cover_art_id.is_some() || album.id == NO_METADATA_ALBUM_ID {
            continue;
        }
        let Some(dirs) = folders.get(&album.id) else {
            continue;
        };
        let album_ref = AlbumRef {
            album_id: album.id,
            artist: album.artist_name.trim().to_string(),
            title: album.title.trim().to_string(),
        };
        if album_ref.artist.is_empty() || album_ref.title.is_empty() {
            plan.skipped.push((album_ref, Skip::NoName));
            continue;
        }
        let Some(folder) = target_folder(dirs) else {
            plan.skipped.push((album_ref, Skip::Scattered));
            continue;
        };
        let artwork_named = folder.file_name().is_some_and(|name| {
            music_indexer::metadata::is_artwork_dir_name(&name.to_string_lossy())
        });
        let mut lookers = vec![folder.as_path()];
        if artwork_named {
            lookers.extend(folder.parent());
        }
        let shared = lookers
            .iter()
            .filter_map(|dir| seen_from.get(dir))
            .any(|ids| ids.iter().any(|&id| id != Some(album.id)));
        if shared {
            plan.skipped.push((album_ref, Skip::SharedFolder));
            continue;
        }
        plan.jobs.push(Job {
            album: album_ref,
            folder,
        });
    }
    plan
}

fn target_folder(dirs: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let first = dirs.iter().next()?;
    [Some(first.as_path()), first.parent()]
        .into_iter()
        .flatten()
        .find(|candidate| {
            dirs.iter()
                .all(|dir| dir == candidate || dir.parent() == Some(*candidate))
        })
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: i64, artist: &str, title: &str, cover: Option<i64>) -> AlbumSummary {
        AlbumSummary {
            id,
            title: title.to_string(),
            year: None,
            cover_art_id: cover,
            artist_name: artist.to_string(),
            artist_id: None,
        }
    }

    fn track(id: i64, album_id: Option<i64>, path: &str) -> Track {
        Track {
            id,
            path: path.to_string(),
            title: format!("t{id}"),
            album_id,
            track_number: None,
            disc_number: 1,
            duration_ms: None,
            year: None,
            cover_art_id: None,
            start_offset_ms: 0,
            liked: false,
            bitrate: None,
            is_cue: false,
            available: true,
        }
    }

    fn job(id: i64, artist: &str, title: &str, folder: &str) -> Job {
        Job {
            album: AlbumRef {
                album_id: id,
                artist: artist.to_string(),
                title: title.to_string(),
            },
            folder: PathBuf::from(folder),
        }
    }

    #[test]
    fn album_in_its_own_folder_is_a_job() {
        let plan = plan(
            &[album(1, "A", "X", None)],
            &[
                track(1, Some(1), "/m/A/X/01.flac"),
                track(2, Some(1), "/m/A/X/02.flac"),
            ],
        );
        assert_eq!(plan.jobs, vec![job(1, "A", "X", "/m/A/X")]);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn albums_with_a_cover_are_left_alone() {
        let plan = plan(
            &[album(1, "A", "X", Some(7))],
            &[track(1, Some(1), "/m/A/X/01.flac")],
        );
        assert_eq!(plan, Plan::default());
    }

    #[test]
    fn multi_disc_album_goes_to_the_common_parent() {
        let plan = plan(
            &[album(1, "A", "X", None)],
            &[
                track(1, Some(1), "/m/A/X/CD1/01.flac"),
                track(2, Some(1), "/m/A/X/CD2/01.flac"),
            ],
        );
        assert_eq!(plan.jobs, vec![job(1, "A", "X", "/m/A/X")]);
    }

    #[test]
    fn album_folder_with_a_bonus_subfolder_goes_to_the_album_folder() {
        let plan = plan(
            &[album(1, "A", "X", None)],
            &[
                track(1, Some(1), "/m/A/X/01.flac"),
                track(2, Some(1), "/m/A/X/Bonus/01.flac"),
            ],
        );
        assert_eq!(plan.jobs, vec![job(1, "A", "X", "/m/A/X")]);
    }

    #[test]
    fn artwork_named_folder_is_shared_with_its_parent() {
        let plan = plan(
            &[
                album(1, "A", "Images", None),
                album(2, "A", "Loose", Some(3)),
            ],
            &[
                track(1, Some(1), "/m/A/Images/01.flac"),
                track(2, Some(2), "/m/A/loose.mp3"),
            ],
        );
        assert!(plan.jobs.is_empty());
        assert_eq!(plan.skipped[0].1, Skip::SharedFolder);
    }

    #[test]
    fn unrelated_folders_are_scattered() {
        let plan = plan(
            &[album(1, "A", "X", None)],
            &[
                track(1, Some(1), "/m/A/X/01.flac"),
                track(2, Some(1), "/n/B/Y/01.flac"),
            ],
        );
        assert!(plan.jobs.is_empty());
        assert_eq!(plan.skipped[0].1, Skip::Scattered);
    }

    #[test]
    fn flat_folder_with_other_albums_is_shared() {
        let plan = plan(
            &[album(1, "A", "X", None), album(2, "B", "Y", Some(3))],
            &[
                track(1, Some(1), "/m/Singles/a.mp3"),
                track(2, Some(2), "/m/Singles/b.mp3"),
            ],
        );
        assert!(plan.jobs.is_empty());
        assert_eq!(plan.skipped[0].1, Skip::SharedFolder);
    }

    #[test]
    fn parent_that_other_albums_look_into_is_shared() {
        let plan = plan(
            &[album(1, "A", "X", None), album(2, "A", "Z", None)],
            &[
                track(1, Some(1), "/m/A/X/CD1/01.flac"),
                track(2, Some(1), "/m/A/X/CD2/01.flac"),
                track(3, Some(2), "/m/A/X/Other/01.flac"),
            ],
        );
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].0.album_id, 1);
        assert_eq!(plan.skipped[0].1, Skip::SharedFolder);
        assert_eq!(plan.jobs, vec![job(2, "A", "Z", "/m/A/X/Other")]);
    }

    #[test]
    fn tracks_without_an_album_make_the_folder_shared() {
        let plan = plan(
            &[album(1, "A", "X", None)],
            &[
                track(1, Some(1), "/m/X/01.flac"),
                track(2, None, "/m/X/loose.mp3"),
            ],
        );
        assert_eq!(plan.skipped[0].1, Skip::SharedFolder);
    }

    #[test]
    fn remote_and_unavailable_albums_are_not_listed() {
        let mut offline = track(2, Some(2), "/gone/Y/01.flac");
        offline.available = false;
        let plan = plan(
            &[album(1, "A", "X", None), album(2, "B", "Y", None)],
            &[track(1, Some(1), "pawse-source://1/abc"), offline],
        );
        assert_eq!(plan, Plan::default());
    }

    #[test]
    fn album_without_artist_or_title_is_skipped_with_reason() {
        let plan = plan(
            &[album(1, "", "X", None), album(2, "B", "  ", None)],
            &[
                track(1, Some(1), "/m/X/01.flac"),
                track(2, Some(2), "/m/Y/01.flac"),
            ],
        );
        assert!(plan.jobs.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        assert!(plan.skipped.iter().all(|(_, skip)| *skip == Skip::NoName));
    }

    #[test]
    fn no_metadata_bucket_is_never_searched() {
        let plan = plan(
            &[album(NO_METADATA_ALBUM_ID, "", "", None)],
            &[track(1, None, "/m/X/01.flac")],
        );
        assert_eq!(plan, Plan::default());
    }
}
