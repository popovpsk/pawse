use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    let _ = std::fs::create_dir_all(&dist);
    if std::env::var("PROFILE").as_deref() == Ok("release") && !dist.join("index.html").exists() {
        println!(
            "cargo:warning=web/dist/index.html missing; run `npm run build` in web/ before a release build or the embedded remote UI will serve 404"
        );
    }
    println!("cargo:rerun-if-changed={}", dist.display());
    walk(&dist);
}

fn walk(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            walk(&path);
        }
    }
}
