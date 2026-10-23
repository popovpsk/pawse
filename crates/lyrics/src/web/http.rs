use anyhow::{Context as _, Result};
use std::time::{Duration, Instant};
use ureq::http::Response;
use ureq::typestate::WithoutBody;
use ureq::{Body, RequestBuilder};

const USER_AGENT: &str = "pawse music player (https://github.com/popovpsk/pawse)";
const TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_BACKOFF_MS: u64 = 250;
const RETRY_AFTER_CAP_SECS: u64 = 10;

pub(super) struct Session {
    agent: ureq::Agent,
    deadline: Instant,
}

impl Session {
    pub(super) fn new(budget: Duration) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(TIMEOUT))
                .timeout_recv_response(Some(TIMEOUT))
                .timeout_recv_body(Some(TIMEOUT))
                .http_status_as_error(false)
                .build(),
        );
        Self {
            agent,
            deadline: Instant::now() + budget,
        }
    }

    pub(super) fn expired(&self) -> bool {
        self.remaining().is_zero()
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub(super) fn get(&self, url: &str) -> RequestBuilder<WithoutBody> {
        self.agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_global(Some(self.remaining()))
            .build()
    }

    pub(super) fn call(
        &self,
        attempts: u32,
        send: impl Fn() -> Result<Response<Body>, ureq::Error>,
    ) -> Result<Response<Body>, ureq::Error> {
        let mut attempt = 0u32;
        loop {
            if self.expired() {
                return Err(ureq::Error::Timeout(ureq::Timeout::Global));
            }
            let result = send();
            attempt += 1;
            if attempt >= attempts {
                return result;
            }
            let delay = match &result {
                Ok(response) if retryable_status(response.status().as_u16()) => {
                    let retry_after = (response.status().as_u16() == 429)
                        .then(|| retry_after_delay(header_str(response, "retry-after")))
                        .flatten();
                    retry_after.unwrap_or(Duration::from_millis(RETRY_BACKOFF_MS))
                }
                Err(e) if is_retryable(e) => Duration::from_millis(RETRY_BACKOFF_MS),
                _ => return result,
            };
            if delay >= self.remaining() {
                return result;
            }
            std::thread::sleep(delay);
        }
    }
}

pub(super) fn body(response: &mut Response<Body>, what: &str) -> Result<String> {
    response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading the {what} response"))
}

fn header_str<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name).and_then(|v| v.to_str().ok())
}

fn is_retryable(e: &ureq::Error) -> bool {
    matches!(
        e,
        ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::Protocol(_)
    )
}

fn retryable_status(code: u16) -> bool {
    code == 429 || code >= 500
}

fn retry_after_delay(header: Option<&str>) -> Option<Duration> {
    header
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|secs| Duration::from_secs(secs.min(RETRY_AFTER_CAP_SECS)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempts_made(session: &Session, attempts: u32) -> u32 {
        let made = std::cell::Cell::new(0);
        let result = session.call(attempts, || {
            made.set(made.get() + 1);
            Err(ureq::Error::ConnectionFailed)
        });
        assert!(result.is_err());
        made.get()
    }

    #[test]
    fn retries_stop_at_the_deadline() {
        assert_eq!(attempts_made(&Session::new(Duration::from_secs(30)), 2), 2);
        assert_eq!(
            attempts_made(&Session::new(Duration::from_millis(100)), 3),
            1
        );
        assert_eq!(attempts_made(&Session::new(Duration::ZERO), 3), 0);
    }

    #[test]
    fn retryable_status_classification() {
        assert!(!retryable_status(404));
        assert!(!retryable_status(400));
        assert!(!retryable_status(200));
        assert!(retryable_status(429));
        assert!(retryable_status(500));
        assert!(retryable_status(503));
    }

    #[test]
    fn retry_after_delay_parses_clamps_and_rejects() {
        assert_eq!(retry_after_delay(None), None);
        assert_eq!(retry_after_delay(Some("nope")), None);
        assert_eq!(retry_after_delay(Some(" 3 ")), Some(Duration::from_secs(3)));
        assert_eq!(
            retry_after_delay(Some("999")),
            Some(Duration::from_secs(10))
        );
    }
}
