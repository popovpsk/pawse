use std::path::{Path, PathBuf};

use rstest::rstest;

use crate::{PathIndex, is_playlist, parse};

fn base() -> PathBuf {
    Path::new("/music").join("lists")
}

fn entries(text: &str, file: &str) -> Vec<PathBuf> {
    parse(text.as_bytes(), &base().join(file)).entries
}

#[test]
fn extended_m3u_keeps_paths_in_order_and_takes_the_playlist_title() {
    let text = "#EXTM3U\r\n#PLAYLIST: Road trip \r\n\r\n#EXTINF:215,Artist - Song\r\n../A/01 Song.flac\r\n./B/02.mp3\r\n/abs/C/03.ogg\r\n";
    let playlist = parse(text.as_bytes(), &base().join("list.m3u8"));
    assert_eq!(playlist.name, "Road trip");
    assert_eq!(
        playlist.entries,
        vec![
            Path::new("/music").join("A").join("01 Song.flac"),
            base().join("B").join("02.mp3"),
            Path::new("/abs").join("C").join("03.ogg"),
        ]
    );
}

#[rstest]
#[case::m3u("#EXTM3U\r#EXTINF:1,x\ra.flac\rb.flac\r", "list.m3u")]
#[case::pls("[playlist]\rFile1=a.flac\rFile2=b.flac\r", "list.pls")]
fn classic_mac_line_endings_split_lines(#[case] text: &str, #[case] file: &str) {
    assert_eq!(
        entries(text, file),
        vec![base().join("a.flac"), base().join("b.flac")]
    );
}

#[test]
fn the_name_falls_back_to_the_file_stem() {
    let playlist = parse(b"a.flac\n", &base().join("Evening Mix.m3u"));
    assert_eq!(playlist.name, "Evening Mix");
}

#[rstest]
#[case::http("http://radio.example/stream")]
#[case::https("https://radio.example/stream.mp3")]
#[case::rtsp("rtsp://camera.example/live")]
fn stream_urls_are_skipped(#[case] line: &str) {
    assert!(entries(line, "list.m3u").is_empty());
}

#[test]
fn a_colon_in_a_file_name_is_not_a_url() {
    assert_eq!(
        entries("Re:Zero.flac", "list.m3u"),
        vec![base().join("Re:Zero.flac")]
    );
}

#[rstest]
#[case::triple_slash("file:///srv/My%20Music/a.flac")]
#[case::localhost("file://localhost/srv/My%20Music/a.flac")]
#[case::single_slash("file:/srv/My%20Music/a.flac")]
fn file_uris_become_paths(#[case] line: &str) {
    assert_eq!(
        entries(line, "list.m3u"),
        vec![Path::new("/srv").join("My Music").join("a.flac")]
    );
}

#[test]
fn file_uris_decode_utf8_escapes() {
    assert_eq!(
        entries("file:///srv/%D0%9A%D0%B8%D0%BD%D0%BE/a.flac", "list.m3u"),
        vec![Path::new("/srv").join("Кино").join("a.flac")]
    );
}

#[rstest]
#[case::triple_slash("file:///C:/My%20Music/a.flac")]
#[case::double_slash("file://C:/My%20Music/a.flac")]
fn file_uris_with_a_drive_name_the_drive_path(#[case] line: &str) {
    assert_eq!(
        entries(line, "list.m3u"),
        entries("C:\\My Music\\a.flac", "list.m3u")
    );
}

#[cfg(not(windows))]
#[test]
fn a_windows_path_keeps_its_components_on_other_systems() {
    assert_eq!(
        entries("C:\\Music\\Artist\\01.flac", "list.m3u"),
        vec![PathBuf::from("C:/Music/Artist/01.flac")]
    );
    assert_eq!(
        entries("Artist\\01.flac", "list.m3u"),
        vec![base().join("Artist").join("01.flac")]
    );
}

