use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NTP_EPOCH_OFFSET: u64 = 2_208_988_800;
const PAYLOAD_ALAC: u8 = 0x60;
const MARKER: u8 = 0x80;

pub(crate) fn ntp_now() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let seconds = since.as_secs() + NTP_EPOCH_OFFSET;
    let fraction = (u64::from(since.subsec_nanos()) << 32) / 1_000_000_000;
    (seconds << 32) | fraction
}

pub(crate) fn audio_header(first: bool, seq: u16, timestamp: u32, ssrc: u32) -> [u8; 12] {
    let mut header = [0u8; 12];
    header[0] = 0x80;
    header[1] = if first {
        PAYLOAD_ALAC | MARKER
    } else {
        PAYLOAD_ALAC
    };
    header[2..4].copy_from_slice(&seq.to_be_bytes());
    header[4..8].copy_from_slice(&timestamp.to_be_bytes());
    header[8..12].copy_from_slice(&ssrc.to_be_bytes());
    header
}

pub(crate) fn sync_packet(first: bool, playing: u32, ntp: u64, next: u32) -> [u8; 20] {
    let mut packet = [0u8; 20];
    packet[0] = if first { 0x90 } else { 0x80 };
    packet[1] = 0xd4;
    packet[2..4].copy_from_slice(&7u16.to_be_bytes());
    packet[4..8].copy_from_slice(&playing.to_be_bytes());
    packet[8..16].copy_from_slice(&ntp.to_be_bytes());
    packet[16..20].copy_from_slice(&next.to_be_bytes());
    packet
}

pub(crate) fn timing_reply(request: &[u8], received: u64) -> Option<[u8; 32]> {
    if request.len() < 32 || request[1] & 0x7f != 0x52 {
        return None;
    }
    let mut reply = [0u8; 32];
    reply[0] = 0x80;
    reply[1] = 0xd3;
    reply[2..4].copy_from_slice(&7u16.to_be_bytes());
    reply[8..16].copy_from_slice(&request[24..32]);
    reply[16..24].copy_from_slice(&received.to_be_bytes());
    reply[24..32].copy_from_slice(&ntp_now().to_be_bytes());
    Some(reply)
}

pub(crate) fn resend_request(packet: &[u8]) -> Option<(u16, u16)> {
    if packet.len() < 8 || packet[1] & 0x7f != 0x55 {
        return None;
    }
    let first = u16::from_be_bytes([packet[4], packet[5]]);
    let count = u16::from_be_bytes([packet[6], packet[7]]);
    Some((first, count))
}

pub(crate) fn futile_resend(seq: u16) -> [u8; 8] {
    let [high, low] = seq.to_be_bytes();
    [0x80, 0xd6, 0x00, 0x01, high, low, 0, 0]
}

pub(crate) fn resend_packet(original: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(4 + original.len());
    packet.extend_from_slice(&[0x80, 0xd6, 0x00, 0x01]);
    packet.extend_from_slice(original);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_packets_carry_the_marker_only_when_first() {
        assert_eq!(
            audio_header(true, 0x0102, 0x0a0b0c0d, 7),
            [0x80, 0xe0, 1, 2, 0x0a, 0x0b, 0x0c, 0x0d, 0, 0, 0, 7]
        );
        assert_eq!(audio_header(false, 1, 1, 1)[1], 0x60);
    }

    #[test]
    fn a_sync_packet_ties_the_playing_frame_to_the_clock() {
        let packet = sync_packet(true, 1000, 0x1122334455667788, 89_200);
        assert_eq!(&packet[..4], &[0x90, 0xd4, 0, 7]);
        assert_eq!(u32::from_be_bytes(packet[4..8].try_into().unwrap()), 1000);
        assert_eq!(
            u64::from_be_bytes(packet[8..16].try_into().unwrap()),
            0x1122334455667788
        );
        assert_eq!(
            u32::from_be_bytes(packet[16..20].try_into().unwrap()),
            89_200
        );
        assert_eq!(sync_packet(false, 0, 0, 0)[0], 0x80);
    }

    #[test]
    fn a_timing_request_is_answered_with_its_own_send_time_as_origin() {
        let mut request = [0u8; 32];
        request[0] = 0x80;
        request[1] = 0xd2;
        request[24..32].copy_from_slice(&42u64.to_be_bytes());
        let reply = timing_reply(&request, 99).unwrap();
        assert_eq!(&reply[..4], &[0x80, 0xd3, 0, 7]);
        assert_eq!(u64::from_be_bytes(reply[8..16].try_into().unwrap()), 42);
        assert_eq!(u64::from_be_bytes(reply[16..24].try_into().unwrap()), 99);
        assert!(timing_reply(&request[..20], 0).is_none());
        request[1] = 0xd4;
        assert!(timing_reply(&request, 0).is_none());
    }

    #[test]
    fn resend_requests_name_the_missing_range() {
        assert_eq!(
            resend_request(&[0x80, 0xd5, 0, 1, 0x12, 0x34, 0, 3]),
            Some((0x1234, 3))
        );
        assert_eq!(resend_request(&[0x80, 0xd4, 0, 1, 0, 0, 0, 0]), None);
        assert_eq!(&resend_packet(&[5, 6])[..], &[0x80, 0xd6, 0, 1, 5, 6]);
        assert_eq!(futile_resend(0xbd0d), [0x80, 0xd6, 0, 1, 0xbd, 0x0d, 0, 0]);
    }

    #[test]
    fn ntp_time_counts_from_1900() {
        let seconds = ntp_now() >> 32;
        assert!(seconds > NTP_EPOCH_OFFSET + 1_700_000_000);
    }
}
