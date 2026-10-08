use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use gpui_component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    input::Input,
    tooltip::Tooltip,
};

use super::{MainView, TabColors};
use crate::cover_backdrop;
use crate::library_views::library_view::LibraryRootTab;
use crate::library_views::view_menu::{self, view_menu};
use crate::localization::tr;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

pub(super) const HEIGHT: f32 = 44.;
pub(super) const TITLE_BAR_HEIGHT: f32 = 52.;
const SEARCH_WIDTH: f32 = 200.;
const VIEW_MENU_GAP: f32 = 6.;
const INDICATOR_GAP: f32 = 4.;

#[derive(Clone, Copy)]
pub(super) enum Placement {
    Below(Hsla),
    TitleBar,
}

fn guard(el: Div) -> Div {
    el.occlude()
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
}

impl MainView {
    pub(super) fn render_header(
        &self,
        placement: Placement,
        colors: TabColors,
        title_bar: Hsla,
        veil: Option<cover_backdrop::Veil>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let in_title_bar = matches!(placement, Placement::TitleBar);
        let show_screen = self.show_settings || self.show_tools;
        let cover_mode = self.cover_mode;
        let has_back = !cover_mode && (show_screen || self.is_drilled_in);
        let active_tab = (!cover_mode).then_some(self.current_tab);
        let muted = Colors::muted(cx);
        let foreground = Colors::foreground(cx);
        let update_ready = self
            .updater
            .as_ref()
            .is_some_and(|entity| entity.read(cx).has_staged_update());

        let settings = cx.global::<SettingsStore>();
        let liked_enabled = settings.liked_enabled();
        let playlists_enabled = settings.playlists_enabled();
        let genres_enabled = settings.genres_enabled();
        let tools_enabled = settings.tools_enabled();
        let view_menu = self
            .view_menu_tab
            .filter(|_| !show_screen && !cover_mode)
            .map(|tab| view_menu(tab, scale));

        let left_buttons = div()
            .flex()
            .items_center()
            .gap_1()
            .when(in_title_bar, guard)
            .when(has_back, |d| {
                d.child(back_button(foreground, muted, scale, cx))
            })
            .when(!has_back, |d| {
                d.child(tab_icon_button(
                    "tab_albums",
                    "icons/s1-albums.svg",
                    active_tab == Some(LibraryRootTab::Albums),
                    LibraryRootTab::Albums,
                    colors,
                    scale,
                    cx,
                ))
                .child(tab_icon_button(
                    "tab_artists",
                    "icons/s1-artists.svg",
                    active_tab == Some(LibraryRootTab::Artists),
                    LibraryRootTab::Artists,
                    colors,
                    scale,
                    cx,
                ))
                .when(genres_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_genres",
                        "icons/s1-genres.svg",
                        active_tab == Some(LibraryRootTab::Genres),
                        LibraryRootTab::Genres,
                        colors,
                        scale,
                        cx,
                    ))
                })
                .when(liked_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_liked",
                        "icons/s1-heart.svg",
                        active_tab == Some(LibraryRootTab::Liked),
                        LibraryRootTab::Liked,
                        colors,
                        scale,
                        cx,
                    ))
                })
                .when(playlists_enabled, |d| {
                    d.child(tab_icon_button(
                        "tab_playlists",
                        "icons/s1-playlists.svg",
                        active_tab == Some(LibraryRootTab::Playlists),
                        LibraryRootTab::Playlists,
                        colors,
                        scale,
                        cx,
                    ))
                })
                .child(cover_mode_button(cover_mode, colors, scale, cx))
            });
        let left_group = div()
            .flex_1()
            .flex()
            .items_center()
            .h_full()
            .child(left_buttons);

        let right_buttons = div()
            .flex()
            .items_center()
            .gap_2()
            .when(in_title_bar, guard)
            .when(update_ready && !show_screen, |d| {
                d.child(update_button(scale, cx))
            })
            .when(!show_screen && tools_enabled, |d| {
                d.child(tools_button(scale, cx))
            })
            .when(!show_screen, |d| d.child(settings_gear_button(scale, cx)))
            .child(self.audio_settings.clone());
        let right_group = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .h_full()
            .child(right_buttons);

        let root = div().flex().items_center().pl_2().pr_2();
        let root = match placement {
            Placement::Below(bg) => root.w_full().flex_shrink_0().h(px(HEIGHT * scale)).bg(bg),
            Placement::TitleBar => root.flex_1().h_full(),
        };
        root.child(left_group)
            .when(!show_screen && !cover_mode, |d| {
                let menu_slot = px(VIEW_MENU_GAP + view_menu::TRIGGER_SIZE * scale);
                d.child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .when(view_menu.is_some(), |d| d.child(div().w(menu_slot)))
                        .child(
                            div()
                                .w(px(SEARCH_WIDTH))
                                .relative()
                                .when(in_title_bar, guard)
                                .child(
                                    Input::new(&self.search_input)
                                        .with_size(Size::Medium)
                                        .focus_bordered(false)
                                        .rounded_full()
                                        .bg(cover_backdrop::field_bg(title_bar, veil)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .h_full()
                                        .left(px(-(INDICATOR_GAP
                                            + crate::library_scan_indicator::WIDTH * scale)))
                                        .flex()
                                        .items_center()
                                        .child(self.scan_indicator.clone()),
                                ),
                        )
                        .when_some(view_menu, |d, menu| {
                            d.child(
                                div()
                                    .w(menu_slot)
                                    .flex()
                                    .justify_end()
                                    .when(in_title_bar, guard)
                                    .child(menu),
                            )
                        }),
                )
            })
            .child(right_group)
    }
}

