use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const STREAMINFO: u8 = 0;
const SEEKTABLE: u8 = 3;
const LAST: u8 = 0x80;
const STREAMINFO_LEN: usize = 34;
const SEEK_POINT_LEN: usize = 18;
const PLACEHOLDER: u64 = u64::MAX;
const POINT_EVERY_SECONDS: u64 = 10;
const MAX_POINTS: usize = 8192;
const MIN_WINDOW: u64 = 16 * 1024;
const UNKNOWN_WINDOW: u64 = 64 * 1024;
const MAX_WINDOW: u64 = 1024 * 1024;
const LONGEST_HEADER: u64 = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Indexed {
    pub head: Vec<u8>,
    pub audio_start: u64,
    pub points: usize,
}

#[derive(Debug, Clone)]
struct StreamInfo {
    raw: [u8; STREAMINFO_LEN],
    min_block: u32,
    max_block: u32,
    max_frame: u32,
    sample_rate: u32,
    channels: u32,
    bits: u32,
    total_samples: u64,
}

impl StreamInfo {
    fn parse(raw: [u8; STREAMINFO_LEN]) -> Self {
        let packed = u64::from_be_bytes(raw[10..18].try_into().unwrap_or_default());
        StreamInfo {
            min_block: u32::from(u16::from_be_bytes([raw[0], raw[1]])),
            max_block: u32::from(u16::from_be_bytes([raw[2], raw[3]])),
            max_frame: u32::from_be_bytes([0, raw[7], raw[8], raw[9]]),
            sample_rate: (packed >> 44) as u32,
            channels: ((packed >> 41) & 0x07) as u32 + 1,
            bits: ((packed >> 36) & 0x1F) as u32 + 1,
            total_samples: packed & 0xF_FFFF_FFFF,
            raw,
        }
    }
}

struct Layout {
    info: StreamInfo,
    audio_start: u64,
    indexed: bool,
}

fn skip_id3(reader: &mut impl Read) -> io::Result<[u8; 4]> {
    let mut marker = [0u8; 4];
    reader.read_exact(&mut marker)?;
    if &marker[..3] != b"ID3" {
        return Ok(marker);
    }
    let mut rest = [0u8; 6];
    reader.read_exact(&mut rest)?;
    let size = rest[2..]
        .iter()
        .fold(0u64, |size, byte| (size << 7) | u64::from(byte & 0x7F));
    let footer = if rest[1] & 0x10 != 0 { 10 } else { 0 };
    io::copy(&mut reader.take(size + footer), &mut io::sink())?;
    reader.read_exact(&mut marker)?;
    Ok(marker)
}

fn layout(reader: &mut (impl Read + Seek)) -> io::Result<Option<Layout>> {
    if &skip_id3(reader)? != b"fLaC" {
        return Ok(None);
    }
    let mut info = None;
    let mut indexed = false;
    loop {
        let mut header = [0u8; 4];
        reader.read_exact(&mut header)?;
        let kind = header[0] & !LAST;
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]);
        match kind {
            STREAMINFO if length as usize == STREAMINFO_LEN => {
                let mut raw = [0u8; STREAMINFO_LEN];
                reader.read_exact(&mut raw)?;
                info = Some(StreamInfo::parse(raw));
            }
            SEEKTABLE => {
                let mut table = vec![0u8; length as usize];
                reader.read_exact(&mut table)?;
                indexed |= table
                    .as_chunks::<SEEK_POINT_LEN>()
                    .0
                    .iter()
                    .any(|point| point[..8] != PLACEHOLDER.to_be_bytes());
            }
            _ => {
                reader.seek(SeekFrom::Current(i64::from(length)))?;
            }
        }
        if header[0] & LAST != 0 {
            break;
        }
    }
    let audio_start = reader.stream_position()?;
    Ok(info.map(|info| Layout {
        info,
        audio_start,
        indexed,
    }))
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn coded_number(bytes: &[u8]) -> Option<(u64, usize)> {
    let first = *bytes.first()?;
    let (len, mut value) = match first {
        0x00..=0x7F => return Some((u64::from(first), 1)),
        0xC0..=0xDF => (2, u64::from(first & 0x1F)),
        0xE0..=0xEF => (3, u64::from(first & 0x0F)),
        0xF0..=0xF7 => (4, u64::from(first & 0x07)),
        0xF8..=0xFB => (5, u64::from(first & 0x03)),
        0xFC..=0xFD => (6, u64::from(first & 0x01)),
        0xFE => (7, 0),
        _ => return None,
    };
    for at in 1..len {
        let byte = *bytes.get(at)?;
        if byte & 0xC0 != 0x80 {
            return None;
        }
        value = (value << 6) | u64::from(byte & 0x3F);
    }
    Some((value, len))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    sample: u64,
    block: u32,
    variable: bool,
}

