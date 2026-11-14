use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Entity, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div, px,
};
use gpui_component::{
    Disableable, Sizable, WindowExt,
    button::Button,
    h_flex,
    radio::{Radio, RadioGroup},
    v_flex,
};
use ui_resources::i18n::playlist_import_strings;

use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::playlist_files::FilesOutcome;
use crate::remote_settings::remote_servers;
use crate::servers::{PlaylistScope, RemoteError};
use crate::services::Services;
use crate::theme_colors::Colors;

const DIALOG_WIDTH: f32 = 480.;
const RADIO_INDENT: f32 = 24.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Files,
    Folder,
}

struct ServerEntry {
    key: String,
    kind: SharedString,
    title: SharedString,
}

pub struct PlaylistImport {
    scope: PlaylistScope,
    servers: Vec<ServerEntry>,
    busy: HashSet<String>,
    syncing: HashSet<String>,
    results: HashMap<String, SharedString>,
    picking: bool,
    files_busy: Option<Pick>,
    files_result: Option<SharedString>,
    _subscription: Subscription,
}

pub fn open(state: Entity<PlaylistImport>, window: &mut Window, cx: &mut App) {
    state.update(cx, |this, cx| this.refresh_servers(cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(DIALOG_WIDTH))
            .title(playlist_import_strings().title.clone())
            .child(state.clone())
    });
}

