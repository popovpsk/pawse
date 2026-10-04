use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use hkdf::Hkdf;
use sha2::Sha512;

pub(crate) const TAG_LEN: usize = 16;
pub(crate) const MAX_FRAME: usize = 1024;
const AUDIO_NONCE_LEN: usize = 8;

pub(crate) fn derive(secret: &[u8], salt: &str, info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), secret)
        .expand(info.as_bytes(), &mut out)
        .expect("32 bytes is within what HKDF-SHA512 can expand to");
    out
}

fn counter_nonce(counter: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_le_bytes());
    nonce
}

pub(crate) struct FrameCipher {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl FrameCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
            counter: 0,
        }
    }

    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(plain.len() + plain.len().div_ceil(MAX_FRAME) * 18);
        for chunk in plain.chunks(MAX_FRAME) {
            let length = (chunk.len() as u16).to_le_bytes();
            let mut sealed = chunk.to_vec();
            let tag = self
                .cipher
                .encrypt_in_place_detached(
                    Nonce::from_slice(&counter_nonce(self.counter)),
                    &length,
                    &mut sealed,
                )
                .expect("a frame of at most 1024 bytes always encrypts");
            self.counter += 1;
            out.extend_from_slice(&length);
            out.extend_from_slice(&sealed);
            out.extend_from_slice(&tag);
        }
        out
    }

    pub fn open(&mut self, length: [u8; 2], sealed: &[u8]) -> Option<Vec<u8>> {
        let split = sealed.len().checked_sub(TAG_LEN)?;
        let (body, tag) = sealed.split_at(split);
        let mut plain = body.to_vec();
        self.cipher
            .decrypt_in_place_detached(
                Nonce::from_slice(&counter_nonce(self.counter)),
                &length,
                &mut plain,
                Tag::from_slice(tag),
            )
            .ok()?;
        self.counter += 1;
        Some(plain)
    }
}

pub(crate) struct AudioCipher(ChaCha20Poly1305);

impl AudioCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self(ChaCha20Poly1305::new(Key::from_slice(key)))
    }

    pub fn seal_packet(&self, header: &[u8; 12], seq: u16, payload: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; 12];
        nonce[4..6].copy_from_slice(&seq.to_le_bytes());
        let mut packet = Vec::with_capacity(12 + payload.len() + TAG_LEN + AUDIO_NONCE_LEN);
        packet.extend_from_slice(header);
        packet.extend_from_slice(payload);
        let tag = self
            .0
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &header[4..], &mut packet[12..])
            .expect("an audio packet always encrypts");
        packet.extend_from_slice(&tag);
        packet.extend_from_slice(&nonce[4..]);
        packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    const SESSION_KEY: &str = "4ab230ee1885c136bcc65025ebbb7e36747af83328b00d3a37a95142b5a8d469750833aa1ad0156523552376a6d8937287c38211414ac2036f425b4ff54fe37d";
    const WRITE_KEY: &str = "e662327a262ab3c7dde0eeae2a42d99c12270b120eba73725516bb3b6ee3533c";
    const READ_KEY: &str = "ed42211a371acdfd0c0e4525a71efaa7d7ccc1e2339b89d8143d09a9a92bb8b4";
    const FIRST_FRAME: &str = "1f00188f8a29295e6be4973b195383c7d8806a5d140b063172a2207faeacdab872bf0aab0962802d6f5480b09b7df231d4";
    const SECOND_FRAME: &str = "1f00a3fef01891f69edfa20e7f1de423f81791bd21f63da6eb2108e0b5a34314f317ff96d79200e60df6de3b10e7e16b7c";
    const AUDIO_PACKET: &str = "8060123401020304050607086662b4b081ab5933d5109f3f0d889b692e290464e1158101911d0f496c36621cff4eb5bf8c5808349630690c651d6d0e05a013f2181f795e3412000000000000";
    const REQUEST: &[u8] = b"GET /info RTSP/1.0\r\nCSeq: 1\r\n\r\n";

    fn control_key(info: &str) -> [u8; 32] {
        derive(&unhex(SESSION_KEY), "Control-Salt", info)
    }

    #[test]
    fn control_keys_and_frames_match_the_python_cryptography_package() {
        let write = control_key("Control-Write-Encryption-Key");
        assert_eq!(write.to_vec(), unhex(WRITE_KEY));
        assert_eq!(
            control_key("Control-Read-Encryption-Key").to_vec(),
            unhex(READ_KEY)
        );
        let mut cipher = FrameCipher::new(&write);
        assert_eq!(cipher.seal(REQUEST), unhex(FIRST_FRAME));
        assert_eq!(cipher.seal(REQUEST), unhex(SECOND_FRAME));
    }

    #[test]
    fn frames_open_in_order_and_a_tampered_frame_does_not() {
        let key = [9u8; 32];
        let plain: Vec<u8> = (0..2500).map(|i| i as u8).collect();
        let sealed = FrameCipher::new(&key).seal(&plain);
        assert_eq!(sealed.len(), plain.len() + 3 * (2 + TAG_LEN));
        let mut reader = FrameCipher::new(&key);
        let mut opened = Vec::new();
        let mut rest = sealed.as_slice();
        while !rest.is_empty() {
            let length = [rest[0], rest[1]];
            let size = usize::from(u16::from_le_bytes(length)) + TAG_LEN;
            opened.extend(reader.open(length, &rest[2..2 + size]).unwrap());
            rest = &rest[2 + size..];
        }
        assert_eq!(opened, plain);
        let mut tampered = FrameCipher::new(&key).seal(b"hello");
        tampered[3] ^= 1;
        let length = [tampered[0], tampered[1]];
        assert!(
            FrameCipher::new(&key)
                .open(length, &tampered[2..])
                .is_none()
        );
    }

    #[test]
    fn an_audio_packet_matches_the_python_cryptography_package() {
        let key: [u8; 32] = unhex(SESSION_KEY)[..32].try_into().unwrap();
        let header = [0x80, 0x60, 0x12, 0x34, 1, 2, 3, 4, 5, 6, 7, 8];
        let payload: Vec<u8> = (0..40).collect();
        let packet = AudioCipher::new(&key).seal_packet(&header, 0x1234, &payload);
        assert_eq!(packet, unhex(AUDIO_PACKET));
    }
}
