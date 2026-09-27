use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, RenderImage,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::theme_colors::Colors;
use ui_components::cover_thumb::cover_tile;

use crate::cache_fill::FillTarget;
use crate::now_playing::NavigateToArtistRequested;
use crate::services::Services;
use crate::track_list::{add_album_to_queue_button, move_to_local_button, save_to_cache_button};

pub struct AlbumInfo {
    album_id: i64,
    title: String,
    artist_name: SharedString,
    artist_id: Option<i64>,
    year: Option<i32>,
    cover: Option<Arc<RenderImage>>,
    genres_inline: SharedString,
    genres_tooltip: Option<SharedString>,
    missing_in_cache: bool,
    has_local_files: bool,
    has_remote: bool,
    tracks: std::rc::Rc<Vec<music_library::Track>>,
    export_meta: std::rc::Rc<crate::album_export::AlbumMeta>,
    fills_seen: u64,
    _fill_subscription: gpui::Subscription,
    _export_subscription: gpui::Subscription,
    _catalog_subscription: gpui::Subscription,
}

impl AlbumInfo {
    pub fn new(album: &music_library::AlbumSummary, cx: &mut Context<Self>) -> Self {
        let (cache, library, fill) = {
            let services = cx.global::<Services>();
            (
                services.cover_art_cache.clone(),
                services.library.clone(),
                services.cache_fill.clone(),
            )
        };
        let export = cx.global::<Services>().album_export.clone();
        let export_subscription = cx.observe(&export, |_, _, cx| cx.notify());
        let library_event_bus = cx.global::<Services>().library_event_bus.clone();
        let catalog_subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &crate::library_service::LibraryEvent, cx| {
                if matches!(event, crate::library_service::LibraryEvent::CatalogChanged) {
                    this.reload_tracks(cx);
                }
            },
        );
        let fills_seen = fill.read(cx).revision();
        let fill_subscription = cx.observe(&fill, |this, fill, cx| {
            let finished = fill.read(cx).revision();
            if finished != this.fills_seen {
                this.fills_seen = finished;
                this.missing_in_cache = crate::cache_fill::has_missing(
                    this.tracks.iter(),
                    &cx.global::<Services>().remote_media,
                );
            }
            cx.notify();
        });
        let cover = cache
            .borrow_mut()
            .get_large(album.cover_art_id, &library, cx);
        let all_genres = library.album_genres(album.id);
        let shown = all_genres
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let (genres_inline, genres_tooltip): (SharedString, Option<SharedString>) =
            if all_genres.len() > 3 {
                (
                    format!("{shown} …").into(),
                    Some(all_genres.join(" · ").into()),
                )
            } else {
                (shown.into(), None)
            };
        let tracks = library.tracks_for_album(album.id);
        let has_local_files = tracks.iter().any(|t| t.local_file().is_some());
        let missing_in_cache =
            crate::cache_fill::has_missing(&tracks, &cx.global::<Services>().remote_media);
        let has_remote = crate::album_export::has_remote(&tracks);
        let export_meta = crate::album_export::AlbumMeta {
            id: album.id,
            title: album.title.clone(),
            artist: album.artist_name.clone(),
            year: album.year,
            genre: all_genres.first().cloned(),
            cover_art_id: album.cover_art_id,
        };
        Self {
            album_id: album.id,
            title: album.title.clone(),
            artist_name: album.artist_name.clone().into(),
            artist_id: album.artist_id,
            year: album.year,
            cover,
            genres_inline,
            genres_tooltip,
            missing_in_cache,
            has_local_files,
            has_remote,
            tracks: std::rc::Rc::new(tracks),
            export_meta: std::rc::Rc::new(export_meta),
            fills_seen,
            _fill_subscription: fill_subscription,
            _export_subscription: export_subscription,
            _catalog_subscription: catalog_subscription,
        }
    }
}

