use std::io::{Read, Seek, SeekFrom};

use crate::error::DsdError;

const DSD_HEADER_SIZE: u64 = 28;
const FMT_CHUNK_SIZE: u64 = 52;
const DATA_CHUNK_HEADER_SIZE: u64 = 12;

#[derive(Debug, Clone, Copy)]
pub struct DsfInfo {
    pub channels: u8,
    pub dsd_rate: u32,
    pub sample_count: u64,
    pub block_size: u32,
    pub data_offset: u64,
    pub id3_offset: Option<u64>,
}

impl DsfInfo {
    pub fn bytes_per_channel(&self) -> u64 {
        self.sample_count.div_ceil(8)
    }

    pub fn total_blocks(&self) -> u64 {
        self.bytes_per_channel().div_ceil(self.block_size as u64)
    }
}

fn read_u32le(r: &mut impl Read) -> Result<u32, DsdError> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64le(r: &mut impl Read) -> Result<u64, DsdError> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_magic(r: &mut impl Read) -> Result<[u8; 4], DsdError> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(b)
}

pub fn parse_header<R: Read + Seek>(reader: &mut R) -> Result<DsfInfo, DsdError> {
    reader.seek(SeekFrom::Start(0))?;

    if &read_magic(reader)? != b"DSD " {
        return Err(DsdError::NotDsd);
    }
    let header_chunk_size = read_u64le(reader)?;
    let _total_file_size = read_u64le(reader)?;
    let id3_pointer = read_u64le(reader)?;
    if header_chunk_size != DSD_HEADER_SIZE {
        return Err(DsdError::MalformedChunk { chunk: "DSD" });
    }

    if &read_magic(reader)? != b"fmt " {
        return Err(DsdError::MalformedChunk { chunk: "fmt " });
    }
    let fmt_chunk_size = read_u64le(reader)?;
    if fmt_chunk_size != FMT_CHUNK_SIZE {
        return Err(DsdError::MalformedChunk { chunk: "fmt " });
    }
    let _format_version = read_u32le(reader)?;
    let _format_id = read_u32le(reader)?;
    let _channel_type = read_u32le(reader)?;
    let channel_num = read_u32le(reader)?;
    let sampling_frequency = read_u32le(reader)?;
    let _bits_per_sample = read_u32le(reader)?;
    let sample_count = read_u64le(reader)?;
    let block_size_per_channel = read_u32le(reader)?;
    let _reserved = read_u32le(reader)?;

    if channel_num == 0 || channel_num > 32 {
        return Err(DsdError::UnsupportedChannels(channel_num));
    }
    if block_size_per_channel == 0 {
        return Err(DsdError::MalformedChunk { chunk: "fmt " });
    }
    if sampling_frequency == 0 {
        return Err(DsdError::MalformedChunk { chunk: "fmt " });
    }

    if &read_magic(reader)? != b"data" {
        return Err(DsdError::MalformedChunk { chunk: "data" });
    }
    let _data_chunk_size = read_u64le(reader)?;
    let data_offset = DSD_HEADER_SIZE + FMT_CHUNK_SIZE + DATA_CHUNK_HEADER_SIZE;

    Ok(DsfInfo {
        channels: channel_num as u8,
        dsd_rate: sampling_frequency,
        sample_count,
        block_size: block_size_per_channel,
        data_offset,
        id3_offset: if id3_pointer == 0 {
            None
        } else {
            Some(id3_pointer)
        },
    })
}

pub struct DsfBlockReader<R> {
    reader: R,
    info: DsfInfo,
    current_block: u64,
}

impl<R: Read + Seek> DsfBlockReader<R> {
    pub fn new(reader: R, info: DsfInfo) -> Self {
        Self {
            reader,
            info,
            current_block: 0,
        }
    }

    pub fn info(&self) -> &DsfInfo {
        &self.info
    }

    pub fn seek_to_block(&mut self, block_index: u64) -> Result<(), DsdError> {
        let byte_offset = self.info.data_offset
            + block_index * self.info.block_size as u64 * self.info.channels as u64;
        self.reader.seek(SeekFrom::Start(byte_offset))?;
        self.current_block = block_index;
        Ok(())
    }