fn settings_gear_button(scale: f32, cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("settings_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40. * scale))
        .h(px(40. * scale))
        .icon(
            Icon::default()
                .path("icons/settings.svg")
                .size(px(20. * scale)),
        )
        .tooltip(tr().settings.clone())
        .on_click(cx.listener(|this, _, window, cx| this.open_settings(0, window, cx)))
}

fn tools_button(scale: f32, cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("tools_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40. * scale))
        .h(px(40. * scale))
        .icon(
            Icon::default()
                .path("icons/tools.svg")
                .size(px(20. * scale)),
        )
        .tooltip(crate::tools::title())
        .on_click(cx.listener(|this, _, window, cx| this.open_tools_page(0, window, cx)))
}

fn update_button(scale: f32, cx: &mut Context<MainView>) -> impl IntoElement {
    Button::new("update_button")
        .ghost()
        .compact()
        .rounded_full()
        .w(px(40. * scale))
        .h(px(40. * scale))
        .icon(
            Icon::default()
                .path("icons/update.svg")
                .size(px(20. * scale)),
        )
        .tooltip(tr().restart_to_update.clone())
        .on_click(cx.listener(|_, _, _, cx| updater::apply_and_restart(cx)))
}

fn back_button(
    fg: Hsla,
    hover_bg: Hsla,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    div()
        .id("back_button")
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .hover(move |style| style.bg(hover_bg))
        .on_click(cx.listener(|this, _, window, cx| {
            this.leave_overlays(window, cx);
            if this.show_settings || this.show_tools {
                this.close_screens();
                cx.notify();
            } else {
                this.library_view.update(cx, |view, cx| view.go_back(cx));
            }
        }))
        .child(
            svg()
                .path("icons/back.svg")
                .size(px(22. * scale))
                .text_color(fg),
        )
}

fn tab_icon_button(
    id: &'static str,
    icon_path: &'static str,
    active: bool,
    tab: LibraryRootTab,
    colors: TabColors,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let fg = if active {
        colors.primary
    } else {
        colors.foreground
    };
    let active_bg = colors.active_bg;
    let hover_bg = colors.hover_bg;

    div()
        .id(id)
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(active, move |d| d.bg(active_bg))
        .hover(move |s| s.bg(hover_bg))
        .tooltip(move |window, cx| {
            let label = match tab {
                LibraryRootTab::Albums => tr().tab_albums.clone(),
                LibraryRootTab::Artists => tr().tab_artists.clone(),
                LibraryRootTab::Genres => tr().tab_genres.clone(),
                LibraryRootTab::Liked => tr().tab_liked.clone(),
                LibraryRootTab::Playlists => tr().tab_playlists.clone(),
            };
            Tooltip::new(label).build(window, cx)
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.leave_overlays(window, cx);
            this.library_view
                .update(cx, |view, cx| view.select_tab(tab, cx));
            cx.notify();
        }))
        .child(svg().path(icon_path).size(px(20. * scale)).text_color(fg))
}

fn cover_mode_button(
    active: bool,
    colors: TabColors,
    scale: f32,
    cx: &mut Context<MainView>,
) -> impl IntoElement {
    let fg = if active {
        colors.primary
    } else {
        colors.foreground
    };
    let active_bg = colors.active_bg;
    let hover_bg = colors.hover_bg;

    div()
        .id("tab_cover_mode")
        .size(px(36. * scale))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(active, move |d| d.bg(active_bg))
        .hover(move |s| s.bg(hover_bg))
        .tooltip(|window, cx| Tooltip::new(tr().cover_mode.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.toggle_cover_mode(window, cx);
        }))
        .child(
            svg()
                .path("icons/s1-cover.svg")
                .size(px(20. * scale))
                .text_color(fg),
        )
}