#[test]
fn pls_orders_by_entry_number_and_ignores_titles() {
    let text = "[playlist]\nNumberOfEntries=3\nfile2=b.mp3\nTitle1=First\nFile10=c.mp3\nFILE1=a.mp3\nLength1=100\nVersion=2\n";
    let playlist = parse(text.as_bytes(), &base().join("set.pls"));
    assert_eq!(playlist.name, "set");
    assert_eq!(
        playlist.entries,
        vec![
            base().join("a.mp3"),
            base().join("b.mp3"),
            base().join("c.mp3")
        ]
    );
}

#[test]
fn a_utf8_bom_is_dropped() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice("Кино.flac\n".as_bytes());
    let playlist = parse(&bytes, &base().join("list.m3u"));
    assert_eq!(playlist.entries, vec![base().join("Кино.flac")]);
}

#[test]
fn a_utf16_playlist_is_decoded_by_its_bom() {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "#EXTM3U\r\nКино.flac\r\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let playlist = parse(&bytes, &base().join("list.m3u"));
    assert_eq!(playlist.entries, vec![base().join("Кино.flac")]);
}

#[test]
fn a_legacy_cyrillic_m3u_is_detected() {
    let text = "#EXTM3U\r\n#EXTINF:281,Кино - Группа крови\r\nКино\\Группа крови\\01 Группа крови.mp3\r\n#EXTINF:254,Кино - Закрой за мной дверь\r\nКино\\Группа крови\\02 Закрой за мной дверь.mp3\r\n";
    let (bytes, _, unmappable) = encoding_rs::WINDOWS_1251.encode(text);
    assert!(!unmappable);
    let playlist = parse(&bytes, &base().join("list.m3u"));
    assert_eq!(
        playlist.entries[0],
        base()
            .join("Кино")
            .join("Группа крови")
            .join("01 Группа крови.mp3")
    );
}

#[rstest]
#[case::m3u("a.m3u", true)]
#[case::m3u8_upper("a.M3U8", true)]
#[case::pls("a.pls", true)]
#[case::cue("a.cue", false)]
#[case::none("m3u", false)]
fn playlist_extensions(#[case] name: &str, #[case] expected: bool) {
    assert_eq!(is_playlist(Path::new(name)), expected);
}

fn index(keys: &[&str]) -> PathIndex {
    PathIndex::new(keys.iter().map(|key| key.to_string()).collect())
}

#[test]
fn a_path_matches_regardless_of_case() {
    let index = index(&["/Music/Artist/Album/01 Song.flac"]);
    assert_eq!(
        index.find("/music/artist/album/01 song.FLAC"),
        Some("/Music/Artist/Album/01 Song.flac")
    );
}

#[test]
fn a_path_matches_across_unicode_normalization() {
    let decomposed = "/Music/Мои\u{438}\u{306}/01.flac";
    let index = index(&[decomposed]);
    assert_eq!(index.find("/Music/Моий/01.flac"), Some(decomposed));
}

#[test]
fn a_moved_library_matches_by_the_path_tail() {
    let index = index(&[
        "/Volumes/NAS/Artist/Album/01.flac",
        "/Volumes/NAS/Other/Album 2/01.flac",
    ]);
    assert_eq!(
        index.find("D:\\Old Music\\Artist\\Album\\01.flac"),
        Some("/Volumes/NAS/Artist/Album/01.flac")
    );
}

#[test]
fn the_longest_shared_tail_wins() {
    let index = index(&["/m/A/Album/01.flac", "/m/B/Album/01.flac"]);
    assert_eq!(
        index.find("/old/A/Album/01.flac"),
        Some("/m/A/Album/01.flac")
    );
}

#[rstest]
#[case::tie("/old/Album/01.flac")]
#[case::file_name_only("/old/Elsewhere/01.flac")]
#[case::unknown_name("/m/A/Album/02.flac")]
fn an_unclear_tail_matches_nothing(#[case] wanted: &str) {
    let index = index(&["/m/A/Album/01.flac", "/m/B/Album/01.flac"]);
    assert_eq!(index.find(wanted), None);
}
