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
            .items_baseline()
            .child(div().h_10().ml_4().mr_4().child("header")) //header
            .child(div().h_full().ml_4().child("center"))
            .child(div().w_full().child(self.footer.clone()))
    }
}
