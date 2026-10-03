#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CastMessage {
    pub source: String,
    pub destination: String,
    pub namespace: String,
    pub payload: String,
}

const SOURCE: u8 = 2;
const DESTINATION: u8 = 3;
const NAMESPACE: u8 = 4;
const PAYLOAD_UTF8: u8 = 6;
const VARINT: u8 = 0;
const FIXED64: u8 = 1;
const BYTES: u8 = 2;
const FIXED32: u8 = 5;
pub(crate) const MAX_FRAME: usize = 64 * 1024;

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn put_bytes(out: &mut Vec<u8>, field: u8, bytes: &[u8]) {
    put_varint(out, u64::from(field << 3 | BYTES));
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

impl CastMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(self.payload.len() + 96);
        put_varint(&mut body, u64::from(1 << 3 | VARINT));
        put_varint(&mut body, 0);
        put_bytes(&mut body, SOURCE, self.source.as_bytes());
        put_bytes(&mut body, DESTINATION, self.destination.as_bytes());
        put_bytes(&mut body, NAMESPACE, self.namespace.as_bytes());
        put_varint(&mut body, u64::from(5 << 3 | VARINT));
        put_varint(&mut body, 0);
        put_bytes(&mut body, PAYLOAD_UTF8, self.payload.as_bytes());
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    pub fn decode(body: &[u8]) -> Result<Self, String> {
        let mut reader = Reader { bytes: body, at: 0 };
        let mut message = CastMessage {
            source: String::new(),
            destination: String::new(),
            namespace: String::new(),
            payload: String::new(),
        };
        while !reader.done() {
            let key = reader.varint()?;
            let field = (key >> 3) as u8;
            match (key & 7) as u8 {
                VARINT => {
                    reader.varint()?;
                }
                FIXED64 => reader.skip(8)?,
                FIXED32 => reader.skip(4)?,
                BYTES => {
                    let bytes = reader.bytes()?;
                    let text = || String::from_utf8_lossy(bytes).into_owned();
                    match field {
                        SOURCE => message.source = text(),
                        DESTINATION => message.destination = text(),
                        NAMESPACE => message.namespace = text(),
                        PAYLOAD_UTF8 => message.payload = text(),
                        _ => {}
                    }
                }
                wire => return Err(format!("unsupported protobuf wire type {wire}")),
            }
        }
        Ok(message)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or("truncated varint")?;
            self.at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("varint too long".into())
    }

    fn skip(&mut self, len: usize) -> Result<(), String> {
        if self.bytes.len() - self.at < len {
            return Err("truncated field".into());
        }
        self.at += len;
        Ok(())
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let len = usize::try_from(self.varint()?).map_err(|_| "field too long")?;
        let start = self.at;
        self.skip(len)?;
        Ok(&self.bytes[start..start + len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_survives_a_round_trip() {
        let message = CastMessage {
            source: "sender-0".into(),
            destination: "receiver-0".into(),
            namespace: "urn:x-cast:com.google.cast.tp.connection".into(),
            payload: "{\"type\":\"CONNECT\"}".repeat(20),
        };
        let frame = message.encode();
        let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(len, frame.len() - 4);
        assert_eq!(CastMessage::decode(&frame[4..]).unwrap(), message);
    }

    #[test]
    fn the_encoding_matches_the_reference_layout() {
        let frame = CastMessage {
            source: "s".into(),
            destination: "d".into(),
            namespace: "n".into(),
            payload: "p".into(),
        }
        .encode();
        assert_eq!(
            &frame[4..],
            &[
                0x08, 0x00, 0x12, 1, b's', 0x1a, 1, b'd', 0x22, 1, b'n', 0x28, 0x00, 0x32, 1, b'p'
            ]
        );
    }

    #[test]
    fn unknown_fields_and_binary_payloads_are_skipped() {
        let mut body = vec![0x08, 0x00];
        put_bytes(&mut body, 7, &[1, 2, 3]);
        body.extend_from_slice(&[0x45, 0, 0, 0, 0]);
        put_bytes(&mut body, NAMESPACE, b"ns");
        let message = CastMessage::decode(&body).unwrap();
        assert_eq!(message.namespace, "ns");
        assert!(message.payload.is_empty());
        assert!(CastMessage::decode(&[0x22, 5, b'a']).is_err());
    }
}
