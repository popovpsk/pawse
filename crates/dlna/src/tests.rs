use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

const UDN: &str = "uuid:4d696e69-444c-164e-9d41-000000000001";

#[derive(Debug, Clone, Default)]
struct Request {
    method: String,
    path: String,
    action: Option<String>,
    body: String,
    range: Option<String>,
    features: bool,
}

type Reply = (u16, &'static str, Vec<u8>);
type Handler = dyn Fn(&Request, &str) -> Reply + Send + Sync;

struct Stub {
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    fn start(handler: impl Fn(&Request, &str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let handler: Arc<Handler> = Arc::new(handler);
        let for_thread = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let mut request = Request {
                    method: parts.next().unwrap_or("").to_string(),
                    path: parts.next().unwrap_or("/").to_string(),
                    ..Request::default()
                };
                let mut length = 0usize;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err()
                        || header == "\r\n"
                        || header.is_empty()
                    {
                        break;
                    }
                    let Some((name, value)) = header.split_once(':') else {
                        continue;
                    };
                    let value = value.trim();
                    match name.to_ascii_lowercase().as_str() {
                        "content-length" => length = value.parse().unwrap_or(0),
                        "soapaction" => {
                            request.action = value
                                .trim_matches('"')
                                .rsplit('#')
                                .next()
                                .map(str::to_string)
                        }
                        "range" => request.range = Some(value.to_string()),
                        "getcontentfeatures.dlna.org" => request.features = true,
                        _ => {}
                    }
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
                request.body = String::from_utf8_lossy(&body).into_owned();
                log.lock().unwrap().push(request.clone());
                let (mut status, content_type, mut body) = handler(&request, &for_thread);
                let mut extra = String::new();
                if status == 200
                    && let Some(start) = request
                        .range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.split('-').next())
                        .and_then(|s| s.parse::<usize>().ok())
                {
                    let total = body.len();
                    body = body[start.min(total)..].to_vec();
                    status = 206;
                    extra = format!("Content-Range: bytes {start}-{}/{total}\r\n", total - 1);
                }
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Self { base, requests }
    }

    fn location(&self) -> String {
        format!("{}/rootDesc.xml", self.base)
    }

    fn client(&self) -> Client {
        Client::new(&Config {
            udn: UDN.into(),
            location: self.location(),
        })
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn actions(&self) -> Vec<(String, Option<String>, Option<String>)> {
        self.requests()
            .into_iter()
            .filter_map(|r| {
                let action = r.action?;
                let id = arg(&r.body, "ObjectID");
                let start = arg(&r.body, "StartingIndex");
                Some((action, id, start))
            })
            .collect()
    }
}

fn arg(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = body.find(&open)? + open.len();
    let end = start + body[start..].find(&format!("</{name}>"))?;
    Some(body[start..end].to_string())
}

fn description() -> Reply {
    let text = format!(
        r#"<?xml version="1.0"?><root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
<friendlyName>pi: minidlna</friendlyName><modelName>Windows Media Connect compatible (MiniDLNA)</modelName>
<UDN>{UDN}</UDN><serviceList><service>
<serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
<controlURL>/ctl/ContentDir</controlURL></service></serviceList></device></root>"#
    );
    (200, "text/xml", text.into_bytes())
}

fn soap(action: &str, args: &[(&str, &str)]) -> Reply {
    let args: String = args
        .iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", xml::escape(value)))
        .collect();
    let text = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">{args}</u:{action}Response></s:Body></s:Envelope>"#
    );
    (200, "text/xml", text.into_bytes())
}

fn upnp_error(code: u32) -> Reply {
    let text = format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>Nope</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
    );
    (500, "text/xml", text.into_bytes())
}

fn listing(action: &str, entries: &[String], total: usize) -> Reply {
    let didl = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">{}</DIDL-Lite>"#,
        entries.concat()
    );
    soap(
        action,
        &[
            ("Result", &didl),
            ("NumberReturned", &entries.len().to_string()),
            ("TotalMatches", &total.to_string()),
            ("UpdateID", "1"),
        ],
    )
}

fn track(base: &str, id: &str, file: &str) -> String {
    format!(
        r#"<item id="{id}" parentID="1" restricted="1"><dc:title>Track {id}</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><upnp:artist>Artist</upnp:artist><upnp:album>Album</upnp:album><res size="1000" duration="0:03:00.000" protocolInfo="http-get:*:audio/x-flac:*">{base}/MediaItems/{file}</res></item>"#
    )
}

fn folder(id: &str) -> String {
    format!(
        r#"<container id="{id}" parentID="0" restricted="1"><dc:title>{id}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>"#
    )
}

