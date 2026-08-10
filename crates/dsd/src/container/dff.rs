use std::io::{Read, Seek, SeekFrom};

use crate::error::DsdError;

#[derive(Debug, Clone, Copy)]
pub struct DffInfo {
    pub channels: u8,
    pub dsd_rate: u32,
    pub data_offset: u64,
    pub data_len: u64,
}

struct ChunkHeader {
    id: [u8; 4],
    size: u64,
}

fn read_chunk_header(r: &mut impl Read) -> Result<ChunkHeader, DsdError> {
    let mut id = [0u8; 4];
    r.read_exact(&mut id)?;
    let mut size_buf = [0u8; 8];
    r.read_exact(&mut size_buf)?;
    Ok(ChunkHeader {
        id,
        size: u64::from_be_bytes(size_buf),
    })
}

fn skip<R: Read + Seek>(r: &mut R, len: u64) -> Result<(), DsdError> {
    r.seek(SeekFrom::Current(len as i64))?;
    Ok(())
}

fn parse_prop<R: Read + Seek>(
    reader: &mut R,
    prop_size: u64,
) -> Result<(Option<u32>, Option<u8>), DsdError> {
    let mut form_type = [0u8; 4];
    reader.read_exact(&mut form_type)?;
    if &form_type != b"SND " {
        skip(reader, prop_size.saturating_sub(4))?;
        return Ok((None, None));
    }

    let mut remaining = prop_size.saturating_sub(4);
    let mut dsd_rate = None;
    let mut channels = None;

    while remaining >= 12 {
        let chunk = read_chunk_header(reader)?;
        remaining -= 12;
        match &chunk.id {
            b"FS  " => {
                let mut b = [0u8; 4];
                reader.read_exact(&mut b)?;
                dsd_rate = Some(u32::from_be_bytes(b));
                if chunk.size > 4 {
                    skip(reader, chunk.size - 4)?;
                }
            }
            b"CHNL" => {
                let mut b = [0u8; 2];
                reader.read_exact(&mut b)?;
                channels = Some(u16::from_be_bytes(b) as u8);
                if chunk.size > 2 {
                    skip(reader, chunk.size - 2)?;
                }
            }
            b"CMPR" => {
                let mut id = [0u8; 4];
                reader.read_exact(&mut id)?;
                if &id == b"DST " {
                    return Err(DsdError::UnsupportedCompression);
                }
                if chunk.size > 4 {
                    skip(reader, chunk.size - 4)?;
                }
            }
            _ => skip(reader, chunk.size)?,
        }
        remaining = remaining.saturating_sub(chunk.size);
    }

    Ok((dsd_rate, channels))
}

pub fn parse_header<R: Read + Seek>(reader: &mut R) -> Result<DffInfo, DsdError> {
    reader.seek(SeekFrom::Start(0))?;

    let frm8 = read_chunk_header(reader)?;
    if &frm8.id != b"FRM8" {
        return Err(DsdError::NotDsd);
    }
    let mut form_type = [0u8; 4];
    reader.read_exact(&mut form_type)?;
    if &form_type != b"DSD " {
        return Err(DsdError::NotDsd);
    }

    let mut remaining = frm8.size.saturating_sub(4);
    let mut dsd_rate: Option<u32> = None;
    let mut channels: Option<u8> = None;
    let mut data_offset: Option<u64> = None;
    let mut data_len: Option<u64> = None;

    while remaining >= 12 {
        let chunk = read_chunk_header(reader)?;
        remaining -= 12;
        match &chunk.id {
            b"PROP" => {
                let (rate, ch) = parse_prop(reader, chunk.size)?;
                dsd_rate = dsd_rate.or(rate);
                channels = channels.or(ch);
            }
            b"DSD " => {
                data_offset = Some(reader.stream_position()?);
                data_len = Some(chunk.size);
                skip(reader, chunk.size)?;
            }
            b"DST " => return Err(DsdError::UnsupportedCompression),
            _ => skip(reader, chunk.size)?,
        }
        remaining = remaining.saturating_sub(chunk.size);
    }

    let channels = channels.ok_or(DsdError::MalformedChunk { chunk: "PROP" })?;
    let dsd_rate = dsd_rate.ok_or(DsdError::MalformedChunk { chunk: "PROP" })?;
    let data_offset = data_offset.ok_or(DsdError::MalformedChunk { chunk: "DSD " })?;
    let data_len = data_len.ok_or(DsdError::MalformedChunk { chunk: "DSD " })?;

    if channels == 0 || channels > 32 {
        return Err(DsdError::UnsupportedChannels(channels as u32));
    }
    if dsd_rate == 0 {
        return Err(DsdError::MalformedChunk { chunk: "PROP" });
    }

    Ok(DffInfo {
        channels,
        dsd_rate,
        data_offset,
        data_len,
    })
}

