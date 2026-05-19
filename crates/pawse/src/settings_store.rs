use std::path::PathBuf;

use gpui::{App, Global};
use gpui_component::{
    WindowExt,
    notification::Notification,
    theme::{Theme, ThemeMode},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub fn as_key(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "light" => ThemeChoice::Light,
            "dark" => ThemeChoice::Dark,
            _ => ThemeChoice::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default)]
    pub music_folder: Option<PathBuf>,
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
        self.settings.theme
    }

    pub fn set_theme(&mut self, theme: ThemeChoice) -> anyhow::Result<()> {
        self.settings.theme = theme;
        self.save()
    }

    pub fn music_folder(&self) -> Option<&PathBuf> {
        self.settings.music_folder.as_ref()
    }

    pub fn set_music_folder(&mut self, path: PathBuf) -> anyhow::Result<()> {
        self.settings.music_folder = Some(path);
        self.save()
    }
}

pub fn apply_startup_theme(store: &SettingsStore, cx: &mut App) {
    match store.theme() {
        ThemeChoice::System => {}
        ThemeChoice::Light => Theme::change(ThemeMode::Light, None, cx),
        ThemeChoice::Dark => Theme::change(ThemeMode::Dark, None, cx),
    }
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
        assert!(store.music_folder().is_none());

        cleanup(&path);
    }

    #[test]
    fn load_returns_defaults_when_file_corrupt() {
        let path = tmp_settings_path();
        fs::write(&path, "{ this is not valid json").unwrap();

        let store = SettingsStore::load_from(path.clone());
        assert_eq!(store.theme(), ThemeChoice::System);
        assert!(store.music_folder().is_none());

        cleanup(&path);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = tmp_settings_path();
        let folder = PathBuf::from("/Users/test/Music");

        {
            let mut store = SettingsStore::load_from(path.clone());
            store.set_theme(ThemeChoice::Dark).unwrap();
            store.set_music_folder(folder.clone()).unwrap();
        }

        let reloaded = SettingsStore::load_from(path.clone());
        assert_eq!(reloaded.theme(), ThemeChoice::Dark);
        assert_eq!(reloaded.music_folder(), Some(&folder));

        cleanup(&path);
    }

    #[test]
    fn save_uses_atomic_rename_via_tmp_file() {
        let path = tmp_settings_path();
        let store = {
            let mut s = SettingsStore::load_from(path.clone());
            s.set_theme(ThemeChoice::Light).unwrap();
            s
        };

        // The tmp file should be gone after a successful save (renamed onto target).
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists());
        assert!(path.exists());
        // And the JSON should be readable / well-formed.
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"theme\""));
        assert!(raw.contains("\"light\""));

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
        let err = store.set_theme(ThemeChoice::Dark);
        assert!(err.is_err(), "expected save to fail when parent is a file");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_choice_serde_roundtrip() {
        for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
            let s = serde_json::to_string(&choice).unwrap();
            let back: ThemeChoice = serde_json::from_str(&s).unwrap();
            assert_eq!(choice, back);
        }
    }
}
