use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

const SSDP_TIMEOUT: Duration = Duration::from_secs(3);
const SSDP_EVERY: Duration = Duration::from_secs(60);
const SSDP_FORGET_AFTER: Duration = Duration::from_secs(200);
const MDNS_PRUNE_AFTER: Duration = Duration::from_secs(6);
const LOOKUP_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReceiverKind {
    Chromecast,
    AirPlay,
    Dlna,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Endpoint {
    Chromecast(SocketAddr),
    Dlna(String),
    AirPlay(airplay::Device),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receiver {
    pub id: String,
    pub name: String,
    pub kind: ReceiverKind,
    pub model: Option<String>,
    pub(crate) endpoint: Endpoint,
}

impl Receiver {
    pub fn from_dlna(device: &dlna::Device) -> Self {
        Receiver {
            id: format!("dlna:{}", device.udn.to_ascii_lowercase()),
            name: device.name.clone(),
            kind: ReceiverKind::Dlna,
            model: device.model.clone(),
            endpoint: Endpoint::Dlna(device.location.clone()),
        }
    }

    pub fn from_chromecast(device: &chromecast::Device) -> Self {
        Receiver {
            id: format!("chromecast:{}", device.id.to_ascii_lowercase()),
            name: device.name.clone(),
            kind: ReceiverKind::Chromecast,
            model: device.model.clone(),
            endpoint: Endpoint::Chromecast(device.address),
        }
    }

    pub fn from_airplay(device: &airplay::Device) -> Self {
        Receiver {
            id: format!("airplay:{}", device.id.to_ascii_lowercase()),
            name: device.name.clone(),
            kind: ReceiverKind::AirPlay,
            model: device.model.clone(),
            endpoint: Endpoint::AirPlay(device.clone()),
        }
    }

    pub fn airplay_device(&self) -> Option<&airplay::Device> {
        match &self.endpoint {
            Endpoint::AirPlay(device) => Some(device),
            _ => None,
        }
    }

    fn has_ipv4(&self) -> bool {
        match &self.endpoint {
            Endpoint::Chromecast(address) => address.is_ipv4(),
            Endpoint::AirPlay(device) => device.address.is_ipv4(),
            Endpoint::Dlna(_) => true,
        }
    }

    fn with_ip(&self, ip: IpAddr) -> Self {
        let mut receiver = self.clone();
        match &mut receiver.endpoint {
            Endpoint::Chromecast(address) => address.set_ip(ip),
            Endpoint::AirPlay(device) => device.address.set_ip(ip),
            Endpoint::Dlna(_) => {}
        }
        receiver
    }
}

struct Seen {
    receiver: Receiver,
    at: Instant,
    service: Option<String>,
}

struct Inner {
    seen: Mutex<Vec<Seen>>,
    changed: flume::Sender<()>,
    stopped: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn upsert(&self, receiver: Receiver, service: Option<String>) {
        let mut seen = lock(&self.seen);
        let changed = match seen.iter_mut().find(|s| s.receiver.id == receiver.id) {
            Some(existing) => {
                let changed = existing.receiver != receiver;
                existing.receiver = receiver;
                existing.at = Instant::now();
                if service.is_some() {
                    existing.service = service;
                }
                changed
            }
            None => {
                log::info!("cast: found {:?} receiver {}", receiver.kind, receiver.name);
                seen.push(Seen {
                    receiver,
                    at: Instant::now(),
                    service,
                });
                true
            }
        };
        drop(seen);
        if changed {
            let _ = self.changed.try_send(());
        }
    }

    fn remove_service(&self, service: &str) {
        let mut seen = lock(&self.seen);
        let before = seen.len();
        seen.retain(|s| s.service.as_deref() != Some(service));
        let changed = seen.len() != before;
        drop(seen);
        if changed {
            let _ = self.changed.try_send(());
        }
    }

    fn forget_mdns_unseen_since(&self, since: Instant) {
        let mut seen = lock(&self.seen);
        let before = seen.len();
        seen.retain(|s| s.service.is_none() || s.at >= since);
        let changed = seen.len() != before;
        drop(seen);
        if changed {
            let _ = self.changed.try_send(());
        }
    }

    fn forget_stale_dlna(&self) {
        let mut seen = lock(&self.seen);
        let before = seen.len();
        seen.retain(|s| {
            s.receiver.kind != ReceiverKind::Dlna || s.at.elapsed() < SSDP_FORGET_AFTER
        });
        let changed = seen.len() != before;
        drop(seen);
        if changed {
            let _ = self.changed.try_send(());
        }
    }
}

pub struct Discovery {
    inner: Arc<Inner>,
    changes: flume::Receiver<()>,
    refresh: flume::Sender<()>,
    rebrowse: flume::Sender<()>,
}

impl Discovery {
    pub fn start() -> Self {
        let (changed, changes) = flume::bounded(1);
        let inner = Arc::new(Inner {
            seen: Mutex::new(Vec::new()),
            changed,
            stopped: AtomicBool::new(false),
        });
        let (refresh, refreshes) = flume::bounded(1);
        let ssdp = inner.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("cast-ssdp".into())
            .spawn(move || search_renderers(ssdp, refreshes))
        {
            log::warn!("cast: SSDP discovery did not start: {e}");
        }
        let (rebrowse, rebrowses) = flume::bounded(1);
        let browsing = inner.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("cast-mdns".into())
            .spawn(move || browse(browsing, rebrowses))
        {
            log::warn!("cast: mDNS discovery did not start: {e}");
        }
        Self {
            inner,
            changes,
            refresh,
            rebrowse,
        }
    }

    pub fn receivers(&self) -> Vec<Receiver> {
        let mut receivers: Vec<Receiver> = lock(&self.inner.seen)
            .iter()
            .map(|s| s.receiver.clone())
            .collect();
        receivers.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.kind.cmp(&b.kind))
        });
        receivers
    }

    pub fn changes(&self) -> flume::Receiver<()> {
        self.changes.clone()
    }

    pub fn refresh(&self) {
        let _ = self.refresh.try_send(());
        let _ = self.rebrowse.try_send(());
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::Release);
        let _ = self.refresh.try_send(());
        let _ = self.rebrowse.try_send(());
    }
}

