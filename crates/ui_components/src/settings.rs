//! A small, self-contained settings widget: a top tab bar (one tab per page)
//! over a scrollable content area. Deliberately minimal — no search box, no
//! category sidebar — so the consuming app fully controls the contents.
//!
//! Hierarchy:
//!
//! ```ignore
//! Settings              <- tab bar + scrollable body, owns the active-tab state
//!   SettingPage         <- one tab
//!     SettingGroup      <- a titled card of rows
//!       SettingItem     <- label + optional description + a field control
//!         SettingField  <- the control element (switch, dropdown, …)
//! ```
//!
//! All business logic (reading/writing values, instant apply, persistence)
//! lives in the `SettingField` render closures supplied by the caller; this
//! widget only lays them out.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Axis, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div, point,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, StyledExt,
    scroll::ScrollableElement,
    tab::{Tab, TabBar},
    v_flex,
};

/// Boxed render closure for a [`SettingField`].
type FieldRenderer = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// The control element rendered on the right (or below) a setting row.
///
/// The closure is invoked on every render, so it can read live state from the
/// app and wire `on_click` handlers that write back immediately.
#[derive(Clone)]
pub struct SettingField(FieldRenderer);

impl SettingField {
    /// Build a field from a render closure.
    pub fn render<E, R>(render: R) -> Self
    where
        E: IntoElement,
        R: Fn(&mut Window, &mut App) -> E + 'static,
    {
        Self(Rc::new(move |window, cx| {
            render(window, cx).into_any_element()
        }))
    }
}

/// A single setting row.
#[derive(Clone)]
pub struct SettingItem {
    label: SharedString,
    description: Option<SharedString>,
    layout: Axis,
    field: SettingField,
}

impl SettingItem {
    /// Create a row with a label and a field. Defaults to a horizontal layout
    /// (label on the left, field on the right).
    pub fn new(label: impl Into<SharedString>, field: SettingField) -> Self {
        Self {
            label: label.into(),
            description: None,
            layout: Axis::Horizontal,
            field,
        }
    }

    /// Add a secondary description line under the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// `Axis::Vertical` stacks the field under the label (used for the folder
    /// list); the default `Axis::Horizontal` puts it on the right.
    pub fn layout(mut self, layout: Axis) -> Self {
        self.layout = layout;
        self
    }

    fn render(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let horizontal = self.layout == Axis::Horizontal;

        let label = v_flex()
            .when(horizontal, |this| this.flex_1().overflow_hidden())
            .when(!horizontal, |this| this.w_full())
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(self.label.clone()),
            )
            .when_some(self.description.clone(), |this, desc| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(desc),
                )
            });

        let field = (self.field.0)(window, cx);
        let field = if horizontal {
            div().flex_shrink_0().child(field).into_any_element()
        } else {
            field
        };

        div()
            .w_full()
            .flex()
            .gap_3()
            .when(horizontal, |this| {
                this.flex_row().justify_between().items_start()
            })
            .when(!horizontal, |this| this.flex_col())
            .child(label)
            .child(field)
            .into_any_element()
    }
}

/// A titled card grouping related rows.
#[derive(Clone, Default)]
pub struct SettingGroup {
    title: Option<SharedString>,
    items: Vec<SettingItem>,
}

impl SettingGroup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Optional heading shown above the card.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn item(mut self, item: SettingItem) -> Self {
        self.items.push(item);
        self
    }

    fn render(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        v_flex()
            .gap_3()
            .when_some(self.title.clone(), |this, title| {
                this.child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
            })
            .child(
                v_flex()
                    .gap_4()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .children(self.items.iter().map(|item| item.render(window, cx))),
            )
            .into_any_element()
    }
}

/// A single tab's worth of settings.
#[derive(Clone)]
pub struct SettingPage {
    title: SharedString,
    groups: Vec<SettingGroup>,
}

impl SettingPage {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            groups: Vec::new(),
        }
    }

    pub fn group(mut self, group: SettingGroup) -> Self {
        self.groups.push(group);
        self
    }
}

/// Persistent state for the widget: which tab is active and the content scroll
/// position. Held via [`Window::use_keyed_state`] so it survives re-renders.
struct SettingsState {
    active: usize,
    scroll: ScrollHandle,
}

/// The settings widget. Construct with [`Settings::new`], add [`Settings::pages`],
/// then render it as a child element.
#[derive(IntoElement)]
pub struct Settings {
    id: ElementId,
    pages: Vec<SettingPage>,
}

impl Settings {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            pages: Vec::new(),
        }
    }

    pub fn pages(mut self, pages: impl IntoIterator<Item = SettingPage>) -> Self {
        self.pages.extend(pages);
        self
    }
}

impl RenderOnce for Settings {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state: Entity<SettingsState> =
            window.use_keyed_state(self.id.clone(), cx, |_, _| SettingsState {
                active: 0,
                scroll: ScrollHandle::new(),
            });

        // Clamped for this render only. The stored `active` can't actually go out of
        // range today (fixed tab set; clicks always store a valid index), so an
        // out-of-range stored value is tolerated rather than written back.
        let active = state
            .read(cx)
            .active
            .min(self.pages.len().saturating_sub(1));
        let scroll = state.read(cx).scroll.clone();

        let tab_bar = TabBar::new("settings-tabs")
            .underline()
            .w_full()
            .flex_shrink_0()
            .px_3()
            .selected_index(active)
            .children(
                self.pages
                    .iter()
                    .map(|page| Tab::new().label(page.title.clone())),
            )
            .on_click({
                let state = state.clone();
                move |index, _, cx| {
                    let i = *index;
                    state.update(cx, |s, cx| {
                        if s.active == i {
                            return;
                        }
                        s.active = i;
                        s.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    });
                }
            });

        let groups = self
            .pages
            .get(active)
            .map(|page| {
                page.groups
                    .iter()
                    .map(|g| g.render(window, cx))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        v_flex()
            .id(self.id.clone())
            .size_full()
            .child(tab_bar)
            .child(
                // `vertical_scrollbar` is only implemented for `Div`, so it goes on
                // this plain (non-stateful) outer container; the inner stateful child
                // owns the scroll position via `track_scroll` against the same handle.
                div()
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .id("settings-content")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .p_4()
                            .gap_6()
                            .children(groups),
                    )
                    .vertical_scrollbar(&scroll),
            )
    }
}
