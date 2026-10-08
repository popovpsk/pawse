use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use artist_info::{ArtistFacts, Error, Kind, Lookup, Membership, Query};
use gpui::{
    App, AppContext, Context, Div, Entity, FontWeight, Hsla, ParentElement, RenderImage,
    SharedString, Styled, Subscription, div, px,
};
use gpui_component::{h_flex, v_flex};
use music_library::ArtistInfoRow;
use ui_components::cover_thumb::cover_tile;
use ui_resources::i18n::{ArtistCardStrings, active, artist_card_strings_for};

use crate::cover_art_cache::render_tile;
use crate::library_service::{LibraryEvent, LibraryService};
use crate::localization::{LangChanged, LangEventBus};
use crate::services::LibraryEventsBus;

const PHOTO_PX: u32 = 300;
const PHOTO_SIZE: f32 = 150.;
const PHOTO_RADIUS: f32 = 6.;
const TOP_PAD: f32 = 12.;
const GENRES_SHOWN: usize = 3;
pub const HEADER_HEIGHT: f32 = 182.;

pub struct Card {
    pub photo: Option<Arc<RenderImage>>,
    pub lines: Vec<SharedString>,
}

enum Slot {
    Loading,
    Missing,
    Ready {
        facts: Box<ArtistFacts>,
        card: Option<Rc<Card>>,
    },
}

enum Step {
    Lookup(i64, String),
    Photo(String, ArtistInfoRow),
}

enum Indexed {
    Saved(String),
    Skipped,
    Offline,
}

pub struct ArtistCards {
    library: Arc<LibraryService>,
    cards: HashMap<String, Slot>,
    lookup: Lookup,
    indexing: bool,
    index_again: bool,
    _library_subscription: Subscription,
    _lang_subscription: Subscription,
}