fn search_renderers(inner: Arc<Inner>, refreshes: flume::Receiver<()>) {
    loop {
        if inner.stopped.load(Ordering::Acquire) {
            return;
        }
        for device in dlna::discover_renderers(SSDP_TIMEOUT) {
            inner.upsert(Receiver::from_dlna(&device), None);
        }
        inner.forget_stale_dlna();
        match refreshes.recv_timeout(SSDP_EVERY) {
            Ok(()) | Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => return,
        }
    }
}

enum Wake {
    Event(ServiceEvent),
    Rebrowse,
    Prune,
    DaemonGone,
    Closed,
}

struct Browsing {
    daemon: ServiceDaemon,
    browsers: [mdns_sd::Receiver<ServiceEvent>; 2],
}

impl Browsing {
    fn start() -> Option<Self> {
        let daemon = ServiceDaemon::new()
            .inspect_err(|e| log::warn!("cast: mDNS discovery did not start: {e}"))
            .ok()?;
        let browsers = [chromecast::SERVICE_TYPE, airplay::SERVICE_TYPE].map(|service_type| {
            daemon
                .browse(service_type)
                .inspect_err(|e| log::warn!("cast: browsing for {service_type} failed: {e}"))
                .ok()
        });
        match browsers {
            [Some(chromecasts), Some(speakers)] => Some(Self {
                daemon,
                browsers: [chromecasts, speakers],
            }),
            _ => {
                let _ = daemon.shutdown();
                None
            }
        }
    }
}

impl Drop for Browsing {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

struct Lookups {
    inner: Arc<Inner>,
    asked: HashMap<String, Instant>,
}

impl Lookups {
    fn find_ipv4(&mut self, receiver: Receiver, host: String, port: u16, service: String) {
        if self
            .asked
            .get(&service)
            .is_some_and(|at| at.elapsed() < LOOKUP_EVERY)
        {
            return;
        }
        self.asked.insert(service.clone(), Instant::now());
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new()
            .name("cast-lookup".into())
            .spawn(move || match ipv4_of(&host, port) {
                Some(ip) => inner.upsert(receiver.with_ip(ip), Some(service)),
                None => log::info!(
                    "cast: {} announced no IPv4 address and {host} does not resolve to one",
                    receiver.name
                ),
            });
        if let Err(e) = spawned {
            log::warn!("cast: an address lookup failed to start: {e}");
        }
    }
}

fn ipv4_of(host: &str, port: u16) -> Option<IpAddr> {
    use std::net::ToSocketAddrs;
    (host.trim_end_matches('.'), port)
        .to_socket_addrs()
        .ok()?
        .map(|address| address.ip())
        .find(IpAddr::is_ipv4)
}

fn browse(inner: Arc<Inner>, rebrowses: flume::Receiver<()>) {
    let mut browsing = Browsing::start();
    let mut prune: Option<Instant> = None;
    let mut lookups = Lookups {
        inner: inner.clone(),
        asked: HashMap::new(),
    };
    loop {
        let wake = match &browsing {
            Some(current) => {
                let selector = flume::Selector::new()
                    .recv(&current.browsers[0], |event| {
                        event.map_or(Wake::DaemonGone, Wake::Event)
                    })
                    .recv(&current.browsers[1], |event| {
                        event.map_or(Wake::DaemonGone, Wake::Event)
                    })
                    .recv(&rebrowses, |asked| {
                        asked.map_or(Wake::Closed, |()| Wake::Rebrowse)
                    });
                match prune {
                    Some(since) => selector
                        .wait_deadline(since + MDNS_PRUNE_AFTER)
                        .unwrap_or(Wake::Prune),
                    None => selector.wait(),
                }
            }
            None => rebrowses.recv().map_or(Wake::Closed, |()| Wake::Rebrowse),
        };
        if inner.stopped.load(Ordering::Acquire) {
            return;
        }
        match wake {
            Wake::Event(event) => handle(&inner, &mut lookups, event),
            Wake::Rebrowse => {
                drop(browsing.take());
                browsing = Browsing::start();
                prune = Some(Instant::now());
                if browsing.is_none() {
                    inner.forget_mdns_unseen_since(Instant::now());
                    prune = None;
                }
                lookups.asked.clear();
            }
            Wake::Prune => {
                if let Some(since) = prune.take() {
                    inner.forget_mdns_unseen_since(since);
                }
            }
            Wake::DaemonGone => {
                log::warn!("cast: the mDNS daemon stopped; it restarts on the next refresh");
                browsing = None;
            }
            Wake::Closed => return,
        }
    }
}

fn instance_name<'a>(fullname: &'a str, service_type: &str) -> &'a str {
    fullname
        .strip_suffix(service_type)
        .map(|name| name.trim_end_matches('.'))
        .unwrap_or(fullname)
}