impl AlbumInfo {
    fn reload_tracks(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let tracks = services.library.tracks_for_album(self.album_id);
        self.has_local_files = tracks.iter().any(|t| t.local_file().is_some());
        self.has_remote = crate::album_export::has_remote(&tracks);
        self.missing_in_cache = crate::cache_fill::has_missing(&tracks, &services.remote_media);
        self.tracks = std::rc::Rc::new(tracks);
        cx.notify();
    }
}

impl EventEmitter<NavigateToArtistRequested> for AlbumInfo {}

impl Render for AlbumInfo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_fg = Colors::muted_foreground(cx);
        let album_id = self.album_id;
        let target = FillTarget::Album(album_id);
        let progress = cx.global::<Services>().cache_fill.read(cx).progress(target);
        let (exporting, exported) = {
            let export = cx.global::<Services>().album_export.read(cx);
            (export.progress(album_id), export.is_done(album_id))
        };
        let has_folders = !cx
            .global::<crate::settings_store::SettingsStore>()
            .music_folders()
            .is_empty();
        let export_button = ((self.has_remote && has_folders && !exported) || exporting.is_some())
            .then(|| {
                let meta = self.export_meta.clone();
                let tracks = self.tracks.clone();
                move_to_local_button(
                    gpui::ElementId::NamedInteger("move-album-to-local".into(), album_id as u64),
                    exporting,
                    42.,
                    22.,
                    cx,
                    move |window, cx| {
                        crate::album_export::request(
                            (*meta).clone(),
                            (*tracks).clone(),
                            window,
                            cx,
                        );
                    },
                )
            });
        let save_button = ((self.missing_in_cache && !exported) || progress.is_some()).then(|| {
            save_to_cache_button(
                gpui::ElementId::NamedInteger("save-album-to-cache".into(), album_id as u64),
                progress,
                42.,
                22.,
                cx,
                move |window, cx| {
                    let tracks = cx.global::<Services>().library.tracks_for_album(album_id);
                    crate::cache_fill::request(target, tracks, window, cx);
                },
            )
        });
        let artist_id = self.artist_id;
        let title: SharedString = if self.album_id == music_library::NO_METADATA_ALBUM_ID {
            crate::localization::tr().no_metadata.clone()
        } else {
            self.title.clone().into()
        };

        h_flex()
            .w_full()
            .px_4()
            .gap_4()
            .items_start()
            .child(cover_tile(
                self.cover.as_ref(),
                150.,
                6.,
                Colors::secondary(cx),
                muted_fg,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .gap_1()
                    .pt_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(if let Some(aid) = artist_id {
                        h_flex()
                            .child(
                                div()
                                    .id(("al_artist", aid as u64))
                                    .text_sm()
                                    .text_color(muted_fg)
                                    .cursor_pointer()
                                    .border_b(px(1.))
                                    .hover(|s| s.border_color(muted_fg))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(NavigateToArtistRequested { artist_id: aid });
                                    }))
                                    .child(self.artist_name.clone()),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.artist_name.clone())
                            .into_any_element()
                    })
                    .child(if let Some(year) = self.year {
                        div().text_sm().text_color(muted_fg).child(year.to_string())
                    } else {
                        div()
                    })
                    .child(if self.genres_inline.is_empty() {
                        div().into_any_element()
                    } else if let Some(tooltip) = self.genres_tooltip.clone() {
                        div()
                            .id(("album_genres", album_id as u64))
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.genres_inline.clone())
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .into_any_element()
                    } else {
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(self.genres_inline.clone())
                            .into_any_element()
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .when(
                        self.has_local_files
                            && cx
                                .global::<crate::settings_store::SettingsStore>()
                                .tag_editor_enabled(),
                        |el| {
                            el.child(crate::track_list::edit_album_tags_button(
                                album_id, 42., 22., cx,
                            ))
                        },
                    )
                    .when_some(save_button, |el, button| el.child(button))
                    .when_some(export_button, |el, button| el.child(button))
                    .child(add_album_to_queue_button(album_id, 42., 26., cx)),
            )
    }
}
