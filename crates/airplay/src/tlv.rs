pub(crate) const METHOD: u8 = 0x00;
pub(crate) const SALT: u8 = 0x02;
pub(crate) const PUBLIC_KEY: u8 = 0x03;
pub(crate) const PROOF: u8 = 0x04;
pub(crate) const STATE: u8 = 0x06;
pub(crate) const ERROR: u8 = 0x07;
pub(crate) const FLAGS: u8 = 0x13;

const FRAGMENT: usize = 255;

pub(crate) fn encode(items: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (tag, value) in items {
        if value.is_empty() {
            out.extend_from_slice(&[*tag, 0]);
        }
        for chunk in value.chunks(FRAGMENT) {
            out.push(*tag);
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
    }
    out
}

pub(crate) struct Items(Vec<(u8, Vec<u8>)>);

impl Items {
    pub fn get(&self, tag: u8) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(found, _)| *found == tag)
            .map(|(_, value)| value.as_slice())
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Option<Items> {
    let mut items: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut continues = false;
    let mut rest = bytes;
    while !rest.is_empty() {
        let [tag, len, tail @ ..] = rest else {
            return None;
        };
        let len = usize::from(*len);
        let value = tail.get(..len)?;
        match items.last_mut() {
            Some((last, merged)) if continues && last == tag => merged.extend_from_slice(value),
            _ => items.push((*tag, value.to_vec())),
        }
        continues = len == FRAGMENT;
        rest = &tail[len..];
    }
    Some(Items(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_values_are_split_into_255_byte_fragments_and_joined_back() {
        let key: Vec<u8> = (0..384).map(|i| i as u8).collect();
        let bytes = encode(&[(STATE, &[3]), (PUBLIC_KEY, &key), (PROOF, &[7; 64])]);
        assert_eq!(&bytes[..3], &[STATE, 1, 3]);
        assert_eq!(&bytes[3..5], &[PUBLIC_KEY, 255]);
        assert_eq!(&bytes[260..262], &[PUBLIC_KEY, 129]);
        let items = decode(&bytes).unwrap();
        assert_eq!(items.get(STATE), Some(&[3u8][..]));
        assert_eq!(items.get(PUBLIC_KEY), Some(key.as_slice()));
        assert_eq!(items.get(PROOF), Some(&[7u8; 64][..]));
        assert_eq!(items.get(SALT), None);
    }

    #[test]
    fn a_value_of_exactly_255_bytes_does_not_swallow_the_next_item_of_another_tag() {
        let bytes = encode(&[(SALT, &[1; 255]), (PUBLIC_KEY, &[2; 3])]);
        let items = decode(&bytes).unwrap();
        assert_eq!(items.get(SALT).map(<[u8]>::len), Some(255));
        assert_eq!(items.get(PUBLIC_KEY), Some(&[2u8; 3][..]));
    }

    #[test]
    fn truncated_input_is_rejected() {
        assert!(decode(&[STATE, 2, 1]).is_none());
        assert!(decode(&[STATE]).is_none());
        assert!(decode(&[]).is_some());
    }
}
