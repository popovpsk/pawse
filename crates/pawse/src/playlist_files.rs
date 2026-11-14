use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use music_indexer::StampedFile;
use music_library::{FilePlaylist, FilePlaylistChanges, LibraryRepository};
use playlist_file::{PathIndex, Playlist};

use crate::remote_sync::{FoundPlaylist, PlaylistImport, copy_into_playlists};

struct Resolved {
    items: Vec<i64>,
    found: usize,
    total: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesOutcome {
    Imported(PlaylistImport),
    NoPlaylists,
    Failed,
}

pub fn import_files(
    repo: &dyn LibraryRepository,
    picked: &[PathBuf],
) -> music_library::Result<(FilesOutcome, Vec<i64>)> {
    let mut files = Vec::new();
    let mut unreadable = false;
    for path in picked {
        if path.is_dir() {
            let start = files.len();
            if let Err(e) = playlists_in(path, &mut files) {
                log::warn!("Failed to list {}: {e}", path.display());
                unreadable = true;
            }
            files[start..].sort();
        } else {
            files.push(path.clone());
        }
    }
    if files.is_empty() {
        let outcome = if unreadable {
            FilesOutcome::Failed
        } else {
            FilesOutcome::NoPlaylists
        };
        return Ok((outcome, Vec::new()));
    }
    let mut playlists = Vec::new();
    for path in &files {
        match playlist_file::read(path) {
            Ok(playlist) => playlists.push(playlist),
            Err(e) => log::warn!("Failed to read playlist {}: {e}", path.display()),
        }
    }
    if playlists.is_empty() {
        return Ok((FilesOutcome::Failed, Vec::new()));
    }
    let resolved = resolve(repo, &playlists, true)?;
    let found = playlists
        .into_iter()
        .zip(resolved)
        .map(|(playlist, resolved)| FoundPlaylist {
            name: playlist.name,
            items: resolved.items,
            found: resolved.found,
            total: resolved.total,
        })
        .collect();
    let (report, touched) = copy_into_playlists(repo, found)?;
    Ok((FilesOutcome::Imported(report), touched))
}

fn playlists_in(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if let Err(e) = playlists_in(&path, found) {
                log::warn!("Failed to list {}: {e}", path.display());
            }
        } else if kind.is_file() && playlist_file::is_playlist(&path) {
            found.push(path);
        }
    }
    Ok(())
}

pub fn sync_folder(
    repo: &dyn LibraryRepository,
    files: &[StampedFile],
    offline: &[PathBuf],
    full: bool,
) -> music_library::Result<FilePlaylistChanges> {
    let stamps = repo.file_playlist_stamps()?;
    let listed: HashSet<String> = files.iter().map(|file| path_key(&file.path)).collect();
    let remove: Vec<String> = stamps
        .keys()
        .filter(|path| {
            !listed.contains(*path) && !offline.iter().any(|root| Path::new(path).starts_with(root))
        })
        .cloned()
        .collect();
    let mut stamped = Vec::new();
    let mut playlists = Vec::new();
    for file in files {
        let path = path_key(&file.path);
        if !full && stamps.get(&path) == Some(&(file.mtime, file.size)) {
            continue;
        }
        match playlist_file::read(&file.path) {
            Ok(playlist) => {
                stamped.push((path, file.mtime, file.size));
                playlists.push(playlist);
            }
            Err(e) => log::warn!("Failed to read playlist {}: {e}", file.path.display()),
        }
    }
    if playlists.is_empty() && remove.is_empty() {
        return Ok(FilePlaylistChanges::default());
    }
    let resolved = resolve(repo, &playlists, false)?;
    let upsert: Vec<FilePlaylist> = stamped
        .into_iter()
        .zip(playlists)
        .zip(resolved)
        .map(|(((path, mtime, size), playlist), resolved)| FilePlaylist {
            path,
            name: playlist.name,
            mtime,
            size,
            items: resolved.items,
        })
        .collect();
    repo.apply_file_playlists(&upsert, &remove)
}

