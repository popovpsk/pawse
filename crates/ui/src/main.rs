
use gpui::*;
use gpui_component::*;

use crate::{
    main_view::MainView,
    services::{run_engine_events_bus, Services},
};

pub mod footer;
pub mod main_view;
pub mod services;
// pub struct AudioApp {
//     command_sender: Option<mpsc::Sender<AudioCommand>>,
//     is_loaded: Arc<AtomicBool>,
// }

// impl AudioApp {
//     fn new() -> Self {
//         let (cmd_sender, cmd_receiver) = mpsc::channel();

//         let is_loaded = Arc::new(AtomicBool::new(false));
//         let is_loaded_for_thread = Arc::clone(&is_loaded);

//         // Создаём Output один раз при старте
//         let output = Arc::new(Output::new());

//         thread::spawn(move || {
//             let mut engine: Option<AudioEngine> = None;

//             for cmd in cmd_receiver {
//                 match cmd {
//                     AudioCommand::Open(path) => {
//                         eprintln!("[AudioThread] Open: {:?}", path);
//                         let eng =
//                             AudioEngine::new(output.clone()).expect("Failed to create AudioEngine");
//                         match eng.load(&path) {
//                             Ok(()) => {
//                                 // Wait for Loaded event
//                                 let mut loaded = false;
//                                 for _ in 0..100 {
//                                     for event in eng.events().try_iter() {
//                                         if let audio_engine::EngineEvent::Loaded { .. } = event {
//                                             loaded = true;

//                                             break;
//                                         }
//                                     }
//                                     if loaded {
//                                         break;
//                                     }
//                                     thread::sleep(Duration::from_millis(10));
//                                 }
//                                 if loaded {
//                                     is_loaded_for_thread.store(true, Ordering::SeqCst);
//                                 }
//                                 engine = Some(eng);
//                             }
//                             Err(e) => eprintln!("[AudioThread] Open error: {:?}", e),
//                         }
//                     }
//                     AudioCommand::Play => {
//                         eprintln!("[AudioThread] Play command");
//                         if let Some(ref mut eng) = engine {
//                             eng.play().ok();
//                             eprintln!("[AudioThread] Playing");
//                         }
//                     }
//                     AudioCommand::Pause => {
//                         eprintln!("[AudioThread] Pause command");
//                         if let Some(ref mut eng) = engine {
//                             eng.pause().ok();
//                             eprintln!("[AudioThread] Paused");
//                         }
//                     }
//                     AudioCommand::Stop => {
//                         eprintln!("[AudioThread] Stop command");
//                         if let Some(ref mut eng) = engine {
//                             eng.stop().ok();
//                             eprintln!("[AudioThread] Stopped");
//                         }
//                     }
//                     AudioCommand::Shutdown => {
//                         eprintln!("[AudioThread] Shutdown");
//                         if let Some(ref mut eng) = engine {
//                             eng.stop().ok();
//                         }
//                         break;
//                     }
//                 }
//             }
//             eprintln!("[AudioThread] Exited");
//         });

//         Self {
//             command_sender: Some(cmd_sender),
//             is_loaded,
//         }
//     }

//     fn audio_path() -> PathBuf {
//         PathBuf::from("/Users/popovaleksa/repo/other/gpui-test/fixtures/02 - Selfless.flac")
//     }
// }

// impl Drop for AudioApp {
//     fn drop(&mut self) {
//         if let Some(ref sender) = self.command_sender {
//             let _ = sender.send(AudioCommand::Shutdown);
//         }
//     }
// }

// impl Render for AudioApp {
//     fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
//         let sender = self.command_sender.clone();
//         let path = Self::audio_path();
//         let is_loaded = Arc::clone(&self.is_loaded);

//         div()
//             .h_flex()
//             .gap_4()
//             .size_auto()
//             .items_center()
//             .justify_center()
//             .child({
//                 let sender = sender.clone();
//                 let path = path.clone();
//                 let is_loaded = Arc::clone(&is_loaded);
//                 Button::new("play")
//                     .primary()
//                     .label("Play")
//                     .on_click(move |_, _, _| {
//                         eprintln!(
//                             "[UI] Play clicked, is_loaded={}",
//                             is_loaded.load(Ordering::SeqCst)
//                         );
//                         if let Some(ref s) = sender {
//                             if !is_loaded.load(Ordering::SeqCst) {
//                                 s.send(AudioCommand::Open(path.clone())).ok();
//                             }
//                             s.send(AudioCommand::Play).ok();
//                         }
//                     })
//             })
//             .child({
//                 let sender = sender.clone();
//                 Button::new("pause")
//                     .label("Pause")
//                     .on_click(move |_, _, _| {
//                         eprintln!("[UI] Pause clicked");
//                         if let Some(ref s) = sender {
//                             s.send(AudioCommand::Pause).ok();
//                         }
//                     })
//             })
//             .child({
//                 let sender = sender.clone();
//                 Button::new("stop")
//                     .danger()
//                     .label("Stop")
//                     .on_click(move |_, _, _| {
//                         eprintln!("[UI] Stop clicked");
//                         if let Some(ref s) = sender {
//                             s.send(AudioCommand::Stop).ok();
//                         }
//                     })
//             })
//     }
// }

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(300.0), px(200.0))),
            ..Default::default()
        };
        let services = Services::initialize(cx);

        let engine_manager = services.engine_manager.clone();
        let engine_event_bus = services.engine_event_bus.clone();
        cx.set_global(services);

        cx.spawn(async move |cx| {
            run_engine_events_bus(cx, engine_manager, engine_event_bus).await;
        })
        .detach();

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| MainView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
