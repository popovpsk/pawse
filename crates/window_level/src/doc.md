# window_level

Keeps a window above other applications' windows, or lets it go back to the normal level,
and caps how far the user can stretch a window, through the native handle gpui exposes
(`HasWindowHandle`). gpui fixes a window's level when it creates the window and has no
setter, has no maximum size at all, and its `WindowKind::PopUp` / `Floating` are wrong for
a normal window (no caption and taskbar entry on Windows, unmanaged on X11, a panel class
on macOS). No GPUI, no pawse dependencies.

Not here: which window is kept on top, the setting and the pin button
(`crates/pawse/src/mini_player/doc.md`).

## Files

- `lib.rs` — `supported`, `set_always_on_top`, `set_size_limits` (`SizeLimits`), `Error`;
  dispatch on the raw handle.
- `macos.rs` — `NSWindow.level` (floating or normal), `contentMinSize` / `contentMaxSize`.
- `win32.rs` — `SetWindowPos` with `HWND_TOPMOST` / `HWND_NOTOPMOST`; a window subclass
  that fills `ptMaxTrackSize` on `WM_GETMINMAXINFO`.
- `x11.rs` — a `_NET_WM_STATE` client message (add or remove `_NET_WM_STATE_ABOVE`) to
  the root window, and the min and max size in `WM_NORMAL_HINTS`, each over a
  short-lived x11rb connection.

## Rules

- `unsafe` is allowed in this crate only for FFI: the `NSView` pointer from the handle,
  `SetWindowPos`, and the Win32 subclass (its procedure, the `MINMAXINFO` pointer). The
  X11 path is safe x11rb.
- Call on the UI thread (AppKit wants the main thread).
- gpui never sets a level, topmost flag or `_NET_WM_STATE` after creating a window, so a
  value set here stays until it is set again.
- X11: the window manager ignores `_NET_WM_STATE` requests for a window that is not mapped
  yet, so apply after the first frame.
- `set_size_limits` takes logical window sizes (content plus any client-side frame) and
  the window's scale factor. It limits what the user can drag to, not the window's current
  size and not programmatic resizes: clamp a restored size yourself.
- Win32 sets only the maximum: gpui's own `WM_GETMINMAXINFO` handler fills the minimum
  correctly and runs before ours. The size sits in the subclass's reference data (16 bits
  per side); the frame measured at the time is added; the subclass removes itself on
  `WM_NCDESTROY`.
- X11 writes both, scaled: gpui stores its minimum in `WM_NORMAL_HINTS` unscaled, so on a
  HiDPI X11 screen it is too small.
- Wayland has no protocol for this: `supported` is false, `set_always_on_top` and
  `set_size_limits` return `Unsupported`. So does every other handle kind.
- macOS: only the level changes. The window stays on its Space and does not show over
  another app's full-screen Space (that would take collection behaviour changes).