fn resolve(
    repo: &dyn LibraryRepository,
    playlists: &[Playlist],
    report_missing: bool,
) -> music_library::Result<Vec<Resolved>> {
    let expanded: Vec<Vec<Vec<String>>> = playlists
        .iter()
        .map(|playlist| {
            let mut seen = HashSet::new();
            playlist
                .entries
                .iter()
                .filter(|entry| seen.insert(*entry))
                .map(|entry| expand(entry))
                .collect()
        })
        .collect();
    let paths: Vec<String> = expanded
        .iter()
        .flatten()
        .flatten()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut items = repo.local_items_by_path(&paths)?;
    let missing: Vec<&String> = paths
        .iter()
        .filter(|path| !items.contains_key(*path))
        .collect();
    if !missing.is_empty() {
        let index = PathIndex::new(repo.local_paths()?);
        let aliases: HashMap<&String, String> = missing
            .iter()
            .filter_map(|path| index.find(path).map(|key| (*path, key.to_string())))
            .collect();
        let keys: Vec<String> = aliases.values().cloned().collect();
        let by_key = repo.local_items_by_path(&keys)?;
        for path in missing {
            match aliases.get(path).and_then(|key| by_key.get(key)) {
                Some(found) => {
                    items.insert(path.clone(), found.clone());
                }
                None if report_missing => log::info!("Playlist entry not in the library: {path}"),
                None => {}
            }
        }
    }
    Ok(expanded
        .into_iter()
        .map(|entries| {
            let mut resolved = Resolved {
                items: Vec::new(),
                found: 0,
                total: entries.len(),
            };
            let mut seen = HashSet::new();
            for paths in entries {
                let ids: Vec<i64> = paths
                    .iter()
                    .filter_map(|path| items.get(path))
                    .flatten()
                    .copied()
                    .collect();
                if !ids.is_empty() {
                    resolved.found += 1;
                }
                resolved
                    .items
                    .extend(ids.into_iter().filter(|id| seen.insert(*id)));
            }
            resolved
        })
        .collect())
}

