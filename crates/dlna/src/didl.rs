use roxmltree::Node;

use crate::address;
use crate::xml::{self, LocalName};

const AUDIO_CLASS: &str = "object.item.audioItem";
const RAW_PCM: [&str; 4] = ["audio/l16", "audio/l24", "audio/l8", "audio/lpcm"];
const NOT_AUDIO: [&str; 3] = ["image/", "video/", "text/"];
const NAMESPACES: [(&str, &str); 6] = [
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("upnp", "urn:schemas-upnp-org:metadata-1-0/upnp/"),
    ("dlna", "urn:schemas-dlna-org:metadata-1-0/"),
    ("sec", "http://www.sec.co.kr/"),
    ("pv", "http://www.pv.com/pvns/"),
    ("pxn", "urn:schemas-panasonic-com:pxn"),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album_artists: Vec<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub date: Option<String>,
    pub genres: Vec<String>,
    pub album_art: Option<String>,
    pub res: Vec<Res>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Res {
    pub key: String,
    pub protocol_info: String,
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u64>,
    pub sample_rate: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub channels: Option<u32>,
}

impl Res {
    pub fn id(&self) -> String {
        address::identity(&self.key, self.size)
    }

    fn field(&self, index: usize) -> &str {
        self.protocol_info
            .splitn(4, ':')
            .nth(index)
            .unwrap_or("")
            .trim()
    }

    pub fn mime(&self) -> &str {
        self.field(2)
    }

    pub fn transcoded(&self) -> bool {
        self.field(3)
            .split(';')
            .any(|param| param.trim().eq_ignore_ascii_case("DLNA.ORG_CI=1"))
    }

    fn over_http(&self) -> bool {
        self.field(0).eq_ignore_ascii_case("http-get")
    }

    fn playable(&self) -> bool {
        let mime = self.mime().to_ascii_lowercase();
        self.over_http()
            && !RAW_PCM.iter().any(|raw| mime.starts_with(raw))
            && !NOT_AUDIO.iter().any(|kind| mime.starts_with(kind))
    }
}

impl Item {
    pub fn cover_id(&self) -> Option<String> {
        let size = self.pick().and_then(|res| res.size);
        self.album_art
            .as_deref()
            .map(|art| address::identity(art, size))
    }

    pub fn pick(&self) -> Option<&Res> {
        let playable = || self.res.iter().filter(|res| res.playable());
        playable()
            .find(|res| !res.transcoded())
            .or_else(|| playable().next())
    }
}

#[derive(Debug, Default)]
pub(crate) struct Page {
    pub items: Vec<Item>,
    pub containers: Vec<String>,
    pub entries: Vec<String>,
}

pub(crate) fn parse(text: &str, location: &str) -> Result<Page, String> {
    let mut page = Page::default();
    if text.trim().is_empty() {
        return Ok(page);
    }
    let declared;
    let document = match xml::parse(text) {
        Ok(document) => document,
        Err(error) => {
            declared = declare_namespaces(text).ok_or(error)?;
            xml::parse(&declared)?
        }
    };
    for node in document.root_element().children() {
        if (node.has_tag_name_local("container") || node.has_tag_name_local("item"))
            && let Some(id) = node.attribute("id")
        {
            page.entries.push(id.to_string());
        }
        if node.has_tag_name_local("container") {
            if let Some(id) = node.attribute("id") {
                page.containers.push(id.to_string());
            }
        } else if node.has_tag_name_local("item")
            && let Some(item) = item(node, location)
        {
            page.items.push(item);
        }
    }
    Ok(page)
}

fn declare_namespaces(text: &str) -> Option<String> {
    let start = text.find("<DIDL-Lite")? + "<DIDL-Lite".len();
    let end = start + text[start..].find('>')?;
    let head = &text[start..end];
    let missing: String = NAMESPACES
        .iter()
        .filter(|(prefix, _)| !head.contains(&format!("xmlns:{prefix}=")))
        .map(|(prefix, uri)| format!(" xmlns:{prefix}=\"{uri}\""))
        .collect();
    (!missing.is_empty()).then(|| format!("{}{missing}{}", &text[..start], &text[start..]))
}

fn item(node: Node<'_, '_>, location: &str) -> Option<Item> {
    let class = xml::child_text(node, "class")?.trim();
    if !class.starts_with(AUDIO_CLASS) {
        return None;
    }
    let mut item = Item {
        id: node.attribute("id")?.to_string(),
        ..Item::default()
    };
    let mut creator = None;
    for child in node.children().filter(|child| child.is_element()) {
        let name = child.tag_name().name();
        let Some(text) = xml::text(child) else {
            continue;
        };
        match name {
            "title" => item.title = text,
            "artist" | "albumArtist" => {
                let role = child.attribute("role").unwrap_or("");
                if name == "albumArtist" || role.eq_ignore_ascii_case("AlbumArtist") {
                    item.album_artists.push(text);
                } else if role.is_empty() || role.eq_ignore_ascii_case("Performer") {
                    item.artists.push(text);
                }
            }
            "creator" => creator = creator.or(Some(text)),
            "album" => item.album = item.album.or(Some(text)),
            "originalTrackNumber" => item.track_number = text.parse().ok(),
            "originalDiscNumber" => item.disc_number = text.parse().ok(),
            "date" => item.date = item.date.or(Some(text)),
            "genre" => item.genres.push(text),
            "albumArtURI" => {
                item.album_art = item.album_art.or(Some(address::key(location, &text)));
            }
            "res" => item.res.push(res(child, &text, location)),
            _ => {}
        }
    }
    if item.artists.is_empty() {
        item.artists.extend(creator);
    }
    Some(item)
}

fn res(node: Node<'_, '_>, url: &str, location: &str) -> Res {
    let number = |name: &str| {
        node.attribute(name)
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    let small = |name: &str| number(name).and_then(|value| u32::try_from(value).ok());
    Res {
        key: address::key(location, url),
        protocol_info: node.attribute("protocolInfo").unwrap_or("").to_string(),
        size: number("size"),
        duration_ms: node.attribute("duration").and_then(parse_duration),
        bitrate: number("bitrate"),
        sample_rate: small("sampleFrequency"),
        bits_per_sample: small("bitsPerSample"),
        channels: small("nrAudioChannels"),
    }
}

pub(crate) fn parse_duration(text: &str) -> Option<u64> {
    let mut parts = text.trim().split(':');
    let hours: u64 = parts.next()?.trim().parse().ok()?;
    let minutes: u64 = parts.next()?.trim().parse().ok()?;
    let seconds = parts.next()?.trim();
    if parts.next().is_some() {
        return None;
    }
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let whole: u64 = whole.parse().ok()?;
    let fraction_ms = match fraction.split_once('/') {
        Some((numerator, denominator)) => {
            let numerator: u64 = numerator.parse().ok()?;
            let denominator: u64 = denominator.parse().ok()?;
            (denominator > 0).then(|| numerator * 1000 / denominator)?
        }
        None if fraction.is_empty() => 0,
        None => {
            let digits: String = fraction.chars().chain("000".chars()).take(3).collect();
            digits.parse().ok()?
        }
    };
    Some(((hours * 60 + minutes) * 60 + whole) * 1000 + fraction_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCATION: &str = "http://192.168.1.5:8200/rootDesc.xml";

    fn res(protocol_info: &str) -> Res {
        Res {
            key: protocol_info.into(),
            protocol_info: protocol_info.into(),
            ..Res::default()
        }
    }

    #[test]
    fn durations_come_in_the_upnp_clock_format() {
        assert_eq!(parse_duration("0:03:05.500"), Some(185_500));
        assert_eq!(parse_duration("1:00:00"), Some(3_600_000));
        assert_eq!(parse_duration("0:00:01.5"), Some(1_500));
        assert_eq!(parse_duration("0:00:01.1/4"), Some(1_250));
        assert_eq!(parse_duration("0:00:00"), Some(0));
        assert_eq!(parse_duration("0:00:01.123456"), Some(1_123));
        assert_eq!(parse_duration("0:00:01.1/0"), None);
        assert_eq!(parse_duration("123:00:00"), Some(442_800_000));
        assert_eq!(parse_duration(" 0:01:00 "), Some(60_000));
        assert_eq!(parse_duration("-1:00:00"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("03:05"), None);
        assert_eq!(parse_duration("garbage"), None);
    }

    #[test]
    fn the_original_file_is_preferred_over_transcodes_and_raw_pcm_is_never_taken() {
        let item = Item {
            res: vec![
                res("http-get:*:audio/L16;rate=44100;channels=2:DLNA.ORG_PN=LPCM"),
                res("http-get:*:audio/mpeg:DLNA.ORG_PN=MP3;DLNA.ORG_CI=1"),
                res("rtsp-rtp-udp:*:audio/flac:*"),
                res("http-get:*:audio/x-flac:DLNA.ORG_OP=01;DLNA.ORG_CI=0"),
            ],
            ..Item::default()
        };
        assert_eq!(item.pick().unwrap().mime(), "audio/x-flac");
    }

    #[test]
    fn cover_and_video_resources_on_an_audio_item_are_never_played() {
        let item = Item {
            res: vec![
                res("http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_TN"),
                res("http-get:*:video/mp4:*"),
                res("http-get:*:audio/mpeg:DLNA.ORG_CI=1"),
                res("http-get:*:application/ogg:*"),
            ],
            ..Item::default()
        };
        assert_eq!(item.pick().unwrap().mime(), "application/ogg");
        let only_art = Item {
            res: vec![res("http-get:*:image/jpeg:*")],
            ..Item::default()
        };
        assert_eq!(only_art.pick(), None);
    }

    #[test]
    fn a_transcode_is_taken_when_there_is_no_original() {
        let item = Item {
            res: vec![
                res("http-get:*:audio/L16;rate=44100;channels=2:*"),
                res("http-get:*:audio/mpeg:DLNA.ORG_CI=1"),
            ],
            ..Item::default()
        };
        assert_eq!(item.pick().unwrap().mime(), "audio/mpeg");
        let only_pcm = Item {
            res: vec![res("http-get:*:audio/L16:*")],
            ..Item::default()
        };
        assert_eq!(only_pcm.pick(), None);
    }

    #[test]
    fn items_containers_and_roles_are_read() {
        let text = r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"
 xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
<container id="64$0" parentID="64" restricted="1"><dc:title>Rock</dc:title>
<upnp:class>object.container.storageFolder</upnp:class></container>
<item id="64$1" parentID="64" refID="1$4$0" restricted="1">
<dc:title>Song &amp; Dance</dc:title>
<upnp:class>object.item.audioItem.musicTrack</upnp:class>
<dc:creator>Creator</dc:creator>
<upnp:artist role="Composer">Bach</upnp:artist>
<upnp:artist>Singer</upnp:artist>
<upnp:artist role="AlbumArtist">Band</upnp:artist>
<upnp:album>Record</upnp:album>
<upnp:genre>Rock</upnp:genre>
<dc:date>1997-01-01</dc:date>
<upnp:originalTrackNumber>3</upnp:originalTrackNumber>
<upnp:albumArtURI dlna:profileID="JPEG_TN" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">http://192.168.1.5:8200/AlbumArt/7-12.jpg</upnp:albumArtURI>
<res size="30000000" duration="0:04:10.000" bitrate="176400" sampleFrequency="44100"
 bitsPerSample="16" nrAudioChannels="2"
 protocolInfo="http-get:*:audio/x-flac:*">http://192.168.1.5:8200/MediaItems/12.flac</res>
</item>
<item id="64$2" parentID="64"><dc:title>Clip</dc:title>
<upnp:class>object.item.videoItem</upnp:class></item>
</DIDL-Lite>"#;
        let page = parse(text, LOCATION).unwrap();
        assert_eq!(page.containers, vec!["64$0".to_string()]);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.entries.len(), 3);
        let item = &page.items[0];
        assert_eq!(item.title, "Song & Dance");
        assert_eq!(item.artists, vec!["Singer".to_string()]);
        assert_eq!(item.album_artists, vec!["Band".to_string()]);
        assert_eq!(item.album.as_deref(), Some("Record"));
        assert_eq!(item.track_number, Some(3));
        assert_eq!(item.date.as_deref(), Some("1997-01-01"));
        assert_eq!(item.album_art.as_deref(), Some("/AlbumArt/7-12.jpg"));
        let res = item.pick().unwrap();
        assert_eq!(res.key, "/MediaItems/12.flac");
        assert_eq!(res.id(), "/MediaItems/12.flac#30000000");
        assert_eq!(
            item.cover_id().as_deref(),
            Some("/AlbumArt/7-12.jpg#30000000")
        );
        assert_eq!(res.size, Some(30_000_000));
        assert_eq!(res.duration_ms, Some(250_000));
        assert_eq!(res.bitrate, Some(176_400));
        assert_eq!(res.sample_rate, Some(44_100));
    }

    #[test]
    fn the_creator_stands_in_for_a_missing_artist() {
        let text = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="1">
<dc:title>T</dc:title><dc:creator>Someone</dc:creator>
<upnp:class>object.item.audioItem</upnp:class></item></DIDL-Lite>"#;
        let page = parse(text, LOCATION).unwrap();
        assert_eq!(page.items[0].artists, vec!["Someone".to_string()]);
    }

    #[test]
    fn every_genre_element_is_kept() {
        let text = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="1">
<dc:title>T</dc:title><upnp:genre>Rock</upnp:genre><upnp:genre> </upnp:genre>
<upnp:genre>Pop</upnp:genre><upnp:class>object.item.audioItem</upnp:class></item>
<item id="2"><dc:title>U</dc:title><upnp:class>object.item.audioItem</upnp:class></item>
</DIDL-Lite>"#;
        let page = parse(text, LOCATION).unwrap();
        assert_eq!(
            page.items[0].genres,
            vec!["Rock".to_string(), "Pop".to_string()]
        );
        assert!(page.items[1].genres.is_empty());
    }

    #[test]
    fn undeclared_namespace_prefixes_are_declared_before_giving_up() {
        let text = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"><item id="1">
<dc:title>T</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class>
<res protocolInfo="http-get:*:audio/mpeg:*" dlna:ifoFileURI="x">http://192.168.1.5:8200/a.mp3</res>
</item></DIDL-Lite>"#;
        let page = parse(text, LOCATION).unwrap();
        assert_eq!(page.items[0].pick().unwrap().key, "/a.mp3");
    }

    #[test]
    fn entities_cdata_and_blank_fields_are_handled() {
        let text = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="1">
<dc:title><![CDATA[Rock & Roll <live>]]></dc:title><upnp:artist>AC&amp;amp;DC</upnp:artist>
<upnp:album>   </upnp:album><upnp:originalTrackNumber>x</upnp:originalTrackNumber>
<upnp:class>object.item.audioItem.musicTrack</upnp:class>
<res protocolInfo="http-get:*:audio/mpeg:*" size="-5" duration="bogus">http://192.168.1.5:8200/a.mp3</res>
</item></DIDL-Lite>"#;
        let item = &parse(text, LOCATION).unwrap().items[0];
        assert_eq!(item.title, "Rock & Roll <live>");
        assert_eq!(item.artists, vec!["AC&amp;DC".to_string()]);
        assert_eq!(item.album, None);
        assert_eq!(item.track_number, None);
        let res = item.pick().unwrap();
        assert_eq!((res.size, res.duration_ms), (None, None));
    }

    #[test]
    fn a_bom_and_a_doctype_do_not_break_parsing() {
        let text = "\u{feff}<?xml version=\"1.0\"?><!DOCTYPE DIDL-Lite []><DIDL-Lite><container id=\"7\"/></DIDL-Lite>";
        assert_eq!(
            parse(text, LOCATION).unwrap().containers,
            vec!["7".to_string()]
        );
    }

    #[test]
    fn an_empty_result_is_an_empty_page() {
        let page = parse("", LOCATION).unwrap();
        assert!(page.items.is_empty() && page.containers.is_empty());
    }
}
