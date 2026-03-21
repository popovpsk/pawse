use gpui::{div, AppContext, Context, Entity, ParentElement, Render, Styled, Window};
use gpui_component::StyledExt;

use crate::footer::Footer;

pub struct MainView {
    footer: Entity<Footer>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
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
            .items_center()
            .justify_end()
            .child(self.footer.clone())
    }
}
