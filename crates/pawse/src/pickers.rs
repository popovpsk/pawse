use gpui::{
    Anchor, AnyElement, App, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, div, px,
};
use gpui_component::{
    Sizable, ThemeRegistry,
    button::Button,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use ui_resources::i18n::Lang;

use crate::localization::tr;
use crate::settings_store::{
    LangChoice, SettingsStore, ThemeChoice, apply_theme, notify_save_error,
};

const MENU_MAX_HEIGHT: f32 = 360.;

fn theme_label(choice: &ThemeChoice) -> SharedString {
    match choice {
        ThemeChoice::System => tr().system.clone(),
        ThemeChoice::Named(name) => SharedString::from(name.clone()),
    }
}

fn theme_options(cx: &App) -> Vec<ThemeChoice> {
    let mut themes = ThemeRegistry::global(cx).sorted_themes();
    themes.sort_by(|a, b| {
        b.mode
            .is_dark()
            .cmp(&a.mode.is_dark())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    std::iter::once(ThemeChoice::System)
        .chain(
            themes
                .into_iter()
                .map(|theme| ThemeChoice::Named(theme.name.to_string())),
        )
        .collect()
}

fn choose_theme(choice: &ThemeChoice, cx: &mut App) {
    if let Err(e) = cx.global_mut::<SettingsStore>().set_theme(choice.clone()) {
        notify_save_error(cx, e);
    }
    apply_theme(choice, cx);
    crate::cover_skin::reapply(cx);
}

fn restore_saved_theme(cx: &mut App) {
    let saved = cx.global::<SettingsStore>().theme();
    apply_theme(&saved, cx);
    crate::cover_skin::reapply(cx);
}

fn theme_menu(menu: PopupMenu, cx: &App) -> PopupMenu {
    let current = cx.global::<SettingsStore>().theme();
    theme_options(cx).into_iter().enumerate().fold(
        menu.max_h(px(MENU_MAX_HEIGHT)).scrollable(true),
        |menu, (ix, choice)| {
            let checked = choice == current;
            let label = theme_label(&choice);
            let preview = choice.clone();
            menu.item(
                PopupMenuItem::element(move |_, _| {
                    let preview = preview.clone();
                    div()
                        .id(("theme-option", ix))
                        .child(label.clone())
                        .on_hover(move |hovered, _, cx| {
                            if *hovered {
                                apply_theme(&preview, cx);
                            }
                        })
                })
                .checked(checked)
                .on_click(move |_, _, cx| choose_theme(&choice, cx)),
            )
        },
    )
}

pub fn theme_dropdown(id: &'static str, cx: &App) -> AnyElement {
    let label = theme_label(&cx.global::<SettingsStore>().theme());
    Button::new(id)
        .small()
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, |menu, _, cx| theme_menu(menu, cx))
        .on_open_change(|open, _, cx| {
            if !*open {
                restore_saved_theme(cx);
            }
        })
        .into_any_element()
}

fn lang_label(choice: &LangChoice) -> SharedString {
    match choice {
        LangChoice::System => tr().system.clone(),
        LangChoice::Named(code) => Lang::all()
            .iter()
            .find(|lang| lang.code() == code)
            .map_or_else(|| tr().unknown.clone(), |lang| lang.display_name().into()),
    }
}

fn choose_lang(choice: LangChoice, cx: &mut App) {
    if let Err(e) = cx.global_mut::<SettingsStore>().set_language(choice) {
        notify_save_error(cx, e);
    }
    crate::localization::notify_lang_changed(cx);
    crate::app_menu::set_menus(cx);
    cx.refresh_windows();
}

fn lang_menu(menu: PopupMenu, cx: &App) -> PopupMenu {
    let current = cx.global::<SettingsStore>().language();
    let options = std::iter::once(LangChoice::System).chain(
        Lang::all()
            .iter()
            .map(|lang| LangChoice::Named(lang.code().to_string())),
    );
    options.fold(
        menu.max_h(px(MENU_MAX_HEIGHT)).scrollable(true),
        |menu, choice| {
            let checked = choice == current;
            menu.item(
                PopupMenuItem::new(lang_label(&choice))
                    .checked(checked)
                    .on_click(move |_, _, cx| choose_lang(choice.clone(), cx)),
            )
        },
    )
}

pub fn language_dropdown(id: &'static str, cx: &App) -> AnyElement {
    let label = lang_label(&cx.global::<SettingsStore>().language());
    Button::new(id)
        .small()
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, |menu, _, cx| lang_menu(menu, cx))
        .into_any_element()
}