impl ArtistCards {
    pub fn create(
        library: Arc<LibraryService>,
        library_bus: &Entity<LibraryEventsBus>,
        lang_bus: &Entity<LangEventBus>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let library_subscription = cx.subscribe(
                library_bus,
                |this: &mut Self, _, event: &LibraryEvent, cx| {
                    if matches!(
                        event,
                        LibraryEvent::ScanComplete | LibraryEvent::CatalogChanged
                    ) {
                        this.index(cx);
                    }
                },
            );
            let lang_subscription = cx
                .subscribe(lang_bus, |this: &mut Self, _, _: &LangChanged, cx| {
                    this.relabel(cx)
                });
            cx.spawn(async move |this, cx| {
                this.update(cx, |this: &mut Self, cx| this.index(cx)).ok()
            })
            .detach();
            Self {
                library,
                cards: HashMap::new(),
                lookup: Lookup::new(),
                indexing: false,
                index_again: false,
                _library_subscription: library_subscription,
                _lang_subscription: lang_subscription,
            }
        })
    }

    pub fn card(&self, name: &str) -> Option<Rc<Card>> {
        match self.cards.get(name)? {
            Slot::Ready { card, .. } => card.clone(),
            Slot::Loading | Slot::Missing => None,
        }
    }

    pub fn load(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.cards.contains_key(name) {
            return;
        }
        self.cards.insert(name.to_string(), Slot::Loading);
        self.fetch(name.to_string(), cx);
    }

    fn refresh(&mut self, name: String, cx: &mut Context<Self>) {
        if self.cards.contains_key(&name) {
            self.fetch(name, cx);
        }
    }

    fn fetch(&mut self, name: String, cx: &mut Context<Self>) {
        let library = self.library.clone();
        cx.spawn(async move |this, cx| {
            let stored = {
                let name = name.clone();
                cx.background_spawn(async move { read_card(&library, &name) })
                    .await
            };
            this.update(cx, |this, cx| {
                if !this.cards.contains_key(&name) {
                    return;
                }
                let slot = match stored {
                    Some((facts, photo)) => Slot::Ready {
                        card: build_card(&facts, photo, strings()).map(Rc::new),
                        facts: Box::new(facts),
                    },
                    None => Slot::Missing,
                };
                this.cards.insert(name, slot);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn relabel(&mut self, cx: &mut Context<Self>) {
        for slot in self.cards.values_mut() {
            if let Slot::Ready { facts, card } = slot {
                let photo = card.as_ref().and_then(|card| card.photo.clone());
                *card = build_card(facts, photo, strings()).map(Rc::new);
            }
        }
        cx.notify();
    }

    fn index(&mut self, cx: &mut Context<Self>) {
        if self.indexing {
            self.index_again = true;
            return;
        }
        self.indexing = true;
        let library = self.library.clone();
        let lookup = self.lookup.clone();
        cx.spawn(async move |this, cx| {
            let steps = {
                let library = library.clone();
                cx.background_spawn(async move { pending_steps(&library) })
                    .await
            };
            if !steps.is_empty() {
                log::info!("artist info: {} steps to index", steps.len());
            }
            for step in steps {
                let library = library.clone();
                let lookup = lookup.clone();
                let indexed = cx
                    .background_spawn(async move { run_step(&lookup, &library, step) })
                    .await;
                match indexed {
                    Indexed::Saved(name) => {
                        if this.update(cx, |this, cx| this.refresh(name, cx)).is_err() {
                            return;
                        }
                    }
                    Indexed::Skipped => {}
                    Indexed::Offline => break,
                }
            }
            this.update(cx, |this, cx| {
                this.indexing = false;
                if std::mem::take(&mut this.index_again) {
                    this.index(cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

fn strings() -> &'static ArtistCardStrings {
    artist_card_strings_for(active())
}

fn pending_steps(library: &LibraryService) -> Vec<Step> {
    let lookups = library
        .artists_without_info()
        .into_iter()
        .map(|(artist_id, name)| Step::Lookup(artist_id, name));
    let photos = library
        .artists_pending_photo()
        .into_iter()
        .map(|(name, row)| Step::Photo(name, row));
    lookups.chain(photos).collect()
}

fn run_step(lookup: &Lookup, library: &LibraryService, step: Step) -> Indexed {
    match step {
        Step::Lookup(artist_id, name) => index_one(lookup, library, artist_id, name),
        Step::Photo(name, row) => retry_photo(lookup, library, name, row),
    }
}

fn index_one(lookup: &Lookup, library: &LibraryService, artist_id: i64, name: String) -> Indexed {
    let titles = library.artist_titles(artist_id);
    if titles.albums.is_empty() && titles.tracks.is_empty() {
        return Indexed::Skipped;
    }
    let query = Query {
        name: &name,
        albums: &titles.albums,
        tracks: &titles.tracks,
    };
    let row = match lookup.find(&query) {
        Ok(Some(found)) => ArtistInfoRow {
            facts: serde_json::to_string(&found.facts).ok(),
            photo: found.photo,
            photo_pending: found.photo_pending,
        },
        Ok(None) => ArtistInfoRow::default(),
        Err(Error::Transport(e)) => {
            log::warn!("artist info: offline, stopping at {name}: {e}");
            return Indexed::Offline;
        }
        Err(e) => {
            log::warn!("artist info: lookup failed for {name}: {e}");
            return Indexed::Skipped;
        }
    };
    library.save_artist_info(&name, &row);
    Indexed::Saved(name)
}

fn retry_photo(
    lookup: &Lookup,
    library: &LibraryService,
    name: String,
    mut row: ArtistInfoRow,
) -> Indexed {
    let deezer = row
        .facts
        .as_deref()
        .and_then(|json| serde_json::from_str::<ArtistFacts>(json).ok())
        .and_then(|facts| facts.deezer);
    if let Some(id) = deezer {
        match lookup.photo(&id) {
            Ok(photo) => row.photo = photo,
            Err(Error::Status(code)) if is_permanent(code) => {
                log::info!("artist info: giving up on the photo of {name}: HTTP {code}");
            }
            Err(Error::Transport(e)) => {
                log::warn!("artist info: offline, photo retry stopped at {name}: {e}");
                return Indexed::Offline;
            }
            Err(e) => {
                log::warn!("artist info: photo retry failed for {name}: {e}");
                return Indexed::Skipped;
            }
        }
    }
    row.photo_pending = false;
    library.save_artist_info(&name, &row);
    Indexed::Saved(name)
}

fn is_permanent(status: u16) -> bool {
    (400..500).contains(&status) && status != 429
}

fn read_card(
    library: &LibraryService,
    name: &str,
) -> Option<(ArtistFacts, Option<Arc<RenderImage>>)> {
    let row = library.artist_info(name)?;
    let facts: ArtistFacts = serde_json::from_str(row.facts.as_deref()?).ok()?;
    let photo = row.photo.as_deref().and_then(decode_photo);
    Some((facts, photo))
}

fn decode_photo(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let raster = decoded
        .resize_to_fill(PHOTO_PX, PHOTO_PX, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some(render_tile(raster))
}

fn build_card(
    facts: &ArtistFacts,
    photo: Option<Arc<RenderImage>>,
    strings: &ArtistCardStrings,
) -> Option<Card> {
    let lines: Vec<SharedString> = [
        origin_line(facts, strings),
        genres_line(facts),
        members_line(facts, strings),
    ]
    .into_iter()
    .flatten()
    .map(SharedString::from)
    .collect();
    if lines.is_empty() && photo.is_none() {
        return None;
    }
    Some(Card { photo, lines })
}

fn origin_line(facts: &ArtistFacts, strings: &ArtistCardStrings) -> Option<String> {
    let mut place: Vec<&str> = Vec::new();
    for name in [&facts.begin_area, &facts.area].into_iter().flatten() {
        if !place.contains(&name.as_str()) {
            place.push(name);
        }
    }
    let parts: Vec<String> = [years(facts, strings), Some(place.join(", "))]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn years(facts: &ArtistFacts, strings: &ArtistCardStrings) -> Option<String> {
    match (facts.begin_year, facts.end_year) {
        (Some(begin), Some(end)) => Some(format!("{begin}–{end}")),
        (Some(begin), None) if facts.ended => Some(begin.to_string()),
        (Some(begin), None) => Some(match facts.kind {
            Kind::Person => strings.born(begin),
            Kind::Group | Kind::Other => strings.since(begin),
        }),
        (None, Some(end)) => Some(format!("–{end}")),
        (None, None) => None,
    }
}

fn genres_line(facts: &ArtistFacts) -> Option<String> {
    let genres: Vec<&str> = facts
        .genres
        .iter()
        .take(GENRES_SHOWN)
        .map(String::as_str)
        .collect();
    (!genres.is_empty()).then(|| genres.join(", "))
}

fn members_line(facts: &ArtistFacts, strings: &ArtistCardStrings) -> Option<String> {
    if !facts.members.is_empty() {
        let current: Vec<&Membership> = facts.members.iter().filter(|m| m.current).collect();
        let shown = if current.is_empty() {
            facts.members.iter().collect()
        } else {
            current
        };
        return Some(strings.members(&names(&shown)));
    }
    if !facts.member_of.is_empty() {
        let mut bands: Vec<&Membership> = facts.member_of.iter().collect();
        bands.sort_by_key(|m| !m.current);
        return Some(strings.member_of(&names(&bands)));
    }
    None
}

fn names(list: &[&Membership]) -> String {
    list.iter()
        .map(|m| m.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn card_header(
    card: &Card,
    name: SharedString,
    controls: Div,
    muted_fg: Hsla,
    fallback_bg: Hsla,
) -> Div {
    h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .pt(px(TOP_PAD))
        .pl_4()
        .pr_6()
        .gap_4()
        .items_start()
        .child(cover_tile(
            card.photo.as_ref(),
            PHOTO_SIZE,
            PHOTO_RADIUS,
            fallback_bg,
            muted_fg,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .pt_1()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(name),
                )
                .children(card.lines.iter().map(|text| {
                    div()
                        .text_sm()
                        .text_color(muted_fg)
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(text.clone())
                })),
        )
        .child(controls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use ui_resources::i18n::Lang;

    fn strings() -> &'static ArtistCardStrings {
        artist_card_strings_for(Lang::En)
    }

    fn member(name: &str, current: bool) -> Membership {
        Membership {
            name: name.into(),
            current,
        }
    }

    fn facts(kind: Kind, begin: Option<i32>, end: Option<i32>, ended: bool) -> ArtistFacts {
        ArtistFacts {
            kind,
            begin_year: begin,
            end_year: end,
            ended,
            ..ArtistFacts::default()
        }
    }

    #[rstest]
    #[case::active_group(facts(Kind::Group, Some(1994), None, false), Some("since 1994"))]
    #[case::living_person(facts(Kind::Person, Some(1972), None, false), Some("born 1972"))]
    #[case::split_group(facts(Kind::Group, Some(1964), Some(2014), true), Some("1964–2014"))]
    #[case::ended_without_end(facts(Kind::Group, Some(1993), None, true), Some("1993"))]
    #[case::only_end(facts(Kind::Other, None, Some(2001), true), Some("–2001"))]
    #[case::unknown(facts(Kind::Group, None, None, false), None)]
    fn years_line(#[case] facts: ArtistFacts, #[case] expected: Option<&str>) {
        assert_eq!(years(&facts, strings()).as_deref(), expected);
    }

    #[test]
    fn origin_joins_years_and_places_once() {
        let mut rammstein = facts(Kind::Group, Some(1994), None, false);
        rammstein.begin_area = Some("Berlin".into());
        rammstein.area = Some("Germany".into());
        assert_eq!(
            origin_line(&rammstein, strings()).as_deref(),
            Some("since 1994 · Berlin, Germany")
        );
        let mut same = facts(Kind::Group, None, None, false);
        same.begin_area = Some("Iceland".into());
        same.area = Some("Iceland".into());
        assert_eq!(origin_line(&same, strings()).as_deref(), Some("Iceland"));
        assert_eq!(
            origin_line(&facts(Kind::Group, None, None, false), strings()),
            None
        );
    }

    #[test]
    fn members_prefer_the_current_lineup() {
        let mut band = facts(Kind::Group, None, None, false);
        band.members = vec![member("Gone", false), member("Here", true)];
        assert_eq!(
            members_line(&band, strings()).as_deref(),
            Some("Members: Here")
        );
        band.members = vec![member("A", false), member("B", false)];
        assert_eq!(
            members_line(&band, strings()).as_deref(),
            Some("Members: A, B")
        );
    }

    #[test]
    fn a_person_lists_current_bands_first() {
        let mut person = facts(Kind::Person, None, None, false);
        person.member_of = vec![member("D12", false), member("Bad Meets Evil", true)];
        assert_eq!(
            members_line(&person, strings()).as_deref(),
            Some("Member of: Bad Meets Evil, D12")
        );
    }

    #[test]
    fn genres_show_the_top_three() {
        let mut band = facts(Kind::Group, None, None, false);
        band.genres = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        assert_eq!(genres_line(&band).as_deref(), Some("a, b, c"));
    }

    #[test]
    fn client_errors_except_throttling_are_permanent() {
        assert!(is_permanent(404));
        assert!(is_permanent(410));
        assert!(!is_permanent(429));
        assert!(!is_permanent(503));
    }

    #[test]
    fn nothing_to_show_is_no_card() {
        let bare = facts(Kind::Group, None, None, false);
        assert!(build_card(&bare, None, strings()).is_none());
    }
}
