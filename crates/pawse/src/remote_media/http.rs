use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use media_stream::{Download, FetchError, Fetched, RangeFetch, StreamReader};
use music_library::remote::RemoteRef;

use super::{KeepAlive, PendingStream, SourceMedia, cache::CacheStore};
use crate::servers::{RemoteError, ServerClient};

pub struct HttpMedia {
    client: Arc<dyn ServerClient>,
    cache: Arc<CacheStore>,
}

impl HttpMedia {
    pub fn new(client: Arc<dyn ServerClient>, cache: Arc<CacheStore>) -> Self {
        Self { client, cache }
    }

    fn download(&self, reference: &RemoteRef, dest: &Path) -> Result<Download, String> {
        let fetch = Arc::new(ServerFetch {
            client: self.client.clone(),
            key: reference.key.clone(),
        });
        self.cache
            .downloads()
            .start(dest, fetch, Some(self.cache.trim_on_complete()))
            .map_err(|e| e.to_string())
    }
}

impl SourceMedia for HttpMedia {
    fn ping(&self) -> Result<(), RemoteError> {
        self.client.ping()
    }

    fn open(&self, reference: &RemoteRef, dest: &Path) -> Result<PendingStream, String> {
        let reader = self
            .download(reference, dest)?
            .reader()
            .map_err(|e| e.to_string())?;
        Ok(PendingStream {
            control: Arc::new(reader.abort_handle()),
            stream: Box::new(ReaderStream(reader)),
            extension: reference.suffix.clone(),
        })
    }

    fn fetch_whole(
        &self,
        reference: &RemoteRef,
        dest: &Path,
        abandoned: &dyn Fn() -> bool,
    ) -> Result<PathBuf, String> {
        self.download(reference, dest)?.wait_while(abandoned)
    }

    fn prefetch(&self, reference: &RemoteRef, dest: &Path) -> Result<KeepAlive, String> {
        Ok(Box::new(self.download(reference, dest)?))
    }
}

struct ServerFetch {
    client: Arc<dyn ServerClient>,
    key: String,
}

impl RangeFetch for ServerFetch {
    fn fetch(&self, start: u64, end: Option<u64>) -> Result<Fetched, FetchError> {
        match self.client.fetch_range(&self.key, start, end) {
            Ok(range) => Ok(Fetched {
                body: range.body,
                offset: range.offset,
                total: range.total,
                ranged: range.ranged,
            }),
            Err(RemoteError::Unreachable(message)) => Err(FetchError::Retry(message)),
            Err(RemoteError::Auth) => Err(FetchError::Fatal("wrong username or password".into())),
            Err(RemoteError::Other(message)) => Err(FetchError::Fatal(message)),
        }
    }
}

struct ReaderStream(StreamReader);

impl Read for ReaderStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for ReaderStream {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl audio_engine::MediaStream for ReaderStream {
    fn byte_len(&self) -> Option<u64> {
        self.0.byte_len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use music_library::RemoteSong;

    use super::*;

    struct Scripted {
        replies: Mutex<Vec<Result<Vec<u8>, RemoteError>>>,
    }

    impl ServerClient for Scripted {
        fn ping(&self) -> Result<(), RemoteError> {
            Ok(())
        }
        fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
            Ok(Vec::new())
        }
        fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
            Ok(Vec::new())
        }
        fn cover_art(&self, _: &str) -> Result<Vec<u8>, RemoteError> {
            Ok(Vec::new())
        }
        fn fetch_range(
            &self,
            _: &str,
            start: u64,
            _: Option<u64>,
        ) -> Result<server_http::RangeBody, RemoteError> {
            let bytes = self.replies.lock().unwrap().remove(0)?;
            Ok(server_http::RangeBody {
                total: Some(start + bytes.len() as u64),
                body: Box::new(std::io::Cursor::new(bytes)),
                offset: start,
                ranged: true,
            })
        }
    }

    fn fetch(replies: Vec<Result<Vec<u8>, RemoteError>>) -> ServerFetch {
        ServerFetch {
            client: Arc::new(Scripted {
                replies: Mutex::new(replies),
            }),
            key: "k".into(),
        }
    }

    #[test]
    fn only_an_unreachable_server_is_retried() {
        let unreachable = fetch(vec![Err(RemoteError::Unreachable("down".into()))]);
        assert!(matches!(
            unreachable.fetch(0, None),
            Err(FetchError::Retry(_))
        ));
        let auth = fetch(vec![Err(RemoteError::Auth)]);
        assert!(matches!(auth.fetch(0, None), Err(FetchError::Fatal(_))));
        let other = fetch(vec![Err(RemoteError::Other("404".into()))]);
        assert!(matches!(other.fetch(0, None), Err(FetchError::Fatal(m)) if m == "404"));
    }

    #[test]
    fn a_range_reply_keeps_its_offset_and_total() {
        let ok = fetch(vec![Ok(vec![1, 2, 3])]);
        let mut fetched = ok.fetch(10, Some(13)).unwrap();
        assert_eq!(
            (fetched.offset, fetched.total, fetched.ranged),
            (10, Some(13), true)
        );
        let mut body = Vec::new();
        fetched.body.read_to_end(&mut body).unwrap();
        assert_eq!(body, vec![1, 2, 3]);
    }
}
