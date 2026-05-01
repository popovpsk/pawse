use gpui::{ClickEvent, Context, ElementId, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Window, div};
use gpui_component::{button::Button, h_flex, v_flex};

use crate::library_service::LibraryEvent;
use crate::services::Services;

#[derive(Clone, Debug)]
pub struct AlbumSelectedEvent {
    pub album_id: i64,
}

pub struct AlbumsView {
    albums: Vec<music_library::AlbumSummary>,
    is_scanning: bool,
    _subscription: Subscription,
}

impl AlbumsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let library_event_bus = services.library_event_bus.clone();
        let library = services.library.clone();

        let albums = library.albums();
        let is_scanning = false;

        let subscription = cx.subscribe(
            &library_event_bus,
            |this, _, event: &LibraryEvent, cx| match event {
                LibraryEvent::ScanStarted => {
                    this.is_scanning = true;
                    cx.notify();
                }
                LibraryEvent::ScanComplete => {
                    this.is_scanning = false;
                    this.albums = cx.global::<Services>().library.albums();
                    cx.notify();
                }
                _ => {}
            },
        );

        Self {
            albums,
            is_scanning,
            _subscription: subscription,
        }
    }

    fn on_select_folder(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let library = cx.global::<Services>().library.clone();
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                library.scan_directory(path);
            }
        });
    }

    fn on_rescan(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let library = cx.global::<Services>().library.clone();
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                library.clear_and_rescan(path);
            }
        });
    }

}

impl EventEmitter<AlbumSelectedEvent> for AlbumsView {}

impl Render for AlbumsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let select_button = Button::new("select_folder")
            .label("Select Music Folder")
            .on_click(cx.listener(AlbumsView::on_select_folder));

        let rescan_button = Button::new("rescan")
            .label("Rescan")
            .on_click(cx.listener(AlbumsView::on_rescan));

        let header = h_flex()
            .gap_2()
            .px_4()
            .py_2()
            .child(select_button)
            .child(rescan_button);

        if self.is_scanning && self.albums.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(div().px_4().child("Scanning..."));
        }

        let albums_list = v_flex()
            .gap_1()
            .id("albums_list")
            .overflow_y_scroll()
            .children(self.albums.iter().map(|album| {
                let album_id = album.id;
                let year_str = album
                    .year
                    .map(|y| format!(" ({})", y))
                    .unwrap_or_default();
                div()
                    .px_4()
                    .py_2()
                    .cursor(gpui::CursorStyle::PointingHand)
                    .child(format!("{}{} - {}", album.artist_name, year_str, album.title))
                    .id(ElementId::Integer(album_id as u64))
                    .on_click(cx.listener(move |_this, _, _, _cx| {
                        _cx.emit(AlbumSelectedEvent { album_id });
                    }))
            }));

        v_flex().size_full().child(header).child(albums_list)
    }
}
