pub mod deezer;
mod lookup;
pub mod musicbrainz;

pub use cover_search::Error;
pub use lookup::{Found, Lookup, Query, names_match};
pub use musicbrainz::{ArtistFacts, Kind, Membership};
