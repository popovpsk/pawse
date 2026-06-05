# Icons

All UI icons are embedded into the binary at compile time via `rust-embed`. No external files are needed at runtime.

## How It Works

The `Assets` struct in `crates/ui_resources/src/assets.rs` uses `#[derive(RustEmbed)]` to compile everything under `assets/icons/` into the binary:

```rust
#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*"]
pub struct Assets;
```

This struct implements GPUI's `AssetSource` trait, which is registered on startup in `main.rs`:

```rust
let app = Application::new().with_assets(ui_resources::assets::Assets);
```

`gpui-component` does not ship any SVG files — its `IconName::path()` returns
paths like `icons/folder.svg`, which must be supplied here. Any `IconName` the app
(or a `gpui-component` component it uses) renders needs a matching kebab-case SVG
in `assets/icons/`, or GPUI logs `asset not found: icons/...` on every render.

## Adding a New Icon

1. Place the SVG file in `assets/icons/`. Use kebab-case names, e.g. `play.svg`, `volume-mute.svg`.
2. The SVG is rendered as a **monochrome** alpha mask by GPUI's `svg()` element. The actual color is applied via `text_color()` at runtime.
3. **Do not** use SVGs with background rectangles or complex multi-color fills. Remove any `fill="white"` background rects. Only the icon shape itself should have paths.
4. Rebuild — the icon is now available at runtime via `"icons/{filename}.svg"`.

## Using an Icon in a Component

Import `svg` from GPUI and wrap it in a clickable `div`:

```rust
use gpui::{svg, InteractiveElement, StatefulInteractiveElement, Styled, div, px};
use gpui_component::ActiveTheme;

div()
    .id("my_button")
    .cursor_pointer()
    .size(px(36.))
    .flex()
    .items_center()
    .justify_center()
    .rounded_full()
    .hover(|style| style.bg(cx.theme().muted))
    .on_click(cx.listener(Self::on_click))
    .child(
        svg()
            .path("icons/my-icon.svg")
            .size(px(16.))
            .text_color(cx.theme().foreground),
    )
```

### Key points

- `svg().path("icons/...")` resolves the path against the embedded asset source.
- `.text_color()` tints the entire icon. Use `cx.theme().foreground` for standard icons, or any `Hsla` color.
- Always wrap the icon in an interactive container (`div` with `on_click`) rather than using `gpui_component::Button` with an image label — the latter is inconsistent with the monochrome icon style.
- Size the container and the icon separately: the container provides the hit area and hover state, the icon provides the visual.

## Transformations

GPUI's `svg()` element supports affine transformations via `.with_transformation()`. This is useful when one icon can serve two directions (e.g., a single `next.svg` used for both next and previous):

```rust
use gpui::{svg, size, Transformation};

// Next track (no flip)
svg().path("icons/next.svg").size(px(16.))

// Previous track — flip horizontally
svg()
    .path("icons/next.svg")
    .size(px(16.))
    .with_transformation(Transformation::scale(size(-1.0, 1.0)))
```

**Note:** The transformation only affects rendering, not layout or hit testing.

## Existing Icons

| Path | Usage |
|------|-------|
| `icons/back.svg` | Back to album list button |
| `icons/check.svg` | Bit-perfect OK indicator or selected device in popover (`IconName::Check`) |
| `icons/next.svg` | Next track button (flipped horizontally for previous track) |
| `icons/triangle-alert.svg` | Bit-perfect warning indicator + `gpui-component` warning toast (`IconName::TriangleAlert`) |
| `icons/volume_mute.svg` | Volume control — shown when muted or volume is 0 |
| `icons/volume_unmute.svg` | Volume control — shown when volume > 0 |
| `icons/folder.svg` | `gpui-component` `IconName::Folder` — settings folder list, onboarding |
| `icons/chevron-down.svg` | `gpui-component` `IconName::ChevronDown` — settings dropdown triggers |
| `icons/circle-x.svg` | `gpui-component` `IconName::CircleX` — search input clear button, error toast |
| `icons/info.svg` | `gpui-component` `IconName::Info` — info toast |
| `icons/circle-check.svg` | `gpui-component` `IconName::CircleCheck` — success toast |
| `icons/close.svg` | `gpui-component` `IconName::Close` — notification close button |

## Do Not Use `img()` for Icons

`img()` rasterizes SVGs as full-color images. This works for cover art but is wrong for UI icons because:

- It bloats the atlas with rasterized pixels instead of crisp vector masks.
- It cannot adapt to theme colors (light / dark mode).
- It does not scale cleanly to arbitrary sizes.

Always use `svg()` for icons and `img()` only for photographs / cover art.