impl PlaylistImport {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let bus = cx.global::<Services>().library_event_bus.clone();
        let subscription = cx.subscribe(&bus, |this, _, event: &LibraryEvent, cx| {
            match event {
                LibraryEvent::RemoteSyncStarted { key } => {
                    this.syncing.insert(key.clone());
                }
                LibraryEvent::RemoteSyncFinished { key, .. } => {
                    this.syncing.remove(key);
                }
                LibraryEvent::RemotePlaylistsImported { key, outcome } => {
                    this.busy.remove(key);
                    let message = match outcome {
                        Ok(report) => Some(SharedString::from(playlist_import_strings().result(
                            report.playlists,
                            report.added,
                            report.found,
                            report.total,
                        ))),
                        Err(RemoteError::Syncing) => None,
                        Err(error) => Some(crate::library_sources::describe_error(error)),
                    };
                    if let Some(message) = message {
                        this.results.insert(key.clone(), message);
                    }
                }
                LibraryEvent::PlaylistFilesImported { outcome } => {
                    let strings = playlist_import_strings();
                    this.files_busy = None;
                    this.files_result = Some(match outcome {
                        FilesOutcome::Imported(report) => SharedString::from(strings.result(
                            report.playlists,
                            report.added,
                            report.found,
                            report.total,
                        )),
                        FilesOutcome::NoPlaylists => strings.no_files.clone(),
                        FilesOutcome::Failed => strings.files_failed.clone(),
                    });
                }
                _ => return,
            }
            cx.notify();
        });
        Self {
            scope: PlaylistScope::default(),
            servers: Vec::new(),
            busy: HashSet::new(),
            syncing: HashSet::new(),
            results: HashMap::new(),
            picking: false,
            files_busy: None,
            files_result: None,
            _subscription: subscription,
        }
    }

    fn refresh_servers(&mut self, cx: &mut Context<Self>) {
        self.servers = remote_servers(cx)
            .into_iter()
            .filter(|server| server.kind().imports_playlists())
            .map(|server| ServerEntry {
                key: server.key(),
                kind: SharedString::new_static(server.kind().title()),
                title: server.uri.into(),
            })
            .collect();
        self.syncing = cx.global::<Services>().library.remote_syncing();
        cx.notify();
    }

    fn choose(&mut self, pick: Pick, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        cx.notify();
        let filter = tr().tab_playlists.to_string();
        let extensions: Vec<String> = playlist_file::EXTENSIONS
            .iter()
            .flat_map(|ext| [ext.to_string(), ext.to_uppercase()])
            .collect();
        cx.spawn(async move |this, cx| {
            let dialog = rfd::AsyncFileDialog::new();
            let picked = match pick {
                Pick::Files => dialog.add_filter(filter, &extensions).pick_files().await,
                Pick::Folder => dialog.pick_folders().await,
            };
            let paths: Vec<PathBuf> = picked
                .unwrap_or_default()
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect();
            let _ = this.update(cx, |this, cx| {
                this.picking = false;
                if !paths.is_empty() {
                    this.files_busy = Some(pick);
                    this.files_result = None;
                    cx.global::<Services>().library.import_playlist_files(paths);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(key) = self.servers.get(ix).map(|entry| entry.key.clone()) else {
            return;
        };
        let Some(server) = remote_servers(cx)
            .into_iter()
            .find(|server| server.key() == key)
        else {
            return;
        };
        self.results.remove(&key);
        self.busy.insert(key);
        cx.notify();
        cx.global::<Services>()
            .library
            .import_remote_playlists(server, self.scope);
    }
}

impl Render for PlaylistImport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strings = playlist_import_strings();
        let muted_fg = Colors::muted_foreground(cx);
        let still_syncing = &tr().server_still_syncing;
        let selected = match self.scope {
            PlaylistScope::Mine => 0,
            PlaylistScope::All => 1,
        };
        let mut servers = v_flex().gap_3();
        for (ix, server) in self.servers.iter().enumerate() {
            let busy = self.busy.contains(&server.key);
            let syncing = self.syncing.contains(&server.key);
            let note = if syncing {
                Some(still_syncing.clone())
            } else {
                self.results.get(&server.key).cloned()
            };
            servers = servers.child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(div().text_sm().child(server.title.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .child(server.kind.clone()),
                                    ),
                            )
                            .child(
                                Button::new(("playlist-import-run", ix))
                                    .small()
                                    .label(strings.import.clone())
                                    .loading(busy)
                                    .disabled(busy || syncing)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.start(ix, cx);
                                    })),
                            ),
                    )
                    .children(note.map(|note| div().text_sm().text_color(muted_fg).child(note))),
            );
        }
        let section_title = |title: &SharedString| {
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.clone())
        };
        let files = v_flex()
            .gap_2()
            .child(section_title(&strings.files_title))
            .child(
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(strings.files_description.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("playlist-import-files")
                            .small()
                            .label(strings.choose_files.clone())
                            .loading(self.files_busy == Some(Pick::Files))
                            .disabled(self.picking || self.files_busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.choose(Pick::Files, cx))),
                    )
                    .child(
                        Button::new("playlist-import-folder")
                            .small()
                            .label(strings.choose_folder.clone())
                            .loading(self.files_busy == Some(Pick::Folder))
                            .disabled(self.picking || self.files_busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.choose(Pick::Folder, cx))),
                    ),
            )
            .children(
                self.files_result
                    .clone()
                    .map(|note| div().text_sm().text_color(muted_fg).child(note)),
            );
        v_flex()
            .gap_6()
            .child(files)
            .when(!self.servers.is_empty(), |dialog| {
                dialog.child(
                    v_flex()
                        .gap_4()
                        .child(
                            v_flex()
                                .gap_2()
                                .child(section_title(&strings.servers_title))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(muted_fg)
                                        .child(strings.description.clone()),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    RadioGroup::vertical("playlist-import-scope")
                                        .child(
                                            Radio::new("playlist-import-mine")
                                                .label(strings.scope_mine.clone()),
                                        )
                                        .child(
                                            Radio::new("playlist-import-all")
                                                .label(strings.scope_all.clone()),
                                        )
                                        .selected_index(Some(selected))
                                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                            this.scope = if *ix == 0 {
                                                PlaylistScope::Mine
                                            } else {
                                                PlaylistScope::All
                                            };
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .pl(px(RADIO_INDENT))
                                        .text_xs()
                                        .text_color(muted_fg)
                                        .child(strings.scope_all_hint.clone()),
                                ),
                        )
                        .child(servers),
                )
            })
    }
}
