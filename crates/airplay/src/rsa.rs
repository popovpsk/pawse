use num_bigint::BigUint;
use sha1::{Digest, Sha1};

use crate::random;

const AIRPORT_MODULUS: &str = concat!(
    "E7D744F2A2E2788B6C1F55A08EB70544A8FA7945AA8BE6C62CE5F51CBDD4DC68",
    "42FE3D1083DD2EDEC1BFD4252DC02E6F398BDF0E6148EA84855E2E442DA6D626",
    "64F674A1F304929ADE4F6893EF2DF6E711A8C77A0D91C9D980822E50D12922AF",
    "EA40EA9F0E14C0F76938C5F3882FC0323DD9FE55155F51BB5921C201629FD733",
    "52D5E2EFAABF9BA048D7B813A2B6767F6C3CCF1EB4CE673D037B0D2EA30C5FFF",
    "EB06F8D08ADDE409571A9C689FEF10728855DD8CFB9A8BEF5C8943EF3B5FAA15",
    "DDE698BEDDF3599603EB3E6F61372BB628F6559F599A78BF500687AA7F4976C0",
    "562D412956F8989E18A6355BD81597825E0FC875343EC782117625CDBF98447B",
);
const EXPONENT: u32 = 65_537;
const HASH_LEN: usize = 20;

pub(crate) fn encrypt_for_airport(key: &[u8; 16]) -> Vec<u8> {
    let modulus = BigUint::parse_bytes(AIRPORT_MODULUS.as_bytes(), 16)
        .expect("the AirPort Express modulus is valid hex");
    oaep_encrypt(&modulus, key, random::<HASH_LEN>())
}

fn oaep_encrypt(modulus: &BigUint, message: &[u8], seed: [u8; HASH_LEN]) -> Vec<u8> {
    let size = modulus.bits().div_ceil(8) as usize;
    let mut block = vec![0u8; size - HASH_LEN - 1];
    block[..HASH_LEN].copy_from_slice(&Sha1::digest(b""));
    let start = block.len() - message.len();
    block[start - 1] = 1;
    block[start..].copy_from_slice(message);
    mask(&mut block, &seed);
    let mut masked_seed = seed;
    mask(&mut masked_seed, &block);
    let encoded = [&[0][..], &masked_seed, &block].concat();
    let sealed = BigUint::from_bytes_be(&encoded).modpow(&BigUint::from(EXPONENT), modulus);
    padded(&sealed, size)
}

fn mask(target: &mut [u8], seed: &[u8]) {
    for (counter, chunk) in target.chunks_mut(HASH_LEN).enumerate() {
        let digest = Sha1::new()
            .chain_update(seed)
            .chain_update((counter as u32).to_be_bytes())
            .finalize();
        for (byte, mask) in chunk.iter_mut().zip(digest) {
            *byte ^= mask;
        }
    }
}

fn padded(number: &BigUint, size: usize) -> Vec<u8> {
    let bytes = number.to_bytes_be();
    let mut out = vec![0; size.saturating_sub(bytes.len())];
    out.extend_from_slice(&bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MODULUS: &str = concat!(
        "C393F598CD955CFFF88B427B815B73DA65152BE0A93E880CAA004518B6DE98B4",
        "5A1B121FE7215F2A557A58DFC1C92357B72714AD339AF8BF1B693CA5051D666C",
        "8AA97506AE51EA483AC0CE7997C34EC3FB19B7698DF02B2E856BFB4EC3F5142E",
        "11958A6BE48F5B19E9CFF7DB4B0A40939B17B92470419F60EFE5CEA11F09E85D",
    );
    const TEST_PRIVATE_EXPONENT: &str = concat!(
        "321045E5FF7A247C419B2441893367CC97E37B7FDED2EAF092F3CEBC2CD37E33",
        "6C7D517E6923D9C07471051F01F402E0093A2FC8C654FD27AE5499F983FDAAA1",
        "DB15DB2949792E9F443C37F0F71B5BD2D1A6E55F6BCDA9663C7EDC5CEB06797F",
        "69531562E2FBC6475AB196CEF936305CCF4F86A3E004878D646855438CA035E1",
    );
    const KEY: &[u8; 16] = b"0123456789abcdef";
    const SEED: [u8; HASH_LEN] = [7; HASH_LEN];
    const SEALED_WITH_SEED: &str = concat!(
        "9b78d9be19264fbc05301d1f89c547ce2f1c683a6daf68fed6363520f483af9a",
        "f6a7dc8688fb3b13c195a326b71480aef434928c9ac48b6fa8e954b3214738bf",
        "8fe4c66260e8020b83dfda43d94a39ea20888aae7a1b72c665e3d5d0e351850d",
        "a8260a4863c9924fec30328d081220895f419e7215d9fdefb5924390ef7acca9",
    );

    fn number(hex: &str) -> BigUint {
        BigUint::parse_bytes(hex.as_bytes(), 16).unwrap()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn open(sealed: &[u8]) -> Vec<u8> {
        let modulus = number(TEST_MODULUS);
        let encoded =
            BigUint::from_bytes_be(sealed).modpow(&number(TEST_PRIVATE_EXPONENT), &modulus);
        let encoded = padded(&encoded, sealed.len());
        assert_eq!(encoded[0], 0);
        let (mut seed, mut block) = (
            encoded[1..=HASH_LEN].to_vec(),
            encoded[HASH_LEN + 1..].to_vec(),
        );
        mask(&mut seed, &block);
        mask(&mut block, &seed);
        assert_eq!(block[..HASH_LEN], Sha1::digest(b"")[..]);
        let start = HASH_LEN
            + block[HASH_LEN..]
                .iter()
                .position(|&byte| byte != 0)
                .unwrap();
        assert_eq!(block[start], 1);
        block[start + 1..].to_vec()
    }

    #[test]
    fn a_key_sealed_with_a_fixed_seed_is_what_openssl_opens() {
        let sealed = oaep_encrypt(&number(TEST_MODULUS), KEY, SEED);
        assert_eq!(hex(&sealed), SEALED_WITH_SEED);
    }

    #[test]
    fn a_key_sealed_with_a_random_seed_opens_to_the_key() {
        let sealed = oaep_encrypt(&number(TEST_MODULUS), KEY, random());
        assert_eq!(sealed.len(), 128);
        assert_eq!(open(&sealed), KEY);
    }

    #[test]
    fn a_key_for_the_airport_is_sealed_to_its_2048_bits() {
        assert_eq!(encrypt_for_airport(KEY).len(), 256);
        assert_ne!(encrypt_for_airport(KEY), encrypt_for_airport(KEY));
    }
}
