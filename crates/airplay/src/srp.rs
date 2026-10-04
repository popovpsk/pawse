use num_bigint::BigUint;
use sha2::{Digest, Sha512};

const PRIME_3072: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74",
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437",
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05",
    "98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB",
    "9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B",
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718",
    "3995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507A33",
    "A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7",
    "ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864",
    "D87602733EC86A64521F2B18177B200CBBE117577A615D6C770988C0BAD946E2",
    "08E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF",
);
const GENERATOR: u32 = 5;

const PRIME_BYTES: usize = 384;

pub(crate) struct Exchange {
    pub public: Vec<u8>,
    pub proof: [u8; 64],
    pub server_proof: [u8; 64],
    pub key: [u8; 64],
}

fn hash(parts: &[&[u8]]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn padded(number: &BigUint) -> Vec<u8> {
    let bytes = number.to_bytes_be();
    let mut out = vec![0; PRIME_BYTES.saturating_sub(bytes.len())];
    out.extend_from_slice(&bytes);
    out
}

pub(crate) fn exchange(
    user: &str,
    password: &str,
    salt: &[u8],
    server_public: &[u8],
    private: &[u8],
) -> Option<Exchange> {
    let prime = BigUint::parse_bytes(PRIME_3072.as_bytes(), 16)?;
    let generator = BigUint::from(GENERATOR);
    let server = BigUint::from_bytes_be(server_public);
    if (&server % &prime) == BigUint::ZERO {
        return None;
    }
    let secret = BigUint::from_bytes_be(private);
    if secret == BigUint::ZERO {
        return None;
    }
    let public = generator.modpow(&secret, &prime);
    let multiplier = BigUint::from_bytes_be(&hash(&[&padded(&prime), &padded(&generator)]));
    let scrambler = BigUint::from_bytes_be(&hash(&[&padded(&public), &padded(&server)]));
    if scrambler == BigUint::ZERO {
        return None;
    }
    let identity = hash(&[format!("{user}:{password}").as_bytes()]);
    let x = BigUint::from_bytes_be(&hash(&[salt, &identity]));
    let subtrahend = (&multiplier * generator.modpow(&x, &prime)) % &prime;
    let base = ((&server % &prime) + &prime - subtrahend) % &prime;
    let shared = base.modpow(&(&secret + &scrambler * &x), &prime);
    let key = hash(&[&shared.to_bytes_be()]);

    let public = public.to_bytes_be();
    let proof = client_proof(&prime, user, salt, &public, &server.to_bytes_be(), &key);
    let server_proof = hash(&[&public, &proof, &key]);
    Some(Exchange {
        public,
        proof,
        server_proof,
        key,
    })
}

fn client_proof(
    prime: &BigUint,
    user: &str,
    salt: &[u8],
    client_public: &[u8],
    server_public: &[u8],
    key: &[u8],
) -> [u8; 64] {
    let prime_hash = hash(&[&prime.to_bytes_be()]);
    let generator_hash = hash(&[&BigUint::from(GENERATOR).to_bytes_be()]);
    let mut mixed = [0u8; 64];
    for (slot, (a, b)) in mixed
        .iter_mut()
        .zip(prime_hash.iter().zip(generator_hash.iter()))
    {
        *slot = a ^ b;
    }
    hash(&[
        &mixed,
        &hash(&[user.as_bytes()]),
        salt,
        client_public,
        server_public,
        key,
    ])
}

#[cfg(test)]
pub(crate) struct Server {
    prime: BigUint,
    verifier: BigUint,
    secret: BigUint,
    public: BigUint,
    salt: Vec<u8>,
}

#[cfg(test)]
impl Server {
    pub fn new(user: &str, password: &str, salt: &[u8], secret: &[u8]) -> Self {
        let prime = BigUint::parse_bytes(PRIME_3072.as_bytes(), 16).unwrap();
        let generator = BigUint::from(GENERATOR);
        let identity = hash(&[format!("{user}:{password}").as_bytes()]);
        let x = BigUint::from_bytes_be(&hash(&[salt, &identity]));
        let verifier = generator.modpow(&x, &prime);
        let multiplier = BigUint::from_bytes_be(&hash(&[&padded(&prime), &padded(&generator)]));
        let secret = BigUint::from_bytes_be(secret);
        let public = (&multiplier * &verifier + generator.modpow(&secret, &prime)) % &prime;
        Self {
            prime,
            verifier,
            secret,
            public,
            salt: salt.to_vec(),
        }
    }

    pub fn public(&self) -> Vec<u8> {
        self.public.to_bytes_be()
    }

    pub fn verify(
        &self,
        user: &str,
        client_public: &[u8],
        proof: &[u8],
    ) -> Option<([u8; 64], [u8; 64])> {
        let client = BigUint::from_bytes_be(client_public);
        let scrambler = BigUint::from_bytes_be(&hash(&[&padded(&client), &padded(&self.public)]));
        let shared = (client * self.verifier.modpow(&scrambler, &self.prime))
            .modpow(&self.secret, &self.prime);
        let key = hash(&[&shared.to_bytes_be()]);
        let expected = client_proof(
            &self.prime,
            user,
            &self.salt,
            client_public,
            &self.public(),
            &key,
        );
        (expected.as_slice() == proof).then(|| (key, hash(&[client_public, &expected, &key])))
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

    const SALT: &str = "c8c9cacbcccdcecfd0d1d2d3d4d5d6d7";
    const SERVER_PUBLIC: &str = concat!(
        "7e9f051b3daa870b2f67842b43aef5a0128148d43c08a528d51e5f4fdf50d788",
        "850d11c7e50dcf36a099b6e3dc1b92e9eac0ccc3a98c615ef5c33e90d6491292",
        "fa2710c5fe0bc2f4b09f87fef2af5b17a4e00f8a4df36db78c8b83dabdc07cba",
        "ca6eac469bf995ea39200c9e0027f03ba0d52dd95e1c8e67e7ed4632e79160eb",
        "4399298b0ed4d7c584fafff3f2d17c543f078f8a6722e140cc6aeeb5cdcf2f83",
        "0fc5e13d75e7c21bbc017b5afa4f3079e37c8bfa4482d9d0726c19cf513912ba",
        "8d2d540fea4cecfa3b367afda16cbfa636681bbdd8af88a0892441c31ad267a7",
        "803b040f989521be1f821f34ffa4bdb4b3ed84b77b24c08a190e4e866f548887",
        "f0e605039a064424eb55e894fbdbfdc8af56f0c91727782116c4d3135878eaa2",
        "cec843bdddfb7798d5d33d959574d24e545d2aa1092deb686d7c7128a7c00c88",
        "46fe4de66fb6f5afdfd060c0ad62b67de08bf7809cacf02a835287a556214c8c",
        "ee44a3b506489d567ff9d7eccaab7f5e1c3161ed74c2883582d7c3111005e9a4",
    );
    const CLIENT_PUBLIC: &str = concat!(
        "bc0e7cf5dc3babf67dcedbb3b140aacc6cac43f4336b43bbd5de48d6ea7c8eda",
        "66924e354255225bccad9debe21182e6bb050f3ff3e6cfbb62c229379968c70c",
        "a436ad649a0b051373184215eef046f6f1f2256838f958581f6c7b2b85fa4afe",
        "326a0e8a951d4489305331aff88a136fd8d108bcc95fceb7e557c889c828bd23",
        "fb0702f053e1ca6470fb3c76bce4843fc005c7ea675740f8550212656cfc8919",
        "d9db805a434a68229e0d9dfe43fc16dc680a5ce74b77cf374353b05759bc1da3",
        "a9dabde30a4209381c87ca83d9483abdf66b86f9b1cbda9ad82c62712b87ce6f",
        "b7069b8fc8df344261821a06d0dc5106af76d4245f3f7737a94dbc484b415555",
        "dc401842d3011204553ba9f611b02bc38de26eba1a76bf8350205a62c436ba1c",
        "3c7c69d59318bd107fd1c1f5d846b3142e85a5d49e522655e020ed1bfe1e186c",
        "f923bf328f0b9b4c6a8aa3266ed9125bb98d63827110713be7803122ee4603c5",
        "4ea31863ce4b10aff31f9073cf63b94733b4f066e72d4ec35687047d5d0db160",
    );
    const PROOF: &str = concat!(
        "52bfd2dae0044b62639ebb625e1dc71bdc5f756d88d80d7794c4aa6316fea57a",
        "0199c2758d0634fd1bc31d285d210c058d4fe31a92badfc8e0e064212354da64",
    );
    const KEY: &str = concat!(
        "4ab230ee1885c136bcc65025ebbb7e36747af83328b00d3a37a95142b5a8d469",
        "750833aa1ad0156523552376a6d8937287c38211414ac2036f425b4ff54fe37d",
    );
    const SERVER_PROOF: &str = concat!(
        "8da886685d4c62ba329548dbc0d52536303872cee367a849e1d2d8299acf1dd6",
        "879d574aff6bb51245e9e9404da970427092c4cb3439a13c559d345fba1c4034",
    );

    #[test]
    fn the_exchange_matches_srptools_as_used_by_pyatv() {
        let private: Vec<u8> = (1..=32).collect();
        let done = exchange(
            "Pair-Setup",
            "3939",
            &unhex(SALT),
            &unhex(SERVER_PUBLIC),
            &private,
        )
        .unwrap();
        assert_eq!(done.public, unhex(CLIENT_PUBLIC));
        assert_eq!(done.key.to_vec(), unhex(KEY));
        assert_eq!(done.proof.to_vec(), unhex(PROOF));
        assert_eq!(done.server_proof.to_vec(), unhex(SERVER_PROOF));
    }

    #[test]
    fn a_server_key_that_is_a_multiple_of_the_prime_is_refused() {
        let prime = BigUint::parse_bytes(PRIME_3072.as_bytes(), 16).unwrap();
        assert!(
            exchange(
                "Pair-Setup",
                "3939",
                &[1; 16],
                &prime.to_bytes_be(),
                &[1; 32]
            )
            .is_none()
        );
        assert!(exchange("Pair-Setup", "3939", &[1; 16], &[0], &[1; 32]).is_none());
    }
}
