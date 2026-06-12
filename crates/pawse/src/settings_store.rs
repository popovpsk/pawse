use std::path::{Path, PathBuf};

use gpui::{App, BackgroundExecutor, Global, Pixels, px};
use gpui_component::{
    WindowExt,
    notification::Notification,
    scroll::ScrollbarShow,
    theme::{Theme, ThemeRegistry},
};
use music_library::Track;
use serde::{Deserialize, Serialize};

use crate::localization::tr;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Named(String),
}

impl ThemeChoice {
    pub fn as_key(&self) -> String {
        match self {
            ThemeChoice::System => "system".to_string(),
            ThemeChoice::Named(name) => name.clone(),
        }
    }

    pub fn from_key(s: &str) -> Self {
        if s == "system" {
            Self::System
        } else {
            Self::Named(s.to_string())
        }
    }
}

impl From<ThemeChoice> for String {
    fn from(c: ThemeChoice) -> Self {
        c.as_key()
    }
}

impl From<String> for ThemeChoice {
    fn from(s: String) -> Self {
        ThemeChoice::from_key(&s)
    }
}

impl Serialize for ThemeChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_key())
    }
}

impl<'de> Deserialize<'de> for ThemeChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(ThemeChoice::from_key(&s))
    }
}

/// UI language. `System` follows the OS locale; `Named` pins a specific
/// language by its short code (e.g. "ru"). Serialized as a plain string,
/// mirroring [`ThemeChoice`].
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LangChoice {
    #[default]
    System,
    Named(String),
}

impl LangChoice {
    pub fn as_key(&self) -> String {
        match self {
            LangChoice::System => "system".to_string(),
            LangChoice::Named(code) => code.clone(),
        }
    }

    pub fn from_key(s: &str) -> Self {
        if s == "system" {
            Self::System
        } else {
            Self::Named(s.to_string())
        }
    }
}

impl From<LangChoice> for String {
    fn from(c: LangChoice) -> Self {
        c.as_key()
    }
}

impl From<String> for LangChoice {
    fn from(s: String) -> Self {
        LangChoice::from_key(&s)
    }
}

impl Serialize for LangChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_key())
    }
}

impl<'de> Deserialize<'de> for LangChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(LangChoice::from_key(&s))
    }
}

fn default_true() -> bool {
    true
}

fn default_volume() -> f32 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepeatModePersist {
    #[default]
    Off,
    All,
    One,
}

impl From<crate::playback_queue::RepeatMode> for RepeatModePersist {
    fn from(mode: crate::playback_queue::RepeatMode) -> Self {
        match mode {
            crate::playback_queue::RepeatMode::Off => Self::Off,
            crate::playback_queue::RepeatMode::All => Self::All,
            crate::playback_queue::RepeatMode::One => Self::One,
        }
    }
}