fn expand(entry: &Path) -> Vec<String> {
    let is_cue = entry
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            music_indexer::CUE_EXTENSIONS
                .iter()
                .any(|cue| cue.eq_ignore_ascii_case(ext))
        });
    if is_cue && let Ok(Some(cue)) = music_indexer::cue::read_cue(entry) {
        let mut seen = HashSet::new();
        return cue
            .audio
            .into_values()
            .map(|audio| path_key(&audio))
            .filter(|audio| seen.insert(audio.clone()))
            .collect();
    }
    vec![path_key(entry)]
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use music_library::{LocalFolder, ScanTrack, SqliteLibrary};

    use super::*;

    struct Fixture {
        repo: SqliteLibrary,
        root: PathBuf,
    }

    impl Fixture {
        fn new(tracks: &[&str]) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "pawse-playlist-files-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("music")).unwrap();
            let repo = SqliteLibrary::open_at(root.join("library.db")).unwrap();
            let fixture = Self { repo, root };
            fixture
                .repo
                .reconcile_local_sources(&[LocalFolder {
                    path: path_key(&fixture.music()),
                    available: true,
                }])
                .unwrap();
            let mut session = fixture.repo.open_scan_session().unwrap();
            session.clear().unwrap();
            for (ix, name) in tracks.iter().enumerate() {
                session
                    .add_track(ScanTrack {
                        path: path_key(&fixture.music().join(name)),
                        title: Some(name.to_string()),
                        artist_names: vec!["Artist".into()],
                        file_size: Some(1000 + ix as u64),
                        duration_ms: Some(1000),
                        ..Default::default()
                    })
                    .unwrap();
            }
            session.finish().unwrap();
            fixture
        }

        fn music(&self) -> PathBuf {
            self.root.join("music")
        }

        fn id(&self, name: &str) -> i64 {
            let path = path_key(&self.music().join(name));
            self.repo
                .all_tracks()
                .unwrap()
                .into_iter()
                .find(|track| track.path == path)
                .unwrap()
                .id
        }

        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.music().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }

        fn entries(&self, playlist_id: i64) -> Vec<i64> {
            self.repo
                .tracks_for_playlist(playlist_id)
                .unwrap()
                .iter()
                .map(|track| track.id)
                .collect()
        }

        fn walk(&self) -> Vec<StampedFile> {
            music_indexer::collect_sources(&[self.music()]).playlist_files
        }
    }

    fn imported(fx: &Fixture, paths: &[PathBuf]) -> PlaylistImport {
        match import_files(&fx.repo, paths).unwrap().0 {
            FilesOutcome::Imported(report) => report,
            other => panic!("nothing imported: {other:?}"),
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_playlist_file_lands_on_library_tracks_and_skips_the_rest() {
        let fx = Fixture::new(&["A/01.flac", "A/02.flac", "B/03.flac"]);
        let mix = fx.write(
            "lists/Mix.m3u8",
            "#EXTM3U\n../A/02.flac\n../A/01.flac\n../Z/missing.flac\nhttp://radio.example/live\n../A/02.flac\n",
        );

        let (outcome, touched) = import_files(&fx.repo, std::slice::from_ref(&mix)).unwrap();
        assert_eq!(
            outcome,
            FilesOutcome::Imported(PlaylistImport {
                playlists: 1,
                added: 2,
                found: 2,
                total: 3
            })
        );
        let playlists = fx.repo.playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Mix");
        assert_eq!(touched, vec![playlists[0].id]);
        assert_eq!(
            fx.entries(playlists[0].id),
            vec![fx.id("A/02.flac"), fx.id("A/01.flac")]
        );

        let other = fx.write("other/Mix.pls", "[playlist]\nFile1=../B/03.flac\n");
        let again = imported(&fx, &[mix, other]);
        assert_eq!((again.playlists, again.added), (1, 1));
        assert_eq!(
            fx.entries(playlists[0].id),
            vec![fx.id("A/02.flac"), fx.id("A/01.flac"), fx.id("B/03.flac")]
        );

        assert_eq!(
            import_files(&fx.repo, &[fx.music().join("nope.m3u")])
                .unwrap()
                .0,
            FilesOutcome::Failed
        );
    }

    #[test]
    fn entries_from_another_place_or_case_find_their_tracks() {
        let fx = Fixture::new(&["Artist/Album/01.flac", "Artist/Album/02.flac"]);
        let list = fx.write(
            "Moved.m3u",
            "D:\\Old Music\\Artist\\Album\\01.flac\nartist/ALBUM/02.flac\n",
        );
        let report = imported(&fx, &[list]);
        assert_eq!((report.found, report.total), (2, 2));
        let playlist = fx.repo.playlists().unwrap()[0].id;
        assert_eq!(
            fx.entries(playlist),
            vec![fx.id("Artist/Album/01.flac"), fx.id("Artist/Album/02.flac")]
        );
    }

    #[test]
    fn a_picked_folder_imports_the_playlists_under_it() {
        let fx = Fixture::new(&["A/01.flac", "A/02.flac"]);
        fx.write("lists/Second.m3u", "../A/02.flac\n");
        fx.write("lists/old/First.pls", "[playlist]\nFile1=../../A/01.flac\n");
        fx.write("lists/.hidden.m3u", "../A/01.flac\n");
        fx.write("lists/notes.txt", "../A/01.flac\n");
        std::fs::create_dir_all(fx.root.join("empty")).unwrap();

        let report = imported(&fx, &[fx.music().join("lists")]);
        assert_eq!((report.playlists, report.added), (2, 2));
        let names: Vec<String> = fx
            .repo
            .playlists()
            .unwrap()
            .into_iter()
            .map(|playlist| playlist.name)
            .collect();
        assert_eq!(names, ["Second", "First"]);

        assert_eq!(
            import_files(&fx.repo, &[fx.root.join("empty")]).unwrap().0,
            FilesOutcome::NoPlaylists
        );
    }

    #[test]
    fn folder_playlists_mirror_their_files() {
        let fx = Fixture::new(&["A/01.flac", "A/02.flac"]);
        let album = fx.write("A/album.m3u", "01.flac\n02.flac\n");

        let created = sync_folder(&fx.repo, &fx.walk(), &[], true).unwrap();
        assert!(created.listing);
        let mirror = fx.repo.playlists().unwrap()[0].clone();
        assert_eq!(mirror.name, "album");
        assert_eq!(mirror.file_path.as_deref(), Some(path_key(&album).as_str()));
        assert_eq!(
            fx.entries(mirror.id),
            vec![fx.id("A/01.flac"), fx.id("A/02.flac")]
        );

        assert_eq!(
            sync_folder(&fx.repo, &fx.walk(), &[], false).unwrap(),
            FilePlaylistChanges::default()
        );

        std::fs::write(&album, "02.flac\n").unwrap();
        let edited = sync_folder(&fx.repo, &fx.walk(), &[], false).unwrap();
        assert_eq!(edited.tracks, vec![mirror.id]);
        assert_eq!(fx.entries(mirror.id), vec![fx.id("A/02.flac")]);

        let copy = imported(&fx, std::slice::from_ref(&album));
        assert_eq!(copy.playlists, 1);
        assert_eq!(fx.entries(mirror.id), vec![fx.id("A/02.flac")]);
        assert_eq!(fx.repo.playlists().unwrap().len(), 2);

        let offline = sync_folder(&fx.repo, &[], &[fx.music()], false).unwrap();
        assert_eq!(offline, FilePlaylistChanges::default());
        let gone = sync_folder(&fx.repo, &[], &[], false).unwrap();
        assert!(gone.listing);
        assert!(
            fx.repo
                .playlists()
                .unwrap()
                .iter()
                .all(|playlist| playlist.file_path.is_none())
        );
    }
}