fn keys(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.pick().unwrap().key.clone())
        .collect()
}

#[test]
fn search_is_paged_until_the_total_and_keys_drop_the_host() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => soap(
            "GetSearchCapabilities",
            &[(
                "SearchCaps",
                "dc:creator,dc:date,dc:title,upnp:album,upnp:class",
            )],
        ),
        Some("Search") => match arg(&request.body, "StartingIndex").as_deref() {
            Some("0") => listing(
                "Search",
                &[track(base, "a", "1.flac"), track(base, "b", "2.flac")],
                3,
            ),
            Some("2") => listing("Search", &[track(base, "c", "3.flac")], 3),
            _ => listing("Search", &[], 3),
        },
        Some(_) => upnp_error(401),
    });
    let items = stub.client().items().unwrap();
    assert_eq!(
        keys(&items),
        vec![
            "/MediaItems/1.flac",
            "/MediaItems/2.flac",
            "/MediaItems/3.flac"
        ]
    );
    assert_eq!(items[0].title, "Track a");
    let searches: Vec<_> = stub
        .actions()
        .into_iter()
        .filter(|(action, ..)| action == "Search")
        .map(|(.., start)| start.unwrap())
        .collect();
    assert_eq!(searches, vec!["0", "2"]);
    let search = stub
        .requests()
        .into_iter()
        .find(|r| r.action.as_deref() == Some("Search"))
        .unwrap();
    assert_eq!(
        arg(&search.body, "SearchCriteria").as_deref(),
        Some("upnp:class derivedfrom &quot;object.item.audioItem&quot;")
    );
    assert_eq!(search.method, "POST");
    assert_eq!(search.path, "/ctl/ContentDir");
}

#[test]
fn without_search_the_tree_is_browsed_once_per_folder_and_copies_are_dropped() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => soap("GetSearchCapabilities", &[("SearchCaps", "")]),
        Some("Browse") => match arg(&request.body, "ObjectID").as_deref() {
            Some("0") => listing("Browse", &[folder("1"), folder("2")], 2),
            Some("1") => listing(
                "Browse",
                &[track(base, "1$a", "1.flac"), track(base, "1$b", "2.flac")],
                2,
            ),
            Some("2") => listing(
                "Browse",
                &[folder("1"), folder("0"), track(base, "2$a", "1.flac")],
                3,
            ),
            _ => upnp_error(701),
        },
        Some(_) => upnp_error(401),
    });
    let items = stub.client().items().unwrap();
    assert_eq!(
        keys(&items),
        vec!["/MediaItems/1.flac", "/MediaItems/2.flac"]
    );
    let browsed: Vec<_> = stub
        .actions()
        .into_iter()
        .filter(|(action, ..)| action == "Browse")
        .map(|(_, id, _)| id.unwrap())
        .collect();
    assert_eq!(browsed, vec!["0", "1", "2"]);
}

#[test]
fn a_search_the_server_rejects_falls_back_to_browsing() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => soap("GetSearchCapabilities", &[("SearchCaps", "*")]),
        Some("Search") => upnp_error(708),
        Some("Browse") => listing("Browse", &[track(base, "a", "1.flac")], 1),
        Some(_) => upnp_error(401),
    });
    let items = stub.client().items().unwrap();
    assert_eq!(keys(&items), vec!["/MediaItems/1.flac"]);
}

#[test]
fn a_failure_in_the_middle_fails_the_whole_listing() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => {
            soap("GetSearchCapabilities", &[("SearchCaps", "upnp:class")])
        }
        Some("Search") => match arg(&request.body, "StartingIndex").as_deref() {
            Some("0") => listing("Search", &[track(base, "a", "1.flac")], 2),
            _ => (503, "text/plain", Vec::new()),
        },
        Some(_) => upnp_error(401),
    });
    assert!(matches!(stub.client().items(), Err(Error::Transient(_))));
}

#[test]
fn a_server_that_ignores_the_starting_index_is_an_error() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => {
            soap("GetSearchCapabilities", &[("SearchCaps", "upnp:class")])
        }
        Some("Search") => listing("Search", &[track(base, "a", "1.flac")], 0),
        Some(_) => upnp_error(401),
    });
    assert!(matches!(stub.client().items(), Err(Error::Server(_))));
}

