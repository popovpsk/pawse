use audio_engine::EngineEvent;
use gpui::{
    AnyElement, Context, Entity, IntoElement, ParentElement, Render, StyleRefinement, Styled,
    Subscription, div,
};

use crate::cast::CastState;
use crate::library_service::LibraryEvent;
use crate::playback_status::StatusChanged;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub fn block<V: Render>(view: &Entity<V>, style: StyleRefinement, reuse: bool) -> AnyElement {
    if reuse {
        return view.clone().cached(style).into_any_element();
    }
    let mut slot = div();
    *slot.style() = style;
    slot.child(view.clone()).into_any_element()
}

pub fn full<V: Render>(view: &Entity<V>) -> AnyElement {
    block(view, StyleRefinement::default().size_full(), true)
}

pub struct AppEvents {
    _subscriptions: [Subscription; 5],
}

pub fn follow_app_events<V: 'static>(cx: &mut Context<V>) -> AppEvents {
    let services = cx.global::<Services>();
    let engine = services.engine_event_bus.clone();
    let library = services.library_event_bus.clone();
    let status = services.playback_status.clone();
    let subscriptions = [
        cx.observe_global::<SettingsStore>(|_, cx| cx.notify()),
        cx.observe_global::<CastState>(|_, cx| cx.notify()),
        cx.subscribe(&engine, |_, _, event: &EngineEvent, cx| {
            if !matches!(event, EngineEvent::PositionChanged(_)) {
                cx.notify();
            }
        }),
        cx.subscribe(&library, |_, _, event: &LibraryEvent, cx| {
            if !matches!(
                event,
                LibraryEvent::ScanProgress { .. } | LibraryEvent::RemoteSyncProgress { .. }
            ) {
                cx.notify();
            }
        }),
        cx.subscribe(&status, |_, _, _: &StatusChanged, cx| cx.notify()),
    ];
    AppEvents {
        _subscriptions: subscriptions,
    }
}
