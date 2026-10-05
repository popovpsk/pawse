use std::sync::atomic::{AtomicBool, Ordering};

use ureq::tls::{RootCerts, TlsConfig};

use crate::agent_with;

pub struct Transport {
    bundled: ureq::Agent,
    system: ureq::Agent,
    on_system: AtomicBool,
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport {
    pub fn new() -> Self {
        Self {
            bundled: agent_with(TlsConfig::default()),
            system: agent_with(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            ),
            on_system: AtomicBool::new(false),
        }
    }

    pub fn call<T>(
        &self,
        send: impl Fn(&ureq::Agent) -> Result<T, ureq::Error>,
    ) -> Result<T, ureq::Error> {
        if self.on_system.load(Ordering::Relaxed) {
            return send(&self.system);
        }
        match send(&self.bundled) {
            Err(first) if untrusted_issuer(&first) => match send(&self.system) {
                Ok(value) => {
                    log::info!(
                        "server certificate is signed by a CA outside the bundled roots; \
                         using the system trust store from now on"
                    );
                    self.on_system.store(true, Ordering::Relaxed);
                    Ok(value)
                }
                Err(second) if tls_failure(&second) => {
                    log::warn!(
                        "server certificate is not trusted by the system store either: {second}"
                    );
                    Err(ureq::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{} (system store: {})", reason(&first), reason(&second)),
                    )))
                }
                Err(second) => Err(second),
            },
            other => other,
        }
    }
}

fn tls_error(error: &ureq::Error) -> Option<&rustls::Error> {
    match error {
        ureq::Error::Io(io) => io.get_ref()?.downcast_ref::<rustls::Error>(),
        _ => None,
    }
}

fn reason(error: &ureq::Error) -> String {
    match tls_error(error) {
        Some(tls) => tls.to_string(),
        None => error.to_string(),
    }
}

fn tls_failure(error: &ureq::Error) -> bool {
    tls_error(error).is_some()
}

fn untrusted_issuer(error: &ureq::Error) -> bool {
    matches!(
        tls_error(error),
        Some(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer
        ))
    )
}