#[test]
fn ranges_are_asked_from_the_media_address_with_dlna_headers() {
    let stub = Stub::start(|request, _| match request.path.as_str() {
        "/rootDesc.xml" => description(),
        "/MediaItems/1.flac" => (200, "audio/x-flac", b"0123456789".to_vec()),
        _ => (404, "text/plain", Vec::new()),
    });
    let client = stub.client();
    client.ping().unwrap();
    let mut range = client.fetch_range("/MediaItems/1.flac", 4, None).unwrap();
    let mut bytes = Vec::new();
    range.body.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"456789");
    assert_eq!(range.offset, 4);
    assert_eq!(range.total, Some(10));
    assert!(range.ranged);
    let media = stub
        .requests()
        .into_iter()
        .find(|r| r.path == "/MediaItems/1.flac")
        .unwrap();
    assert_eq!(media.range.as_deref(), Some("bytes=4-"));
    assert!(media.features);
    assert!(matches!(
        client.fetch_range("/MediaItems/9.flac", 0, None),
        Err(Error::Server(_))
    ));
}

#[test]
fn a_bare_address_finds_the_description_under_a_usual_path() {
    let stub = Stub::start(|request, _| match request.path.as_str() {
        "/description.xml" => description(),
        _ => (404, "text/plain", Vec::new()),
    });
    let address = stub.base.trim_start_matches("http://").to_string();
    let device = describe(&address).unwrap();
    assert_eq!(device.udn, UDN);
    assert_eq!(device.name, "pi: minidlna");
    assert_eq!(device.location, format!("{}/description.xml", stub.base));
}

#[test]
fn search_capabilities_are_matched_by_name() {
    assert!(searchable("*"));
    assert!(searchable("dc:title, upnp:class ,upnp:artist"));
    assert!(!searchable("dc:title,upnp:classic"));
    assert!(!searchable(""));
}

fn bulk(base: &str, from: usize, to: usize) -> Vec<String> {
    (from..to)
        .map(|n| track(base, &n.to_string(), &format!("{n}.mp3")))
        .collect()
}

#[rstest::rstest]
#[case::empty(0, vec!["0"])]
#[case::one(1, vec!["0"])]
#[case::one_short_of_a_page(199, vec!["0"])]
#[case::exactly_a_page(200, vec!["0"])]
#[case::one_past_a_page(201, vec!["0", "200"])]
#[case::two_pages_and_one(401, vec!["0", "200", "400"])]
fn search_pages_stop_exactly_at_the_total(#[case] total: usize, #[case] expected: Vec<&str>) {
    let stub = Stub::start(move |request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => {
            soap("GetSearchCapabilities", &[("SearchCaps", "upnp:class")])
        }
        Some("Search") => {
            let start: usize = arg(&request.body, "StartingIndex")
                .unwrap()
                .parse()
                .unwrap();
            let end = (start + 200).min(total);
            listing("Search", &bulk(base, start.min(end), end), total)
        }
        Some("Browse") => listing("Browse", &[], 0),
        Some(_) => upnp_error(401),
    });
    let items = stub.client().items().unwrap();
    assert_eq!(items.len(), total);
    let starts: Vec<String> = stub
        .actions()
        .into_iter()
        .filter(|(action, ..)| action == "Search")
        .filter_map(|(.., start)| start)
        .collect();
    assert_eq!(starts, expected);
}

#[test]
fn short_pages_without_counts_are_followed_until_an_empty_one() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => {
            soap("GetSearchCapabilities", &[("SearchCaps", "upnp:class")])
        }
        Some("Search") => {
            let start: usize = arg(&request.body, "StartingIndex")
                .unwrap()
                .parse()
                .unwrap();
            let entries = if start < 5 {
                bulk(base, start, start + 1)
            } else {
                vec![]
            };
            let didl = format!(
                r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">{}</DIDL-Lite>"#,
                entries.concat()
            );
            soap("Search", &[("Result", &didl)])
        }
        Some(_) => upnp_error(401),
    });
    assert_eq!(stub.client().items().unwrap().len(), 5);
}

#[test]
fn a_broken_page_fails_the_listing_instead_of_shortening_it() {
    let stub = Stub::start(|request, base| match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => {
            soap("GetSearchCapabilities", &[("SearchCaps", "upnp:class")])
        }
        Some("Search") => match arg(&request.body, "StartingIndex").as_deref() {
            Some("0") => listing("Search", &bulk(base, 0, 1), 2),
            _ => soap(
                "Search",
                &[
                    ("Result", "<DIDL-Lite><item id=\"1\">"),
                    ("NumberReturned", "1"),
                ],
            ),
        },
        Some(_) => upnp_error(401),
    });
    assert!(matches!(stub.client().items(), Err(Error::Server(_))));
}

