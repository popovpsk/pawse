use std::collections::HashMap;

use anyhow::Context;
use gpui::{App, AppContext, Entity, SharedString};

use crate::localization::tr;
use crate::scrobble_settings::ScrobbleUiState;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const PAGE_SIZE: u32 = 200;
const MAX_PAGES: u32 = 100;

pub fn start(cx: &mut App, ui: Entity<ScrobbleUiState>) {
    let Some(session) = cx
        .global::<SettingsStore>()
        .scrobble()
        .lastfm
        .session
        .clone()
    else {
        return;
    };
    let library = cx.global::<Services>().library.clone();

    ui.update(cx, |state, cx| {
        state.import_busy = true;
        state.import_result = None;
        cx.notify();
    });

    cx.spawn(async move |cx| {
        let outcome = cx
            .background_spawn(async move {
                let client =
                    scrobble::lastfm_client().context("Last.fm is not configured in this build")?;
                let mut loved = Vec::new();
                let mut page = 1;
                loop {
                    let (tracks, total_pages) =
                        client.loved_tracks(&session.name, page, PAGE_SIZE)?;
                    let exhausted = tracks.is_empty();
                    loved.extend(tracks);
                    if exhausted || page >= total_pages || page >= MAX_PAGES {
                        break;
                    }
                    page += 1;
                }

                let index = build_index(&library);
                let mut found = 0usize;
                let mut to_like: Vec<i64> = Vec::new();
                for track in &loved {
                    let Some(entries) = index.get(&key(&track.artist, &track.title)) else {
                        continue;
                    };
                    found += 1;
                    to_like.extend(
                        entries
                            .iter()
                            .filter(|(_, liked)| !liked)
                            .map(|(id, _)| *id),
                    );
                }
                to_like.sort_unstable();
                to_like.dedup();
                anyhow::Ok((loved.len(), found, to_like))
            })
            .await;

        let (total, found, to_like) = match outcome {
            Ok(outcome) => outcome,
            Err(e) => {
                cx.update(|cx| {
                    ui.update(cx, |state, cx| {
                        state.import_busy = false;
                        state.import_result = Some(SharedString::from(format!("{e:#}")));
                        cx.notify();
                    })
                })
                .ok();
                return;
            }
        };

        cx.update(|cx| {
            let stored = cx.global::<Services>().library.like_many(to_like);
            ui.update(cx, |state, cx| {
                state.import_busy = false;
                state.import_result = Some(match stored {
                    Ok(()) => SharedString::from(tr().scrobble_import_result(found, total)),
                    Err(e) => SharedString::from(format!("{e}")),
                });
                cx.notify();
            })
        })
        .ok();
    })
    .detach();
}

fn build_index(
    library: &crate::library_service::LibraryService,
) -> HashMap<String, Vec<(i64, bool)>> {
    let tracks = library.all_tracks();
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let artists = library.track_artists_map(&ids);
    let mut index: HashMap<String, Vec<(i64, bool)>> = HashMap::with_capacity(tracks.len());
    for track in &tracks {
        let Some(names) = artists.get(&track.id) else {
            continue;
        };
        for name in names {
            index
                .entry(key(name, &track.title))
                .or_default()
                .push((track.id, track.liked));
        }
    }
    index
}

fn key(artist: &str, title: &str) -> String {
    format!(
        "{}\u{1}{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_case_and_surrounding_space() {
        assert_eq!(
            key("  Boards of Canada ", "Roygbiv"),
            key("boards of canada", "ROYGBIV")
        );
    }

    #[test]
    fn keys_do_not_collide_across_the_separator() {
        assert_ne!(key("a", "b c"), key("a b", "c"));
    }
}
