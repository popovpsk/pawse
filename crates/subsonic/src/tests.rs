use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

type Params = HashMap<String, String>;
type Handler = dyn Fn(&str, &Params) -> (u16, &'static str, Vec<u8>) + Send + Sync;

struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<(String, Params)>>>,
}

impl Stub {
    fn start(
        handler: impl Fn(&str, &Params) -> (u16, &'static str, Vec<u8>) + Send + Sync + 'static,
    ) -> Self {
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
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("range")
                    {
                        range = Some(value.trim().to_string());
                    }
                }
                let target = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let method = path.rsplit('/').next().unwrap_or("").to_string();
                let mut params: Params = query
                    .split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .map(|(k, v)| (decode(k), decode(v)))
                    .collect();
                if let Some(range) = &range {
                    params.insert("Range".into(), range.clone());
                }
                log.lock().unwrap().push((method.clone(), params.clone()));
                let (mut status, content_type, mut body) = handler(&method, &params);
                let mut extra = String::new();
                if status == 200
                    && !content_type.contains("norange")
                    && !content_type.contains("json")
                    && let Some((start, end)) = range.as_deref().and_then(parse_range)
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

    fn client(&self, password: &str) -> Client {
        Client::new(&Config {
            url: self.url.clone(),
            username: "me".into(),
            password: password.into(),
        })
    }

    fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
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

fn ok(body: serde_json::Value) -> (u16, &'static str, Vec<u8>) {
    let mut inner = serde_json::json!({"status": "ok", "version": "1.16.1"});
    if let (Some(target), Some(extra)) = (inner.as_object_mut(), body.as_object()) {
        for (k, v) in extra {
            target.insert(k.clone(), v.clone());
        }
    }
    (
        200,
        "application/json",
        serde_json::to_vec(&serde_json::json!({ "subsonic-response": inner })).unwrap(),
    )
}

fn failed(code: i64) -> (u16, &'static str, Vec<u8>) {
    (
        200,
        "application/json",
        serde_json::to_vec(&serde_json::json!({"subsonic-response": {
            "status": "failed", "version": "1.16.1",
            "error": {"code": code, "message": "nope"}
        }}))
        .unwrap(),
    )
}

fn song(id: usize) -> serde_json::Value {
    serde_json::json!({"id": id.to_string(), "title": format!("Song {id}"), "artist": "A", "duration": 100})
}

fn authorized(params: &Params, password: &str) -> bool {
    match (params.get("t"), params.get("s"), params.get("p")) {
        (Some(t), Some(s), _) => *t == hex(&Md5::digest(format!("{password}{s}").as_bytes())),
        (_, _, Some(p)) => *p == format!("enc:{}", hex(password.as_bytes())),
        _ => false,
    }
}

#[test]
fn ping_signs_with_a_salted_token() {
    let stub = Stub::start(|_, params| {
        if authorized(params, "secret") {
            ok(serde_json::json!({}))
        } else {
            failed(40)
        }
    });
    assert_eq!(stub.client("secret").ping(), Ok(()));
    assert_eq!(stub.client("wrong").ping(), Err(Error::Auth));
    let requests = stub.requests.lock().unwrap();
    assert_ne!(requests[0].1["s"], requests[1].1["s"]);
    assert!(!requests[0].1.contains_key("p"));
}

#[test]
fn a_server_without_token_auth_gets_the_encoded_password_instead() {
    let stub = Stub::start(|_, params| {
        if params.contains_key("t") {
            failed(41)
        } else if authorized(params, "pässword") {
            ok(serde_json::json!({}))
        } else {
            failed(40)
        }
    });
    let client = stub.client("pässword");
    assert_eq!(client.ping(), Ok(()));
    assert_eq!(client.ping(), Ok(()));
    assert_eq!(stub.methods().len(), 3);
}

#[test]
fn an_unreachable_server_is_transient_not_an_auth_failure() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let client = Client::new(&Config {
        url: format!("http://127.0.0.1:{port}"),
        username: "me".into(),
        password: "x".into(),
    });
    assert!(matches!(client.ping(), Err(Error::Transient(_))));
}

#[test]
fn a_page_that_is_not_subsonic_is_a_server_error() {
    let stub = Stub::start(|_, _| (200, "application/json", b"{\"hello\":1}".to_vec()));
    assert!(matches!(stub.client("x").ping(), Err(Error::Server(_))));
}

#[test]
fn songs_page_through_search3_until_an_empty_page() {
    let stub = Stub::start(|method, params| {
        assert_eq!(method, "search3");
        let offset: usize = params["songOffset"].parse().unwrap();
        let ids: Vec<_> = (offset..(offset + 2).min(5)).map(song).collect();
        ok(serde_json::json!({"searchResult3": {"song": ids}}))
    });
    let mut client = stub.client("x");
    client.page_size = 2;
    let songs = client.songs().unwrap();
    assert_eq!(
        songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["0", "1", "2", "3", "4"]
    );
}

#[test]
fn an_empty_search_falls_back_to_walking_albums_and_accepts_numeric_ids() {
    let stub = Stub::start(|method, params| match method {
        "search3" => ok(serde_json::json!({"searchResult3": {}})),
        "getAlbumList2" if params["offset"] != "0" => ok(serde_json::json!({"albumList2": {}})),
        "getAlbumList2" => {
            ok(serde_json::json!({"albumList2": {"album": [{"id": 7}, {"id": "8"}]}}))
        }
        "getAlbum" => {
            let base = if params["id"] == "7" { 10 } else { 20 };
            ok(
                serde_json::json!({"album": {"song": [song(base), {"id": base + 1, "title": "N", "coverArt": 99}]}}),
            )
        }
        _ => failed(0),
    });
    let songs = stub.client("x").songs().unwrap();
    assert_eq!(
        songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["10", "11", "20", "21"]
    );
    assert_eq!(songs[1].cover_art.as_deref(), Some("99"));
}

#[test]
fn a_failure_mid_listing_fails_the_whole_listing() {
    let stub = Stub::start(|method, params| match method {
        "search3" if params["songOffset"] == "0" => {
            ok(serde_json::json!({"searchResult3": {"song": [song(0), song(1)]}}))
        }
        _ => (503, "text/plain", b"down".to_vec()),
    });
    let mut client = stub.client("x");
    client.page_size = 2;
    assert!(matches!(client.songs(), Err(Error::Transient(_))));
}

fn parse_range(value: &str) -> Option<(usize, Option<usize>)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()))
}