#[test]
fn items_without_a_playable_resource_are_left_out() {
    let stub = Stub::start(|request, base| {
        match request.action.as_deref() {
        None => description(),
        Some("GetSearchCapabilities") => soap("GetSearchCapabilities", &[("SearchCaps", "")]),
        Some("Browse") => listing(
            "Browse",
            &[
                track(base, "ok", "1.mp3"),
                r#"<item id="nores"><dc:title>x</dc:title><upnp:class>object.item.audioItem</upnp:class></item>"#.into(),
                r#"<item id="empty"><dc:title>y</dc:title><upnp:class>object.item.audioItem</upnp:class><res protocolInfo="http-get:*:audio/mpeg:*"> </res></item>"#.into(),
                r#"<item id="rel"><dc:title>z</dc:title><upnp:class>object.item.audioItem</upnp:class><res protocolInfo="http-get:*:audio/mpeg:*">/MediaItems/rel.mp3</res></item>"#.into(),
            ],
            4,
        ),
        Some(_) => upnp_error(401),
    }
    });
    let items = stub.client().items().unwrap();
    assert_eq!(
        keys(&items),
        vec!["/MediaItems/1.mp3", "/MediaItems/rel.mp3"]
    );
}

#[rstest::rstest]
#[case::ignored_range(200, "", Ok(false))]
#[case::no_access(403, "", Err(Error::Auth))]
#[case::server_down(503, "", Err(Error::Transient("HTTP 503".into())))]
#[case::bad_content_range(206, "Content-Range: bytes x\r\n", Err(Error::Server("malformed Content-Range".into())))]
fn media_replies_map_to_the_right_outcome(
    #[case] status: u16,
    #[case] header: &'static str,
    #[case] expected: Result<bool, Error>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let (status, extra, body) = if line.contains("rootDesc") {
                (200, String::new(), description().2)
            } else {
                (status, header.to_string(), b"0123456789".to_vec())
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    let client = Client::new(&Config {
        udn: UDN.into(),
        location: format!("{base}/rootDesc.xml"),
    });
    client.ping().unwrap();
    let outcome = client
        .fetch_range("/MediaItems/1.flac", 4, None)
        .map(|range| range.ranged);
    assert_eq!(outcome, expected);
}

#[test]
fn a_range_past_the_end_is_asked_again_open_ended_like_minidlna_needs() {
    let stub = Stub::start(|request, _| match request.path.as_str() {
        "/rootDesc.xml" => description(),
        _ => {
            let body = b"0123456789".to_vec();
            let bounds: Vec<Option<usize>> = request
                .range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .map(|r| r.split('-').map(|n| n.parse().ok()).collect())
                .unwrap_or_default();
            let past_end = bounds.iter().flatten().any(|n| *n >= body.len());
            if past_end {
                (416, "text/plain", Vec::new())
            } else {
                (200, "audio/x-flac", body)
            }
        }
    });
    let client = stub.client();
    let mut range = client
        .fetch_range("/MediaItems/1.flac", 2, Some(4_194_304))
        .unwrap();
    let mut bytes = Vec::new();
    range.body.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"23456789");
    assert_eq!(
        (range.offset, range.total, range.ranged),
        (2, Some(10), true)
    );
    let ranges: Vec<_> = stub
        .requests()
        .into_iter()
        .filter_map(|r| r.range)
        .collect();
    assert_eq!(ranges, vec!["bytes=2-4194303", "bytes=2-"]);
    assert!(matches!(
        client.fetch_range("/MediaItems/1.flac", 20, Some(30)),
        Err(Error::Server(_))
    ));
}

#[test]
fn a_server_that_refuses_connections_is_unreachable() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let result = describe(&format!("http://127.0.0.1:{port}/rootDesc.xml"));
    assert!(matches!(result, Err(Error::Transient(_))));
}

mod captured {
    use rstest::rstest;

    use crate::didl::{self, Page};
    use crate::{device, soap};

    fn read(server: &str, file: &str) -> String {
        let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        std::fs::read_to_string(format!("{root}/testdata/{server}/{file}")).unwrap()
    }

    fn listing(server: &str, file: &str, location: &str) -> Page {
        let args = soap::out_args(&read(server, file)).unwrap();
        didl::parse(&args["Result"], location).unwrap()
    }

