use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui_component::{
    ActiveTheme, Icon, Root, Sizable, Size, StyledExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    setting::SettingPage,
};

use crate::audio_settings::AudioSettings;
use crate::footer::Footer;
use crate::library_views::library_view::{LibraryView, LibraryViewEvent};
use crate::media_bridge::MediaBridge;

pub struct MainView {
    audio_settings: Entity<AudioSettings>,
    library_view: Entity<LibraryView>,
    footer: Entity<Footer>,
    is_tracks_view: bool,
    show_settings: bool,
    settings_pages: Vec<SettingPage>,
    search_input: Entity<InputState>,
    _media_bridge: Entity<MediaBridge>,
    _library_subscription: Subscription,
    _search_subscription: Subscription,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_view = cx.new(|cx| LibraryView::new(window, cx));

        let library_subscription = cx.subscribe(
            &library_view,
            {
                let library_view = library_view.clone();
                move |this: &mut MainView, _, _: &LibraryViewEvent, cx| {
                    this.is_tracks_view = library_view.read(cx).is_tracks_view();
                    let query = this.search_input.read(cx).value().to_string();
                    library_view.update(cx, |v, cx| v.apply_search(&query, cx));
                    cx.notify();
                }
            },
        );

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search artists, albums, tracks")
        });

        let search_subscription = cx.subscribe(&search_input, {
            let library_view = library_view.clone();
            move |this: &mut MainView, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    let query = this.search_input.read(cx).value().to_string();
                    library_view.update(cx, |v, cx| v.apply_search(&query, cx));
                }
            }
        });

        Self {
            audio_settings: cx.new(|cx| AudioSettings::new(window, cx)),
            library_view,
            footer: cx.new(|cx| Footer::new(window, cx)),
            is_tracks_view: false,
            show_settings: false,
            settings_pages: crate::settings_view::build_settings_pages(),
            search_input,
            _media_bridge: cx.new(|cx| MediaBridge::new(window, cx)),
            _library_subscription: library_subscription,
            _search_subscription: search_subscription,
        }
    }

    fn clear_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let library_view = self.library_view.clone();
        library_view.update(cx, |v, cx| v.apply_search("", cx));
    }
}

impl Render for MainView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let library_view = self.library_view.clone();
        let show_settings = self.show_settings;
        let has_back = show_settings || self.is_tracks_view;

        let back_button = div()
            .id("back_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.clear_search(window, cx);
                if this.show_settings {
                    this.show_settings = false;
                    cx.notify();
                } else {
                    library_view.update(cx, |view, cx| view.go_back(cx));
                }
            }))
            .child(
                svg()
                    .path("icons/back.svg")
                    .size(px(22.))
                    .text_color(cx.theme().foreground),
            );

        let left_group = div()
            .flex_1()
            .flex()
            .items_center()
            .h_full()
            .when(has_back, |d| d.child(back_button));

        let right_group = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .h_full()
            .when(!show_settings, |d| d.child(settings_gear_button(cx)))
            .child(self.audio_settings.clone());

        div()
            .v_flex()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_2()
                    .child(left_group)
                    .when(!show_settings, |d| {
                        d.child(
                            div().w(px(260.)).child(
                                Input::new(&self.search_input)
                                    .with_size(Size::Medium)
                                    .focus_bordered(false)
                                    .rounded_full()
                                    .cleanable(true),
                            ),
                        )
                    })
                    .child(right_group),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .ml_4()
                    .mr_4()
                    .child(if show_settings {
                        // Match gpui-component's StoryContainer wrap chain
                        // exactly: outer `size_full` + Scrollable, then inner
                        // `size_full`, then Settings. The Scrollable wrap is
                        // what gives `h_resizable` panels a definite-width
                        // parent so flex_basis on the sidebar panel resolves.
                        div()
                            .size_full()
                            .overflow_y_scrollbar()
                            .child(
                                div().size_full().child(crate::settings_view::settings_widget(
                                    self.settings_pages.clone(),
                                )),
                            )
                            .into_any_element()
                    } else {
                        self.library_view.clone().into_any_element()
                    }),
            )
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .h(px(80.))
                    .child(self.footer.clone()),
            )
            .children(Root::render_notification_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn settings_gear_button(cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("settings_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40.))
        .h(px(40.))
        .icon(Icon::default().path("icons/settings.svg").size(px(20.)))
        .tooltip("Settings")
        .on_click(cx.listener(|this, _, window, cx| {
            this.clear_search(window, cx);
            this.show_settings = true;
            cx.notify();
        }))
}