impl From<RepeatModePersist> for crate::playback_queue::RepeatMode {
    fn from(mode: RepeatModePersist) -> Self {
        match mode {
            RepeatModePersist::Off => Self::Off,
            RepeatModePersist::All => Self::All,
            RepeatModePersist::One => Self::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontScale {
    #[default]
    Small,
    Medium,
    Large,
}

impl FontScale {
    pub fn px(self) -> Pixels {
        match self {
            FontScale::Small => px(16.),
            FontScale::Medium => px(19.),
            FontScale::Large => px(22.),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum QueueSourcePersist {
    #[default]
    Unknown,
    Playlist(i64),
    AllTracks,
}

impl From<crate::playback_queue::QueueSource> for QueueSourcePersist {
    fn from(s: crate::playback_queue::QueueSource) -> Self {
        match s {
            crate::playback_queue::QueueSource::Unknown => Self::Unknown,
            crate::playback_queue::QueueSource::Playlist(id) => Self::Playlist(id),
            crate::playback_queue::QueueSource::AllTracks => Self::AllTracks,
        }
    }
}

impl From<QueueSourcePersist> for crate::playback_queue::QueueSource {
    fn from(s: QueueSourcePersist) -> Self {
        match s {
            QueueSourcePersist::Unknown => Self::Unknown,
            QueueSourcePersist::Playlist(id) => Self::Playlist(id),
            QueueSourcePersist::AllTracks => Self::AllTracks,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaybackState {
    #[serde(default)]
    pub queue: Vec<Track>,
    #[serde(default)]
    pub original_queue: Option<Vec<Track>>,
    #[serde(default)]
    pub current_index: Option<usize>,
    #[serde(default)]
    pub position_ms: u64,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: RepeatModePersist,
    #[serde(default)]
    pub source: QueueSourcePersist,
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default)]
    pub language: LangChoice,
    #[serde(default)]
    pub music_folders: Vec<PathBuf>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub playback: PlaybackState,
    #[serde(default)]
    pub show_hog_button: bool,
    #[serde(default = "default_true")]
    pub show_repeat_shuffle: bool,
    #[serde(default = "default_true")]
    pub show_time_labels: bool,
    #[serde(default = "default_true")]
    pub liked_enabled: bool,
    #[serde(default = "default_true")]
    pub playlists_enabled: bool,
    #[serde(default = "default_true")]
    pub show_track_duration: bool,
    #[serde(default = "default_true")]
    pub show_queue_actions: bool,
    #[serde(default = "default_true")]
    pub show_queue_artist: bool,
    #[serde(default = "default_true")]
    pub cover_show_artist: bool,
    #[serde(default = "default_true")]
    pub cover_show_progress: bool,
    #[serde(default = "default_true")]
    pub cover_show_controls: bool,
    #[serde(default)]
    pub queue_deduplication: bool,
    #[serde(default)]
    pub font_scale: FontScale,
    #[serde(default)]
    pub onboarding_complete: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: LangChoice::default(),
            music_folders: Vec::new(),
            volume: 1.0,
            playback: PlaybackState::default(),
            show_hog_button: false,
            show_repeat_shuffle: true,
            show_time_labels: true,
            liked_enabled: true,
            playlists_enabled: true,
            show_track_duration: true,
            show_queue_actions: true,
            show_queue_artist: true,
            cover_show_artist: true,
            cover_show_progress: true,
            cover_show_controls: true,
            queue_deduplication: false,
            font_scale: FontScale::default(),
            onboarding_complete: false,
        }
    }
}

pub struct SettingsStore {
    pub settings: UserSettings,
    path: PathBuf,
    save_tx: Option<flume::Sender<UserSettings>>,
}

impl Global for SettingsStore {}

fn write_settings(path: &Path, settings: &UserSettings) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl SettingsStore {
    pub fn load() -> Self {
        Self::load_from(Self::default_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            settings,
            path,
            save_tx: None,
        }
    }

    pub fn start_background_writer(&mut self, executor: BackgroundExecutor) {
        let (tx, rx) = flume::unbounded::<UserSettings>();
        self.save_tx = Some(tx);
        let path = self.path.clone();
        executor
            .spawn(async move {
                while let Ok(mut latest) = rx.recv_async().await {
                    while let Ok(newer) = rx.try_recv() {
                        latest = newer;
                    }
                    if let Err(e) = write_settings(&path, &latest) {
                        log::error!("settings: background save failed: {e}");
                    }
                }
            })
            .detach();
    }

    fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| {
                log::warn!("settings: config dir unavailable, using current directory");
                PathBuf::from(".")
            })
            .join("pawse")
            .join("settings.json")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        match &self.save_tx {
            Some(tx) => {
                let _ = tx.send(self.settings.clone());
                Ok(())
            }
            None => write_settings(&self.path, &self.settings),
        }
    }

    pub fn save_playback_blocking(&mut self, playback: PlaybackState) -> anyhow::Result<()> {
        self.settings.playback = playback;
        self.save_tx = None;
        write_settings(&self.path, &self.settings)
    }

    pub fn theme(&self) -> ThemeChoice {
        self.settings.theme.clone()
    }

    pub fn set_theme(&mut self, theme: ThemeChoice) -> anyhow::Result<()> {
        self.settings.theme = theme;
        self.save()
    }

    pub fn language(&self) -> LangChoice {
        self.settings.language.clone()
    }

    pub fn set_language(&mut self, language: LangChoice) -> anyhow::Result<()> {
        self.settings.language = language;
        self.save()
    }

    pub fn music_folders(&self) -> &[PathBuf] {
        &self.settings.music_folders
    }

    pub fn add_music_folder(&mut self, path: PathBuf) -> anyhow::Result<()> {
        if self.settings.music_folders.iter().any(|p| p == &path) {
            return Ok(());
        }
        self.settings.music_folders.push(path);
        self.save()
    }

    pub fn remove_music_folder(&mut self, path: &Path) -> anyhow::Result<()> {
        let before = self.settings.music_folders.len();
        self.settings.music_folders.retain(|p| p.as_path() != path);
        if self.settings.music_folders.len() == before {
            return Ok(());
        }
        self.save()
    }

    pub fn playback(&self) -> &PlaybackState {
        &self.settings.playback
    }

    pub fn set_playback(&mut self, state: PlaybackState) -> anyhow::Result<()> {
        self.settings.playback = state;
        self.save()
    }

    pub fn volume(&self) -> f32 {
        self.settings.volume
    }

    pub fn set_volume(&mut self, volume: f32) -> anyhow::Result<()> {
        self.settings.volume = volume;
        self.save()
    }

    pub fn show_hog_button(&self) -> bool {
        self.settings.show_hog_button
    }

    pub fn set_show_hog_button(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_hog_button = show;
        self.save()
    }

    pub fn show_repeat_shuffle(&self) -> bool {
        self.settings.show_repeat_shuffle
    }

    pub fn set_show_repeat_shuffle(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_repeat_shuffle = show;
        self.save()
    }

    pub fn show_time_labels(&self) -> bool {
        self.settings.show_time_labels
    }

    pub fn set_show_time_labels(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_time_labels = show;
        self.save()
    }

    pub fn liked_enabled(&self) -> bool {
        self.settings.liked_enabled
    }

    pub fn set_liked_enabled(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.settings.liked_enabled = enabled;
        self.save()
    }

    pub fn playlists_enabled(&self) -> bool {
        self.settings.playlists_enabled
    }

    pub fn set_playlists_enabled(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.settings.playlists_enabled = enabled;
        self.save()
    }

    pub fn show_track_duration(&self) -> bool {
        self.settings.show_track_duration
    }

    pub fn set_show_track_duration(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_track_duration = show;
        self.save()
    }

    pub fn show_queue_actions(&self) -> bool {
        self.settings.show_queue_actions
    }

    pub fn set_show_queue_actions(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_queue_actions = show;
        self.save()
    }

    pub fn show_queue_artist(&self) -> bool {
        self.settings.show_queue_artist
    }

    pub fn set_show_queue_artist(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.show_queue_artist = show;
        self.save()
    }

    pub fn cover_show_artist(&self) -> bool {
        self.settings.cover_show_artist
    }

    pub fn set_cover_show_artist(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.cover_show_artist = show;
        self.save()
    }

    pub fn cover_show_progress(&self) -> bool {
        self.settings.cover_show_progress
    }

    pub fn set_cover_show_progress(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.cover_show_progress = show;
        self.save()
    }

    pub fn cover_show_controls(&self) -> bool {
        self.settings.cover_show_controls
    }

    pub fn set_cover_show_controls(&mut self, show: bool) -> anyhow::Result<()> {
        self.settings.cover_show_controls = show;
        self.save()
    }

    pub fn queue_deduplication(&self) -> bool {
        self.settings.queue_deduplication
    }

    pub fn set_queue_deduplication(&mut self, value: bool) -> anyhow::Result<()> {
        self.settings.queue_deduplication = value;
        self.save()
    }

    pub fn font_scale(&self) -> FontScale {
        self.settings.font_scale
    }

    pub fn set_font_scale(&mut self, scale: FontScale) -> anyhow::Result<()> {
        self.settings.font_scale = scale;
        self.save()
    }

    pub fn onboarding_complete(&self) -> bool {
        self.settings.onboarding_complete
    }

    pub fn set_onboarding_complete(&mut self, complete: bool) -> anyhow::Result<()> {
        self.settings.onboarding_complete = complete;
        self.save()
    }
}

/// Set the UI base font size (the rem unit) from a [`FontScale`]. `Root::render`
/// reapplies `cx.theme().font_size` as the window rem size every frame, so this
/// rescales every rem-relative text size across the app.
pub fn apply_font_scale(scale: FontScale, cx: &mut App) {
    Theme::global_mut(cx).font_size = scale.px();
}

// why: Theme::change -> apply_config resets font_size on every theme switch, so
// the chosen scale must be reasserted right after any theme application.
fn reassert_font_scale(cx: &mut App) {
    if cx.has_global::<SettingsStore>() {
        apply_font_scale(cx.global::<SettingsStore>().font_scale(), cx);
    }
}

/// Apply a theme choice to the UI without saving to disk. Used for live preview.
pub fn apply_theme(choice: &ThemeChoice, cx: &mut App) {
    match choice {
        ThemeChoice::System => Theme::sync_system_appearance(None, cx),
        ThemeChoice::Named(name) => apply_named_theme(name, cx),
    }
    Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Scrolling;
    reassert_font_scale(cx);
}

pub fn apply_startup_theme(store: &SettingsStore, cx: &mut App) {
    let t = store.theme();
    apply_theme(&t, cx);
    apply_font_scale(store.font_scale(), cx);
}

/// Apply a theme by name from `ThemeRegistry`. No-op if the name is not yet registered
/// (e.g. bundled themes haven't finished loading). Called again in `on_loaded`.
pub fn apply_named_theme(name: &str, cx: &mut App) {
    let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() else {
        return;
    };
    let mode = config.mode;
    let theme = cx.global_mut::<Theme>();
    if mode.is_dark() {
        theme.dark_theme = config;
    } else {
        theme.light_theme = config;
    }
    Theme::change(mode, None, cx);
    reassert_font_scale(cx);
}

/// Push a user-visible notification (and log) when saving settings fails.
/// Looks up the active window via `cx.active_window()` — settings writes
/// always happen in response to UI actions, so a window is present.
pub fn notify_save_error(cx: &mut App, err: anyhow::Error) {
    log::error!("settings: save failed: {err}");
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, cx| {
            let s = tr();
            window.push_notification(
                Notification::error(s.failed_save_settings(&err.to_string()))
                    .title(s.settings.clone()),
                cx,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn tmp_settings_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pawse-settings-test-{}-{}",
            std::process::id(),
            // monotonically-ish unique per test invocation
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn cleanup(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn load_returns_defaults_when_file_missing() {
        let path = tmp_settings_path();
        // Sanity: file doesn't exist yet.
        assert!(!path.exists());

        let store = SettingsStore::load_from(path.clone());
        assert_eq!(store.theme(), ThemeChoice::System);
        assert!(store.music_folders().is_empty());

        cleanup(&path);
    }

    #[test]
    fn load_returns_defaults_when_file_corrupt() {
        let path = tmp_settings_path();
        fs::write(&path, "{ this is not valid json").unwrap();

        let store = SettingsStore::load_from(path.clone());
        assert_eq!(store.theme(), ThemeChoice::System);
        assert!(store.music_folders().is_empty());

        cleanup(&path);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = tmp_settings_path();
        let folder_a = PathBuf::from("/Users/test/Music");
        let folder_b = PathBuf::from("/Users/test/More Music");
        let dark = ThemeChoice::Named("Default Dark".into());

        {
            let mut store = SettingsStore::load_from(path.clone());
            store.set_theme(dark.clone()).unwrap();
            store.add_music_folder(folder_a.clone()).unwrap();
            store.add_music_folder(folder_b.clone()).unwrap();
        }

        let reloaded = SettingsStore::load_from(path.clone());
        assert_eq!(reloaded.theme(), dark);
        assert_eq!(reloaded.music_folders(), &[folder_a, folder_b]);

        cleanup(&path);
    }

    #[test]
    fn add_music_folder_is_idempotent() {
        let path = tmp_settings_path();
        let folder = PathBuf::from("/Users/test/Music");

        let mut store = SettingsStore::load_from(path.clone());
        store.add_music_folder(folder.clone()).unwrap();
        store.add_music_folder(folder.clone()).unwrap();

        assert_eq!(store.music_folders(), &[folder]);

        cleanup(&path);
    }

    #[test]
    fn remove_music_folder_removes_matching_path() {
        let path = tmp_settings_path();
        let a = PathBuf::from("/a");
        let b = PathBuf::from("/b");
        let c = PathBuf::from("/c");

        let mut store = SettingsStore::load_from(path.clone());
        store.add_music_folder(a.clone()).unwrap();
        store.add_music_folder(b.clone()).unwrap();
        store.add_music_folder(c.clone()).unwrap();

        store.remove_music_folder(&b).unwrap();
        assert_eq!(store.music_folders(), &[a, c]);

        // Removing a non-existent path is a no-op.
        store.remove_music_folder(Path::new("/missing")).unwrap();

        cleanup(&path);
    }

    #[test]
    fn save_uses_atomic_rename_via_tmp_file() {
        let path = tmp_settings_path();
        let store = {
            let mut s = SettingsStore::load_from(path.clone());
            s.set_theme(ThemeChoice::Named("Solarized Light".into()))
                .unwrap();
            s
        };

        // The tmp file should be gone after a successful save (renamed onto target).
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists());
        assert!(path.exists());
        // And the JSON should be readable / well-formed.
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"theme\""));
        assert!(raw.contains("Solarized Light"));

        drop(store);
        cleanup(&path);
    }

    #[test]
    fn save_returns_error_when_parent_path_is_a_file() {
        // Force a save failure by making the parent of `settings.json` a file,
        // not a directory — `create_dir_all` will reject it.
        let dir = std::env::temp_dir().join(format!(
            "pawse-settings-test-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        // `parent_as_file` is a regular file masquerading as a directory.
        let parent_as_file = dir.join("not_a_dir");
        fs::write(&parent_as_file, b"").unwrap();
        let path = parent_as_file.join("settings.json");

        let mut store = SettingsStore::load_from(path);
        let err = store.set_theme(ThemeChoice::Named("Default Dark".into()));
        assert!(err.is_err(), "expected save to fail when parent is a file");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn playback_state_roundtrip() {
        let track = music_library::Track {
            id: 1,
            path: "/music/song.flac".to_string(),
            title: "Song".to_string(),
            album_id: Some(2),
            track_number: Some(3),
            disc_number: 1,
            duration_ms: Some(240_000),
            year: Some(2020),
            cover_art_id: None,
            start_offset_ms: 0,
            liked: false,
            bitrate: None,
        };
        let settings = UserSettings {
            theme: ThemeChoice::System,
            language: LangChoice::System,
            music_folders: vec![],
            volume: 0.42,
            playback: PlaybackState {
                queue: vec![track.clone()],
                original_queue: Some(vec![track.clone()]),
                current_index: Some(0),
                position_ms: 12_345,
                shuffle: true,
                repeat: RepeatModePersist::All,
                source: QueueSourcePersist::Playlist(7),
                custom: true,
            },
            show_hog_button: true,
            show_repeat_shuffle: true,
            show_time_labels: true,
            liked_enabled: true,
            playlists_enabled: true,
            show_track_duration: true,
            show_queue_actions: true,
            show_queue_artist: true,
            cover_show_artist: true,
            cover_show_progress: true,
            cover_show_controls: true,
            queue_deduplication: false,
            font_scale: FontScale::Large,
            onboarding_complete: false,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: UserSettings = serde_json::from_str(&json).unwrap();
        assert!((back.volume - 0.42).abs() < f32::EPSILON);
        assert_eq!(back.font_scale, FontScale::Large);
        assert_eq!(back.playback.queue.len(), 1);
        assert_eq!(back.playback.queue[0], track);
        assert_eq!(back.playback.current_index, Some(0));
        assert_eq!(back.playback.position_ms, 12_345);
        assert!(back.playback.shuffle);
        assert_eq!(back.playback.repeat, RepeatModePersist::All);
        assert_eq!(back.playback.source, QueueSourcePersist::Playlist(7));
        assert!(back.playback.custom);
    }

    #[test]
    fn queue_source_all_tracks_roundtrip() {
        use crate::playback_queue::QueueSource;
        let persisted: QueueSourcePersist = QueueSource::AllTracks.into();
        assert_eq!(persisted, QueueSourcePersist::AllTracks);
        let back: QueueSource = persisted.into();
        assert_eq!(back, QueueSource::AllTracks);
        let json = serde_json::to_string(&persisted).unwrap();
        let de: QueueSourcePersist = serde_json::from_str(&json).unwrap();
        assert_eq!(de, QueueSourcePersist::AllTracks);
    }

    #[test]
    fn old_settings_without_playback_field_gets_defaults() {
        let json = r#"{"theme":"system","music_folders":[]}"#;
        let settings: UserSettings = serde_json::from_str(json).unwrap();
        assert!((settings.volume - 1.0).abs() < f32::EPSILON);
        assert!(settings.playback.queue.is_empty());
        assert_eq!(settings.playback.current_index, None);
        assert_eq!(settings.playback.position_ms, 0);
        assert_eq!(settings.font_scale, FontScale::Small);
    }

    #[test]
    fn theme_choice_serde_roundtrip() {
        for (choice, expected) in [
            (ThemeChoice::System, "\"system\""),
            (
                ThemeChoice::Named("Solarized Dark".into()),
                "\"Solarized Dark\"",
            ),
            (
                ThemeChoice::Named("Default Light".into()),
                "\"Default Light\"",
            ),
        ] {
            let s = serde_json::to_string(&choice).unwrap();
            assert_eq!(s, expected);
            let back: ThemeChoice = serde_json::from_str(&s).unwrap();
            assert_eq!(choice, back);
        }
    }
}
