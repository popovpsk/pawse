use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use ureq::Agent;
use ureq::RequestBuilder;
use ureq::typestate::WithoutBody;

const USER_AGENT: &str = concat!(
    "Pawse/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/popovpsk/pawse )"
);
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MUSICBRAINZ_GAP: Duration = Duration::from_millis(1100);

static MUSICBRAINZ_NEXT: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request failed: {0}")]
    Transport(String),
    #[error("HTTP {0}")]
    Status(u16),
    #[error("unexpected response: {0}")]
    Parse(String),
}

pub fn musicbrainz_turn() {
    let wait = {
        let mut next = MUSICBRAINZ_NEXT
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let at = next.map_or(now, |at| at.max(now));
        *next = Some(at + MUSICBRAINZ_GAP);
        at - now
    };
    std::thread::sleep(wait);
}

pub fn agent() -> Agent {
    Agent::new_with_config(
        Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .build(),
    )
}

pub fn get_text(request: RequestBuilder<WithoutBody>) -> Result<String, Error> {
    let mut response = request
        .call()
        .map_err(|e| Error::Transport(e.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Transport(e.to_string()))
}

pub fn get_bytes(agent: &Agent, url: &str) -> Result<Vec<u8>, Error> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| Error::Transport(e.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    response
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES)
        .read_to_vec()
        .map_err(|e| Error::Transport(e.to_string()))
}
