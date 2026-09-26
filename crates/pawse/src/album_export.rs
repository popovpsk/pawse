use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, AppContext, Context, Entity, ParentElement, Styled, Subscription, Window, px};
use gpui_component::{
    WindowExt,
    button::Button,
    dialog::{Cancel, DialogFooter},
    notification::Notification,
    v_flex,
};
use music_library::Track;

use crate::cache_fill::FillProgress;
use crate::library_service::{LibraryEvent, LibraryService};
use crate::localization::tr;
use crate::remote_media::RemoteMedia;
use crate::services::{LibraryEventsBus, Services};
use crate::settings_store::SettingsStore;

const MAX_NAME_CHARS: usize = 100;
const COVER_NAME: &str = "cover.jpg";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumMeta {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub cover_art_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueTrack {
    pub number: u32,
    pub title: String,
    pub performer: Option<String>,
    pub start_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueSpec {
    pub performer: String,
    pub title: String,
    pub date: Option<i32>,
    pub genre: Option<String>,
    pub tracks: Vec<CueTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub locator: PathBuf,
    pub size: u64,
    pub target: PathBuf,
    pub cue: Option<CueSpec>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportPlan {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
}

impl ExportPlan {
    pub fn total(&self) -> u64 {
        self.entries.iter().map(|entry| entry.size).sum()
    }
}

pub fn has_remote<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> bool {
    tracks.into_iter().any(Track::is_remote)
}

pub fn component(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(MAX_NAME_CHARS)
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else if is_reserved_on_windows(trimmed) {
        match trimmed.split_once('.') {
            Some((stem, rest)) => format!("{stem}_.{rest}"),
            None => format!("{trimmed}_"),
        }
    } else {
        trimmed.to_string()
    }
}

fn is_reserved_on_windows(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
        })
}

fn claim(taken: &mut HashSet<PathBuf>, folder: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = component(stem, "Track");
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let path = folder.join(name);
        if taken.insert(path.clone()) {
            return path;
        }
        n += 1;
    }
}

pub fn build_plan(
    meta: &AlbumMeta,
    tracks: &[Track],
    artists: &HashMap<i64, Vec<String>>,
    sizes: &HashMap<PathBuf, u64>,
) -> ExportPlan {
    let artist = component(&meta.artist, "Unknown Artist");
    let title = component(&meta.title, "Unknown Album");
    let album = match meta.year {
        Some(year) => format!("{year} - {title}"),
        None => title.clone(),
    };
    let dir = PathBuf::from(&artist).join(component(&album, "Unknown Album"));
    let multi_disc = tracks
        .iter()
        .map(|track| track.disc_number)
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let mut entries: Vec<Entry> = Vec::new();
    let mut by_locator: HashMap<PathBuf, usize> = HashMap::new();
    let mut taken: HashSet<PathBuf> = HashSet::new();
    for track in tracks {
        let Some(reference) = track.remote() else {
            continue;
        };
        let locator = PathBuf::from(&track.path);
        let known = by_locator.get(&locator).copied();
        if !track.is_cue {
            if known.is_some() {
                continue;
            }
            let number = track.track_number.filter(|n| *n > 0);
            let stem = match number {
                Some(n) if multi_disc => format!("{}-{n:02} - {}", track.disc_number, track.title),
                Some(n) => format!("{n:02} - {}", track.title),
                None => track.title.clone(),
            };
            let target = claim(&mut taken, &dir, &stem, &reference.suffix);
            by_locator.insert(locator.clone(), entries.len());
            entries.push(Entry {
                size: sizes.get(&locator).copied().unwrap_or(0),
                locator,
                target,
                cue: None,
            });
            continue;
        }
        let ix = match known {
            Some(ix) => ix,
            None => {
                let folder = if multi_disc {
                    dir.join(format!("CD{}", track.disc_number))
                } else {
                    dir.clone()
                };
                let target = claim(
                    &mut taken,
                    &folder,
                    &format!("{artist} - {title}"),
                    &reference.suffix,
                );
                by_locator.insert(locator.clone(), entries.len());
                entries.push(Entry {
                    size: sizes.get(&locator).copied().unwrap_or(0),
                    locator,
                    target,
                    cue: Some(CueSpec {
                        performer: meta.artist.clone(),
                        title: meta.title.clone(),
                        date: meta.year,
                        genre: meta.genre.clone(),
                        tracks: Vec::new(),
                    }),
                });
                entries.len() - 1
            }
        };
        if let Some(cue) = entries[ix].cue.as_mut() {
            cue.tracks.push(CueTrack {
                number: track.track_number.filter(|n| *n > 0).unwrap_or(0) as u32,
                title: track.title.clone(),
                performer: artists.get(&track.id).and_then(|a| a.first()).cloned(),
                start_ms: track.start_offset_ms.max(0) as u64,
            });
        }
    }
    for entry in &mut entries {
        if let Some(cue) = entry.cue.as_mut() {
            cue.tracks.sort_by_key(|track| track.start_ms);
            let numbered: HashSet<u32> = cue.tracks.iter().map(|t| t.number).collect();
            if numbered.contains(&0) || numbered.len() != cue.tracks.len() {
                for (ix, track) in cue.tracks.iter_mut().enumerate() {
                    track.number = ix as u32 + 1;
                }
            }
        }
    }
    ExportPlan { dir, entries }
}

