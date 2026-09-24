use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::{Context, SharedString, Subscription};

use crate::library_service::LibraryEvent;
use crate::localization::{LangChanged, tr};
use crate::servers::{RemoteError, RemoteServer, ServerKind};
use crate::services::Services;
use crate::settings_store::{JellyfinServer, SettingsStore, SubsonicServer};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStatus {
    Online,
    Offline,
    Syncing,
}

pub struct ServerRow {
    pub server: RemoteServer,
    pub title: SharedString,
    pub status: ServerStatus,
    pub status_label: SharedString,
    pub count_label: SharedString,
    pub message: Option<SharedString>,
}

pub fn server_status(
    kind: ServerKind,
    uri: &str,
    summaries: &[music_library::SourceSummary],
    syncing: bool,
) -> (ServerStatus, i64) {
    let summary = summaries
        .iter()
        .find(|s| s.kind == kind.as_str() && s.enabled && s.uri == uri);
    let status = match summary {
        _ if syncing => ServerStatus::Syncing,
        Some(s) if s.available => ServerStatus::Online,
        _ => ServerStatus::Offline,
    };
    (status, summary.map_or(0, |s| s.track_count))
}

pub fn describe_error(error: &RemoteError) -> SharedString {
    match error {
        RemoteError::Auth => tr().server_auth_failed.clone(),
        RemoteError::Unreachable(reason) => tr().server_unreachable(reason).into(),
        RemoteError::Other(message) => message.clone().into(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct ConnectState {
    pub connecting: bool,
    pub error: Option<SharedString>,
}

pub struct LibrarySources {
    folders: Vec<PathBuf>,
    subsonic: Vec<SubsonicServer>,
    jellyfin: Vec<JellyfinServer>,
    summaries: Vec<music_library::SourceSummary>,
    local: Vec<LocalFolderRow>,
    remote: Vec<ServerRow>,
    syncing: HashSet<String>,
    messages: HashMap<String, SharedString>,
    connect: HashMap<ServerKind, ConnectState>,
    cache_bytes: Option<u64>,
    cache_label: Option<SharedString>,
    clearing_cache: bool,
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
                LibraryEvent::CatalogChanged | LibraryEvent::ScanFailed => this.load(cx),
                LibraryEvent::RemoteSyncStarted { key } => {
                    this.syncing.insert(key.clone());
                    this.messages.remove(key);
                    this.refresh_rows(cx);
                    cx.notify();
                }
                LibraryEvent::RemoteSyncFinished { key, outcome } => {
                    this.syncing.remove(key);
                    if let Err(error) = outcome {
                        this.messages.insert(key.clone(), describe_error(error));
                    }
                    this.load(cx);
                }
                LibraryEvent::RemoteStarsImported { key, outcome } => {
                    let message = match outcome {
                        Ok((found, total)) => tr().scrobble_import_result(*found, *total).into(),
                        Err(error) => describe_error(error),
                    };
                    this.messages.insert(key.clone(), message);
                    this.refresh_rows(cx);
                    cx.notify();
                }
                LibraryEvent::ScanStarted | LibraryEvent::ScanIdle => {
                    this.refresh_rows(cx);
                    cx.notify();
                }
                _ => {}
            },
        );
        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            if let Some(bytes) = this.cache_bytes {
                this.set_cache_bytes(bytes);
            }
            this.refresh_rows(cx);
            cx.notify();
        });
        let settings_observer = cx.observe_global::<SettingsStore>(|this, cx| {
            let store = cx.global::<SettingsStore>();
            if store.music_folders() != this.folders.as_slice()
                || store.subsonic_servers() != this.subsonic.as_slice()
                || store.jellyfin_servers() != this.jellyfin.as_slice()
            {
                this.load(cx);
            }
        });
        let mut state = Self {
            folders: Vec::new(),
            subsonic: Vec::new(),
            jellyfin: Vec::new(),
            summaries: Vec::new(),
            local: Vec::new(),
            remote: Vec::new(),
            syncing: HashSet::new(),
            messages: HashMap::new(),
            connect: HashMap::new(),
            cache_bytes: None,
            cache_label: None,
            clearing_cache: false,
            _library_subscription: library_subscription,
            _lang_subscription: lang_subscription,
            _settings_observer: settings_observer,
        };
        state.load(cx);
        state.refresh_cache(cx);
        state
    }

    pub fn cache_bytes(&self) -> Option<u64> {
        self.cache_bytes
    }

    pub fn cache_label(&self) -> Option<SharedString> {
        self.cache_label.clone()
    }

    fn set_cache_bytes(&mut self, bytes: u64) {
        self.cache_bytes = Some(bytes);
        self.cache_label = Some(tr().size(bytes).into());
    }

    pub fn clearing_cache(&self) -> bool {
        self.clearing_cache
    }

    pub fn refresh_cache(&mut self, cx: &mut Context<Self>) {
        let media = cx.global::<Services>().remote_media.clone();
        let size = cx
            .background_executor()
            .spawn(async move { media.cache_size() });
        cx.spawn(async move |this, cx| {
            let bytes = size.await;
            let _ = this.update(cx, |this, cx| {
                this.set_cache_bytes(bytes);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn clear_cache(&mut self, cx: &mut Context<Self>) {
        if self.clearing_cache {
            return;
        }
        self.clearing_cache = true;
        cx.notify();
        let media = cx.global::<Services>().remote_media.clone();
        let cleared = cx.background_executor().spawn(async move {
            media.clear_cache();
            media.cache_size()
        });
        cx.spawn(async move |this, cx| {
            let bytes = cleared.await;
            let _ = this.update(cx, |this, cx| {
                this.clearing_cache = false;
                this.set_cache_bytes(bytes);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn local(&self) -> &[LocalFolderRow] {
        &self.local
    }

    pub fn remote(&self, kind: ServerKind) -> impl Iterator<Item = &ServerRow> {
        self.remote
            .iter()
            .filter(move |row| row.server.kind() == kind)
    }

    pub fn connect_state(&self, kind: ServerKind) -> ConnectState {
        self.connect.get(&kind).cloned().unwrap_or_default()
    }

    pub fn set_connect_state(&mut self, kind: ServerKind, state: ConnectState) {
        self.connect.insert(kind, state);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.folders = cx.global::<SettingsStore>().music_folders().to_vec();
        self.subsonic = cx.global::<SettingsStore>().subsonic_servers().to_vec();
        self.jellyfin = cx.global::<SettingsStore>().jellyfin_servers().to_vec();
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
        self.remote = crate::remote_settings::configured_servers(&self.subsonic, &self.jellyfin)
            .into_iter()
            .map(|server| {
                let key = server.key();
                let (status, count) = server_status(
                    server.kind(),
                    &server.uri,
                    &self.summaries,
                    self.syncing.contains(&key),
                );
                let status_label = match status {
                    ServerStatus::Online => tr().server_online.clone(),
                    ServerStatus::Offline => tr().server_offline.clone(),
                    ServerStatus::Syncing => tr().source_syncing.clone(),
                };
                ServerRow {
                    title: server.uri.clone().into(),
                    status,
                    status_label,
                    count_label: tr().n_tracks(count).into(),
                    message: self.messages.get(&key).cloned(),
                    server,
                }
            })
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
    fn server_status_prefers_syncing_then_the_source_flag() {
        let mut online = summary("me@http://nas", true, 7);
        online.kind = ServerKind::Subsonic.as_str().into();
        assert_eq!(
            server_status(
                ServerKind::Subsonic,
                "me@http://nas",
                std::slice::from_ref(&online),
                false
            ),
            (ServerStatus::Online, 7)
        );
        assert_eq!(
            server_status(
                ServerKind::Subsonic,
                "me@http://nas",
                std::slice::from_ref(&online),
                true
            )
            .0,
            ServerStatus::Syncing
        );
        assert_eq!(
            server_status(
                ServerKind::Jellyfin,
                "me@http://nas",
                std::slice::from_ref(&online),
                false
            ),
            (ServerStatus::Offline, 0)
        );
        online.available = false;
        assert_eq!(
            server_status(ServerKind::Subsonic, "me@http://nas", &[online], false).0,
            ServerStatus::Offline
        );
        assert_eq!(
            server_status(ServerKind::Subsonic, "other", &[], false),
            (ServerStatus::Offline, 0)
        );
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
