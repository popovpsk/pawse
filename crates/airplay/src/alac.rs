struct BitWriter {
    bytes: Vec<u8>,
    bits: u32,
    filled: u8,
}

impl BitWriter {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
            bits: 0,
            filled: 0,
        }
    }

    fn put(&mut self, value: u32, count: u8) {
        for shift in (0..count).rev() {
            self.bits = (self.bits << 1) | ((value >> shift) & 1);
            self.filled += 1;
            if self.filled == 8 {
                self.bytes.push(self.bits as u8);
                self.bits = 0;
                self.filled = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.filled > 0 {
            self.bytes.push((self.bits << (8 - self.filled)) as u8);
        }
        self.bytes
    }
}

const ELEMENT_CPE: u32 = 1;
const ELEMENT_END: u32 = 7;

pub(crate) fn encode_uncompressed(stereo: &[i16]) -> Vec<u8> {
    let mut writer = BitWriter::with_capacity(stereo.len() * 2 + 8);
    writer.put(ELEMENT_CPE, 3);
    writer.put(0, 4);
    writer.put(0, 12);
    writer.put(0, 1);
    writer.put(0, 2);
    writer.put(1, 1);
    for sample in stereo {
        writer.put(u32::from(*sample as u16), 16);
    }
    writer.put(ELEMENT_END, 3);
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_marks_an_uncompressed_stereo_frame() {
        let frame = encode_uncompressed(&[0x1234, -2]);
        assert_eq!(frame.len(), 3 + 4 + 1);
        assert_eq!(frame[0], 0b0010_0000);
        assert_eq!(frame[1], 0);
        assert_eq!(frame[2] & 0b1111_1110, 0b0000_0010);
    }

    #[test]
    fn samples_follow_the_header_big_endian_and_bit_shifted() {
        let samples = [0x1234i16, -2, 0x7fff, i16::MIN];
        let frame = encode_uncompressed(&samples);
        let mut bits: Vec<u8> = Vec::new();
        for byte in &frame {
            for shift in (0..8).rev() {
                bits.push((byte >> shift) & 1);
            }
        }
        let read = |from: usize, count: usize| -> u32 {
            bits[from..from + count]
                .iter()
                .fold(0u32, |acc, bit| (acc << 1) | u32::from(*bit))
        };
        assert_eq!(read(22, 1), 1);
        for (i, sample) in samples.iter().enumerate() {
            assert_eq!(read(23 + i * 16, 16), u32::from(*sample as u16));
        }
        assert_eq!(read(23 + samples.len() * 16, 3), ELEMENT_END);
    }
}