fn frame_at(bytes: &[u8], info: &StreamInfo, blocking: Option<bool>) -> Option<Frame> {
    if bytes.len() < 6 || bytes[0] != 0xFF || bytes[1] & 0xFE != 0xF8 {
        return None;
    }
    let variable = bytes[1] & 1 == 1;
    if blocking.is_some_and(|expected| expected != variable) {
        return None;
    }
    let block_code = bytes[2] >> 4;
    let rate_code = bytes[2] & 0x0F;
    let channels = bytes[3] >> 4;
    let size_code = (bytes[3] >> 1) & 0x07;
    if block_code == 0 || rate_code == 0x0F || channels > 10 || size_code == 3 || bytes[3] & 1 != 0
    {
        return None;
    }
    let rate = match rate_code {
        1 => 88_200,
        2 => 176_400,
        3 => 192_000,
        4 => 8_000,
        5 => 16_000,
        6 => 22_050,
        7 => 24_000,
        8 => 32_000,
        9 => 44_100,
        10 => 48_000,
        11 => 96_000,
        _ => info.sample_rate,
    };
    let bits = match size_code {
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => info.bits,
    };
    let channels_agree = match channels {
        0..=7 => u32::from(channels) + 1 == info.channels,
        _ => info.channels == 2,
    };
    if rate != info.sample_rate || bits != info.bits || !channels_agree {
        return None;
    }
    let (number, len) = coded_number(&bytes[4..])?;
    let mut at = 4 + len;
    let block = match block_code {
        1 => 192,
        2..=5 => 576 << (block_code - 2),
        6 => {
            at += 1;
            u32::from(*bytes.get(at - 1)?) + 1
        }
        7 => {
            at += 2;
            u32::from(u16::from_be_bytes([
                *bytes.get(at - 2)?,
                *bytes.get(at - 1)?,
            ])) + 1
        }
        _ => 256 << (block_code - 8),
    };
    at += match rate_code {
        12 => 1,
        13 | 14 => 2,
        _ => 0,
    };
    if crc8(bytes.get(..at)?) != *bytes.get(at)? {
        return None;
    }
    let sample = if variable {
        number
    } else {
        number.checked_mul(u64::from(info.max_block))?
    };
    if info.total_samples > 0 && sample >= info.total_samples {
        return None;
    }
    let last = info.total_samples > 0 && sample + u64::from(block) == info.total_samples;
    if block > u32::from(u16::MAX)
        || (!variable && info.min_block == info.max_block && block != info.max_block && !last)
    {
        return None;
    }
    Some(Frame {
        sample,
        block,
        variable,
    })
}

fn frame_from(
    file: &mut File,
    from: u64,
    end: u64,
    window: u64,
    info: &StreamInfo,
    blocking: Option<bool>,
    buffer: &mut Vec<u8>,
) -> io::Result<Option<(u64, Frame)>> {
    file.seek(SeekFrom::Start(from))?;
    buffer.clear();
    file.take(window.min(end.saturating_sub(from)))
        .read_to_end(buffer)?;
    Ok(buffer
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] == 0xFF && pair[1] & 0xFE == 0xF8)
        .find_map(|(at, _)| {
            frame_at(&buffer[at..], info, blocking).map(|frame| (from + at as u64, frame))
        }))
}

