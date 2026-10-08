use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::EmbedError;

pub struct ModelSpec<'a> {
    pub url: &'a str,
    pub file_name: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

pub const MODEL: ModelSpec<'static> = ModelSpec {
    url: "https://github.com/popovpsk/pawse-models/releases/download/effnet-multi-1/discogs_multi_embeddings-effnet-bs64-1.onnx",
    file_name: "discogs_multi_embeddings-effnet-bs64-1.onnx",
    size: 15_998_047,
    sha256: "65cfde30655a939de420e5c09a49a43648336fdbbf79eb3af6d3b40176339d8e",
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const BODY_TIMEOUT: Duration = Duration::from_secs(10 * 60);

static DOWNLOADING: Mutex<()> = Mutex::new(());

pub fn ensure(dir: &Path, cancelled: &dyn Fn() -> bool) -> Result<PathBuf, EmbedError> {
    ensure_spec(dir, &MODEL, cancelled)
}

pub fn discard_if_corrupt(dir: &Path) -> Result<bool, EmbedError> {
    discard_if_corrupt_spec(dir, &MODEL)
}

fn discard_if_corrupt_spec(dir: &Path, spec: &ModelSpec) -> Result<bool, EmbedError> {
    let path = dir.join(spec.file_name);
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(&path)?, &mut hasher)?;
    if format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(spec.sha256) {
        return Ok(false);
    }
    std::fs::remove_file(&path)?;
    Ok(true)
}

fn ensure_spec(
    dir: &Path,
    spec: &ModelSpec,
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf, EmbedError> {
    let _one_at_a_time = DOWNLOADING.lock().unwrap_or_else(PoisonError::into_inner);
    let path = dir.join(spec.file_name);
    if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == spec.size) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{}.partial", spec.file_name));
    log::info!("audio_embedding: downloading {}", spec.url);
    let fetched = download(spec, &partial, cancelled);
    let digest = match fetched {
        Ok(digest) => digest,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    if !digest.eq_ignore_ascii_case(spec.sha256) {
        let _ = std::fs::remove_file(&partial);
        log::warn!(
            "audio_embedding: {} has SHA-256 {digest}, expected {}",
            spec.file_name,
            spec.sha256
        );
        return Err(EmbedError::Checksum);
    }
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

fn download(
    spec: &ModelSpec,
    dest: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String, EmbedError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .build(),
    );
    let response = agent
        .get(spec.url)
        .header("User-Agent", "pawse")
        .call()
        .map_err(|e| EmbedError::Network(e.to_string()))?;
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(spec.size + 1)
        .reader();
    let mut file = File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if cancelled() {
            return Err(EmbedError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(EmbedError::Network(e.to_string())),
        };
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
    }
    file.sync_all()?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Server {
        url: String,
        hits: Arc<AtomicUsize>,
    }

    fn serve(body: Vec<u8>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.onnx", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Server { url, hits }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pawse-audio-embedding-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn spec<'a>(server: &'a Server, body: &[u8], sha256: &'a str) -> ModelSpec<'a> {
        ModelSpec {
            url: &server.url,
            file_name: "model.onnx",
            size: body.len() as u64,
            sha256,
        }
    }

    fn sha_of(body: &[u8]) -> String {
        format!("{:x}", Sha256::digest(body))
    }

    #[test]
    fn a_verified_download_lands_under_its_name() {
        let body = b"model weights".repeat(1000);
        let server = serve(body.clone());
        let dir = temp_dir("ok");
        let sha = sha_of(&body);
        let spec = spec(&server, &body, &sha);

        let path = ensure_spec(&dir, &spec, &|| false).unwrap();

        assert_eq!(path, dir.join("model.onnx"));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert!(!dir.join("model.onnx.partial").exists());
        assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_file_of_the_right_size_is_not_fetched_again() {
        let body = b"cached".to_vec();
        let server = serve(body.clone());
        let dir = temp_dir("present");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model.onnx"), &body).unwrap();

        let path = ensure_spec(&dir, &spec(&server, &body, &sha_of(&body)), &|| false).unwrap();

        assert_eq!(path, dir.join("model.onnx"));
        assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_wrong_checksum_leaves_nothing_behind() {
        let body = b"tampered".to_vec();
        let server = serve(body.clone());
        let dir = temp_dir("bad");
        let sha = sha_of(b"genuine");
        let spec = spec(&server, &body, &sha);

        let err = ensure_spec(&dir, &spec, &|| false).unwrap_err();

        assert!(matches!(err, EmbedError::Checksum), "{err}");
        assert!(!dir.join("model.onnx").exists());
        assert!(!dir.join("model.onnx.partial").exists());
    }

    #[test]
    fn a_truncated_file_is_replaced_atomically() {
        let body = b"complete weights".to_vec();
        let server = serve(body.clone());
        let dir = temp_dir("truncated");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model.onnx"), b"comp").unwrap();

        let path = ensure_spec(&dir, &spec(&server, &body, &sha_of(&body)), &|| false).unwrap();

        assert_eq!(std::fs::read(path).unwrap(), body);
        assert!(!dir.join("model.onnx.partial").exists());
    }

    #[test]
    fn an_unreachable_server_is_a_network_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.onnx", listener.local_addr().unwrap());
        drop(listener);
        let dir = temp_dir("offline");
        let spec = ModelSpec {
            url: &url,
            file_name: "model.onnx",
            size: 1,
            sha256: "00",
        };

        let err = ensure_spec(&dir, &spec, &|| false).unwrap_err();

        assert!(matches!(err, EmbedError::Network(_)), "{err}");
        assert!(!dir.join("model.onnx.partial").exists());
    }

    #[test]
    fn a_cancelled_download_leaves_nothing_behind() {
        let body = b"weights".repeat(100_000);
        let server = serve(body.clone());
        let dir = temp_dir("cancelled");
        let sha = sha_of(&body);

        let err = ensure_spec(&dir, &spec(&server, &body, &sha), &|| true).unwrap_err();

        assert!(matches!(err, EmbedError::Cancelled), "{err}");
        assert!(!dir.join("model.onnx").exists());
        assert!(!dir.join("model.onnx.partial").exists());
    }

    #[test]
    fn a_corrupt_file_of_the_right_size_is_discarded_and_a_genuine_one_kept() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.onnx");
        let sha = sha_of(b"genuine");
        let spec = ModelSpec {
            url: "http://127.0.0.1:9/unused",
            file_name: "model.onnx",
            size: 7,
            sha256: &sha,
        };

        std::fs::write(&path, b"genuine").unwrap();
        assert!(!discard_if_corrupt_spec(&dir, &spec).unwrap());
        assert!(path.exists());

        std::fs::write(&path, b"garbage").unwrap();
        assert!(discard_if_corrupt_spec(&dir, &spec).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn the_published_model_is_pinned() {
        assert!(MODEL.url.ends_with(MODEL.file_name));
        assert!(
            MODEL
                .url
                .starts_with("https://github.com/popovpsk/pawse-models/")
        );
        assert_eq!(MODEL.sha256.len(), 64);
        assert!(crate::EMBEDDING_VERSION.contains(&MODEL.sha256[..8]));
    }
}
