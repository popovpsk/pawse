use std::sync::Arc;

mod airplay_output;
mod chromecast_driver;
mod dacp;
mod discovery;
mod dlna_driver;
mod flac;
mod media;
mod net;
mod pcm;
mod server;
mod session;
mod unicast_mdns;

pub use airplay::RemoteCommand;
pub use airplay_output::AirPlayOutput;
pub use discovery::{Discovery, Receiver, ReceiverKind};
pub use media::{Cover, Media, Source, StreamOpener, TrackInfo};
pub use server::MediaServer;
pub use session::{Load, Session, SessionEvent};

use discovery::Endpoint;

pub fn connect(receiver: &Receiver, server: Arc<MediaServer>) -> Result<Session, String> {
    let driver: Box<dyn session::Driver> = match &receiver.endpoint {
        Endpoint::Dlna(location) => Box::new(dlna_driver::DlnaDriver::connect(location)?),
        Endpoint::Chromecast(address) => {
            Box::new(chromecast_driver::ChromecastDriver::connect(*address)?)
        }
        Endpoint::AirPlay(_) => {
            return Err("AirPlay speakers play through the audio engine".into());
        }
    };
    Ok(Session::start(driver, server))
}

#[cfg(test)]
mod tests;
