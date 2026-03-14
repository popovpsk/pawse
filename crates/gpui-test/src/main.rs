use audio_engine::AudioEngine;
use gpui::*;
use gpui_component::{button::*, *};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct HelloWorld {
    engine: Mutex<Option<AudioEngine>>,
    is_playing: Mutex<bool>,
}

impl HelloWorld {
    fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            is_playing: Mutex::new(false),
        }
    }

    fn audio_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("fixtures");
        path.push("sine_440_16_44_stereo.wav");
        path
    }
}

impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_playing = *self.is_playing.lock().unwrap();

        let label = if is_playing {
            "Playing..."
        } else {
            "Audio Engine Ready"
        };
        let button_label = if is_playing { "Stop" } else { "Play Music" };

        let engine = self.engine.clone();
        let is_playing_ref = self.is_playing.clone();

        div()
            .v_flex()
            .gap_2()
            .size_full()
            .items_center()
            .justify_center()
            .child(label)
            .child(
                Button::new("play")
                    .primary()
                    .label(button_label)
                    .on_click(move |_, _, _| {
                        let mut is_playing = is_playing_ref.lock().unwrap();
                        let mut engine_guard = engine.lock().unwrap();

                        if *is_playing {
                            if let Some(ref mut eng) = *engine_guard {
                                let _ = eng.stop();
                            }
                            *engine_guard = None;
                            *is_playing = false;
                        } else {
                            let path = HelloWorld::audio_path();
                            eprintln!("[DEBUG] Creating new AudioEngine...");
                            let mut eng = AudioEngine::new();

                            eprintln!("[DEBUG] Opening file...");
                            match eng.open(&path) {
                                Ok(info) => {
                                    eprintln!(
                                        "[DEBUG] File opened: {:?}, duration: {:?}",
                                        info.params, info.duration
                                    );
                                    if eng.play().is_ok() {
                                        *engine_guard = Some(eng);
                                        *is_playing = true;
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[DEBUG] Failed to open audio file: {:?}", e);
                                }
                            }
                        }
                    }),
            )
    }
}

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| HelloWorld::new());
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