pub fn cue_time(ms: u64) -> String {
    let minutes = ms / 60_000;
    let seconds = ms % 60_000 / 1000;
    let frames = ((ms % 1000 * 75 + 500) / 1000).min(74);
    format!("{minutes:02}:{seconds:02}:{frames:02}")
}

pub fn cue_text(spec: &CueSpec, file_name: &str) -> String {
    let quoted = |text: &str| text.replace('"', "'");
    let mut out = String::new();
    if let Some(genre) = spec.genre.as_deref().filter(|g| !g.is_empty()) {
        let _ = writeln!(out, "REM GENRE \"{}\"", quoted(genre));
    }
    if let Some(date) = spec.date {
        let _ = writeln!(out, "REM DATE {date}");
    }
    if !spec.performer.is_empty() {
        let _ = writeln!(out, "PERFORMER \"{}\"", quoted(&spec.performer));
    }
    if !spec.title.is_empty() {
        let _ = writeln!(out, "TITLE \"{}\"", quoted(&spec.title));
    }
    let _ = writeln!(out, "FILE \"{}\" WAVE", quoted(file_name));
    for track in &spec.tracks {
        let _ = writeln!(out, "  TRACK {:02} AUDIO", track.number);
        let _ = writeln!(out, "    TITLE \"{}\"", quoted(&track.title));
        if let Some(performer) = &track.performer {
            let _ = writeln!(out, "    PERFORMER \"{}\"", quoted(performer));
        }
        let _ = writeln!(out, "    INDEX 01 {}", cue_time(track.start_ms));
    }
    out
}

fn free_path(wanted: &Path, siblings: &[&str]) -> PathBuf {
    let busy =
        |path: &Path| path.exists() || siblings.iter().any(|ext| path.with_extension(ext).exists());
    if !busy(wanted) {
        return wanted.to_path_buf();
    }
    let stem = wanted
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = wanted
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (2..)
        .map(|n| wanted.with_file_name(format!("{stem} ({n}).{ext}")))
        .find(|candidate| !busy(candidate))
        .unwrap_or_else(|| wanted.to_path_buf())
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    if let Err(e) = std::fs::remove_file(from) {
        log::warn!("{} stays in the cache: {e}", from.display());
    }
    Ok(())
}

fn already_placed(target: &Path, size: u64) -> bool {
    size > 0 && std::fs::metadata(target).is_ok_and(|meta| meta.len() == size)
}

