use crate::rtsp::Rtsp;
use crate::secure::derive;
use crate::{Error, random, srp, tlv};

const USER: &str = "Pair-Setup";
const TRANSIENT_PIN: &str = "3939";
const TRANSIENT: u8 = 0x10;

pub(crate) struct Keys {
    pub write: [u8; 32],
    pub read: [u8; 32],
    pub events_write: [u8; 32],
    pub events_read: [u8; 32],
    pub audio: [u8; 32],
}

fn post(rtsp: &mut Rtsp, body: &[u8], step: &str) -> Result<tlv::Items, Error> {
    let response = rtsp
        .request(
            "POST",
            Some("/pair-setup"),
            &[("X-Apple-HKP", "4".to_string())],
            Some(("application/octet-stream", body)),
        )
        .map_err(|e| match e {
            Error::Refused(message) if message.ends_with(" 470") => {
                Error::Refused("the device only pairs with a code shown on it".into())
            }
            other => other,
        })?;
    let items = tlv::decode(&response.body)
        .ok_or_else(|| Error::Refused(format!("pairing {step}: a malformed answer")))?;
    if let Some(code) = items.get(tlv::ERROR) {
        return Err(Error::Refused(format!(
            "pairing {step} was refused (error {})",
            code.first().copied().unwrap_or_default()
        )));
    }
    Ok(items)
}

fn expect_state(items: &tlv::Items, state: u8, step: &str) -> Result<(), Error> {
    match items.get(tlv::STATE) {
        Some([found]) if *found == state => Ok(()),
        _ => Err(Error::Refused(format!(
            "pairing {step}: an unexpected answer"
        ))),
    }
}

pub(crate) fn transient(rtsp: &mut Rtsp) -> Result<Keys, Error> {
    let start = tlv::encode(&[
        (tlv::METHOD, &[0]),
        (tlv::STATE, &[1]),
        (tlv::FLAGS, &[TRANSIENT]),
    ]);
    let challenge = post(rtsp, &start, "M1")?;
    expect_state(&challenge, 2, "M2")?;
    let invalid = || Error::Refused("pairing M2: no salt or key".into());
    let salt = challenge.get(tlv::SALT).ok_or_else(invalid)?;
    let server_public = challenge.get(tlv::PUBLIC_KEY).ok_or_else(invalid)?;
    let exchange = srp::exchange(USER, TRANSIENT_PIN, salt, server_public, &random::<32>())
        .ok_or_else(|| Error::Refused("pairing M2: the device's key is invalid".into()))?;
    let proof = tlv::encode(&[
        (tlv::STATE, &[3]),
        (tlv::PUBLIC_KEY, &exchange.public),
        (tlv::PROOF, &exchange.proof),
    ]);
    let verified = post(rtsp, &proof, "M3")?;
    expect_state(&verified, 4, "M4")?;
    if verified.get(tlv::PROOF) != Some(&exchange.server_proof[..]) {
        return Err(Error::Refused(
            "pairing M4: the device did not prove the same key".into(),
        ));
    }
    let mut audio = [0u8; 32];
    audio.copy_from_slice(&exchange.key[..32]);
    Ok(Keys {
        write: derive(
            &exchange.key,
            "Control-Salt",
            "Control-Write-Encryption-Key",
        ),
        read: derive(&exchange.key, "Control-Salt", "Control-Read-Encryption-Key"),
        events_write: derive(&exchange.key, "Events-Salt", "Events-Read-Encryption-Key"),
        events_read: derive(&exchange.key, "Events-Salt", "Events-Write-Encryption-Key"),
        audio,
    })
}
