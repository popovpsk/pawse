use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use crate::net;

const PORT: u16 = 5353;
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const QUERY_ID: u16 = 0x7061;
const A: u16 = 1;
const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;
const CLASS_IN: u16 = 1;
const RESPONSE: u16 = 0x8000;
const FOLLOW_UP_WAIT: Duration = Duration::from_millis(800);
const POLL: Duration = Duration::from_millis(20);
const MAX_JUMPS: usize = 16;
const MAX_NAME: usize = 255;
const MAX_PACKET: usize = 9000;

type Name = Vec<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub service_type: String,
    pub fullname: String,
    pub port: u16,
    pub txt: HashMap<String, String>,
    pub addresses: Vec<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Ptr {
        owner: Name,
        target: Name,
    },
    Srv {
        owner: Name,
        port: u16,
        target: Name,
    },
    Txt {
        owner: Name,
        entries: HashMap<String, String>,
    },
    A {
        owner: Name,
        address: Ipv4Addr,
    },
}

pub(crate) fn ask_hosts(service_types: &[&str], hosts: &[Ipv4Addr], wait: Duration) -> Vec<Found> {
    ask(service_types, Vec::new(), &destinations(hosts), wait)
}

fn destinations(hosts: &[Ipv4Addr]) -> Vec<SocketAddr> {
    hosts
        .iter()
        .map(|host| SocketAddr::from((*host, PORT)))
        .collect()
}

pub(crate) fn browse(service_types: &[&str], hosts: &[Ipv4Addr], wait: Duration) -> Vec<Found> {
    let packet = query(&pointer_questions(service_types));
    let mut sockets = Vec::new();
    for interface in net::ipv4_interfaces() {
        match net::multicast_socket(interface) {
            Ok(socket) => {
                if let Err(e) = socket.send_to(&packet, SocketAddrV4::new(GROUP, PORT)) {
                    log::debug!("cast: mDNS query on {interface} failed: {e}");
                }
                sockets.push(socket);
            }
            Err(e) => log::debug!("cast: no mDNS socket on {interface}: {e}"),
        }
    }
    ask(service_types, sockets, &destinations(hosts), wait)
}

fn ask(
    service_types: &[&str],
    multicast: Vec<UdpSocket>,
    destinations: &[SocketAddr],
    wait: Duration,
) -> Vec<Found> {
    let unicast = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| socket.set_nonblocking(true).map(|()| socket))
    {
        Ok(socket) => socket,
        Err(e) => {
            log::debug!("cast: no socket for unicast mDNS queries: {e}");
            return Vec::new();
        }
    };
    let packet = query(&pointer_questions(service_types));
    for destination in destinations {
        if let Err(e) = unicast.send_to(&packet, destination) {
            log::debug!("cast: mDNS query to {destination} failed: {e}");
        }
    }
    let sockets: Vec<&UdpSocket> = multicast.iter().chain([&unicast]).collect();
    let mut heard: HashMap<SocketAddr, Vec<Record>> = HashMap::new();
    receive(&sockets, wait, &mut heard);
    let mut asked = false;
    for (source, records) in &heard {
        let questions = missing(service_types, records);
        if questions.is_empty() {
            continue;
        }
        if let Err(e) = unicast.send_to(&query(&questions), source) {
            log::debug!("cast: mDNS follow-up to {source} failed: {e}");
        }
        asked = true;
    }
    if asked {
        receive(&sockets, FOLLOW_UP_WAIT, &mut heard);
    }
    assemble(service_types, &heard)
}

fn receive(sockets: &[&UdpSocket], wait: Duration, heard: &mut HashMap<SocketAddr, Vec<Record>>) {
    let deadline = Instant::now() + wait;
    let mut buffer = vec![0u8; MAX_PACKET];
    while Instant::now() < deadline {
        let mut received = false;
        for socket in sockets {
            while let Ok((len, source)) = socket.recv_from(&mut buffer) {
                received = true;
                if let Some(records) = parse(&buffer[..len]) {
                    heard.entry(source).or_default().extend(records);
                }
            }
        }
        if !received {
            std::thread::sleep(POLL);
        }
    }
}

