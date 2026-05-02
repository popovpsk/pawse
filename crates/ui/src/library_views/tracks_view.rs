use std::path::PathBuf;
use std::rc::Rc;

use gpui::{ClickEvent, Context, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px, size, Size, Pixels};
use gpui_component::{button::Button, h_flex, v_flex, v_virtual_list, ActiveTheme, VirtualListScrollHandle};

use crate::services::Services;

#[derive(Clone, Debug)]
pub struct BackEvent;

const TRACK_ROW_HEIGHT: f32 = 36.;

pub struct TracksView {
    tracks: Vec<music_library::Track>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: VirtualListScrollHandle,
}

impl TracksView {
    pub fn new(album_id: i64, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let tracks = services.library.tracks_for_album(album_id);
        let item_sizes = Rc::new(vec![size(px(300.), px(TRACK_ROW_HEIGHT + 1.)); tracks.len()]);
        Self {
            tracks,
            item_sizes,
            scroll_handle: VirtualListScrollHandle::new(),
        }
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

        if self.tracks.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("No tracks found for this album."));
        }

        let item_sizes = self.item_sizes.clone();
        v_flex()
            .size_full()
            .child(header)
            .child(
                v_virtual_list(
                    cx.entity().clone(),
                    "tracks_list",
                    item_sizes,
                    |view, visible_range, _window, cx| {
                        visible_range
                            .map(|ix| {
                                let track = &view.tracks[ix];
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
                                    .w_full()
                                    .h(px(TRACK_ROW_HEIGHT))
                                    .px_4()
                                    .gap_2()
                                    .border_b(px(1.))
                                    .border_color(Hsla {
                                        h: 0.,
                                        s: 0.,
                                        l: 1.,
                                        a: 0.1,
                                    })
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|style| style.bg(cx.theme().secondary))
                                    .child(div().w_8().child(track_num_str))
                                    .child(div().flex_1().child(track.title.clone()))
                                    .child(div().w_16().child(duration_str))
                                    .id(ElementId::Integer(track_id as u64))
                                    .on_click(cx.listener(move |this, _, _, _cx| {
                                        let services = _cx.global::<Services>();
                                        let mut queue = services.playback_queue.borrow_mut();
                                        queue.set_tracks(this.tracks.clone());
                                        if let Some(track) = queue.play_track_at(ix) {
                                            services.engine_manager.set_track(PathBuf::from(&track.path));
                                            services.engine_manager.play();
                                        }
                                    }))
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll_handle)
                .flex_1(),
            )
    }
}