fn audio(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 253) as u8).collect()
}

fn read_all(mut range: RangeBody) -> Vec<u8> {
    let mut bytes = Vec::new();
    range.body.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_ranged_download_reports_where_it_starts_and_how_long_the_file_is() {
    let stub = Stub::start(|method, _| match method {
        "download" => (200, "audio/flac", audio(100_000)),
        _ => failed(70),
    });
    let client = stub.client("x");
    let part = client.fetch_range("1", 1000, Some(3000)).unwrap();
    assert!(part.ranged);
    assert_eq!(part.offset, 1000);
    assert_eq!(part.total, Some(100_000));
    assert_eq!(read_all(part), audio(100_000)[1000..3000]);

    let tail = client.fetch_range("1", 99_000, None).unwrap();
    assert_eq!(tail.offset, 99_000);
    assert_eq!(read_all(tail), audio(100_000)[99_000..]);

    let requests = stub.requests.lock().unwrap();
    assert_eq!(requests[0].1["Range"], "bytes=1000-2999");
    assert_eq!(requests[0].1["id"], "1");
    assert_eq!(requests[1].1["Range"], "bytes=99000-");
}

#[test]
fn a_server_without_range_support_sends_the_whole_file_from_the_start() {
    let stub = Stub::start(|method, params| match method {
        "download" if params.contains_key("Range") => (200, "audio/flac; norange", audio(5000)),
        _ => failed(70),
    });
    let part = stub.client("x").fetch_range("1", 1000, Some(2000)).unwrap();
    assert!(!part.ranged);
    assert_eq!(part.offset, 0);
    assert_eq!(part.total, Some(5000));
    assert_eq!(read_all(part), audio(5000));
}

#[test]
fn a_ranged_download_with_an_error_reply_is_an_error() {
    let stub = Stub::start(|_, _| failed(70));
    assert!(matches!(
        stub.client("x").fetch_range("missing", 0, Some(10)),
        Err(Error::Server(_))
    ));
    let stub = Stub::start(|_, _| failed(40));
    assert!(matches!(
        stub.client("x").fetch_range("1", 0, Some(10)),
        Err(Error::Auth)
    ));
}

#[test]
fn starred_songs_and_cover_art() {
    let stub = Stub::start(|method, _| match method {
        "getStarred2" => ok(serde_json::json!({"starred2": {"song": [song(3)]}})),
        "getCoverArt" => (200, "image/jpeg", vec![1, 2, 3]),
        _ => failed(70),
    });
    let client = stub.client("x");
    assert_eq!(client.starred_songs().unwrap()[0].id, "3");
    assert_eq!(client.cover_art("c").unwrap(), vec![1, 2, 3]);
}

#[test]
fn a_server_that_clamps_the_page_size_is_still_read_to_the_end() {
    let stub = Stub::start(|_, params| {
        let offset: usize = params["songOffset"].parse().unwrap();
        let ids: Vec<_> = (offset..(offset + 3).min(7)).map(song).collect();
        ok(serde_json::json!({"searchResult3": {"song": ids}}))
    });
    assert_eq!(stub.client("x").songs().unwrap().len(), 7);
}

#[test]
fn a_server_that_ignores_the_offset_falls_back_to_the_album_walk() {
    let stub = Stub::start(|method, params| match method {
        "search3" => ok(serde_json::json!({"searchResult3": {"song": [song(1), song(2)]}})),
        "getAlbumList2" if params["offset"] != "0" => ok(serde_json::json!({"albumList2": {}})),
        "getAlbumList2" => ok(serde_json::json!({"albumList2": {"album": [{"id": "a"}]}})),
        "getAlbum" => ok(serde_json::json!({"album": {"song": [song(1), song(2), song(3)]}})),
        _ => failed(0),
    });
    let mut client = stub.client("x");
    client.page_size = 2;
    assert_eq!(client.songs().unwrap().len(), 3);
}

#[test]
fn odd_field_types_do_not_sink_the_listing() {
    let stub = Stub::start(|_, params| {
        if params["songOffset"] != "0" {
            return ok(serde_json::json!({"searchResult3": {}}));
        }
        ok(serde_json::json!({"searchResult3": {"song": [
            {"id": "1", "title": 42, "duration": 181.6, "year": "1999", "track": -1,
             "artists": "not a list", "size": null}
        ]}}))
    });
    let songs = stub.client("x").songs().unwrap();
    assert_eq!(songs[0].title, "42");
    assert_eq!(songs[0].duration, Some(182));
    assert_eq!(songs[0].year, Some(1999));
    assert_eq!(songs[0].track, None);
    assert!(songs[0].artists.is_empty());
}
