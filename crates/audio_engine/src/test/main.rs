use audio_engine::{AudioEngine, EngineEvent};
use audio_output::Output;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("fixtures");
    path.push("sine_440_16_44_stereo.wav");

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return;
    }

    println!("Opening: {:?}", path);

    let engine = AudioEngine::new(Arc::new(Output::new()));
    let events = engine.events();
    engine.set_track(path);
    engine.play();
    let event = events.recv().unwrap();
    debug_assert!(event == EngineEvent::TrackEnded);
}
