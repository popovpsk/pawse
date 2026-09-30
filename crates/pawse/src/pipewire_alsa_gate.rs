use gpui::{
    App, AppContext, ClipboardItem, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    theme::ThemeRegistry,
    v_flex,
};

use crate::localization::tr;
use crate::theme_colors::Colors;

const INSTALL_COMMANDS: &[(&str, &str)] = &[
    (
        "Debian, Ubuntu, Raspberry Pi OS",
        "sudo apt install pipewire-alsa",
    ),
    ("Fedora", "sudo dnf install pipewire-alsa"),
    ("Arch, Manjaro, CachyOS", "sudo pacman -S pipewire-alsa"),
    ("openSUSE", "sudo zypper install pipewire-alsa"),
    (
        "Void",
        "sudo xbps-install -S alsa-pipewire && sudo mkdir -p /etc/alsa/conf.d && sudo ln -sf /usr/share/alsa/alsa.conf.d/50-pipewire.conf /usr/share/alsa/alsa.conf.d/99-pipewire-default.conf /etc/alsa/conf.d/",
    ),
    ("NixOS", "services.pipewire.alsa.enable = true;"),
];

pub fn should_block_startup() -> bool {
    audio_output::pipewire_alsa_missing() && !std::path::Path::new("/.flatpak-info").exists()
}

pub fn open(cx: &mut App) {
    let options = crate::build_window_options(cx);
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| PipewireAlsaGate::new(window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    })
    .expect("Failed to open PipeWire ALSA gate window");
}

pub struct PipewireAlsaGate {
    scroll: ScrollHandle,
    _theme_registry_subscription: gpui::Subscription,
}

impl PipewireAlsaGate {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            _theme_registry_subscription: cx.observe_global::<ThemeRegistry>(|_, cx| cx.notify()),
        }
    }
}

fn command_row(
    ix: usize,
    distro: &'static str,
    command: &'static str,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .text_xs()
                .text_color(Colors::muted_foreground(cx))
                .child(SharedString::new_static(distro)),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .w_full()
                .px_3()
                .py_2()
                .rounded(px(6.))
                .bg(Colors::muted(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_sm()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(Colors::foreground(cx))
                        .child(SharedString::new_static(command)),
                )
                .child(
                    Button::new(("pw-alsa-copy", ix))
                        .small()
                        .label(tr().copy.clone())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(command.to_string()))
                        }),
                ),
        )
}

impl Render for PipewireAlsaGate {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = tr();
        let mut commands = v_flex().gap_3().w_full();
        for (ix, (distro, command)) in INSTALL_COMMANDS.iter().enumerate() {
            commands = commands.child(command_row(ix, distro, command, cx));
        }

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(Colors::title_bar(cx))
            .child(
                div()
                    .id("pw-alsa-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .w_full()
                            .min_h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p_4()
                            .child(
                                v_flex()
                                    .w_full()
                                    .max_w(px(560.))
                                    .gap_5()
                                    .p_8()
                                    .rounded(px(12.))
                                    .bg(Colors::background(cx))
                                    .border_1()
                                    .border_color(Colors::border(cx))
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_xl()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(Colors::foreground(cx))
                                                    .child(s.pipewire_alsa_missing_title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(Colors::muted_foreground(cx))
                                                    .child(s.pipewire_alsa_missing_body.clone()),
                                            ),
                                    )
                                    .child(commands)
                                    .child(
                                        h_flex().justify_end().child(
                                            Button::new("pw-alsa-quit")
                                                .primary()
                                                .small()
                                                .label(s.quit_pawse.clone())
                                                .on_click(|_, _, cx| cx.quit()),
                                        ),
                                    ),
                            ),
                    ),
            )
            .vertical_scrollbar(&self.scroll)
    }
}