fn seek_points(
    file: &mut File,
    layout: &Layout,
    end: u64,
    step: u64,
) -> io::Result<Vec<(u64, u64, u32)>> {
    let info = &layout.info;
    let audio_len = end.saturating_sub(layout.audio_start);
    if info.total_samples == 0 || audio_len == 0 || step == 0 {
        return Ok(Vec::new());
    }
    let window = match info.max_frame {
        0 => UNKNOWN_WINDOW,
        known => (u64::from(known) + LONGEST_HEADER).clamp(MIN_WINDOW, MAX_WINDOW),
    };
    let step = step.max(info.total_samples.div_ceil(MAX_POINTS as u64));
    let mut buffer = Vec::new();
    let mut points: Vec<(u64, u64, u32)> = Vec::new();
    let mut blocking = None;
    let mut target = 0u64;
    while target < info.total_samples && points.len() < MAX_POINTS {
        let estimate = layout.audio_start
            + (u128::from(audio_len) * u128::from(target) / u128::from(info.total_samples)) as u64;
        if let Some((offset, frame)) =
            frame_from(file, estimate, end, window, info, blocking, &mut buffer)?
            && points
                .last()
                .is_none_or(|(sample, ..)| frame.sample > *sample)
        {
            blocking.get_or_insert(frame.variable);
            points.push((frame.sample, offset - layout.audio_start, frame.block));
        }
        target += step;
    }
    Ok(points)
}

fn head(info: &StreamInfo, points: &[(u64, u64, u32)]) -> Vec<u8> {
    let table_len = (points.len() * SEEK_POINT_LEN) as u32;
    let mut head = b"fLaC".to_vec();
    head.push(STREAMINFO);
    head.extend_from_slice(&(STREAMINFO_LEN as u32).to_be_bytes()[1..]);
    head.extend_from_slice(&info.raw);
    head.push(SEEKTABLE | LAST);
    head.extend_from_slice(&table_len.to_be_bytes()[1..]);
    for (sample, offset, block) in points {
        head.extend_from_slice(&sample.to_be_bytes());
        head.extend_from_slice(&offset.to_be_bytes());
        head.extend_from_slice(&(*block as u16).to_be_bytes());
    }
    head
}

fn indexed_every(path: &Path, step: Option<u64>) -> io::Result<Option<Indexed>> {
    let mut file = File::open(path)?;
    let end = file.metadata()?.len();
    let Some(layout) = layout(&mut file)? else {
        return Ok(None);
    };
    if layout.indexed {
        return Ok(None);
    }
    let step = step.unwrap_or(u64::from(layout.info.sample_rate) * POINT_EVERY_SECONDS);
    let points = seek_points(&mut file, &layout, end, step)?;
    if points.is_empty() {
        return Ok(None);
    }
    Ok(Some(Indexed {
        head: head(&layout.info, &points),
        audio_start: layout.audio_start,
        points: points.len(),
    }))
}