fn labels(name: &str) -> Name {
    name.split('.')
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect()
}

fn key(name: &[String]) -> String {
    name.join(".").to_ascii_lowercase()
}

fn pointer_questions(service_types: &[&str]) -> Vec<(Name, u16)> {
    service_types
        .iter()
        .map(|service_type| (labels(service_type), PTR))
        .collect()
}

fn query(questions: &[(Name, u16)]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(512);
    for field in [QUERY_ID, 0, questions.len() as u16, 0, 0, 0] {
        packet.extend_from_slice(&field.to_be_bytes());
    }
    for (name, kind) in questions {
        for label in name {
            let bytes = &label.as_bytes()[..label.len().min(63)];
            packet.push(bytes.len() as u8);
            packet.extend_from_slice(bytes);
        }
        packet.push(0);
        packet.extend_from_slice(&kind.to_be_bytes());
        packet.extend_from_slice(&CLASS_IN.to_be_bytes());
    }
    packet
}

fn field(packet: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(packet.get(at..at + 2)?.try_into().ok()?))
}

fn name(packet: &[u8], start: usize) -> Option<(Name, usize)> {
    let mut labels = Vec::new();
    let mut at = start;
    let mut end = None;
    let mut jumps = 0;
    let mut length = 0;
    loop {
        let byte = *packet.get(at)?;
        match byte & 0xC0 {
            0xC0 => {
                let target = (usize::from(byte & 0x3F) << 8) | usize::from(*packet.get(at + 1)?);
                end.get_or_insert(at + 2);
                jumps += 1;
                if jumps > MAX_JUMPS || target >= at {
                    return None;
                }
                at = target;
            }
            0x00 if byte == 0 => return Some((labels, end.unwrap_or(at + 1))),
            0x00 => {
                let len = usize::from(byte);
                let label = packet.get(at + 1..at + 1 + len)?;
                length += len + 1;
                if length > MAX_NAME {
                    return None;
                }
                labels.push(String::from_utf8_lossy(label).into_owned());
                at += 1 + len;
            }
            _ => return None,
        }
    }
}

fn txt(data: &[u8]) -> HashMap<String, String> {
    let mut entries = HashMap::new();
    let mut at = 0;
    while let Some(&len) = data.get(at) {
        let Some(entry) = data.get(at + 1..at + 1 + usize::from(len)) else {
            break;
        };
        at += 1 + usize::from(len);
        let entry = String::from_utf8_lossy(entry);
        let (name, value) = entry.split_once('=').unwrap_or((&entry, ""));
        if !name.is_empty() {
            entries
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| value.to_string());
        }
    }
    entries
}

fn parse(packet: &[u8]) -> Option<Vec<Record>> {
    if field(packet, 2)? & RESPONSE == 0 {
        return None;
    }
    let questions = field(packet, 4)?;
    let answers = usize::from(field(packet, 6)?)
        + usize::from(field(packet, 8)?)
        + usize::from(field(packet, 10)?);
    let mut at = 12;
    for _ in 0..questions {
        at = name(packet, at)?.1 + 4;
    }
    let mut records = Vec::new();
    for _ in 0..answers {
        let (owner, next) = name(packet, at)?;
        let kind = field(packet, next)?;
        let length = usize::from(field(packet, next + 8)?);
        let start = next + 10;
        let data = packet.get(start..start + length)?;
        let record = match kind {
            PTR => Some(Record::Ptr {
                owner,
                target: name(packet, start)?.0,
            }),
            SRV if length >= 7 => Some(Record::Srv {
                owner,
                port: u16::from_be_bytes([data[4], data[5]]),
                target: name(packet, start + 6)?.0,
            }),
            TXT => Some(Record::Txt {
                owner,
                entries: txt(data),
            }),
            A if length == 4 => Some(Record::A {
                owner,
                address: Ipv4Addr::new(data[0], data[1], data[2], data[3]),
            }),
            _ => None,
        };
        records.extend(record);
        at = start + length;
    }
    Some(records)
}

