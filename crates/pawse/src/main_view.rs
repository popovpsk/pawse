use gpui::{AppContext, Context, Entity, ParentElement, Render, Styled, Window, div, px};
use gpui_component::StyledExt;

use crate::footer::Footer;
use crate::library_views::library_view::LibraryView;
use crate::media_bridge::MediaBridge;

pub struct MainView {
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    _media_bridge: Entity<MediaBridge>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            library_view: cx.new(|cx| LibraryView::new(window, cx)),
            footer: cx.new(|cx| Footer::new(window, cx)),
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
        }
    }
}

impl Render for MainView {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        div()
            .v_flex()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .ml_4()
                    .mr_4()
                    .child(self.library_view.clone()),
            )
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(80.))
                    .child(self.footer.clone()),
            )
    }
}
