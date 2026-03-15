use audio_engine::AudioEngine;
use audio_output::Output;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

    let output = Output::default_output().expect("Failed to create Output");
    let engine = AudioEngine::new(Arc::new(output)).expect("Failed to create AudioEngine");

    if let Err(e) = engine.load(&path) {
        println!("Load error: {:?}", e);
        return;
    }

    // Wait for Loaded event
    println!("Waiting for track to load...");
    let mut info = None;
    for _ in 0..50 {
        for event in engine.events().try_iter() {
            println!("Event: {:?}", event);
            if let audio_engine::EngineEvent::Loaded { params, duration } = event {
                info = Some((params, duration));
            }
        }
        if info.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    if let Some((params, duration)) = info {
        println!("Opened: {:?}", params);
        println!("Duration: {:?}", duration);

        if let Err(e) = engine.play() {
            println!("Play error: {:?}", e);
            return;
        }

        println!("Playing... (waiting for track to finish)");
        thread::sleep(duration + Duration::from_millis(200));

        engine.stop().ok();
        println!("Done");
    } else {
        println!("Failed to load track");
    }
}
