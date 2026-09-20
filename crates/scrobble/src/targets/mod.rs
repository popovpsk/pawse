pub mod audioscrobbler;
pub mod csv_log;
pub mod listenbrainz;

use std::time::Duration;

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .timeout_recv_body(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build(),
    )
}

pub(crate) fn read(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, String), String> {
    match result {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            match resp.body_mut().read_to_string() {
                Ok(body) => Ok((status, body)),
                Err(e) => Err(format!("read response: {e}")),
            }
        }
        Err(e) => Err(format!("request failed: {e}")),
    }
}
