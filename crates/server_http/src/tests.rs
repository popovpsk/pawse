use serde::Deserialize;

use super::*;

#[test]
fn content_range_headers_are_parsed() {
    assert_eq!(
        parse_content_range("bytes 100-199/1000"),
        Some((100, Some(1000)))
    );
    assert_eq!(parse_content_range("bytes 0-9/*"), Some((0, None)));
    assert_eq!(parse_content_range("items 0-9/10"), None);
    assert_eq!(parse_content_range("bytes x-9/10"), None);
}

#[test]
fn range_headers_are_inclusive_and_open_ended_without_an_end() {
    assert_eq!(range_header(100, Some(200)), "bytes=100-199");
    assert_eq!(range_header(0, None), "bytes=0-");
}

#[test]
fn error_text_never_carries_the_query_string() {
    assert_eq!(
        redact("bad uri http://nas/rest/ping?u=me&t=abc&s=1 is missing host"),
        "bad uri http://nas/rest/ping is missing host"
    );
    assert_eq!(redact("a?x (b?y) c"), "a (b) c");
}

#[test]
fn statuses_split_into_auth_transient_and_failed() {
    assert_eq!(classify(200), Status::Success);
    assert_eq!(classify(206), Status::Success);
    assert_eq!(classify(401), Status::Auth);
    assert_eq!(classify(403), Status::Auth);
    assert_eq!(classify(404), Status::Failed);
    assert_eq!(classify(302), Status::Failed);
    assert_eq!(classify(502), Status::Transient);
}

#[derive(Debug, Default, Deserialize)]
struct Inner {
    #[serde(default, deserialize_with = "lenient::boolean")]
    flag: bool,
}

#[derive(Debug, Deserialize)]
struct Sample {
    #[serde(deserialize_with = "lenient::id")]
    id: String,
    #[serde(default, deserialize_with = "lenient::opt_id")]
    other: Option<String>,
    #[serde(default, deserialize_with = "lenient::number")]
    n: Option<u32>,
    #[serde(default, deserialize_with = "lenient::text")]
    name: String,
    #[serde(default, deserialize_with = "lenient::list")]
    tags: Vec<String>,
    #[serde(default, deserialize_with = "lenient::object")]
    inner: Inner,
}

fn sample(json: &str) -> Result<Sample, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn odd_values_degrade_instead_of_failing() {
    let s = sample(r#"{"id": 7, "other": "", "n": 3.6, "name": 5, "tags": ["a", 1, "b"], "inner": {"flag": "TRUE"}}"#).unwrap();
    assert_eq!(s.id, "7");
    assert_eq!(s.other, None);
    assert_eq!(s.n, Some(4));
    assert_eq!(s.name, "5");
    assert_eq!(s.tags, vec!["a".to_string(), "b".to_string()]);
    assert!(s.inner.flag);

    let s = sample(r#"{"id": "x", "n": "12", "tags": "oops", "inner": []}"#).unwrap();
    assert_eq!(s.n, Some(12));
    assert!(s.tags.is_empty());
    assert!(!s.inner.flag);

    let s = sample(r#"{"id": "x", "n": -1}"#).unwrap();
    assert_eq!(s.n, None);
}

#[test]
fn a_missing_or_empty_id_fails_the_record() {
    assert!(sample(r#"{"name": "x"}"#).is_err());
    assert!(sample(r#"{"id": ""}"#).is_err());
    assert!(sample(r#"{"id": null}"#).is_err());
}
