use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

use crate::{net, unicast_mdns};

const SSDP_TIMEOUT: Duration = Duration::from_secs(3);
const SEARCH_EVERY: Duration = Duration::from_secs(60);
const LOOKUP_EVERY: Duration = Duration::from_secs(10);
const UNICAST_WAIT: Duration = Duration::from_millis(1500);
const CHECK_EVERY: Duration = Duration::from_secs(2);
const QUIET_AFTER: Duration = Duration::from_secs(150);
const CONFIRM_WITHIN: Duration = Duration::from_secs(6);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_WAIT: Duration = Duration::from_secs(1);
const DESCRIBE_AGAIN_AFTER: Duration = Duration::from_secs(30);
const FORGET_HOST_AFTER: Duration = Duration::from_secs(30 * 60);
const MAX_HOSTS: usize = 64;
const NOTIFY_POLL: Duration = Duration::from_secs(1);
const SERVICE_TYPES: [&str; 3] = [
    chromecast::SERVICE_TYPE,
    airplay::RAOP_SERVICE_TYPE,
    airplay::AIRPLAY_SERVICE_TYPE,
];

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

fn dlna_id(udn: &str) -> String {
    format!("dlna:{}", udn.to_ascii_lowercase())
}

fn location_host(location: &str) -> Option<IpAddr> {
    let authority = location.split("://").nth(1)?.split('/').next()?;
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    host.parse().ok()
}

