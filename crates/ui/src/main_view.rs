use gpui::{AppContext, Context, Entity, ParentElement, Render, Styled, Window, div};
use gpui_component::StyledExt;

use crate::footer::Footer;
use crate::library_views::library_view::LibraryView;

pub struct MainView {
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            library_view: cx.new(|cx| LibraryView::new(window, cx)),
            footer: cx.new(|cx| Footer::new(window, cx)),
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
            .gap_4()
            .size_full()
            .child(div().h_full().ml_4().child(self.library_view.clone()))
            .child(div().w_full().child(self.footer.clone()))
    }
}
