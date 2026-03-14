use audio_engine::AudioEngine;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn main() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("fixtures");
    path.push("sine_440_16_44_mono.wav");

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return;
    }

    println!("Opening: {:?}", path);

    let mut engine = AudioEngine::new();

    match engine.open(&path) {
        Ok(info) => {
            println!("Opened: {:?}", info.params);
            println!("Duration: {:?}", info.duration);

            if let Err(e) = engine.play() {
                println!("Play error: {:?}", e);
                return;
            }

            println!("Playing... (waiting for track to finish)");
            thread::sleep(info.duration + Duration::from_millis(200));

            engine.stop().ok();
            println!("Done");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }
}