impl Receiver {
    pub fn from_dlna(device: &dlna::Device) -> Self {
        Receiver {
            id: dlna_id(&device.udn),
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

    fn airplay_protocol(&self) -> Option<airplay::Protocol> {
        self.airplay_device().map(|device| device.protocol)
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

    fn host(&self) -> Option<IpAddr> {
        match &self.endpoint {
            Endpoint::Chromecast(address) => Some(address.ip()),
            Endpoint::AirPlay(device) => Some(device.address.ip()),
            Endpoint::Dlna(location) => location_host(location),
        }
    }
}

fn reachable(receiver: &Receiver) -> bool {
    match &receiver.endpoint {
        Endpoint::Chromecast(address) => announces(receiver, chromecast::SERVICE_TYPE, *address),
        Endpoint::AirPlay(device) => {
            let service_type = match device.protocol {
                airplay::Protocol::Raop => airplay::RAOP_SERVICE_TYPE,
                airplay::Protocol::AirPlay2 => airplay::AIRPLAY_SERVICE_TYPE,
            };
            announces(receiver, service_type, device.address)
        }
        Endpoint::Dlna(location) => dlna::describe_renderer(location)
            .is_ok_and(|device| dlna_id(&device.udn) == receiver.id),
    }
}

fn announces(receiver: &Receiver, service_type: &str, address: SocketAddr) -> bool {
    let found = match address.ip() {
        IpAddr::V4(host) => unicast_mdns::ask_hosts(&[service_type], &[host], PROBE_WAIT),
        IpAddr::V6(_) => Vec::new(),
    };
    if found.is_empty() {
        return TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok();
    }
    found.iter().any(|found| {
        let txt = |key: &str| found.txt.get(key).map(String::as_str);
        receiver_of(
            &found.service_type,
            &found.fullname,
            found.addresses.clone(),
            found.port,
            txt,
        )
        .is_some_and(|announced| announced.id == receiver.id)
    })
}

struct Seen {
    receiver: Receiver,
    heard: Instant,
    service: Option<String>,
    doubted: bool,
}

struct Inner {
    seen: Mutex<Vec<Seen>>,
    hosts: Mutex<HashMap<Ipv4Addr, Instant>>,
    in_use: Mutex<Vec<String>>,
    changed: flume::Sender<()>,
    stopped: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_due(seen: &Seen, in_use: &[String], now: Instant) -> bool {
    !in_use.contains(&seen.receiver.id)
        && (seen.doubted || now.saturating_duration_since(seen.heard) >= QUIET_AFTER)
}

impl Inner {
    fn new(changed: flume::Sender<()>) -> Self {
        Inner {
            seen: Mutex::new(Vec::new()),
            hosts: Mutex::new(HashMap::new()),
            in_use: Mutex::new(Vec::new()),
            changed,
            stopped: AtomicBool::new(false),
        }
    }

    fn notify_changed(&self) {
        let _ = self.changed.try_send(());
    }

    fn upsert(&self, receiver: Receiver, service: Option<String>) {
        let mut seen = lock(&self.seen);
        let changed = match seen.iter_mut().find(|s| s.receiver.id == receiver.id) {
            Some(existing)
                if existing.receiver.airplay_protocol() == Some(airplay::Protocol::Raop)
                    && receiver.airplay_protocol() == Some(airplay::Protocol::AirPlay2)
                    && existing.receiver.host() == receiver.host()
                    && !existing.doubted =>
            {
                existing.heard = Instant::now();
                existing.doubted = false;
                false
            }
            Some(existing) => {
                let changed = existing.receiver != receiver;
                existing.receiver = receiver;
                existing.heard = Instant::now();
                existing.doubted = false;
                if service.is_some() {
                    existing.service = service;
                }
                changed
            }
            None => {
                log::info!("cast: found {:?} receiver {}", receiver.kind, receiver.name);
                seen.push(Seen {
                    receiver,
                    heard: Instant::now(),
                    service,
                    doubted: false,
                });
                true
            }
        };
        drop(seen);
        if changed {
            self.notify_changed();
        }
    }

    fn touch(&self, id: &str) {
        if let Some(seen) = lock(&self.seen).iter_mut().find(|s| s.receiver.id == id) {
            seen.heard = Instant::now();
            seen.doubted = false;
        }
    }

    fn doubt(&self, id: &str) {
        if let Some(seen) = lock(&self.seen).iter_mut().find(|s| s.receiver.id == id) {
            seen.doubted = true;
        }
    }

    fn doubt_service(&self, service: &str) {
        for seen in lock(&self.seen).iter_mut() {
            if seen.service.as_deref() == Some(service) {
                seen.doubted = true;
            }
        }
    }

    fn doubt_unheard_since(&self, since: Instant) {
        for seen in lock(&self.seen).iter_mut() {
            if seen.heard < since {
                seen.doubted = true;
            }
        }
    }

    fn to_check(&self, now: Instant) -> Vec<Receiver> {
        let in_use = lock(&self.in_use).clone();
        lock(&self.seen)
            .iter()
            .filter(|s| is_due(s, &in_use, now))
            .map(|s| s.receiver.clone())
            .collect()
    }

    fn due(&self, id: &str, now: Instant) -> Option<Receiver> {
        let in_use = lock(&self.in_use).clone();
        lock(&self.seen)
            .iter()
            .find(|s| s.receiver.id == id && is_due(s, &in_use, now))
            .map(|s| s.receiver.clone())
    }

    fn checked(&self, id: &str, alive: bool, started: Instant) {
        if alive {
            self.touch(id);
            return;
        }
        let in_use = lock(&self.in_use).clone();
        let mut seen = lock(&self.seen);
        let before = seen.len();
        seen.retain(|s| {
            s.receiver.id != id || s.heard > started || in_use.iter().any(|used| used == id)
        });
        let removed = seen.len() != before;
        drop(seen);
        if removed {
            log::info!("cast: receiver {id} is gone");
            self.notify_changed();
        }
    }

    fn set_in_use(&self, ids: Vec<String>) {
        *lock(&self.in_use) = ids;
    }

    fn dlna_location(&self, id: &str) -> Option<String> {
        lock(&self.seen)
            .iter()
            .find(|s| s.receiver.id == id)
            .and_then(|s| match &s.receiver.endpoint {
                Endpoint::Dlna(location) => Some(location.clone()),
                _ => None,
            })
    }

    fn note_host(&self, host: IpAddr) {
        if let IpAddr::V4(host) = host
            && !host.is_loopback()
            && !host.is_unspecified()
            && !host.is_link_local()
        {
            let mut noted = lock(&self.hosts);
            noted.insert(host, Instant::now());
            if noted.len() > MAX_HOSTS
                && let Some(oldest) = noted
                    .iter()
                    .min_by_key(|(_, at)| **at)
                    .map(|(host, _)| *host)
            {
                noted.remove(&oldest);
            }
        }
    }

    fn hosts(&self) -> Vec<Ipv4Addr> {
        let own = net::ipv4_interfaces();
        let mut hosts: Vec<Ipv4Addr> = {
            let mut noted = lock(&self.hosts);
            noted.retain(|_, at| at.elapsed() < FORGET_HOST_AFTER);
            noted.keys().copied().collect()
        };
        hosts.extend(
            lock(&self.seen)
                .iter()
                .filter_map(|s| match s.receiver.host() {
                    Some(IpAddr::V4(host)) => Some(host),
                    _ => None,
                }),
        );
        hosts.retain(|host| !own.contains(host));
        hosts.sort();
        hosts.dedup();
        hosts
    }
}

pub struct Discovery {
    inner: Arc<Inner>,
    changes: flume::Receiver<()>,
    wakers: Vec<flume::Sender<()>>,
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::warn!("cast: {name} did not start: {e}");
    }
}

impl Discovery {
    pub fn start() -> Self {
        crate::dacp::warm_up();
        let (changed, changes) = flume::bounded(1);
        let inner = Arc::new(Inner::new(changed));
        let mut wakers = Vec::new();
        let mut waker = || {
            let (wake, wakes) = flume::bounded(1);
            wakers.push(wake);
            wakes
        };
        let (ssdp, ssdp_wakes) = (inner.clone(), waker());
        spawn("cast-ssdp", move || search_renderers(ssdp, ssdp_wakes));
        let (browsing, rebrowses) = (inner.clone(), waker());
        spawn("cast-mdns", move || browse(browsing, rebrowses));
        let (listening, relistens) = (inner.clone(), waker());
        spawn("cast-notify", move || listen(listening, relistens));
        let (checking, rechecks) = (inner.clone(), waker());
        spawn("cast-check", move || keep_checking(checking, rechecks));
        Self {
            inner,
            changes,
            wakers,
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
        for waker in &self.wakers {
            let _ = waker.try_send(());
        }
    }

    pub fn set_in_use(&self, ids: Vec<String>) {
        self.inner.set_in_use(ids);
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::Release);
        self.refresh();
    }
}

fn wait_or_wake(wakes: &flume::Receiver<()>, timeout: Duration) -> bool {
    !matches!(
        wakes.recv_timeout(timeout),
        Err(flume::RecvTimeoutError::Disconnected)
    )
}

fn search_renderers(inner: Arc<Inner>, wakes: flume::Receiver<()>) {
    loop {
        if inner.stopped.load(Ordering::Acquire) {
            return;
        }
        for device in dlna::discover_renderers(&inner.hosts(), SSDP_TIMEOUT) {
            inner.upsert(Receiver::from_dlna(&device), None);
        }
        if !wait_or_wake(&wakes, SEARCH_EVERY) {
            return;
        }
    }
}

fn keep_checking(inner: Arc<Inner>, wakes: flume::Receiver<()>) {
    let mut asked: Option<Instant> = None;
    let mut confirm_since: Option<Instant> = None;
    loop {
        if inner.stopped.load(Ordering::Acquire) {
            return;
        }
        if asked.is_none_or(|at| at.elapsed() >= SEARCH_EVERY) {
            ask_directly(&inner);
            asked = Some(Instant::now());
        }
        if let Some(since) = confirm_since
            && since.elapsed() >= CONFIRM_WITHIN
        {
            inner.doubt_unheard_since(since);
            confirm_since = None;
        }
        check(&inner, Instant::now(), reachable);
        match wakes.recv_timeout(CHECK_EVERY) {
            Ok(()) => {
                asked = None;
                confirm_since = Some(Instant::now());
            }
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn check(inner: &Inner, now: Instant, reachable: impl Fn(&Receiver) -> bool) {
    for candidate in inner.to_check(now) {
        if inner.stopped.load(Ordering::Acquire) {
            return;
        }
        let Some(receiver) = inner.due(&candidate.id, now) else {
            continue;
        };
        let started = Instant::now();
        let alive = reachable(&receiver);
        log::debug!(
            "cast: checked {} directly: {}",
            receiver.name,
            if alive { "there" } else { "not answering" }
        );
        inner.checked(&receiver.id, alive, started);
    }
}

fn is_own(addresses: &[IpAddr], own: &[Ipv4Addr]) -> bool {
    addresses
        .iter()
        .any(|address| matches!(address, IpAddr::V4(v4) if own.contains(v4)))
}

fn ask_directly(inner: &Inner) {
    let own = net::ipv4_interfaces();
    for found in unicast_mdns::browse(&SERVICE_TYPES, &inner.hosts(), UNICAST_WAIT) {
        if is_own(&found.addresses, &own) {
            continue;
        }
        for address in &found.addresses {
            inner.note_host(*address);
        }
        let txt = |key: &str| found.txt.get(key).map(String::as_str);
        if let Some(receiver) = receiver_of(
            &found.service_type,
            &found.fullname,
            found.addresses.clone(),
            found.port,
            txt,
        )
        .filter(Receiver::has_ipv4)
        {
            inner.upsert(receiver, Some(found.fullname.clone()));
        }
    }
}

fn listen(inner: Arc<Inner>, wakes: flume::Receiver<()>) {
    let mut described: HashMap<String, Instant> = HashMap::new();
    let mut failed = false;
    loop {
        let listener = match dlna::NotifyListener::open() {
            Ok(listener) => listener,
            Err(e) => {
                if failed {
                    log::debug!("cast: still not listening for SSDP announcements: {e}");
                } else {
                    log::info!("cast: not listening for SSDP announcements: {e}");
                }
                failed = true;
                if !wait_or_wake(&wakes, SEARCH_EVERY) || inner.stopped.load(Ordering::Acquire) {
                    return;
                }
                continue;
            }
        };
        loop {
            if inner.stopped.load(Ordering::Acquire) {
                return;
            }
            match wakes.try_recv() {
                Ok(()) => break,
                Err(flume::TryRecvError::Empty) => {}
                Err(flume::TryRecvError::Disconnected) => return,
            }
            if let Some(notification) = listener.next(NOTIFY_POLL) {
                described.retain(|_, at| at.elapsed() < DESCRIBE_AGAIN_AFTER);
                announced(&inner, &mut described, notification, |location| {
                    dlna::describe_renderer(location)
                        .inspect_err(|e| {
                            log::debug!("cast: renderer {location} did not describe itself: {e}")
                        })
                        .ok()
                });
            }
        }
    }
}

fn is_renderer(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains(":device:mediarenderer:") || kind.contains(":service:avtransport:")
}

fn announced(
    inner: &Inner,
    described: &mut HashMap<String, Instant>,
    notification: dlna::Notification,
    describe: impl Fn(&str) -> Option<dlna::Device>,
) {
    let id = dlna_id(&notification.udn);
    if !notification.alive {
        inner.doubt(&id);
        return;
    }
    let known = inner.dlna_location(&id);
    if known.is_some() || is_renderer(&notification.kind) {
        inner.note_host(notification.source);
    }
    let moved =
        matches!((&known, &notification.location), (Some(known), Some(now)) if known != now);
    if known.is_some() && !moved {
        inner.touch(&id);
        return;
    }
    if known.is_none() && !is_renderer(&notification.kind) {
        return;
    }
    if described.contains_key(&id) {
        return;
    }
    described.insert(id, Instant::now());
    if let Some(device) = notification.location.as_deref().and_then(describe) {
        inner.upsert(Receiver::from_dlna(&device), None);
    }
}

enum Wake {
    Event(ServiceEvent),
    Rebrowse,
    DaemonGone,
    Closed,
}

struct Browsing {
    daemon: ServiceDaemon,
    browsers: [mdns_sd::Receiver<ServiceEvent>; 3],
}

impl Browsing {
    fn start() -> Option<Self> {
        let daemon = ServiceDaemon::new()
            .inspect_err(|e| log::warn!("cast: mDNS discovery did not start: {e}"))
            .ok()?;
        let browsers = SERVICE_TYPES.map(|service_type| {
            daemon
                .browse(service_type)
                .inspect_err(|e| log::warn!("cast: browsing for {service_type} failed: {e}"))
                .ok()
        });
        match browsers {
            [Some(chromecasts), Some(raop), Some(airplay)] => Some(Self {
                daemon,
                browsers: [chromecasts, raop, airplay],
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
                Some(ip) if is_own(&[ip], &net::ipv4_interfaces()) => {}
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
    let mut lookups = Lookups {
        inner: inner.clone(),
        asked: HashMap::new(),
    };
    loop {
        let wake = match &browsing {
            Some(current) => flume::Selector::new()
                .recv(&current.browsers[0], |event| {
                    event.map_or(Wake::DaemonGone, Wake::Event)
                })
                .recv(&current.browsers[1], |event| {
                    event.map_or(Wake::DaemonGone, Wake::Event)
                })
                .recv(&current.browsers[2], |event| {
                    event.map_or(Wake::DaemonGone, Wake::Event)
                })
                .recv(&rebrowses, |asked| {
                    asked.map_or(Wake::Closed, |()| Wake::Rebrowse)
                })
                .wait(),
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
                lookups.asked.clear();
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

fn receiver_of<'a>(
    service_type: &str,
    fullname: &str,
    addresses: Vec<IpAddr>,
    port: u16,
    txt: impl Fn(&str) -> Option<&'a str>,
) -> Option<Receiver> {
    if service_type == chromecast::SERVICE_TYPE {
        chromecast::Device::from_service(addresses, port, txt)
            .map(|device| Receiver::from_chromecast(&device))
    } else if service_type == airplay::RAOP_SERVICE_TYPE {
        airplay::Device::from_service(
            instance_name(fullname, airplay::RAOP_SERVICE_TYPE),
            addresses,
            port,
            txt,
        )
        .map(|device| Receiver::from_airplay(&device))
    } else if service_type == airplay::AIRPLAY_SERVICE_TYPE {
        airplay::Device::from_airplay_service(
            instance_name(fullname, airplay::AIRPLAY_SERVICE_TYPE),
            addresses,
            port,
            txt,
        )
        .map(|device| Receiver::from_airplay(&device))
    } else {
        None
    }
}

fn handle(inner: &Inner, lookups: &mut Lookups, event: ServiceEvent) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            let addresses: Vec<IpAddr> = service
                .addresses
                .iter()
                .map(|address| address.to_ip_addr())
                .collect();
            if is_own(&addresses, &net::ipv4_interfaces()) {
                return;
            }
            for address in &addresses {
                inner.note_host(*address);
            }
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
            match receiver_of(
                &service.ty_domain,
                &service.fullname,
                addresses,
                service.port,
                txt,
            ) {
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
        ServiceEvent::ServiceRemoved(_, fullname) => inner.doubt_service(&fullname),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> (Arc<Inner>, flume::Receiver<()>) {
        let (changed, changes) = flume::bounded(1);
        (Arc::new(Inner::new(changed)), changes)
    }

    fn chromecast_at(ip: &str) -> Receiver {
        Receiver::from_chromecast(&chromecast::Device {
            id: "stick".into(),
            name: "Android TV".into(),
            model: None,
            address: SocketAddr::new(ip.parse().unwrap(), 8009),
        })
    }

    fn r1() -> Receiver {
        Receiver::from_dlna(&dlna::Device {
            udn: "uuid:R1".into(),
            name: "HiBy R1".into(),
            model: None,
            location: "http://192.168.3.6:49152/description.xml".into(),
        })
    }

    fn names(inner: &Inner) -> Vec<String> {
        let mut names: Vec<String> = lock(&inner.seen)
            .iter()
            .map(|s| s.receiver.name.clone())
            .collect();
        names.sort();
        names
    }

    fn later() -> Instant {
        Instant::now() + QUIET_AFTER
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
    fn a_quiet_receiver_is_dropped_only_when_it_cannot_be_reached() {
        let (inner, _changes) = inner();
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.upsert(r1(), None);
        check(&inner, Instant::now(), |_| panic!("nothing is quiet yet"));
        check(&inner, later(), |receiver| {
            receiver.kind == ReceiverKind::Dlna
        });
        assert_eq!(names(&inner), ["HiBy R1"]);
        check(&inner, Instant::now(), |_| panic!("the R1 was just heard"));
    }

    #[test]
    fn the_receiver_in_use_is_never_checked_or_dropped() {
        let (inner, _changes) = inner();
        inner.upsert(r1(), None);
        inner.set_in_use(vec![r1().id]);
        inner.doubt(&r1().id);
        check(&inner, later(), |_| {
            panic!("the receiver in use was checked")
        });
        inner.checked(&r1().id, false, later());
        assert_eq!(names(&inner), ["HiBy R1"]);
    }

    #[test]
    fn a_goodbye_only_puts_a_receiver_up_for_a_check() {
        let (inner, _changes) = inner();
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.doubt_service("stick");
        assert_eq!(inner.to_check(Instant::now()).len(), 1);
        check(&inner, Instant::now(), |_| true);
        assert_eq!(names(&inner), ["Android TV"]);
        assert!(inner.to_check(Instant::now()).is_empty());
    }

    #[test]
    fn receivers_not_confirmed_after_a_refresh_are_checked() {
        let (inner, _changes) = inner();
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.upsert(r1(), None);
        std::thread::sleep(Duration::from_millis(2));
        let since = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        inner.upsert(r1(), None);
        inner.doubt_unheard_since(since);
        let doubted: Vec<String> = inner
            .to_check(Instant::now())
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(doubted, ["Android TV"]);
    }

    #[test]
    fn a_receiver_heard_while_it_was_checked_is_kept() {
        let (inner, _changes) = inner();
        inner.upsert(r1(), None);
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        inner.upsert(r1(), None);
        inner.checked(&r1().id, false, started);
        assert_eq!(names(&inner), ["HiBy R1"]);
    }

    #[test]
    fn known_hosts_are_the_receivers_and_whoever_announced_something() {
        let (inner, _changes) = inner();
        inner.upsert(r1(), None);
        inner.upsert(chromecast_at("fe80::1"), None);
        inner.note_host("192.168.3.22".parse().unwrap());
        inner.note_host("127.0.0.1".parse().unwrap());
        inner.note_host("169.254.3.4".parse().unwrap());
        assert_eq!(
            inner.hosts(),
            [
                "192.168.3.6".parse::<Ipv4Addr>().unwrap(),
                "192.168.3.22".parse().unwrap()
            ]
        );
    }

    fn notification(alive: bool, kind: &str, location: Option<&str>) -> dlna::Notification {
        dlna::Notification {
            alive,
            udn: "uuid:R1".into(),
            kind: kind.into(),
            location: location.map(str::to_string),
            source: "192.168.3.6".parse().unwrap(),
        }
    }

    const RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
    const LOCATION: &str = "http://192.168.3.6:49152/description.xml";

    fn r1_device(location: &str) -> dlna::Device {
        dlna::Device {
            udn: "uuid:R1".into(),
            name: "HiBy R1".into(),
            model: None,
            location: location.into(),
        }
    }

    #[test]
    fn an_announced_renderer_is_described_once_and_then_only_heard() {
        let (inner, _changes) = inner();
        let mut described = HashMap::new();
        announced(
            &inner,
            &mut described,
            notification(true, "upnp:rootdevice", Some(LOCATION)),
            |_| panic!("not a renderer announcement"),
        );
        assert!(names(&inner).is_empty());
        announced(
            &inner,
            &mut described,
            notification(true, RENDERER, Some(LOCATION)),
            |location| Some(r1_device(location)),
        );
        assert_eq!(names(&inner), ["HiBy R1"]);
        inner.doubt(&r1().id);
        announced(
            &inner,
            &mut described,
            notification(true, "upnp:rootdevice", Some(LOCATION)),
            |_| panic!("a known renderer is not described again"),
        );
        assert!(inner.to_check(Instant::now()).is_empty());
        assert_eq!(inner.hosts(), ["192.168.3.6".parse::<Ipv4Addr>().unwrap()]);
    }

    #[test]
    fn a_check_uses_the_receiver_as_it_is_when_its_turn_comes() {
        let (inner, _changes) = inner();
        inner.upsert(r1(), None);
        inner.upsert(chromecast_at("192.168.3.26"), Some("stick".into()));
        inner.doubt(&r1().id);
        inner.doubt(&chromecast_at("192.168.3.26").id);
        check(&inner, Instant::now(), |receiver| {
            if receiver.kind == ReceiverKind::Dlna {
                inner.upsert(chromecast_at("192.168.3.30"), Some("stick".into()));
                return true;
            }
            panic!("the stick was heard again and is not due any more");
        });
        assert_eq!(names(&inner), ["Android TV", "HiBy R1"]);
    }

    #[test]
    fn only_renderers_make_their_senders_known_hosts() {
        let (inner, _changes) = inner();
        let mut described = HashMap::new();
        announced(
            &inner,
            &mut described,
            notification(
                true,
                "urn:schemas-upnp-org:device:MediaServer:1",
                Some(LOCATION),
            ),
            |_| panic!("a media server is not described"),
        );
        assert!(inner.hosts().is_empty());
        for last in 0..=MAX_HOSTS as u8 {
            inner.note_host(IpAddr::from([10, 0, 1, last]));
        }
        assert_eq!(inner.hosts().len(), MAX_HOSTS);
    }

    #[test]
    fn a_byebye_puts_the_renderer_up_for_a_check_and_a_move_is_followed() {
        let (inner, _changes) = inner();
        inner.upsert(r1(), None);
        let mut described = HashMap::new();
        announced(
            &inner,
            &mut described,
            notification(false, RENDERER, None),
            |_| panic!("a byebye is not described"),
        );
        assert_eq!(inner.to_check(Instant::now()).len(), 1);
        let moved = "http://192.168.3.9:49152/description.xml";
        announced(
            &inner,
            &mut described,
            notification(true, RENDERER, Some(moved)),
            |location| Some(r1_device(location)),
        );
        assert_eq!(inner.dlna_location(&r1().id).as_deref(), Some(moved));
        assert!(inner.to_check(Instant::now()).is_empty());
    }

    #[test]
    fn the_host_of_a_description_url() {
        assert_eq!(
            location_host("http://192.168.3.22:49494/description.xml"),
            Some("192.168.3.22".parse().unwrap())
        );
        assert_eq!(location_host("http://tv.local/d.xml"), None);
        assert_eq!(location_host("garbage"), None);
    }

    #[test]
    fn the_instance_name_drops_the_service_type() {
        assert_eq!(
            instance_name(
                "2863813C4503@Pi AirPlay._raop._tcp.local.",
                airplay::RAOP_SERVICE_TYPE
            ),
            "2863813C4503@Pi AirPlay"
        );
        assert_eq!(instance_name("odd", airplay::RAOP_SERVICE_TYPE), "odd");
    }

    fn speaker(protocol: airplay::Protocol, ip: &str) -> Receiver {
        Receiver::from_airplay(&airplay::Device {
            id: "2863813C4503".into(),
            name: "Pi AirPlay".into(),
            model: None,
            address: SocketAddr::new(ip.parse().unwrap(), 7000),
            protocol,
            shows: airplay::Shows::ALL,
        })
    }

    #[test]
    fn a_speaker_announcing_both_protocols_is_listed_once_over_airplay_1() {
        let (inner, _changes) = inner();
        inner.upsert(speaker(airplay::Protocol::AirPlay2, "192.168.3.22"), None);
        inner.upsert(speaker(airplay::Protocol::Raop, "192.168.3.22"), None);
        let listed = || -> Vec<Receiver> {
            lock(&inner.seen)
                .iter()
                .map(|s| s.receiver.clone())
                .collect()
        };
        assert_eq!(listed(), [speaker(airplay::Protocol::Raop, "192.168.3.22")]);
        assert_eq!(listed()[0].id, "airplay:2863813c4503");
        inner.doubt("airplay:2863813c4503");
        inner.upsert(speaker(airplay::Protocol::AirPlay2, "192.168.3.22"), None);
        assert_eq!(
            listed(),
            [speaker(airplay::Protocol::AirPlay2, "192.168.3.22")]
        );
        inner.upsert(speaker(airplay::Protocol::Raop, "192.168.3.22"), None);
        inner.upsert(speaker(airplay::Protocol::AirPlay2, "192.168.3.23"), None);
        assert_eq!(
            listed(),
            [speaker(airplay::Protocol::AirPlay2, "192.168.3.23")]
        );
    }

    #[test]
    fn an_airplay_2_record_lists_a_tv_that_has_no_raop_service() {
        let txt: HashMap<&str, &str> = [
            ("deviceid", "4F:CB:77:B2:06:25"),
            ("features", "0x7F8AD0,0x38BCF46"),
            ("flags", "0x244"),
            ("model", "55U7SE"),
        ]
        .into_iter()
        .collect();
        let tv = receiver_of(
            airplay::AIRPLAY_SERVICE_TYPE,
            "Guest Room TV._airplay._tcp.local.",
            vec!["192.168.3.7".parse().unwrap()],
            7000,
            |key| txt.get(key).copied(),
        )
        .unwrap();
        assert_eq!(tv.id, "airplay:4fcb77b20625");
        assert_eq!(tv.name, "Guest Room TV");
        assert_eq!(tv.kind, ReceiverKind::AirPlay);
        assert_eq!(tv.airplay_protocol(), Some(airplay::Protocol::AirPlay2));
    }
}