    /// Reads the next super-block (one `block_size`-byte chunk per channel,
    /// DSF's native block-interleaved layout) and returns one contiguous
    /// byte vector per channel. The final block is trimmed to the actual
    /// remaining sample bytes. Returns `Ok(None)` at end of stream.
    pub fn next_block(&mut self) -> Result<Option<Vec<Vec<u8>>>, DsdError> {
        if self.current_block >= self.info.total_blocks() {
            return Ok(None);
        }

        let bytes_per_channel = self.info.bytes_per_channel();
        let block_start = self.current_block * self.info.block_size as u64;
        let remaining = bytes_per_channel.saturating_sub(block_start);
        let this_block_len = remaining.min(self.info.block_size as u64) as usize;

        let mut channels = Vec::with_capacity(self.info.channels as usize);
        for _ in 0..self.info.channels {
            let mut raw = vec![0u8; self.info.block_size as usize];
            self.reader.read_exact(&mut raw)?;
            raw.truncate(this_block_len);
            channels.push(raw);
        }

        self.current_block += 1;
        Ok(Some(channels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build_dsf(channels: u32, dsd_rate: u32, block_size: u32, sample_count: u64) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.extend_from_slice(b"DSD ");
        buf.extend_from_slice(&DSD_HEADER_SIZE.to_le_bytes());
        let bytes_per_channel = sample_count.div_ceil(8);
        let total_blocks = bytes_per_channel.div_ceil(block_size as u64);
        let data_size = total_blocks * block_size as u64 * channels as u64;
        let total_file_size = DSD_HEADER_SIZE + FMT_CHUNK_SIZE + DATA_CHUNK_HEADER_SIZE + data_size;
        buf.extend_from_slice(&total_file_size.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes()); // no id3

        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&FMT_CHUNK_SIZE.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // format version
        buf.extend_from_slice(&0u32.to_le_bytes()); // format id (raw DSD)
        buf.extend_from_slice(&(channels.min(2) + 1).to_le_bytes()); // channel type (approx)
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&dsd_rate.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // bits per sample
        buf.extend_from_slice(&sample_count.to_le_bytes());
        buf.extend_from_slice(&block_size.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // reserved

        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(DATA_CHUNK_HEADER_SIZE + data_size).to_le_bytes());

        for block in 0..total_blocks {
            for ch in 0..channels {
                for i in 0..block_size {
                    buf.push(((block * 1000 + ch as u64 * 10 + i as u64) % 256) as u8);
                }
            }
        }

        buf
    }

    #[test]
    fn parses_header_fields() {
        let data = build_dsf(2, 2_822_400, 4096, 4096 * 8 * 3);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.dsd_rate, 2_822_400);
        assert_eq!(info.block_size, 4096);
        assert_eq!(info.sample_count, 4096 * 8 * 3);
        assert_eq!(info.total_blocks(), 3);
        assert!(info.id3_offset.is_none());
    }

    #[test]
    fn rejects_non_dsf_magic() {
        let mut cursor = Cursor::new(vec![0u8; 64]);
        assert!(matches!(parse_header(&mut cursor), Err(DsdError::NotDsd)));
    }

    #[test]
    fn rejects_zero_sample_rate() {
        // A zero `sampling_frequency` must fail to parse cleanly — letting
        // it through would later feed 0 into a division when computing
        // duration/seek and panic (NaN/inf into `Duration::from_secs_f64`).
        let data = build_dsf(2, 0, 4096, 4096 * 8 * 3);
        let mut cursor = Cursor::new(data);
        assert!(matches!(
            parse_header(&mut cursor),
            Err(DsdError::MalformedChunk { chunk: "fmt " })
        ));
    }

    #[test]
    fn reads_block_interleaved_channels_in_order() {
        let data = build_dsf(2, 2_822_400, 8, 8 * 8 * 2);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        let mut reader = DsfBlockReader::new(cursor, info);

        let block0 = reader.next_block().unwrap().unwrap();
        assert_eq!(block0.len(), 2);
        assert_eq!(block0[0], vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(block0[1], vec![10, 11, 12, 13, 14, 15, 16, 17]);

        let block1 = reader.next_block().unwrap().unwrap();
        assert_eq!(
            block1[0],
            vec![1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007]
                .into_iter()
                .map(|v: u32| (v % 256) as u8)
                .collect::<Vec<u8>>()
        );

        assert!(reader.next_block().unwrap().is_none());
    }

    #[test]
    fn seek_to_block_repositions_reader() {
        let data = build_dsf(1, 2_822_400, 8, 8 * 8 * 4);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        let mut reader = DsfBlockReader::new(cursor, info);

        reader.seek_to_block(2).unwrap();
        let block = reader.next_block().unwrap().unwrap();
        assert_eq!(
            block[0],
            vec![2000, 2001, 2002, 2003, 2004, 2005, 2006, 2007]
                .into_iter()
                .map(|v: u32| (v % 256) as u8)
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn trims_final_partial_block() {
        // sample_count not a multiple of block_size * 8: last block is short.
        let data = build_dsf(1, 2_822_400, 8, 8 * 8 + 3 * 8);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        assert_eq!(info.total_blocks(), 2);
        let mut reader = DsfBlockReader::new(cursor, info);
        let _ = reader.next_block().unwrap().unwrap();
        let last = reader.next_block().unwrap().unwrap();
        assert_eq!(last[0].len(), 3);
    }
}
