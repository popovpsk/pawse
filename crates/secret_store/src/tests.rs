use std::fs;

use crate::crypto::{self, Key};
use crate::{Source, assemble, file_key, watch_reseal};

fn random_key() -> Key {
    Key::new(crypto::random_bytes().unwrap())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Account {
    #[serde(with = "crate::serde_string")]
    password: String,
}

fn save(password: &str) -> String {
    serde_json::to_string(&Account {
        password: password.to_string(),
    })
    .unwrap()
}

fn load(json: &str) -> (Account, bool) {
    watch_reseal(|| serde_json::from_str(json).unwrap())
}

#[test]
fn a_secret_round_trips_and_is_never_written_in_the_clear() {
    let json = save("hunter2");
    assert!(!json.contains("hunter2"));
    assert!(json.contains("pawse:v1:"));
    let (account, reseal) = load(&json);
    assert_eq!(account.password, "hunter2");
    assert!(!reseal);
}

#[test]
fn every_save_draws_a_fresh_nonce() {
    assert_ne!(save("hunter2"), save("hunter2"));
}

#[test]
fn a_plain_value_reads_as_is_and_asks_for_a_reseal() {
    let (account, reseal) = load(r#"{"password":"hunter2"}"#);
    assert_eq!(account.password, "hunter2");
    assert!(reseal);
}

#[test]
fn an_empty_value_stays_empty() {
    assert_eq!(save(""), r#"{"password":""}"#);
    let (account, reseal) = load(r#"{"password":""}"#);
    assert_eq!(account.password, "");
    assert!(!reseal);
}

#[test]
fn a_value_sealed_with_an_unknown_key_survives_a_save_unchanged() {
    let foreign = crypto::seal(&random_key(), b"hunter2").unwrap();
    let json = format!(r#"{{"password":"{foreign}"}}"#);
    let (account, reseal) = load(&json);
    assert_eq!(account.password, foreign);
    assert!(!reseal);
    assert_eq!(save(&account.password), json);
}

#[test]
fn the_keychain_key_seals_first_then_the_file_then_the_binary() {
    let (keychain, file, binary) = (random_key(), random_key(), random_key());

    let keys = assemble(Some(keychain.clone()), Some(file.clone()), binary.clone());
    assert_eq!(keys.source, Source::Keychain);
    assert_eq!(keys.seal.id(), keychain.id());
    assert_eq!(keys.all.len(), 3);

    let keys = assemble(None, Some(file.clone()), binary.clone());
    assert_eq!(keys.source, Source::File);
    assert_eq!(keys.seal.id(), file.id());

    let keys = assemble(None, None, binary.clone());
    assert_eq!(keys.source, Source::Binary);
    assert_eq!(keys.all.len(), 1);
}

#[test]
fn any_key_at_hand_opens_what_it_sealed() {
    let (keychain, file, binary) = (random_key(), random_key(), random_key());
    let keys = assemble(Some(keychain), Some(file.clone()), binary);
    let sealed = crypto::seal(&file, b"hunter2").unwrap();
    let (plain, key) = crypto::open(&keys.all, &sealed).unwrap();
    assert_eq!(plain, b"hunter2");
    assert_eq!(key.id(), file.id());
}

#[test]
fn a_tampered_value_does_not_open() {
    let key = random_key();
    let sealed = crypto::seal(&key, b"hunter2").unwrap();
    let (head, body) = sealed.rsplit_once(':').unwrap();
    let flipped: String = body
        .chars()
        .enumerate()
        .map(|(i, c)| match (i, c) {
            (4, 'A') => 'B',
            (4, _) => 'A',
            _ => c,
        })
        .collect();
    let tampered = format!("{head}:{flipped}");
    assert!(crypto::open(std::slice::from_ref(&key), &tampered).is_none());
}

#[test]
fn the_file_key_is_made_once_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let wrap = random_key();
    let made = file_key::load(dir.path(), &wrap, true).unwrap();
    let again = file_key::load(dir.path(), &wrap, true).unwrap();
    assert_eq!(made.id(), again.id());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file_key::path(dir.path(), &wrap))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn no_file_key_is_made_unless_asked() {
    let dir = tempfile::tempdir().unwrap();
    let wrap = random_key();
    assert!(file_key::load(dir.path(), &wrap, false).is_none());
    assert!(!file_key::path(dir.path(), &wrap).exists());
}

#[test]
fn a_file_key_that_does_not_open_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let wrap = random_key();
    let path = file_key::path(dir.path(), &wrap);
    fs::write(&path, crypto::seal(&random_key(), &[7; 32]).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(file_key::load(dir.path(), &wrap, true).is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn builds_with_different_binary_keys_keep_separate_files() {
    let dir = tempfile::tempdir().unwrap();
    let (release, public) = (random_key(), random_key());
    let first = file_key::load(dir.path(), &release, true).unwrap();
    let second = file_key::load(dir.path(), &public, true).unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(
        file_key::load(dir.path(), &release, false).unwrap().id(),
        first.id()
    );
}
