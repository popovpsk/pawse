use audio_engine::AudioEngine;
use std::path::PathBuf;
use std::thread;

fn main() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("fixtures");
    path.push("1khz_16_44_1.wav");

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return;
    }

    println!("Opening: {:?}", path);

    let engine = AudioEngine::new();
    engine.set_track(path);
    engine.play();
    thread::sleep(std::time::Duration::from_secs(3));
}