fn handle(inner: &Inner, lookups: &mut Lookups, event: ServiceEvent) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            let addresses: Vec<IpAddr> = service
                .addresses
                .iter()
                .map(|address| address.to_ip_addr())
                .collect();
            let properties: HashMap<String, String> = service
                .txt_properties
                .iter()
                .map(|property| {
                    (
                        property.key().to_ascii_lowercase(),
                        property.val_str().to_string(),
                    )
                })
                .collect();
            let txt = |key: &str| properties.get(key).map(String::as_str);
            let receiver = if service.ty_domain == chromecast::SERVICE_TYPE {
                chromecast::Device::from_service(addresses, service.port, txt)
                    .map(|device| Receiver::from_chromecast(&device))
            } else if service.ty_domain == airplay::SERVICE_TYPE {
                airplay::Device::from_service(
                    instance_name(&service.fullname, airplay::SERVICE_TYPE),
                    addresses,
                    service.port,
                    txt,
                )
                .map(|device| Receiver::from_airplay(&device))
            } else {
                None
            };
            match receiver {
                Some(receiver) if receiver.has_ipv4() => {
                    inner.upsert(receiver, Some(service.fullname.clone()));
                }
                Some(receiver) => lookups.find_ipv4(
                    receiver,
                    service.host.clone(),
                    service.port,
                    service.fullname.clone(),
                ),
                None => {}
            }
        }
        ServiceEvent::ServiceRemoved(_, fullname) => inner.remove_service(&fullname),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> (Arc<Inner>, flume::Receiver<()>) {
        let (changed, changes) = flume::bounded(1);
        let inner = Arc::new(Inner {
            seen: Mutex::new(Vec::new()),
            changed,
            stopped: AtomicBool::new(false),
        });
        (inner, changes)
    }

    fn chromecast_at(ip: &str) -> Receiver {
        Receiver::from_chromecast(&chromecast::Device {
            id: "stick".into(),
            name: "Android TV".into(),
            model: None,
            address: SocketAddr::new(ip.parse().unwrap(), 8009),
        })
    }

    #[test]
    fn an_ipv6_only_receiver_waits_for_an_ipv4_address() {
        let linked = chromecast_at("fe80::8402:cbff:fe2d:e369");
        assert!(!linked.has_ipv4());
        let fixed = linked.with_ip("192.168.3.26".parse().unwrap());
        assert!(fixed.has_ipv4());
        assert_eq!(
            fixed.endpoint,
            Endpoint::Chromecast("192.168.3.26:8009".parse().unwrap())
        );
        assert_eq!(fixed.id, linked.id);
    }

    #[test]
    fn a_fresh_browse_forgets_only_mdns_receivers_it_did_not_see_again() {
        let (inner, _changes) = inner();
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.upsert(
            Receiver::from_dlna(&dlna::Device {
                udn: "uuid:r1".into(),
                name: "HiBy R1".into(),
                model: None,
                location: "http://192.168.3.6:49152/description.xml".into(),
            }),
            None,
        );
        let since = Instant::now();
        inner.forget_mdns_unseen_since(since);
        let names: Vec<String> = lock(&inner.seen)
            .iter()
            .map(|s| s.receiver.name.clone())
            .collect();
        assert_eq!(names, ["HiBy R1"]);
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.forget_mdns_unseen_since(since);
        assert_eq!(lock(&inner.seen).len(), 2);
    }

    #[test]
    fn the_instance_name_drops_the_service_type() {
        assert_eq!(
            instance_name(
                "2863813C4503@Pi AirPlay._raop._tcp.local.",
                airplay::SERVICE_TYPE
            ),
            "2863813C4503@Pi AirPlay"
        );
        assert_eq!(instance_name("odd", airplay::SERVICE_TYPE), "odd");
    }
}