/// DFF stores DSD data sample-interleaved (one byte per channel, cycling),
/// unlike DSF's block-interleaved layout. Reads `frame_bytes` interleaved
/// bytes (i.e. `frame_bytes / channels` bytes per channel) and de-interleaves
/// into one contiguous byte vector per channel.
pub fn read_interleaved<R: Read>(
    reader: &mut R,
    channels: u8,
    bytes_per_channel: usize,
) -> Result<Vec<Vec<u8>>, DsdError> {
    let channels = channels as usize;
    let mut raw = vec![0u8; bytes_per_channel * channels];
    let read = read_up_to(reader, &mut raw)?;
    raw.truncate(read - (read % channels));

    let mut out = vec![Vec::with_capacity(raw.len() / channels.max(1)); channels];
    for (i, &b) in raw.iter().enumerate() {
        out[i % channels].push(b);
    }
    Ok(out)
}

fn read_up_to<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize, DsdError> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

pub struct DffBlockReader<R> {
    reader: R,
    channels: u8,
    data_offset: u64,
    bytes_per_channel_total: u64,
    block_bytes_per_channel: u64,
    current_block: u64,
}

impl<R: Read + Seek> DffBlockReader<R> {
    pub fn new(
        mut reader: R,
        info: DffInfo,
        block_bytes_per_channel: u64,
    ) -> Result<Self, DsdError> {
        reader.seek(SeekFrom::Start(info.data_offset))?;
        Ok(Self {
            reader,
            channels: info.channels,
            data_offset: info.data_offset,
            bytes_per_channel_total: info.data_len / info.channels as u64,
            block_bytes_per_channel,
            current_block: 0,
        })
    }

    pub fn bytes_per_channel_total(&self) -> u64 {
        self.bytes_per_channel_total
    }

    pub fn total_blocks(&self) -> u64 {
        self.bytes_per_channel_total
            .div_ceil(self.block_bytes_per_channel)
    }

    pub fn seek_to_block(&mut self, block_index: u64) -> Result<(), DsdError> {
        let byte_offset =
            self.data_offset + block_index * self.block_bytes_per_channel * self.channels as u64;
        self.reader.seek(SeekFrom::Start(byte_offset))?;
        self.current_block = block_index;
        Ok(())
    }

