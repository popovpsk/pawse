use std::collections::{HashMap, HashSet};

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, px,
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
use crate::remote_settings::remote_servers;
use crate::servers::PlaylistScope;
use crate::services::Services;
use crate::theme_colors::Colors;

const DIALOG_WIDTH: f32 = 480.;
const RADIO_INDENT: f32 = 24.;

struct ServerEntry {
    key: String,
    kind: SharedString,
    title: SharedString,
}

pub struct PlaylistImport {
    scope: PlaylistScope,
    servers: Vec<ServerEntry>,
    busy: HashSet<String>,
    results: HashMap<String, SharedString>,
    _subscription: Subscription,
}

pub fn available(cx: &App) -> bool {
    remote_servers(cx)
        .iter()
        .any(|server| server.kind().imports_playlists())
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
            let LibraryEvent::RemotePlaylistsImported { key, outcome } = event else {
                return;
            };
            let message = match outcome {
                Ok(report) => SharedString::from(playlist_import_strings().result(
                    report.playlists,
                    report.found,
                    report.total,
                )),
                Err(error) => crate::library_sources::describe_error(error),
            };
            this.busy.remove(key);
            this.results.insert(key.clone(), message);
            cx.notify();
        });
        Self {
            scope: PlaylistScope::default(),
            servers: Vec::new(),
            busy: HashSet::new(),
            results: HashMap::new(),
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
        cx.notify();
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
        let selected = match self.scope {
            PlaylistScope::Mine => 0,
            PlaylistScope::All => 1,
        };
        let mut servers = v_flex().gap_3();
        for (ix, server) in self.servers.iter().enumerate() {
            let busy = self.busy.contains(&server.key);
            servers =
                servers.child(
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
                                        .disabled(busy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.start(ix, cx);
                                        })),
                                ),
                        )
                        .children(self.results.get(&server.key).map(|result| {
                            div().text_sm().text_color(muted_fg).child(result.clone())
                        })),
                );
        }
        v_flex()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(strings.description.clone()),
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
                                Radio::new("playlist-import-all").label(strings.scope_all.clone()),
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
            .child(servers)
    }
}
