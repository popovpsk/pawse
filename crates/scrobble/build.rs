use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_API_KEY");
    println!("cargo:rerun-if-env-changed=LASTFM_API_SECRET");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let Some(env_path) = manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join(".env"))
    else {
        return;
    };
    println!("cargo:rerun-if-changed={}", env_path.display());

    let Ok(contents) = std::fs::read_to_string(&env_path) else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "LASTFM_API_KEY" && key != "LASTFM_API_SECRET" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            println!("cargo:rustc-env={key}={value}");
        }
    }
}