pub fn place(
    media: &RemoteMedia,
    entry: &Entry,
    root: &Path,
    abandoned: &dyn Fn() -> bool,
) -> Result<(), String> {
    let wanted = root.join(&entry.target);
    if already_placed(&wanted, entry.size) {
        return Ok(());
    }
    let cached = media.resolve(&entry.locator, abandoned)?;
    let siblings: &[&str] = if entry.cue.is_some() { &["cue"] } else { &[] };
    let target = free_path(&wanted, siblings);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    move_file(&cached, &target).map_err(|e| e.to_string())?;
    if let Some(cue) = &entry.cue {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::fs::write(target.with_extension("cue"), cue_text(cue, &name))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_cover(dir: &Path, cover: &[u8]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let has_image = entries.flatten().any(|entry| {
        music_indexer::metadata::is_cover_image_name(&entry.file_name().to_string_lossy())
    });
    if !has_image && let Err(e) = std::fs::write(dir.join(COVER_NAME), cover) {
        log::warn!("cover for {} not written: {e}", dir.display());
    }
}

struct Job {
    progress: FillProgress,
    cancel: Arc<AtomicBool>,
}

pub struct AlbumExport {
    jobs: HashMap<i64, Job>,
    done: HashSet<i64>,
    _catalog_subscription: Subscription,
}

impl AlbumExport {
    pub fn create(library_event_bus: &Entity<LibraryEventsBus>, cx: &mut App) -> Entity<Self> {
        let bus = library_event_bus.clone();
        cx.new(|cx| Self {
            jobs: HashMap::new(),
            done: HashSet::new(),
            _catalog_subscription: cx.subscribe(
                &bus,
                |this: &mut Self, _, event: &LibraryEvent, cx| {
                    if matches!(event, LibraryEvent::CatalogChanged) && !this.done.is_empty() {
                        this.done.clear();
                        cx.notify();
                    }
                },
            ),
        })
    }

    pub fn progress(&self, album_id: i64) -> Option<FillProgress> {
        self.jobs.get(&album_id).map(|job| job.progress)
    }

    pub fn is_done(&self, album_id: i64) -> bool {
        self.done.contains(&album_id)
    }

    fn start(
        &mut self,
        album_id: i64,
        plan: ExportPlan,
        root: PathBuf,
        cover: Option<Vec<u8>>,
        cx: &mut Context<Self>,
    ) {
        if plan.entries.is_empty() || self.jobs.contains_key(&album_id) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            album_id,
            Job {
                progress: FillProgress {
                    done_bytes: 0,
                    total_bytes: plan.total(),
                },
                cancel: cancel.clone(),
            },
        );
        cx.notify();
        let media = cx.global::<Services>().remote_media.clone();
        cx.spawn(async move |this, cx| {
            let mut placed = 0usize;
            let mut failure: Option<String> = None;
            for entry in plan.entries {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let size = entry.size;
                let media = media.clone();
                let root = root.clone();
                let stop = cancel.clone();
                let result = cx
                    .background_spawn(async move {
                        place(&media, &entry, &root, &|| stop.load(Ordering::Acquire))
                    })
                    .await;
                match result {
                    Ok(()) => placed += 1,
                    Err(e) => {
                        log::warn!("moving to the library failed: {e}");
                        failure.get_or_insert(e);
                    }
                }
                let alive = this.update(cx, |this, cx| {
                    if let Some(job) = this.jobs.get_mut(&album_id) {
                        job.progress.done_bytes += size;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let album_dir = root.join(&plan.dir);
            if placed > 0
                && let Some(cover) = cover
            {
                cx.background_spawn(async move { write_cover(&album_dir, &cover) })
                    .await;
            }
            let cancelled = cancel.load(Ordering::Acquire);
            let _ = this.update(cx, |this, cx| {
                this.jobs.remove(&album_id);
                if failure.is_none() && !cancelled {
                    this.done.insert(album_id);
                }
                cx.notify();
            });
            cx.update(|cx| {
                if placed > 0 {
                    let folders = cx.global::<SettingsStore>().music_folders().to_vec();
                    let services = cx.global::<Services>();
                    services.library.request_rescan(folders, false, false);
                    let fill = services.cache_fill.clone();
                    fill.update(cx, |fill, cx| fill.cache_changed(cx));
                }
                if cancelled {
                    return;
                }
                let notification = match failure {
                    Some(error) => Notification::error(tr().album_to_local_failed(&error)),
                    None => Notification::success(
                        tr().album_to_local_done(&root.join(&plan.dir).to_string_lossy()),
                    ),
                };
                notify(notification, cx);
            });
        })
        .detach();
    }
}

fn notify(notification: Notification, cx: &mut App) {
    let Some(handle) = cx.windows().into_iter().next() else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(notification, cx);
    });
}

pub fn request(meta: AlbumMeta, tracks: Vec<Track>, window: &mut Window, cx: &mut App) {
    let export = cx.global::<Services>().album_export.clone();
    if let Some(job) = export.read(cx).jobs.get(&meta.id) {
        job.cancel.store(true, Ordering::Release);
        return;
    }
    let folders = cx.global::<SettingsStore>().music_folders().to_vec();
    match folders.as_slice() {
        [] => {}
        [root] => start(&export, meta, tracks, root.clone(), cx),
        _ => choose_folder(export, meta, tracks, folders, window, cx),
    }
}

fn start(
    export: &Entity<AlbumExport>,
    meta: AlbumMeta,
    tracks: Vec<Track>,
    root: PathBuf,
    cx: &mut App,
) {
    let library = cx.global::<Services>().library.clone();
    let export = export.clone();
    cx.spawn(async move |cx| {
        let album_id = meta.id;
        let (plan, cover) = cx
            .background_spawn(async move { plan_export(&library, &meta, &tracks) })
            .await;
        cx.update(|cx| {
            export.update(cx, |export, cx| {
                export.start(album_id, plan, root, cover, cx)
            })
        });
    })
    .detach();
}

fn plan_export(
    library: &LibraryService,
    meta: &AlbumMeta,
    tracks: &[Track],
) -> (ExportPlan, Option<Vec<u8>>) {
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let artists = library.track_artists_map(&ids);
    let mut keys: HashMap<i64, Vec<String>> = HashMap::new();
    for track in tracks {
        if let Some(reference) = track.remote() {
            keys.entry(reference.source_id)
                .or_default()
                .push(reference.key);
        }
    }
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();
    for (source_id, keys) in keys {
        let by_key = library.remote_file_sizes(source_id, &keys);
        for track in tracks {
            if let Some(reference) = track.remote()
                && reference.source_id == source_id
                && let Some(size) = by_key.get(&reference.key)
            {
                sizes.insert(PathBuf::from(&track.path), (*size).max(0) as u64);
            }
        }
    }
    let cover = meta
        .cover_art_id
        .and_then(|id| library.get_cover_art_large(id));
    (build_plan(meta, tracks, &artists, &sizes), cover)
}

fn choose_folder(
    export: Entity<AlbumExport>,
    meta: AlbumMeta,
    tracks: Vec<Track>,
    folders: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let tracks = Arc::new(tracks);
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let choices = folders.iter().enumerate().map(|(ix, folder)| {
            let export = export.clone();
            let meta = meta.clone();
            let tracks = tracks.clone();
            let root = folder.clone();
            Button::new(("album-to-local-folder", ix))
                .w_full()
                .label(folder.to_string_lossy().into_owned())
                .on_click(move |_, window, cx| {
                    start(&export, meta.clone(), (*tracks).clone(), root.clone(), cx);
                    window.close_dialog(cx);
                })
        });
        dialog
            .overlay_closable(false)
            .close_button(false)
            .w(px(520.))
            .title(tr().album_to_local_pick_title.clone())
            .child(v_flex().gap_2().children(choices))
            .footer(
                DialogFooter::new().child(
                    Button::new("album-to-local-cancel")
                        .label(tr().cancel.clone())
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                ),
            )
    });
}

#[cfg(test)]
mod tests {
    use music_library::remote;

    use super::*;

    fn track(id: i64, path: &str, title: &str, number: Option<i32>) -> Track {
        Track {
            id,
            path: path.into(),
            title: title.into(),
            album_id: Some(1),
            track_number: number,
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

    fn meta() -> AlbumMeta {
        AlbumMeta {
            id: 1,
            title: "Absolution".into(),
            artist: "Muse".into(),
            year: Some(2003),
            genre: Some("Rock".into()),
            cover_art_id: None,
        }
    }

    #[test]
    fn names_are_safe_and_never_empty() {
        assert_eq!(component("AC/DC: Live?", "x"), "AC_DC_ Live_");
        assert_eq!(component("  ...  ", "Unknown"), "Unknown");
        assert_eq!(component("Album. ", "x"), "Album");
        assert_eq!(component("con", "x"), "con_");
        assert_eq!(component("Nul.flac", "x"), "Nul_.flac");
        assert_eq!(component("COM1", "x"), "COM1_");
        assert_eq!(component("COM0", "x"), "COM0");
        assert_eq!(component("Console", "x"), "Console");
        assert_eq!(component(&"я".repeat(300), "x").chars().count(), 100);
    }

    #[test]
    fn only_server_tracks_are_planned_under_artist_and_year_album() {
        let a = remote::locator(3, "a", "flac");
        let b = remote::locator(3, "b", "mp3");
        let tracks = vec![
            track(1, &a, "Apocalypse Please", Some(1)),
            track(2, "/music/local.flac", "Local", Some(2)),
            track(3, &b, "Time Is Running Out", Some(3)),
        ];
        let sizes = HashMap::from([(PathBuf::from(&a), 10), (PathBuf::from(&b), 20)]);
        let plan = build_plan(&meta(), &tracks, &HashMap::new(), &sizes);
        assert_eq!(plan.dir, PathBuf::from("Muse/2003 - Absolution"));
        let targets: Vec<PathBuf> = plan.entries.iter().map(|e| e.target.clone()).collect();
        assert_eq!(
            targets,
            vec![
                PathBuf::from("Muse/2003 - Absolution/01 - Apocalypse Please.flac"),
                PathBuf::from("Muse/2003 - Absolution/03 - Time Is Running Out.mp3"),
            ]
        );
        assert_eq!(plan.total(), 30);
    }

    #[test]
    fn several_discs_prefix_the_disc_and_equal_names_do_not_collide() {
        let one = remote::locator(3, "1", "flac");
        let two = remote::locator(3, "2", "flac");
        let three = remote::locator(3, "3", "flac");
        let mut second = track(2, &two, "Intro", Some(1));
        second.disc_number = 2;
        let tracks = vec![
            track(1, &one, "Intro", Some(1)),
            second,
            track(3, &three, "Intro", Some(1)),
        ];
        let plan = build_plan(&meta(), &tracks, &HashMap::new(), &HashMap::new());
        let names: Vec<String> = plan
            .entries
            .iter()
            .map(|e| e.target.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "1-01 - Intro.flac",
                "2-01 - Intro.flac",
                "1-01 - Intro (2).flac"
            ]
        );
    }

    #[test]
    fn cue_pieces_become_one_image_with_a_generated_sheet() {
        let image = remote::locator(7, "39", "flac");
        let mut second = track(11, &image, "Yes Please", Some(2));
        second.is_cue = true;
        second.start_offset_ms = 4 * 60_000 + 33 * 1000 + 8 * 1000 / 75;
        let mut first = track(10, &image, "Intro", Some(1));
        first.is_cue = true;
        let artists = HashMap::from([(11, vec!["Muse".to_string()])]);
        let plan = build_plan(
            &meta(),
            &[second, first],
            &artists,
            &HashMap::from([(PathBuf::from(&image), 335)]),
        );
        assert_eq!(plan.entries.len(), 1);
        let entry = &plan.entries[0];
        assert_eq!(
            entry.target,
            PathBuf::from("Muse/2003 - Absolution/Muse - Absolution.flac")
        );
        let text = cue_text(entry.cue.as_ref().unwrap(), "Muse - Absolution.flac");
        assert_eq!(
            text,
            "REM GENRE \"Rock\"\nREM DATE 2003\nPERFORMER \"Muse\"\nTITLE \"Absolution\"\n\
             FILE \"Muse - Absolution.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Intro\"\n\
             \x20   INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Yes Please\"\n\
             \x20   PERFORMER \"Muse\"\n    INDEX 01 04:33:08\n"
        );
    }

    #[test]
    fn cue_times_round_trip_the_frames_the_scan_turned_into_milliseconds() {
        for frames in 0..75u64 {
            let ms = 61_000 + frames * 1000 / 75;
            assert_eq!(cue_time(ms), format!("01:01:{frames:02}"));
        }
        assert_eq!(cue_time(999), "00:00:74");
    }

    #[test]
    fn a_taken_name_moves_to_the_next_free_one() {
        let dir = tempfile::tempdir().unwrap();
        let wanted = dir.path().join("a.flac");
        assert_eq!(free_path(&wanted, &["cue"]), wanted);
        std::fs::write(dir.path().join("a.cue"), b"x").unwrap();
        assert_eq!(free_path(&wanted, &["cue"]), dir.path().join("a (2).flac"));
        assert_eq!(free_path(&wanted, &[]), wanted);
    }
}
