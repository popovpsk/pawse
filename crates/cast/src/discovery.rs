use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

const SSDP_TIMEOUT: Duration = Duration::from_secs(3);
const SSDP_EVERY: Duration = Duration::from_secs(60);
const SSDP_FORGET_AFTER: Duration = Duration::from_secs(200);

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
    daemon: Option<ServiceDaemon>,
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
        let daemon = match ServiceDaemon::new() {
            Ok(daemon) => {
                browse(daemon.clone(), inner.clone(), rebrowses);
                Some(daemon)
            }
            Err(e) => {
                log::warn!("cast: mDNS discovery did not start: {e}");
                None
            }
        };
        Self {
            inner,
            changes,
            refresh,
            rebrowse,
            daemon,
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
        if let Some(daemon) = self.daemon.take() {
            let _ = daemon.shutdown();
        }
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
    Closed,
}

fn start_browsing(daemon: &ServiceDaemon) -> Option<[mdns_sd::Receiver<ServiceEvent>; 2]> {
    let chromecasts = daemon
        .browse(chromecast::SERVICE_TYPE)
        .inspect_err(|e| log::warn!("cast: browsing for Chromecasts failed: {e}"))
        .ok()?;
    let speakers = daemon
        .browse(airplay::SERVICE_TYPE)
        .inspect_err(|e| log::warn!("cast: browsing for AirPlay failed: {e}"))
        .ok()?;
    Some([chromecasts, speakers])
}

fn browse(daemon: ServiceDaemon, inner: Arc<Inner>, rebrowses: flume::Receiver<()>) {
    let Some(mut browsers) = start_browsing(&daemon) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("cast-mdns".into())
        .spawn(move || {
            loop {
                let wake = flume::Selector::new()
                    .recv(&browsers[0], |event| {
                        event.map_or(Wake::Closed, Wake::Event)
                    })
                    .recv(&browsers[1], |event| {
                        event.map_or(Wake::Closed, Wake::Event)
                    })
                    .recv(&rebrowses, |asked| {
                        asked.map_or(Wake::Closed, |()| Wake::Rebrowse)
                    })
                    .wait();
                if inner.stopped.load(Ordering::Acquire) {
                    return;
                }
                match wake {
                    Wake::Event(event) => handle(&inner, event),
                    Wake::Rebrowse => match start_browsing(&daemon) {
                        Some(fresh) => browsers = fresh,
                        None => return,
                    },
                    Wake::Closed => return,
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("cast: mDNS thread did not start: {e}");
    }
}

fn instance_name<'a>(fullname: &'a str, service_type: &str) -> &'a str {
    fullname
        .strip_suffix(service_type)
        .map(|name| name.trim_end_matches('.'))
        .unwrap_or(fullname)
}

fn handle(inner: &Inner, event: ServiceEvent) {
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
            if let Some(receiver) = receiver {
                inner.upsert(receiver, Some(service.fullname.clone()));
            }
        }
        ServiceEvent::ServiceRemoved(_, fullname) => inner.remove_service(&fullname),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
