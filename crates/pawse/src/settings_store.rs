use std::path::{Path, PathBuf};

use gpui::{App, Global};
use gpui_component::{
    WindowExt,
    notification::Notification,
    theme::{Theme, ThemeRegistry},
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default)]
    pub music_folders: Vec<PathBuf>,
}

pub struct SettingsStore {
    pub settings: UserSettings,
    path: PathBuf,
}

impl Global for SettingsStore {}

impl SettingsStore {
    pub fn load() -> Self {
        Self::load_from(Self::default_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { settings, path }
    }

    fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| {
                eprintln!("settings: config dir unavailable, using current directory");
                PathBuf::from(".")
            })
            .join("pawse")
            .join("settings.json")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.settings)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn theme(&self) -> ThemeChoice {
        self.settings.theme.clone()
    }

    pub fn set_theme(&mut self, theme: ThemeChoice) -> anyhow::Result<()> {
        self.settings.theme = theme;
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
}

/// Apply a theme choice to the UI without saving to disk. Used for live preview.
pub fn apply_theme(choice: &ThemeChoice, cx: &mut App) {
    match choice {
        ThemeChoice::System => Theme::sync_system_appearance(None, cx),
        ThemeChoice::Named(name) => apply_named_theme(name, cx),
    }
}

pub fn apply_startup_theme(store: &SettingsStore, cx: &mut App) {
    let t = store.theme();
    apply_theme(&t, cx);
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
}

/// Push a user-visible notification (and log) when saving settings fails.
/// Looks up the active window via `cx.active_window()` — settings writes
/// always happen in response to UI actions, so a window is present.
pub fn notify_save_error(cx: &mut App, err: anyhow::Error) {
    eprintln!("settings: save failed: {err}");
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, cx| {
            window.push_notification(
                Notification::error(format!("Failed to save settings: {err}")).title("Settings"),
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
