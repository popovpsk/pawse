use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

#[derive(Clone, Debug, Default)]
struct Request {
    method: String,
    path: String,
    params: HashMap<String, String>,
    authorization: String,
    range: Option<String>,
    body: String,
}

type Reply = (u16, &'static str, Vec<u8>);
type Handler = dyn Fn(&Request) -> Reply + Send + Sync;

struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let handler: Arc<Handler> = Arc::new(handler);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut request = Request::default();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        let value = value.trim().to_string();
                        match name.to_ascii_lowercase().as_str() {
                            "range" => request.range = Some(value),
                            "authorization" => request.authorization = value,
                            "content-length" => length = value.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                request.body = String::from_utf8_lossy(&body).into_owned();
                let mut parts = request_line.split_whitespace();
                request.method = parts.next().unwrap_or("").to_string();
                let target = parts.next().unwrap_or("/").to_string();
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                request.path = path.to_string();
                request.params = query
                    .split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .map(|(k, v)| (decode(k), decode(v)))
                    .collect();
                log.lock().unwrap().push(request.clone());
                let (mut status, content_type, mut body) = handler(&request);
                let mut extra = String::new();
                if status == 200
                    && !content_type.contains("norange")
                    && !content_type.contains("json")
                    && let Some((start, end)) = request.range.as_deref().and_then(parse_range)
                {
                    let total = body.len();
                    let end = end.map_or(total, |end| (end + 1).min(total));
                    body = body[start.min(total)..end].to_vec();
                    status = 206;
                    extra = format!("Content-Range: bytes {start}-{}/{total}\r\n", end - 1);
                }
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Self { url, requests }
    }

    fn client(&self) -> Client {
        Client::new(&self.config())
    }

    fn config(&self) -> Config {
        Config {
            url: self.url.clone(),
            user_id: "u1".into(),
            token: "tok".into(),
            device_id: "dev".into(),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b'%' if ix + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[ix + 1..ix + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                ix += 3;
            }
            b'+' => {
                out.push(b' ');
                ix += 1;
            }
            b => {
                out.push(b);
                ix += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

fn parse_range(value: &str) -> Option<(usize, Option<usize>)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()))
}

fn json(value: serde_json::Value) -> Reply {
    (200, "application/json", serde_json::to_vec(&value).unwrap())
}

fn song(id: usize) -> serde_json::Value {
    serde_json::json!({"Id": format!("s{id}"), "Name": format!("Song {id}"), "Type": "Audio"})
}

fn paged(total: usize, request: &Request) -> Reply {
    let start: usize = request.params["StartIndex"].parse().unwrap();
    let limit: usize = request.params["Limit"].parse().unwrap();
    let items: Vec<_> = (start..total.min(start + limit)).map(song).collect();
    json(serde_json::json!({"Items": items, "TotalRecordCount": total}))
}

#[test]
fn authentication_returns_the_token_and_user_and_a_wrong_password_is_auth() {
    let stub = Stub::start(|request| {
        if request.body.contains("\"Pw\":\"right\"") {
            json(serde_json::json!({"AccessToken": "abc", "User": {"Id": "user-7"}}))
        } else {
            (401, "text/plain", b"nope".to_vec())
        }
    });
    let config = authenticate(&stub.url, "me", "right", "device-1").unwrap();
    assert_eq!(config.token, "abc");
    assert_eq!(config.user_id, "user-7");
    assert_eq!(config.device_id, "device-1");
    assert!(!config.url.ends_with('/'));
    assert_eq!(
        authenticate(&stub.url, "me", "wrong", "device-1"),
        Err(Error::Auth)
    );
    let first = &stub.requests()[0];
    assert_eq!(first.method, "POST");
    assert_eq!(first.path, "/Users/AuthenticateByName");
    assert!(first.authorization.starts_with("MediaBrowser "));
    assert!(first.authorization.contains("DeviceId=\"device-1\""));
    assert!(!first.authorization.contains("Token="));
}

#[test]
fn ping_tells_a_foreign_server_from_a_revoked_token() {
    let foreign = Stub::start(|_| (200, "text/html", b"<html>".to_vec()));
    assert!(matches!(foreign.client().ping(), Err(Error::Server(_))));

    let revoked = Stub::start(|request| match request.path.as_str() {
        "/System/Info/Public" => json(serde_json::json!({"Id": "srv"})),
        _ => (401, "text/plain", Vec::new()),
    });
    assert_eq!(revoked.client().ping(), Err(Error::Auth));

    let fine = Stub::start(|request| match request.path.as_str() {
        "/System/Info/Public" => json(serde_json::json!({"Id": "srv"})),
        _ => json(serde_json::json!({"Id": "u1"})),
    });
    fine.client().ping().unwrap();
    let requests = fine.requests();
    assert!(requests[1].authorization.contains("Token=\"tok\""));
    assert!(
        requests
            .iter()
            .all(|r| !r.params.values().any(|v| v == "tok"))
    );

    let down = Stub::start(|_| (503, "text/plain", Vec::new()));
    assert!(matches!(down.client().ping(), Err(Error::Transient(_))));
}

#[test]
fn songs_are_paged_to_the_reported_total() {
    let stub = Stub::start(|request| paged(1203, request));
    let songs = stub.client().songs().unwrap();
    assert_eq!(songs.len(), 1203);
    assert_eq!(songs[1202].id, "s1202");
    let requests = stub.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].params["userId"], "u1");
    assert_eq!(requests[0].params["IncludeItemTypes"], "Audio");
    assert_eq!(requests[0].params["Recursive"], "true");
}

