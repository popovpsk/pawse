const ITUNES_SIZE_TOKEN: &str = "100x100bb";
const CAA_URL: &str = "https://coverartarchive.org/release-group";
const CAA_SIZES: [u32; 3] = [250, 500, 1200];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Itunes,
    MusicBrainz,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Art {
    Itunes(String),
    ReleaseGroup(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub source: Source,
    pub artist: String,
    pub album: String,
    art: Art,
}

impl Candidate {
    pub(crate) fn itunes(artist: String, album: String, artwork_url_100: String) -> Self {
        Self {
            source: Source::Itunes,
            artist,
            album,
            art: Art::Itunes(artwork_url_100),
        }
    }

    pub(crate) fn release_group(artist: String, album: String, mbid: String) -> Self {
        Self {
            source: Source::MusicBrainz,
            artist,
            album,
            art: Art::ReleaseGroup(mbid),
        }
    }

    pub fn art_url(&self, size: u32) -> String {
        match &self.art {
            Art::Itunes(url) => url.replace(ITUNES_SIZE_TOKEN, &format!("{size}x{size}bb")),
            Art::ReleaseGroup(mbid) => {
                let size = CAA_SIZES
                    .into_iter()
                    .find(|&s| s >= size)
                    .unwrap_or(CAA_SIZES[CAA_SIZES.len() - 1]);
                format!("{CAA_URL}/{mbid}/front-{size}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn itunes_url_is_resized_by_its_size_token() {
        let c = Candidate::itunes(
            "A".into(),
            "B".into(),
            "https://is1.mzstatic.com/x/100x100bb.jpg".into(),
        );
        assert_eq!(
            c.art_url(1200),
            "https://is1.mzstatic.com/x/1200x1200bb.jpg"
        );
    }

    #[test]
    fn caa_url_rounds_up_to_an_available_size() {
        let c = Candidate::release_group("A".into(), "B".into(), "rg-1".into());
        assert_eq!(
            c.art_url(100),
            "https://coverartarchive.org/release-group/rg-1/front-250"
        );
        assert_eq!(
            c.art_url(3000),
            "https://coverartarchive.org/release-group/rg-1/front-1200"
        );
    }
}
