use std::io::{Read, Seek, SeekFrom};

use crate::error::DsdError;

#[derive(Debug, Clone, Default)]
pub struct Id3Tag {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub cover_art: Option<Vec<u8>>,
}

pub fn read_from<R: Read + Seek>(reader: &mut R, offset: u64) -> Result<Option<Id3Tag>, DsdError> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut header = [0u8; 10];
    if reader.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    if &header[0..3] != b"ID3" {
        return Ok(None);
    }
    let tag_size = synchsafe(&header[6..10]) as usize;
    let mut full = Vec::with_capacity(10 + tag_size);
    full.extend_from_slice(&header);
    let mut body = vec![0u8; tag_size];
    reader.read_exact(&mut body)?;
    full.extend_from_slice(&body);
    Ok(parse_id3v2(&full))
}

fn synchsafe(b: &[u8]) -> u32 {
    ((b[0] as u32) << 21) | ((b[1] as u32) << 14) | ((b[2] as u32) << 7) | (b[3] as u32)
}

fn parse_id3v2(data: &[u8]) -> Option<Id3Tag> {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return None;
    }
    let major = data[3];
    let flags = data[5];
    let tag_size = synchsafe(&data[6..10]) as usize;
    let unsynchronized = flags & 0x80 != 0;
    let has_extended_header = flags & 0x40 != 0;

    let body_end = (10 + tag_size).min(data.len());
    let raw_body = &data[10..body_end];
    let body = if unsynchronized {
        remove_unsync(raw_body)
    } else {
        raw_body.to_vec()
    };

    let mut offset = 0usize;
    if has_extended_header {
        if body.len() < 4 {
            return Some(Id3Tag::default());
        }
        offset = if major >= 4 {
            synchsafe(&body[0..4]) as usize
        } else {
            u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize + 4
        };
        if offset > body.len() {
            return Some(Id3Tag::default());
        }
    }

    let mut tag = Id3Tag::default();
    while offset + 10 <= body.len() {
        let id = &body[offset..offset + 4];
        if id == [0, 0, 0, 0] {
            break;
        }
        let frame_size = if major >= 4 {
            synchsafe(&body[offset + 4..offset + 8]) as usize
        } else {
            u32::from_be_bytes([
                body[offset + 4],
                body[offset + 5],
                body[offset + 6],
                body[offset + 7],
            ]) as usize
        };
        let frame_start = offset + 10;
        let frame_end = frame_start + frame_size;
        if frame_end > body.len() {
            break;
        }
        if frame_size == 0 {
            // A legitimate empty frame (some taggers write these) — skip
            // just this one. Only the all-zero id above means end-of-tag
            // padding.
            offset = frame_start;
            continue;
        }
        let frame_data = &body[frame_start..frame_end];

        match id {
            b"TIT2" => tag.title = decode_text_frame(frame_data).into_iter().next(),
            b"TALB" => tag.album = decode_text_frame(frame_data).into_iter().next(),
            b"TPE1" => tag.artists = decode_text_frame(frame_data),
            b"TPE2" => tag.album_artists = decode_text_frame(frame_data),
            b"TRCK" => {
                tag.track_number = decode_text_frame(frame_data)
                    .into_iter()
                    .next()
                    .and_then(|s| parse_leading_number(&s))
            }
            b"TPOS" => {
                tag.disc_number = decode_text_frame(frame_data)
                    .into_iter()
                    .next()
                    .and_then(|s| parse_leading_number(&s))
            }
            b"TDRC" | b"TYER" => {
                tag.year = decode_text_frame(frame_data)
                    .into_iter()
                    .next()
                    .and_then(|s| parse_leading_number(&s))
                    .map(|n| n as i32)
            }
            b"TCON" => tag.genres = decode_text_frame(frame_data),
            b"APIC" => {
                if let Some(pic) = parse_apic(frame_data)
                    && (tag.cover_art.is_none() || pic.is_front)
                {
                    tag.cover_art = Some(pic.data);
                }
            }
            _ => {}
        }

        offset = frame_end;
    }

    Some(tag)
}