#[test]
fn favorites_ask_the_server_to_filter() {
    let stub = Stub::start(|request| paged(2, request));
    assert_eq!(stub.client().favorites().unwrap().len(), 2);
    assert_eq!(stub.requests()[0].params["Filters"], "IsFavorite");
}

#[test]
fn a_server_that_ignores_the_offset_is_an_error() {
    let stub = Stub::start(|_| {
        json(serde_json::json!({"Items": [song(1), song(2)], "TotalRecordCount": 10}))
    });
    assert!(matches!(stub.client().songs(), Err(Error::Server(_))));
}

#[test]
fn a_short_paged_listing_is_redone_in_one_request() {
    let stub = Stub::start(
        |request| match request.params.get("StartIndex").map(String::as_str) {
            Some("0") => json(serde_json::json!({"Items": [song(1)], "TotalRecordCount": 3})),
            Some(_) => json(serde_json::json!({"Items": [], "TotalRecordCount": 3})),
            None => json(
                serde_json::json!({"Items": [song(1), song(2), song(3)], "TotalRecordCount": 3}),
            ),
        },
    );
    let mut client = stub.client();
    client.page_size = 1;
    let ids: Vec<String> = client.songs().unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["s1", "s2", "s3"]);
    assert!(!stub.requests().last().unwrap().params.contains_key("Limit"));
}

#[test]
fn odd_fields_become_none_and_items_without_an_id_are_skipped() {
    let stub = Stub::start(|_| {
        json(serde_json::json!({"Items": [
            {"Id": "a", "Name": "A", "RunTimeTicks": 1_800_000_000.0, "IndexNumber": "3",
             "ParentIndexNumber": {"x": 1}, "ProductionYear": "1999", "Artists": "oops",
             "Genres": ["Rock", 5], "MediaSources": [{"Size": "1234", "Bitrate": 320000,
             "Container": "flac", "Path": "/m/a.flac"}], "ImageTags": [],
             "AlbumId": "al", "AlbumPrimaryImageTag": "t"},
            {"Name": "no id"},
            {"Id": "b", "Name": 7, "ImageTags": {"Primary": "p"}}
        ], "TotalRecordCount": 3}))
    });
    let songs = stub.client().songs().unwrap();
    assert_eq!(songs.len(), 2);
    let a = &songs[0];
    assert_eq!(a.duration_ms(), Some(180_000));
    assert_eq!(a.index_number, Some(3));
    assert_eq!(a.parent_index_number, None);
    assert_eq!(a.production_year, Some(1999));
    assert!(a.artists.is_empty());
    assert_eq!(a.genres, vec!["Rock".to_string()]);
    assert_eq!(a.size(), Some(1234));
    assert_eq!(a.bitrate(), Some(320_000));
    assert_eq!(a.file_path(), Some("/m/a.flac"));
    assert_eq!(a.cover_key(), Some("al"));
    let b = &songs[1];
    assert_eq!(b.name, "7");
    assert_eq!(b.cover_key(), Some("b"));
}

#[test]
fn ranges_come_from_the_static_stream_and_a_full_reply_is_not_ranged() {
    let bytes: Vec<u8> = (0..=255u8).cycle().take(4000).collect();
    let body = bytes.clone();
    let stub = Stub::start(move |_| (200, "audio/flac", body.clone()));
    let mut range = stub.client().fetch_range("s 1", 100, Some(200)).unwrap();
    assert!(range.ranged);
    assert_eq!((range.offset, range.total), (100, Some(4000)));
    let mut got = Vec::new();
    range.body.read_to_end(&mut got).unwrap();
    assert_eq!(got, bytes[100..200]);
    let request = &stub.requests()[0];
    assert_eq!(request.path, "/Audio/s%201/stream");
    assert_eq!(request.params["static"], "true");
    assert_eq!(request.range.as_deref(), Some("bytes=100-199"));

    let body = bytes.clone();
    let stub = Stub::start(move |_| (200, "audio/flac;norange", body.clone()));
    let range = stub.client().fetch_range("s1", 100, None).unwrap();
    assert!(!range.ranged);
    assert_eq!((range.offset, range.total), (0, Some(4000)));
}

#[test]
fn covers_are_read_whole() {
    let stub = Stub::start(|_| (200, "image/png;norange", vec![7u8; 10]));
    assert_eq!(stub.client().cover_art("al").unwrap(), vec![7u8; 10]);
    let request = &stub.requests()[0];
    assert_eq!(request.path, "/Items/al/Images/Primary");
}
