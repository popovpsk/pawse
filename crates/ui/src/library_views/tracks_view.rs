use std::path::PathBuf;

use gpui::{ClickEvent, Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div};
use gpui_component::{button::Button, h_flex, v_flex};

use crate::services::Services;

#[derive(Clone, Debug)]
pub struct BackEvent;

pub struct TracksView {
    #[allow(dead_code)]
    album_id: i64,
    tracks: Vec<music_library::Track>,
}

impl TracksView {
    pub fn new(album_id: i64, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let tracks = services.library.tracks_for_album(album_id);
        Self { album_id, tracks }
    }

    fn on_back(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(BackEvent);
    }
}

impl gpui::EventEmitter<BackEvent> for TracksView {}

impl Render for TracksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let back_button = Button::new("back")
            .label("Back")
            .on_click(cx.listener(TracksView::on_back));

        let header = h_flex().px_4().py_2().child(back_button);

        let tracks_list = v_flex()
            .gap_1()
            .id("tracks_list")
            .overflow_y_scroll()
            .children(self.tracks.iter().map(|track| {
                let path = PathBuf::from(&track.path);
                let track_id = track.id;
                let track_num_str = track
                    .track_number
                    .map(|n| format!("{}.", n))
                    .unwrap_or_default();
                let duration_str = track
                    .duration_ms
                    .map(|ms| {
                        let secs = (ms / 1000) as u32;
                        format!("{:02}:{:02}", secs / 60, secs % 60)
                    })
                    .unwrap_or_default();

                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .cursor(gpui::CursorStyle::PointingHand)
                    .child(div().w_8().child(track_num_str))
                    .child(div().flex_1().child(track.title.clone()))
                    .child(div().w_16().child(duration_str))
                    .id(ElementId::Integer(track_id as u64))
                    .on_click(cx.listener(move |_this, _, _, _cx| {
                        _cx.global::<Services>().engine_manager.set_track(path.clone());
                    }))
            }));

        v_flex().size_full().child(header).child(tracks_list)
    }
}