#[derive(Default)]
struct Index<'a> {
    pointers: Vec<(&'a Name, &'a Name)>,
    services: HashMap<String, (u16, &'a Name)>,
    texts: HashMap<String, &'a HashMap<String, String>>,
    addresses: HashMap<String, Vec<Ipv4Addr>>,
}

impl<'a> Index<'a> {
    fn of(records: &'a [Record]) -> Self {
        let mut index = Index::default();
        for record in records {
            match record {
                Record::Ptr { owner, target } => index.pointers.push((owner, target)),
                Record::Srv {
                    owner,
                    port,
                    target,
                } => {
                    index.services.insert(key(owner), (*port, target));
                }
                Record::Txt { owner, entries } => {
                    index.texts.entry(key(owner)).or_insert(entries);
                }
                Record::A { owner, address } => {
                    let addresses = index.addresses.entry(key(owner)).or_default();
                    if !addresses.contains(address) {
                        addresses.push(*address);
                    }
                }
            }
        }
        index
    }

    fn instances<'t>(&self, service_types: &[&'t str]) -> Vec<(&'t str, &'a Name, usize)> {
        let mut instances: Vec<(&'t str, &'a Name, usize)> = Vec::new();
        for (owner, target) in &self.pointers {
            let Some(service_type) = service_types
                .iter()
                .find(|service_type| key(&labels(service_type)) == key(owner))
            else {
                continue;
            };
            if target.len() > owner.len()
                && key(&target[target.len() - owner.len()..]) == key(owner)
                && !instances
                    .iter()
                    .any(|(_, known, _)| key(known) == key(target))
            {
                instances.push((service_type, target, target.len() - owner.len()));
            }
        }
        instances
    }
}

fn missing(service_types: &[&str], records: &[Record]) -> Vec<(Name, u16)> {
    let index = Index::of(records);
    let mut questions: Vec<(Name, u16)> = Vec::new();
    for (_, instance, _) in index.instances(service_types) {
        let instance_key = key(instance);
        match index.services.get(&instance_key) {
            None => questions.push((instance.clone(), SRV)),
            Some((_, host)) if !index.addresses.contains_key(&key(host)) => {
                questions.push((host.to_vec(), A));
            }
            Some(_) => {}
        }
        if !index.texts.contains_key(&instance_key) {
            questions.push((instance.clone(), TXT));
        }
    }
    questions
}