    pub fn next_block(&mut self) -> Result<Option<Vec<Vec<u8>>>, DsdError> {
        if self.current_block >= self.total_blocks() {
            return Ok(None);
        }
        let block_start = self.current_block * self.block_bytes_per_channel;
        let remaining = self.bytes_per_channel_total.saturating_sub(block_start);
        let this_len = remaining.min(self.block_bytes_per_channel) as usize;

        let channels = read_interleaved(&mut self.reader, self.channels, this_len)?;
        self.current_block += 1;
        Ok(Some(channels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn be_chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(id);
        v.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn build_dff(channels: u16, dsd_rate: u32, samples_per_channel: usize) -> Vec<u8> {
        let fs_chunk = be_chunk(b"FS  ", &dsd_rate.to_be_bytes());
        let mut chnl_payload = Vec::new();
        chnl_payload.extend_from_slice(&channels.to_be_bytes());
        for i in 0..channels {
            chnl_payload.extend_from_slice(if i == 0 { b"SLFT" } else { b"SRGT" });
        }
        let chnl_chunk = be_chunk(b"CHNL", &chnl_payload);
        let mut cmpr_payload = Vec::new();
        cmpr_payload.extend_from_slice(b"DSD ");
        cmpr_payload.push(11);
        cmpr_payload.extend_from_slice(b"not compr.");
        let cmpr_chunk = be_chunk(b"CMPR", &cmpr_payload);

        let mut snd_payload = Vec::new();
        snd_payload.extend_from_slice(b"SND ");
        snd_payload.extend_from_slice(&fs_chunk);
        snd_payload.extend_from_slice(&chnl_chunk);
        snd_payload.extend_from_slice(&cmpr_chunk);
        let prop_chunk = be_chunk(b"PROP", &snd_payload);

        let bytes_per_channel = samples_per_channel / 8;
        let mut dsd_payload = Vec::new();
        for i in 0..(bytes_per_channel * channels as usize) {
            dsd_payload.push((i % 256) as u8);
        }
        let dsd_chunk = be_chunk(b"DSD ", &dsd_payload);

        let mut form_payload = Vec::new();
        form_payload.extend_from_slice(b"DSD ");
        form_payload.extend_from_slice(&prop_chunk);
        form_payload.extend_from_slice(&dsd_chunk);

        let mut buf = Vec::new();
        buf.extend_from_slice(b"FRM8");
        buf.extend_from_slice(&(form_payload.len() as u64).to_be_bytes());
        buf.extend_from_slice(&form_payload);
        buf
    }

    #[test]
    fn parses_header_fields() {
        let data = build_dff(2, 2_822_400, 8 * 64);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.dsd_rate, 2_822_400);
        assert_eq!(info.data_len, 64 * 2);
    }

    #[test]
    fn rejects_non_dff_magic() {
        let mut cursor = Cursor::new(vec![0u8; 64]);
        assert!(matches!(parse_header(&mut cursor), Err(DsdError::NotDsd)));
    }

    #[test]
    fn rejects_zero_sample_rate() {
        // Same hazard as DSF: a zero rate must not reach `DsdSource`, or
        // duration/seek later divide by it and panic on NaN/inf.
        let data = build_dff(2, 0, 8 * 64);
        let mut cursor = Cursor::new(data);
        assert!(matches!(
            parse_header(&mut cursor),
            Err(DsdError::MalformedChunk { chunk: "PROP" })
        ));
    }

    #[test]
    fn truncated_prop_chunk_does_not_panic() {
        // PROP declares a size too small to even hold the mandatory 4-byte
        // form type — must fail cleanly, not panic on `prop_size - 4`
        // underflow (the arithmetic is `saturating_sub` now).
        let prop_chunk = be_chunk(b"PROP", &[0u8; 2]);
        let mut form_payload = Vec::new();
        form_payload.extend_from_slice(b"DSD ");
        form_payload.extend_from_slice(&prop_chunk);
        form_payload.extend_from_slice(&[0u8; 16]);

        let mut buf = Vec::new();
        buf.extend_from_slice(b"FRM8");
        buf.extend_from_slice(&(form_payload.len() as u64).to_be_bytes());
        buf.extend_from_slice(&form_payload);

        let mut cursor = Cursor::new(buf);
        assert!(parse_header(&mut cursor).is_err());
    }

    #[test]
    fn block_reader_deinterleaves_across_blocks() {
        let data = build_dff(2, 2_822_400, 8 * 8);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        let mut reader = DffBlockReader::new(cursor, info, 4).unwrap();

        assert_eq!(reader.total_blocks(), 2);
        let block0 = reader.next_block().unwrap().unwrap();
        assert_eq!(block0[0], vec![0, 2, 4, 6]);
        assert_eq!(block0[1], vec![1, 3, 5, 7]);
        let block1 = reader.next_block().unwrap().unwrap();
        assert_eq!(block1[0], vec![8, 10, 12, 14]);
        assert_eq!(block1[1], vec![9, 11, 13, 15]);
        assert!(reader.next_block().unwrap().is_none());
    }

    #[test]
    fn block_reader_seek_to_block_repositions() {
        let data = build_dff(2, 2_822_400, 8 * 12);
        let mut cursor = Cursor::new(data);
        let info = parse_header(&mut cursor).unwrap();
        let mut reader = DffBlockReader::new(cursor, info, 4).unwrap();

        reader.seek_to_block(2).unwrap();
        let block = reader.next_block().unwrap().unwrap();
        assert_eq!(block[0], vec![16, 18, 20, 22]);
        assert_eq!(block[1], vec![17, 19, 21, 23]);
    }

    #[test]
    fn deinterleaves_sample_interleaved_data() {
        let data = build_dff(2, 2_822_400, 8 * 4);
        let mut cursor = Cursor::new(data.clone());
        let info = parse_header(&mut cursor).unwrap();
        cursor.seek(SeekFrom::Start(info.data_offset)).unwrap();

        let channels = read_interleaved(&mut cursor, info.channels, 4).unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0], vec![0, 2, 4, 6]);
        assert_eq!(channels[1], vec![1, 3, 5, 7]);
    }

    #[test]
    fn rejects_dst_compression() {
        let mut cmpr_payload = Vec::new();
        cmpr_payload.extend_from_slice(b"DST ");
        cmpr_payload.push(0);
        let cmpr_chunk = be_chunk(b"CMPR", &cmpr_payload);
        let fs_chunk = be_chunk(b"FS  ", &2_822_400u32.to_be_bytes());
        let mut chnl_payload = Vec::new();
        chnl_payload.extend_from_slice(&2u16.to_be_bytes());
        chnl_payload.extend_from_slice(b"SLFT");
        chnl_payload.extend_from_slice(b"SRGT");
        let chnl_chunk = be_chunk(b"CHNL", &chnl_payload);

        let mut snd_payload = Vec::new();
        snd_payload.extend_from_slice(b"SND ");
        snd_payload.extend_from_slice(&fs_chunk);
        snd_payload.extend_from_slice(&chnl_chunk);
        snd_payload.extend_from_slice(&cmpr_chunk);
        let prop_chunk = be_chunk(b"PROP", &snd_payload);

        let mut form_payload = Vec::new();
        form_payload.extend_from_slice(b"DSD ");
        form_payload.extend_from_slice(&prop_chunk);

        let mut buf = Vec::new();
        buf.extend_from_slice(b"FRM8");
        buf.extend_from_slice(&(form_payload.len() as u64).to_be_bytes());
        buf.extend_from_slice(&form_payload);

        let mut cursor = Cursor::new(buf);
        assert!(matches!(
            parse_header(&mut cursor),
            Err(DsdError::UnsupportedCompression)
        ));
    }
}