    #[rstest]
    #[case::minidlna(
        "minidlna",
        "http://192.168.1.10:8201/rootDesc.xml",
        "uuid:4d696e69-444c-164e-9d41-000000000010",
        "Pawse Test DLNA",
        "http://192.168.1.10:8201/ctl/ContentDir"
    )]
    #[case::minidlna_pi(
        "minidlna-pi",
        "http://192.168.1.20:8200/rootDesc.xml",
        "uuid:4d696e69-444c-164e-9d41-000000000020",
        "pawse-test (rpi)",
        "http://192.168.1.20:8200/ctl/ContentDir"
    )]
    fn the_description_names_the_media_server(
        #[case] server: &str,
        #[case] location: &str,
        #[case] udn: &str,
        #[case] name: &str,
        #[case] control: &str,
    ) {
        let description = device::parse(location, &read(server, "rootDesc.xml")).unwrap();
        assert_eq!(description.device.udn, udn);
        assert_eq!(description.device.name, name);
        assert_eq!(description.control, control);
    }

    #[test]
    fn minidlna_on_debian_searches_in_one_page_and_lists_cue_images_whole() {
        let caps = soap::out_args(&read("minidlna-pi", "caps.xml")).unwrap();
        assert!(crate::searchable(&caps["SearchCaps"]));
        let args = soap::out_args(&read("minidlna-pi", "search.xml")).unwrap();
        assert_eq!(args["NumberReturned"], "38");
        assert_eq!(args["TotalMatches"], "38");
        let page = didl::parse(&args["Result"], "http://192.168.1.20:8200/rootDesc.xml").unwrap();
        assert_eq!(page.items.len(), 38);
        let ids: std::collections::HashSet<String> = page
            .items
            .iter()
            .map(|item| item.pick().unwrap().id())
            .collect();
        assert_eq!(ids.len(), 38);
        assert!(
            ids.iter()
                .all(|id| id.starts_with("/MediaItems/") && id.contains(".flac#"))
        );
        assert!(page.items.iter().any(|item| item.cover_id().is_some()));
        let untagged: Vec<_> = page
            .items
            .iter()
            .filter(|item| item.artists.is_empty())
            .collect();
        assert_eq!(untagged.len(), 8);
        assert!(untagged.iter().all(|image| {
            image.album.is_none() && image.pick().unwrap().duration_ms > Some(30 * 60 * 1000)
        }));
    }

    #[test]
    fn minidlna_rejects_the_audio_search_with_a_upnp_error() {
        assert!(matches!(
            soap::fault(&read("minidlna", "search_fault.xml")),
            Some(soap::Failure::Upnp(708, _))
        ));
    }

    #[test]
    fn a_minidlna_album_folder_reads_with_tags_and_art() {
        let page = listing(
            "minidlna",
            "browse_album.xml",
            "http://192.168.1.10:8201/rootDesc.xml",
        );
        let shape: Vec<_> = page
            .items
            .iter()
            .map(|item| {
                (
                    item.title.as_str(),
                    item.track_number,
                    item.pick().unwrap().key.as_str(),
                    item.album_art.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (
                    "Песня 1",
                    Some(1),
                    "/MediaItems/445.flac",
                    Some("/AlbumArt/1-445.jpg")
                ),
                (
                    "Песня 2",
                    Some(2),
                    "/MediaItems/451.flac",
                    Some("/AlbumArt/1-451.jpg")
                ),
                (
                    "Песня 3",
                    Some(3),
                    "/MediaItems/452.flac",
                    Some("/AlbumArt/1-452.jpg")
                ),
            ]
        );
        let first = &page.items[0];
        assert_eq!(first.artists, vec!["Тестовый Исполнитель".to_string()]);
        assert_eq!(first.album.as_deref(), Some("Альбом 1997"));
        assert_eq!(first.date.as_deref(), Some("1997-01-01"));
        assert_eq!(first.genre.as_deref(), Some("Rock"));
        let res = first.pick().unwrap();
        assert_eq!(res.size, Some(312_140));
        assert_eq!(res.duration_ms, Some(20_000));
        assert_eq!(res.mime(), "audio/x-flac");
    }

    #[test]
    fn a_minidlna_folder_of_mixed_formats_keeps_every_playable_file() {
        let page = listing(
            "minidlna",
            "browse_mixed.xml",
            "http://192.168.1.10:8201/rootDesc.xml",
        );
        let mut shape: Vec<_> = page
            .items
            .iter()
            .map(|item| {
                let res = item.pick().unwrap();
                (item.title.as_str(), res.key.as_str(), res.mime())
            })
            .collect();
        shape.sort();
        assert_eq!(
            shape,
            vec![
                ("05 wave", "/MediaItems/33.wav", "audio/x-wav"),
                ("AAC track", "/MediaItems/28.m4a", "audio/mp4"),
                ("Long name", "/MediaItems/38.mp3", "audio/mpeg"),
                ("Vorbis track", "/MediaItems/32.dat", "audio/ogg"),
                ("untagged", "/MediaItems/37.flac", "audio/x-flac"),
                ("テスト曲 & <co>", "/MediaItems/34.mp3", "audio/mpeg"),
            ]
        );
    }
}
