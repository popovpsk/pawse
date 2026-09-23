use std::path::PathBuf;

use gpui::{Context, SharedString, Subscription};

use crate::library_service::LibraryEvent;
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceStatus {
    Online,
    Offline,
    Scanning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRow {
    pub path: PathBuf,
    pub location: SharedString,
    pub status: SourceStatus,
    pub track_count: i64,
}

pub fn source_rows(
    folders: &[PathBuf],
    summaries: &[music_library::SourceSummary],
    scanning: bool,
) -> Vec<SourceRow> {
    folders
        .iter()
        .map(|path| {
            let uri = path.to_string_lossy();
            let summary = summaries
                .iter()
                .find(|s| s.kind == "local" && s.enabled && s.uri == uri);
            let status = match summary {
                Some(s) if !s.available => SourceStatus::Offline,
                _ if scanning => SourceStatus::Scanning,
                _ => SourceStatus::Online,
            };
            SourceRow {
                path: path.clone(),
                location: uri.into_owned().into(),
                status,
                track_count: summary.map_or(0, |s| s.track_count),
            }
        })
        .collect()
}

pub struct LocalFolderRow {
    pub row: SourceRow,
    pub status_label: SharedString,
    pub count_label: SharedString,
}

impl LocalFolderRow {
    fn new(row: SourceRow) -> Self {
        let status_label = match row.status {
            SourceStatus::Online => tr().source_online.clone(),
            SourceStatus::Offline => tr().source_offline.clone(),
            SourceStatus::Scanning => tr().source_scanning.clone(),
        };
        let count_label = tr().n_tracks(row.track_count).into();
        Self {
            row,
            status_label,
            count_label,
        }
    }
}

pub struct LibrarySources {
    folders: Vec<PathBuf>,
    summaries: Vec<music_library::SourceSummary>,
    local: Vec<LocalFolderRow>,
    _library_subscription: Subscription,
    _lang_subscription: Subscription,
    _settings_observer: Subscription,
}

impl LibrarySources {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let lang_event_bus = services.lang_event_bus.clone();
        let library_subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::ScanComplete { changed: true } | LibraryEvent::ScanFailed => {
                    this.load(cx)
                }
                LibraryEvent::ScanStarted | LibraryEvent::ScanIdle => {
                    this.refresh_rows(cx);
                    cx.notify();
                }
                _ => {}
            },
        );
        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            this.refresh_rows(cx);
            cx.notify();
        });
        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            if cx.global::<SettingsStore>().music_folders() != this.folders.as_slice() {
                this.load(cx);
            }
        });
        let mut state = Self {
            folders: Vec::new(),
            summaries: Vec::new(),
            local: Vec::new(),
            _library_subscription: library_subscription,
            _lang_subscription: lang_subscription,
            _settings_observer: settings_observer,
        };
        state.load(cx);
        state
    }

    pub fn local(&self) -> &[LocalFolderRow] {
        &self.local
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.folders = cx.global::<SettingsStore>().music_folders().to_vec();
        self.summaries = cx.global::<Services>().library.sources();
        self.refresh_rows(cx);
        cx.notify();
    }

    fn refresh_rows(&mut self, cx: &Context<Self>) {
        let scanning = cx.global::<Services>().library.is_scanning();
        self.local = source_rows(&self.folders, &self.summaries, scanning)
            .into_iter()
            .map(LocalFolderRow::new)
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use music_library::SourceSummary;

    fn summary(uri: &str, available: bool, tracks: i64) -> SourceSummary {
        SourceSummary {
            id: 0,
            kind: "local".into(),
            uri: uri.into(),
            enabled: true,
            available,
            track_count: tracks,
        }
    }

    #[test]
    fn rows_follow_the_configured_folders_in_order_with_their_status() {
        let folders = vec![PathBuf::from("/b/Music"), PathBuf::from("/a/Drive")];
        let rows = source_rows(
            &folders,
            &[
                summary("/a/Drive", false, 12),
                summary("/b/Music", true, 40),
            ],
            false,
        );
        let shape: Vec<(&str, SourceStatus, i64)> = rows
            .iter()
            .map(|r| (r.location.as_ref(), r.status, r.track_count))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("/b/Music", SourceStatus::Online, 40),
                ("/a/Drive", SourceStatus::Offline, 12)
            ]
        );
    }

    #[test]
    fn an_offline_folder_stays_offline_during_a_scan_and_the_rest_show_scanning() {
        let folders = vec![PathBuf::from("/a"), PathBuf::from("/new")];
        let rows = source_rows(&folders, &[summary("/a", false, 3)], true);
        assert_eq!(rows[0].status, SourceStatus::Offline);
        assert_eq!(rows[1].status, SourceStatus::Scanning);
        assert_eq!(rows[1].track_count, 0);
    }

    #[test]
    fn a_disabled_source_row_for_the_same_path_is_ignored() {
        let mut removed = summary("/a", false, 9);
        removed.enabled = false;
        let rows = source_rows(&[PathBuf::from("/a")], &[removed], false);
        assert_eq!(rows[0].status, SourceStatus::Online);
        assert_eq!(rows[0].track_count, 0);
    }
}
