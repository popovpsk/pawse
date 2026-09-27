use std::io::Read;
use std::time::Duration;

use ureq::http::Response;
use ureq::typestate::WithoutBody;
use ureq::{Body, RequestBuilder};

pub mod lenient;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
const BODY_TIMEOUT: Duration = Duration::from_secs(600);
const RANGE_TIMEOUT: Duration = Duration::from_secs(15);
const RANGE_BODY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct RangeBody {
    pub body: Box<dyn Read + Send>,
    pub offset: u64,
    pub total: Option<u64>,
    pub ranged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Success,
    Auth,
    Transient,
    Failed,
}

pub fn classify(status: u16) -> Status {
    match status {
        401 | 403 => Status::Auth,
        500.. => Status::Transient,
        200..=299 => Status::Success,
        _ => Status::Failed,
    }
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .http_status_as_error(false)
            .build(),
    )
}

pub fn range_header(start: u64, end: Option<u64>) -> String {
    match end {
        Some(end) => format!("bytes={start}-{}", end.saturating_sub(1)),
        None => format!("bytes={start}-"),
    }
}

pub fn with_range(
    request: RequestBuilder<WithoutBody>,
    range: &str,
) -> RequestBuilder<WithoutBody> {
    let config = request
        .header("Range", range)
        .config()
        .timeout_recv_response(Some(RANGE_TIMEOUT));
    if range.ends_with('-') {
        config.build()
    } else {
        config.timeout_recv_body(Some(RANGE_BODY_TIMEOUT)).build()
    }
}

pub fn range_body(response: Response<Body>) -> Result<RangeBody, String> {
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let length = header("content-length").and_then(|value| value.trim().parse().ok());
    if status == 206 {
        let (offset, total) = header("content-range")
            .as_deref()
            .and_then(parse_content_range)
            .ok_or_else(|| "malformed Content-Range".to_string())?;
        return Ok(RangeBody {
            body: Box::new(response.into_body().into_reader()),
            offset,
            total,
            ranged: true,
        });
    }
    Ok(RangeBody {
        body: Box::new(response.into_body().into_reader()),
        offset: 0,
        total: length,
        ranged: false,
    })
}

pub fn read_capped(body: Body, cap: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    body.into_reader().take(cap).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn is_json(response: &Response<Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("json"))
}

pub fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let spec = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = spec.split_once('/')?;
    let (first, _) = span.split_once('-')?;
    let total = match total.trim() {
        "*" => None,
        text => Some(text.parse().ok()?),
    };
    Some((first.trim().parse().ok()?, total))
}

pub fn redact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('?') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
