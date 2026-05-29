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

/// Resolve the persisted choice to a concrete language (System → OS locale).
/// Used only at sync points, NOT per render.
pub fn resolve_from_settings(cx: &App) -> Lang {
    match cx.global::<SettingsStore>().language() {
        LangChoice::System => Lang::from_locale(&sys_locale::get_locale().unwrap_or_default()),
        LangChoice::Named(code) => Lang::from_code(&code).unwrap_or(Lang::En),
    }
}

/// Push the resolved language into the global cache. Call at startup and after
/// any `set_language`.
pub fn sync_active_lang(cx: &App) {
    ui_resources::i18n::set_active(resolve_from_settings(cx));
}

/// Active language's string table — the per-label hot path. Cheap: atomic load
/// + match. (`cx` is currently kept for signature stability; see Phase 2.)
pub fn tr() -> &'static Strings {
    ui_resources::i18n::strings()
}
