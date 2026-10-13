pub mod deezer;
mod lookup;
pub mod musicbrainz;

pub use cover_search::Error;
pub use lookup::{Found, Lookup, Query, names_match};
pub use musicbrainz::{ArtistFacts, Kind, Membership};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitlePage {
    pub titles: Vec<String>,
    pub total: usize,
}