fn assemble(service_types: &[&str], heard: &HashMap<SocketAddr, Vec<Record>>) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for (source, records) in heard {
        let index = Index::of(records);
        for (service_type, instance, labels_before_type) in index.instances(service_types) {
            let fullname = format!(
                "{}.{}",
                instance[..labels_before_type].join("."),
                service_type
            );
            if found
                .iter()
                .any(|known| known.fullname.eq_ignore_ascii_case(&fullname))
            {
                continue;
            }
            let Some(&(port, host)) = index.services.get(&key(instance)) else {
                continue;
            };
            let mut addresses: Vec<IpAddr> = index
                .addresses
                .get(&key(host))
                .map(|addresses| addresses.iter().copied().map(IpAddr::V4).collect())
                .unwrap_or_default();
            if addresses.is_empty() && source.is_ipv4() {
                addresses.push(source.ip());
            }
            found.push(Found {
                service_type: service_type.to_string(),
                fullname,
                port,
                txt: index
                    .texts
                    .get(&key(instance))
                    .map(|entries| (*entries).clone())
                    .unwrap_or_default(),
                addresses,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAOP: &str = "_raop._tcp.local.";
    const CAST: &str = "_googlecast._tcp.local.";

    fn encode(name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for label in labels(name) {
            bytes.push(label.len() as u8);
            bytes.extend_from_slice(label.as_bytes());
        }
        bytes.push(0);
        bytes
    }

    fn record(owner: &[u8], kind: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = owner.to_vec();
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&0x8001u16.to_be_bytes());
        bytes.extend_from_slice(&10u32.to_be_bytes());
        bytes.extend_from_slice(&(data.len() as u16).to_be_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    fn response(records: &[Vec<u8>]) -> Vec<u8> {
        let mut packet = Vec::new();
        for field in [QUERY_ID, 0x8400, 0, records.len() as u16, 0, 0] {
            packet.extend_from_slice(&field.to_be_bytes());
        }
        for record in records {
            packet.extend_from_slice(record);
        }
        packet
    }

    fn srv(port: u16, host: &str) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 0];
        data.extend_from_slice(&port.to_be_bytes());
        data.extend_from_slice(&encode(host));
        data
    }

    fn txt_data(entries: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        for entry in entries {
            data.push(entry.len() as u8);
            data.extend_from_slice(entry.as_bytes());
        }
        data
    }

    fn pi_answer() -> Vec<u8> {
        let instance = "2863813C4503@Pi AirPlay._raop._tcp.local";
        response(&[
            record(&encode(RAOP), PTR, &encode(instance)),
            record(&encode(instance), SRV, &srv(7000, "rpi.local")),
            record(
                &encode(instance),
                TXT,
                &txt_data(&["et=0,1", "cn=0,1", "AM=ShairportSync", "flag"]),
            ),
            record(&encode("rpi.local"), A, &[192, 168, 3, 22]),
        ])
    }

    #[test]
    fn a_query_asks_each_name_without_the_unicast_bit() {
        let packet = query(&pointer_questions(&[RAOP, CAST]));
        assert_eq!(field(&packet, 0), Some(QUERY_ID));
        assert_eq!(field(&packet, 2), Some(0));
        assert_eq!(field(&packet, 4), Some(2));
        let (first, next) = name(&packet, 12).unwrap();
        assert_eq!(first, labels(RAOP));
        assert_eq!(field(&packet, next), Some(PTR));
        assert_eq!(field(&packet, next + 2), Some(CLASS_IN));
        assert_eq!(name(&packet, next + 4).unwrap().0, labels(CAST));
    }

    #[test]
    fn a_full_answer_is_one_service() {
        let records = parse(&pi_answer()).unwrap();
        assert!(missing(&[RAOP], &records).is_empty());
        let heard = HashMap::from([("192.168.3.22:5353".parse().unwrap(), records)]);
        let found = assemble(&[RAOP, CAST], &heard);
        assert_eq!(
            found,
            [Found {
                service_type: RAOP.into(),
                fullname: "2863813C4503@Pi AirPlay._raop._tcp.local.".into(),
                port: 7000,
                txt: HashMap::from([
                    ("et".into(), "0,1".into()),
                    ("cn".into(), "0,1".into()),
                    ("am".into(), "ShairportSync".into()),
                    ("flag".into(), String::new()),
                ]),
                addresses: vec!["192.168.3.22".parse().unwrap()],
            }]
        );
    }

    #[test]
    fn compressed_names_are_followed() {
        let mut packet = response(&[]);
        let type_at = packet.len();
        packet.extend_from_slice(&encode(CAST));
        packet.extend_from_slice(&PTR.to_be_bytes());
        packet.extend_from_slice(&[0, 1, 0, 0, 0, 10]);
        let mut target = vec![10];
        target.extend_from_slice(b"Android-TV");
        target.extend_from_slice(&[0xC0 | (type_at >> 8) as u8, type_at as u8]);
        packet.extend_from_slice(&(target.len() as u16).to_be_bytes());
        packet.extend_from_slice(&target);
        packet[7] = 1;
        let records = parse(&packet).unwrap();
        assert_eq!(
            records,
            [Record::Ptr {
                owner: labels(CAST),
                target: labels("Android-TV._googlecast._tcp.local"),
            }]
        );
    }

    #[test]
    fn broken_packets_are_rejected_without_panicking() {
        let answer = pi_answer();
        for len in 0..answer.len() {
            let _ = parse(&answer[..len]);
        }
        let mut looped = response(&[]);
        looped[7] = 1;
        let at = looped.len();
        looped.extend_from_slice(&[0xC0 | (at >> 8) as u8, at as u8]);
        assert_eq!(parse(&looped), None);
        assert_eq!(parse(&query(&pointer_questions(&[RAOP]))), None);
    }

    #[test]
    fn a_type_sent_as_one_label_is_not_cut_short() {
        let mut owner = vec![16];
        owner.extend_from_slice(b"_raop._tcp.local");
        owner.push(0);
        let mut target = vec![1, b'x'];
        target.extend_from_slice(&owner);
        let records = parse(&response(&[
            record(&owner, PTR, &target),
            record(&target, SRV, &srv(5000, "x.local")),
        ]))
        .unwrap();
        let heard = HashMap::from([("192.168.3.9:5353".parse().unwrap(), records)]);
        let found = assemble(&[RAOP], &heard);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].fullname, "x._raop._tcp.local.");
    }

    #[test]
    fn what_a_bare_pointer_misses_is_asked_for() {
        let instance = "Android-TV._googlecast._tcp.local";
        let records = parse(&response(&[record(&encode(CAST), PTR, &encode(instance))])).unwrap();
        assert_eq!(
            missing(&[CAST], &records),
            [(labels(instance), SRV), (labels(instance), TXT)]
        );
        let records = parse(&response(&[
            record(&encode(CAST), PTR, &encode(instance)),
            record(&encode(instance), SRV, &srv(8009, "stick.local")),
            record(&encode(instance), TXT, &txt_data(&["id=abc"])),
        ]))
        .unwrap();
        assert_eq!(missing(&[CAST], &records), [(labels("stick.local"), A)]);
    }

    fn fake_responder(answers: Vec<Vec<u8>>) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 1500];
            for answer in answers {
                let Ok((_, from)) = socket.recv_from(&mut buffer) else {
                    return;
                };
                let _ = socket.send_to(&answer, from);
            }
        });
        address
    }

    #[test]
    fn a_device_asked_directly_is_followed_up_until_it_is_complete() {
        let instance = "Android-TV._googlecast._tcp.local";
        let responder = fake_responder(vec![
            response(&[record(&encode(CAST), PTR, &encode(instance))]),
            response(&[
                record(&encode(instance), SRV, &srv(8009, "stick.local")),
                record(
                    &encode(instance),
                    TXT,
                    &txt_data(&["id=abc", "fn=Android TV"]),
                ),
                record(&encode("stick.local"), A, &[192, 168, 3, 26]),
            ]),
        ]);
        let found = ask(
            &[RAOP, CAST],
            Vec::new(),
            &[responder],
            Duration::from_millis(300),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].service_type, CAST);
        assert_eq!(found[0].fullname, "Android-TV._googlecast._tcp.local.");
        assert_eq!(found[0].port, 8009);
        assert_eq!(
            found[0].txt.get("fn").map(String::as_str),
            Some("Android TV")
        );
        assert_eq!(
            found[0].addresses,
            ["192.168.3.26".parse::<IpAddr>().unwrap()]
        );
    }

    #[test]
    fn without_an_address_record_the_responder_is_the_address() {
        let instance = "AA@Speaker._raop._tcp.local";
        let complete_but_a = response(&[
            record(&encode(RAOP), PTR, &encode(instance)),
            record(&encode(instance), SRV, &srv(5000, "speaker.local")),
            record(&encode(instance), TXT, &txt_data(&["et=0"])),
        ]);
        let responder = fake_responder(vec![complete_but_a.clone(), complete_but_a]);
        let found = ask(
            &[RAOP],
            Vec::new(),
            &[responder],
            Duration::from_millis(300),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].addresses, ["127.0.0.1".parse::<IpAddr>().unwrap()]);
    }
}
