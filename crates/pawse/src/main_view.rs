use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui::prelude::FluentBuilder;
use gpui_component::{ActiveTheme, Root, StyledExt};

use crate::audio_settings::AudioSettings;
use crate::footer::Footer;
use crate::library_views::library_view::{LibraryView, LibraryViewEvent};
use crate::media_bridge::MediaBridge;

pub struct MainView {
    audio_settings: Entity<AudioSettings>,
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    is_tracks_view: bool,
    _media_bridge: Entity<MediaBridge>,
    _library_subscription: Subscription,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_view = cx.new(|cx| LibraryView::new(window, cx));
        let library_subscription = cx.subscribe(
            &library_view,
            |this, _, _: &LibraryViewEvent, cx| {
                this.is_tracks_view = this.library_view.read(cx).is_tracks_view();
                cx.notify();
            },
        );

        Self {
            audio_settings: cx.new(|cx| AudioSettings::new(window, cx)),
            library_view,
            footer: cx.new(|cx| Footer::new(window, cx)),
            is_tracks_view: false,
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
            _library_subscription: library_subscription,
        }
    }
}

impl Render for MainView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let library_view = self.library_view.clone();
        let back_button = div()
            .id("back_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(cx.listener(move |_this, _, _, cx| {
                library_view.update(cx, |view, cx| view.go_back(cx));
            }))
            .child(
                svg()
                    .path("icons/back.svg")
                    .size(px(22.))
                    .text_color(cx.theme().foreground),
            );

        div()
            .v_flex()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_2()
                    .when(self.is_tracks_view, |d| d.child(back_button))
                    .child(div().flex_1())
                    .child(self.audio_settings.clone()),
            )
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
            .children(Root::render_notification_layer(window, cx))
    }
}
