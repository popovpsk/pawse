//! Resolves the active UI language from settings and exposes [`tr`], the entry
//! point every view uses to read localized strings.
//!
//! The translation tables live in the `ui_resources` crate (the UI data layer);
//! which language is *active* is app state, so it lives here in `pawse` next to
//! `SettingsStore`. This keeps the dependency one-directional
//! (`pawse → ui_resources`).

use gpui::{App, EventEmitter};
use ui_resources::i18n::{Lang, Strings};

use crate::services::Services;
use crate::settings_store::{LangChoice, SettingsStore};

/// Emitted app-wide when the active UI language changes. Views that cache
/// localized strings off the render hot path (e.g. precomputed `SharedString`s)
/// subscribe to this to rebuild them; views that only call [`tr`] inside
/// `render` need not subscribe — `refresh_windows` already repaints them.
pub struct LangChanged;

/// Tiny event bus carrying [`LangChanged`]. Stored on `Services` so any view
/// can subscribe. Mirrors the other per-domain buses (library, engine).
pub struct LangEventBus;
impl EventEmitter<LangChanged> for LangEventBus {}

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

/// Sync the active language and notify subscribers via [`LangEventBus`]. Call
/// after a user-driven language change (not at startup — no views exist yet, so
/// [`sync_active_lang`] alone is enough there).
pub fn notify_lang_changed(cx: &mut App) {
    sync_active_lang(cx);
    let bus = cx.global::<Services>().lang_event_bus.clone();
    bus.update(cx, |_, cx| cx.emit(LangChanged));
}

/// Active language's string table — the per-label hot path. Cheap: atomic load
/// + match. (`cx` is currently kept for signature stability; see Phase 2.)
pub fn tr() -> &'static Strings {
    ui_resources::i18n::strings()
}
