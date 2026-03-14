use audio_engine::AudioEngine;
use gpui::*;
use gpui_component::{button::*, *};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
enum AudioCommand {
    Open(PathBuf),
    Play,
    Pause,
    Stop,
    Shutdown,
}

pub struct AudioApp {
    command_sender: Option<mpsc::Sender<AudioCommand>>,
}

impl AudioApp {
    fn new() -> Self {
        let (cmd_sender, cmd_receiver) = mpsc::channel();

        thread::spawn(move || {
            let mut engine: Option<AudioEngine> = None;

            for cmd in cmd_receiver {
                match cmd {
                    AudioCommand::Open(path) => {
                        eprintln!("[AudioThread] Open: {:?}", path);
                        let eng = AudioEngine::new().expect("Failed to create AudioEngine");
                        match eng.load(&path) {
                            Ok(()) => {
                                // Wait for Loaded event
                                let mut loaded = false;
                                for _ in 0..100 {
                                    for event in eng.events().try_iter() {
                                        if let audio_engine::EngineEvent::Loaded { .. } = event {
                                            loaded = true;
                                            if let Some(info) = eng.track_info() {
                                                eprintln!("[AudioThread] Opened: duration={:?}", info.duration);
                                            }
                                            break;
                                        }
                                    }
                                    if loaded {
                                        break;
                                    }
                                    thread::sleep(Duration::from_millis(10));
                                }
                                engine = Some(eng);
                            }
                            Err(e) => eprintln!("[AudioThread] Open error: {:?}", e),
                        }
                    }
                    AudioCommand::Play => {
                        eprintln!("[AudioThread] Play command");
                        if let Some(ref mut eng) = engine {
                            eng.play().ok();
                            eprintln!("[AudioThread] Playing");
                        }
                    }
                    AudioCommand::Pause => {
                        eprintln!("[AudioThread] Pause command");
                        if let Some(ref mut eng) = engine {
                            eng.pause().ok();
                            eprintln!("[AudioThread] Paused");
                        }
                    }
                    AudioCommand::Stop => {
                        eprintln!("[AudioThread] Stop command");
                        if let Some(ref mut eng) = engine {
                            eng.stop().ok();
                            eprintln!("[AudioThread] Stopped");
                        }
                    }
                    AudioCommand::Shutdown => {
                        eprintln!("[AudioThread] Shutdown");
                        if let Some(ref mut eng) = engine {
                            eng.stop().ok();
                        }
                        break;
                    }
                }
            }
            eprintln!("[AudioThread] Exited");
        });

        Self {
            command_sender: Some(cmd_sender),
        }
    }

    fn audio_path() -> PathBuf {
        PathBuf::from("/Users/popovaleksa/repo/other/gpui-test/fixtures/02 - Selfless.flac")
    }
}

impl Drop for AudioApp {
    fn drop(&mut self) {
        if let Some(ref sender) = self.command_sender {
            let _ = sender.send(AudioCommand::Shutdown);
        }
    }
}

impl Render for AudioApp {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let sender = self.command_sender.clone();
        let path = Self::audio_path();

        div()
            .v_flex()
            .gap_2()
            .size_full()
            .items_center()
            .justify_center()
            .child("Audio Player")
            .child({
                let sender = sender.clone();
                let path = path.clone();
                Button::new("play")
                    .primary()
                    .label("Play")
                    .on_click(move |_, _, _| {
                        eprintln!("[UI] Play clicked");
                        if let Some(ref s) = sender {
                            s.send(AudioCommand::Open(path.clone())).ok();
                            s.send(AudioCommand::Play).ok();
                        }
                    })
            })
            .child({
                let sender = sender.clone();
                Button::new("pause")
                    .label("Pause")
                    .on_click(move |_, _, _| {
                        eprintln!("[UI] Pause clicked");
                        if let Some(ref s) = sender {
                            s.send(AudioCommand::Pause).ok();
                        }
                    })
            })
            .child({
                let sender = sender.clone();
                Button::new("stop")
                    .danger()
                    .label("Stop")
                    .on_click(move |_, _, _| {
                        eprintln!("[UI] Stop clicked");
                        if let Some(ref s) = sender {
                            s.send(AudioCommand::Stop).ok();
                        }
                    })
            })
    }
}

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| AudioApp::new());
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