pub(crate) fn indexed(path: &Path) -> io::Result<Option<Indexed>> {
    indexed_every(path, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../fixtures")
            .join(name)
    }

    fn table(head: &[u8]) -> Vec<(u64, u64, u16)> {
        let start = 4 + 4 + STREAMINFO_LEN;
        assert_eq!(head[start], SEEKTABLE | LAST);
        head[start + 4..]
            .as_chunks::<SEEK_POINT_LEN>()
            .0
            .iter()
            .map(|point| {
                (
                    u64::from_be_bytes(point[..8].try_into().unwrap()),
                    u64::from_be_bytes(point[8..16].try_into().unwrap()),
                    u16::from_be_bytes([point[16], point[17]]),
                )
            })
            .collect()
    }

    #[test]
    fn every_seek_point_is_a_frame_with_its_own_sample_number() {
        let path = fixture("tagged_with_cover.flac");
        let indexed = indexed_every(&path, Some(1000)).unwrap().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let info = layout(&mut Cursor::new(&bytes)).unwrap().unwrap().info;
        let points = table(&indexed.head);
        assert_eq!(points.len(), indexed.points);
        assert_eq!(points[0], (0, 0, 4608));
        assert!(points.len() >= 4, "{points:?}");
        assert!(points.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (sample, offset, block) in points {
            let at = (indexed.audio_start + offset) as usize;
            assert_eq!(
                frame_at(&bytes[at..], &info, Some(false)),
                Some(Frame {
                    sample,
                    block: u32::from(block),
                    variable: false,
                })
            );
        }
    }

    #[test]
    fn the_new_head_replaces_the_metadata_and_keeps_the_stream_info() {
        let path = fixture("tagged_with_cover.flac");
        let indexed = indexed(&path).unwrap().unwrap();
        let mut spliced = indexed.head.clone();
        let bytes = std::fs::read(&path).unwrap();
        spliced.extend_from_slice(&bytes[indexed.audio_start as usize..]);
        let original = layout(&mut Cursor::new(&bytes)).unwrap().unwrap();
        let rebuilt = layout(&mut Cursor::new(&spliced)).unwrap().unwrap();
        assert!(!original.indexed);
        assert!(rebuilt.indexed);
        assert_eq!(rebuilt.info.raw, original.info.raw);
        assert_eq!(rebuilt.audio_start as usize, indexed.head.len());
    }

    #[test]
    fn a_file_with_a_seek_table_or_another_format_is_left_alone() {
        let mut bytes = std::fs::read(fixture("tagless.flac")).unwrap();
        let first = indexed(&fixture("tagless.flac")).unwrap().unwrap();
        let audio = bytes.split_off(first.audio_start as usize);
        let mut spliced = first.head.clone();
        spliced.extend_from_slice(&audio);
        let dir = std::env::temp_dir().join(format!("pawse-flac-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("indexed.flac");
        std::fs::write(&path, &spliced).unwrap();
        assert_eq!(indexed(&path).unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            indexed(&fixture("sine_440_16_44_stereo.wav")).unwrap(),
            None
        );
    }

    #[test]
    fn a_damaged_frame_header_is_not_a_frame() {
        let bytes = std::fs::read(fixture("tagless.flac")).unwrap();
        let layout = layout(&mut Cursor::new(&bytes)).unwrap().unwrap();
        let first = &bytes[layout.audio_start as usize..];
        assert_eq!(
            frame_at(first, &layout.info, None),
            Some(Frame {
                sample: 0,
                block: 4608,
                variable: false,
            })
        );
        assert_eq!(frame_at(first, &layout.info, Some(true)), None);
        let mut other_channels = first[..16].to_vec();
        other_channels[3] = (other_channels[3] & 0x0F) | 0x40;
        assert_eq!(frame_at(&other_channels, &layout.info, None), None);
        let mut damaged = first[..16].to_vec();
        damaged[4] ^= 0x01;
        assert_eq!(frame_at(&damaged, &layout.info, None), None);
        assert_eq!(coded_number(&[0xC2, 0x80]), Some((0x80, 2)));
        assert_eq!(coded_number(&[0xC2, 0x00]), None);
        assert_eq!(crc8(b"123456789"), 0xF4);
    }

    #[test]
    fn an_id3_tag_before_the_stream_is_skipped() {
        let bytes = std::fs::read(fixture("tagless.flac")).unwrap();
        let mut tagged = b"ID3\x04\x00\x00\x00\x00\x00\x05hello".to_vec();
        tagged.extend_from_slice(&bytes);
        let plain = layout(&mut Cursor::new(&bytes)).unwrap().unwrap();
        let skipped = layout(&mut Cursor::new(&tagged)).unwrap().unwrap();
        assert_eq!(skipped.audio_start, plain.audio_start + 15);
    }
}
