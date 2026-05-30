# `settings` module

A small, self-contained settings widget used by the app's settings screen.

## What it is

A **top tab bar** (one tab per `SettingPage`) over a **scrollable content area**.
Intentionally minimal: **no search box and no category sidebar**. It replaces the
`gpui-component` `setting::Settings` widget, which forced a sidebar + search layout we
couldn't shape.

## Hierarchy

```
Settings              tab bar + scrollable body; owns active-tab + scroll state
  SettingPage         one tab
    SettingGroup      a titled card of rows
      SettingItem     label + optional description + a field
        SettingField  the control element (switch, dropdown, button row, …)
```

## Responsibility split (important)

This widget owns **only layout**. All *behavior* — reading current values, applying
changes instantly, persistence, events — lives in the `SettingField::render` closures
that the **caller** supplies (in `pawse`, see `crates/pawse/src/settings_view.rs`). That
keeps `ui_components` free of any dependency on the app's settings store / localization
and avoids a dependency cycle (`pawse` depends on `ui_components`, not the reverse).

## State

Active tab index and the content `ScrollHandle` are stored via
`Window::use_keyed_state` keyed on the `Settings` id, so they persist across the
re-renders triggered by every settings change. A single scroll handle is shared by all
tabs, so switching tabs resets the offset to the top — otherwise a shorter page would
render scrolled past its content.

## Theming

Colors come straight from `cx.theme()` (`gpui_component::ActiveTheme`) — `border`,
`foreground`, `muted_foreground`, `secondary`, `secondary_hover`, `primary`,
`background` — matching the rest of the app without importing `pawse`'s `Colors`.