fn remove_unsync(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        out.push(data[i]);
        if data[i] == 0xFF && i + 1 < data.len() && data[i + 1] == 0x00 {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn parse_leading_number(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn decode_text(encoding: u8, data: &[u8]) -> String {
    match encoding {
        0x00 => data.iter().map(|&b| b as char).collect(),
        0x01 => decode_utf16_with_bom(data),
        0x02 => decode_utf16(data, true),
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

fn decode_utf16_with_bom(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }
    let big_endian = data[0] == 0xFE && data[1] == 0xFF;
    decode_utf16(&data[2..], big_endian)
}

fn decode_utf16(data: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| {
            if big_endian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn split_null_separated(s: &str) -> Vec<String> {
    s.split('\u{0}')
        .map(|p| p.trim_end_matches('\u{0}').trim())
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

fn decode_text_frame(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return vec![];
    }
    let encoding = data[0];
    let text = decode_text(encoding, &data[1..]);
    split_null_separated(&text)
}

struct ApicPicture {
    data: Vec<u8>,
    is_front: bool,
}

fn terminator_len(encoding: u8) -> usize {
    match encoding {
        0x01 | 0x02 => 2,
        _ => 1,
    }
}

fn find_text_terminator(data: &[u8], encoding: u8) -> Option<usize> {
    match encoding {
        0x01 | 0x02 => {
            let mut i = 0;
            while i + 1 < data.len() {
                if data[i] == 0 && data[i + 1] == 0 {
                    return Some(i);
                }
                i += 2;
            }
            None
        }
        _ => data.iter().position(|&b| b == 0),
    }
}

fn parse_apic(data: &[u8]) -> Option<ApicPicture> {
    if data.is_empty() {
        return None;
    }
    let encoding = data[0];
    let mut i = 1;

    let mime_end = data[i..].iter().position(|&b| b == 0)? + i;
    i = mime_end + 1;
    if i >= data.len() {
        return None;
    }

    let picture_type = data[i];
    i += 1;

    let desc_end = find_text_terminator(&data[i..], encoding)? + i;
    i = desc_end + terminator_len(encoding);
    if i > data.len() {
        return None;
    }

    Some(ApicPicture {
        data: data[i..].to_vec(),
        is_front: picture_type == 3,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn synchsafe_bytes(v: u32) -> [u8; 4] {
        [
            ((v >> 21) & 0x7F) as u8,
            ((v >> 14) & 0x7F) as u8,
            ((v >> 7) & 0x7F) as u8,
            (v & 0x7F) as u8,
        ]
    }

    fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
        let mut payload = vec![0x03u8]; // UTF-8
        payload.extend_from_slice(text.as_bytes());
        let mut frame = Vec::new();
        frame.extend_from_slice(id);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]); // flags
        frame.extend_from_slice(&payload);
        frame
    }

    fn build_id3v23(frames: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = frames.iter().flatten().copied().collect();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ID3");
        buf.push(3); // major version
        buf.push(0); // minor
        buf.push(0); // flags
        buf.extend_from_slice(&synchsafe_bytes(body.len() as u32));
        buf.extend_from_slice(&body);
        buf
    }

    #[test]
    fn parses_basic_text_frames() {
        let data = build_id3v23(&[
            text_frame(b"TIT2", "Test Title"),
            text_frame(b"TALB", "Test Album"),
            text_frame(b"TPE1", "Artist One"),
            text_frame(b"TRCK", "3/12"),
            text_frame(b"TYER", "2021"),
        ]);
        let mut cursor = Cursor::new(data);
        let tag = read_from(&mut cursor, 0).unwrap().unwrap();
        assert_eq!(tag.title.as_deref(), Some("Test Title"));
        assert_eq!(tag.album.as_deref(), Some("Test Album"));
        assert_eq!(tag.artists, vec!["Artist One"]);
        assert_eq!(tag.track_number, Some(3));
        assert_eq!(tag.year, Some(2021));
    }

    #[test]
    fn a_legitimate_empty_frame_does_not_swallow_the_rest_of_the_tag() {
        // Some non-strict taggers write a zero-size frame (e.g. an emptied
        // TCON). That must not be treated as end-of-tag padding — only the
        // all-zero frame id means that — or every frame after it silently
        // disappears.
        let empty_tcon = {
            let mut frame = Vec::new();
            frame.extend_from_slice(b"TCON");
            frame.extend_from_slice(&0u32.to_be_bytes());
            frame.extend_from_slice(&[0, 0]);
            frame
        };
        let data = build_id3v23(&[empty_tcon, text_frame(b"TIT2", "Still Here")]);
        let mut cursor = Cursor::new(data);
        let tag = read_from(&mut cursor, 0).unwrap().unwrap();
        assert_eq!(tag.title.as_deref(), Some("Still Here"));
        assert!(tag.genres.is_empty());
    }

    #[test]
    fn returns_none_without_id3_magic() {
        let data = vec![0u8; 20];
        let mut cursor = Cursor::new(data);
        assert!(read_from(&mut cursor, 0).unwrap().is_none());
    }

    #[test]
    fn parses_apic_front_cover() {
        let mut apic_payload = vec![0x00u8]; // Latin1
        apic_payload.extend_from_slice(b"image/jpeg\0");
        apic_payload.push(3); // front cover
        apic_payload.push(0); // empty description
        apic_payload.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xAA]);

        let mut frame = Vec::new();
        frame.extend_from_slice(b"APIC");
        frame.extend_from_slice(&(apic_payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&apic_payload);

        let data = build_id3v23(&[frame]);
        let mut cursor = Cursor::new(data);
        let tag = read_from(&mut cursor, 0).unwrap().unwrap();
        assert_eq!(
            tag.cover_art.as_deref(),
            Some(&[0xFF, 0xD8, 0xFF, 0xAA][..])
        );
    }

    #[test]
    fn decodes_utf16_with_bom() {
        let mut payload = vec![0x01u8];
        payload.extend_from_slice(&[0xFF, 0xFE]); // LE BOM
        for c in "Hi".encode_utf16() {
            payload.extend_from_slice(&c.to_le_bytes());
        }
        let mut frame = Vec::new();
        frame.extend_from_slice(b"TIT2");
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&payload);

        let data = build_id3v23(&[frame]);
        let mut cursor = Cursor::new(data);
        let tag = read_from(&mut cursor, 0).unwrap().unwrap();
        assert_eq!(tag.title.as_deref(), Some("Hi"));
    }

    /// Forward unsynchronisation (test-only): inserts a stuffing 0x00 after
    /// every 0xFF that is followed by a byte which would otherwise look like
    /// a false sync (0x00, or top 3 bits set) — the mirror of `remove_unsync`.
    fn apply_unsync(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for (i, &b) in data.iter().enumerate() {
            out.push(b);
            if b == 0xFF {
                let next_looks_like_sync = data
                    .get(i + 1)
                    .is_some_and(|&n| n == 0x00 || n & 0xE0 == 0xE0);
                if next_looks_like_sync {
                    out.push(0x00);
                }
            }
        }
        out
    }

    #[test]
    fn handles_unsynchronisation() {
        // Logical (pre-stuffing) TIT2 payload: Latin1 encoding byte, then
        // 'A', 0xFF, NUL, 'B' — the 0xFF-then-NUL is the one pattern an
        // encoder must always stuff, since decoders unconditionally strip
        // any 0x00 immediately following a 0xFF.
        let logical_payload = vec![0x00u8, 0x41, 0xFF, 0x00, 0x42];
        let mut frame = Vec::new();
        frame.extend_from_slice(b"TIT2");
        frame.extend_from_slice(&(logical_payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&logical_payload);

        let stuffed_body = apply_unsync(&frame);
        assert_eq!(
            stuffed_body.len(),
            frame.len() + 1,
            "sanity: stuffing added exactly one byte"
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(b"ID3");
        buf.push(3);
        buf.push(0);
        buf.push(0x80); // unsynchronisation flag
        buf.extend_from_slice(&synchsafe_bytes(stuffed_body.len() as u32));
        buf.extend_from_slice(&stuffed_body);

        let mut cursor = Cursor::new(buf);
        let tag = read_from(&mut cursor, 0).unwrap().unwrap();
        // The decoded text is "A\u{ff}\0B"; split_null_separated cuts it at
        // the embedded NUL, so title (first value) is "A\u{ff}".
        assert_eq!(tag.title.as_deref(), Some("A\u{ff}"));
    }
}
