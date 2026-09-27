use std::collections::HashMap;

use gpui::{App, AppContext, Entity, SharedString};

use crate::localization::tr;
use crate::scrobble_settings::ScrobbleUiState;
use crate::services::Services;
use crate::settings_store::SettingsStore;

const PAGE_SIZE: u32 = 200;
const MAX_PAGES: u32 = 100;
const CAP: usize = PAGE_SIZE as usize * MAX_PAGES as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Lastfm,
    Librefm,
    ListenBrainz,
}

enum Fetcher {
    Audioscrobbler {
        client: scrobble::AudioscrobblerClient,
        user: String,
    },
    ListenBrainz {
        client: scrobble::ListenBrainzClient,
        user: String,
    },
}

impl Fetcher {
    fn collect(&self) -> anyhow::Result<Vec<scrobble::LovedTrack>> {
        let mut loved = Vec::new();
        match self {
            Fetcher::Audioscrobbler { client, user } => {
                let mut page = 1;
                loop {
                    let (tracks, total_pages) = client.loved_tracks(user, page, PAGE_SIZE)?;
                    let exhausted = tracks.is_empty();
                    loved.extend(tracks);
                    if exhausted || page >= total_pages || page >= MAX_PAGES {
                        break;
                    }
                    page += 1;
                }
            }
            Fetcher::ListenBrainz { client, user } => {
                let mut offset = 0usize;
                loop {
                    let (tracks, total) = client.loved_tracks(user, PAGE_SIZE as usize, offset)?;
                    let fetched = tracks.len();
                    loved.extend(tracks);
                    offset += fetched;
                    if fetched == 0 || offset >= total || offset >= CAP {
                        break;
                    }
                }
            }
        }
        Ok(loved)
    }
}

fn fetcher(cx: &App, source: ImportSource) -> Option<Fetcher> {
    let settings = cx.global::<SettingsStore>().scrobble();
    match source {
        ImportSource::Lastfm => {
            let session = settings.lastfm.session.clone()?;
            let (key, secret) = scrobble::creds()?;
            Some(Fetcher::Audioscrobbler {
                client: scrobble::AudioscrobblerClient::new(scrobble::Profile::lastfm(key, secret)),
                user: session.name,
            })
        }
        ImportSource::Librefm => {
            let session = settings.librefm.session.clone()?;
            Some(Fetcher::Audioscrobbler {
                client: scrobble::AudioscrobblerClient::new(scrobble::Profile::librefm()),
                user: session.name,
            })
        }
        ImportSource::ListenBrainz => {
            let user = settings.listenbrainz.user.clone();
            if user.is_empty() || settings.listenbrainz.token.is_empty() {
                return None;
            }
            Some(Fetcher::ListenBrainz {
                client: scrobble::ListenBrainzClient::new(
                    settings.listenbrainz.api_root.clone(),
                    settings.listenbrainz.token.clone(),
                ),
                user,
            })
        }
    }
}

pub fn start(cx: &mut App, ui: Entity<ScrobbleUiState>, source: ImportSource) {
    let Some(fetcher) = fetcher(cx, source) else {
        return;
    };
    let library = cx.global::<Services>().library.clone();

    ui.update(cx, |state, cx| {
        state.import_busy = Some(source);
        state.import_result = None;
        cx.notify();
    });

    cx.spawn(async move |cx| {
        let outcome = cx
            .background_spawn(async move {
                let loved = fetcher.collect()?;

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
                        state.import_busy = None;
                        state.import_result = Some((source, SharedString::from(format!("{e:#}"))));
                        cx.notify();
                    })
                });
                return;
            }
        };

        cx.update(|cx| {
            let stored = cx.global::<Services>().library.like_many(to_like);
            ui.update(cx, |state, cx| {
                state.import_busy = None;
                let text = match stored {
                    Ok(()) => SharedString::from(tr().scrobble_import_result(found, total)),
                    Err(e) => SharedString::from(format!("{e}")),
                };
                state.import_result = Some((source, text));
                cx.notify();
            })
        });
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
        music_library::normalize_tag(artist),
        music_library::normalize_tag(title)
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
