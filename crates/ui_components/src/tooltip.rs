use gpui::{InteractiveElement, SharedString};
use gpui_component::{button::Button, tooltip::Tooltip};

pub trait NativeTooltip: InteractiveElement + Sized {
    fn native_tooltip(mut self, text: SharedString) -> Self {
        self.interactivity()
            .tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx));
        self
    }
}

impl NativeTooltip for Button {}
