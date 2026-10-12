use std::collections::HashMap;

use audio_embedding::EMBEDDING_VERSION;
use gpui::{
    AnyView, App, ParentElement, SharedString, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_component::{tooltip::Tooltip, v_flex};
use music_library::{LibraryRepository, SoundGenres};

use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

const TRACK_MIN: f32 = 0.2;
const ALBUM_MIN: f32 = 0.12;
const PER_ALBUM: usize = 3;
const MIN_TRACKS: i64 = 5;
const DIMMED: f32 = 0.6;

pub fn shown(cx: &App) -> bool {
    let settings = cx.global::<SettingsStore>();
    settings.similar_tracks_enabled() && settings.sound_genres_shown() && !super::gpu_missing(cx)
}

pub fn filter(cx: &App) -> Option<SoundGenres<'static>> {
    shown(cx).then_some(SoundGenres {
        version: EMBEDDING_VERSION,
        min_score: TRACK_MIN,
        min_tracks: MIN_TRACKS,
    })
}

pub fn albums(
    repo: &dyn LibraryRepository,
    tags: &HashMap<i64, Vec<String>>,
    album_id: Option<i64>,
) -> HashMap<i64, Vec<String>> {
    let mut map = repo
        .album_sound_genres(EMBEDDING_VERSION, ALBUM_MIN, 2 * PER_ALBUM, album_id)
        .unwrap_or_else(|e| {
            log::warn!("sound genres of albums: {e}");
            HashMap::new()
        });
    for (album, names) in &mut map {
        let tagged: Vec<String> = tags
            .get(album)
            .into_iter()
            .flatten()
            .map(|t| t.to_lowercase())
            .collect();
        names.retain(|name| !tagged.contains(&name.to_lowercase()));
        names.truncate(PER_ALBUM);
    }
    map.retain(|_, names| !names.is_empty());
    map
}

pub fn dimmed(cx: &App) -> gpui::Hsla {
    Colors::muted_foreground(cx).opacity(DIMMED)
}

pub fn tooltip(
    tags: SharedString,
    heard: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    Tooltip::element(move |_, cx| {
        v_flex()
            .gap_0p5()
            .when(!tags.is_empty(), |list| list.child(tags.clone()))
            .when(!heard.is_empty(), |list| {
                list.child(div().text_color(dimmed(cx)).child(heard.clone()))
            })
    })
    .build(window, cx)
}
