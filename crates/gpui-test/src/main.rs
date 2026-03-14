use audio_engine::AudioEngine;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn main() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("fixtures");
    path.push("sine_440_16_44_stereo.wav");

    println!("Opening: {:?}", path);

    let mut engine = AudioEngine::new();

    match engine.open(&path) {
        Ok(info) => {
            println!("Opened: {:?}, duration: {:?}", info.params, info.duration);

            if let Err(e) = engine.play() {
                println!("Play error: {:?}", e);
                return;
            }

            println!("Playing... waiting 2 seconds");
            thread::sleep(Duration::from_secs(2));

            println!("Position: {:?}", engine.position());
            println!("Stopping...");
            let _ = engine.stop();

            println!("Done");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }
}
