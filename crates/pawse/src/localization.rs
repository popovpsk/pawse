//! Resolves the active UI language from settings and exposes [`tr`], the entry
//! point every view uses to read localized strings.
//!
//! The translation tables live in the `ui_resources` crate (the UI data layer);
//! which language is *active* is app state, so it lives here in `pawse` next to
//! `SettingsStore`. This keeps the dependency one-directional
//! (`pawse → ui_resources`).

use gpui::App;
use ui_resources::i18n::{Lang, Strings};

use crate::settings_store::{LangChoice, SettingsStore};

/// The active language, resolving `System` to the detected OS locale.
pub fn current_lang(cx: &App) -> Lang {
    match cx.global::<SettingsStore>().language() {
        LangChoice::System => Lang::from_locale(&sys_locale::get_locale().unwrap_or_default()),
        LangChoice::Named(code) => Lang::from_code(&code).unwrap_or(Lang::En),
    }
}

/// The string table for the active language. Returns a `'static` reference, so
/// reading it in a render closure is allocation-free; clone the `SharedString`
/// fields directly (the static variant clones for free).
pub fn tr(cx: &App) -> &'static Strings {
    current_lang(cx).strings()
}
