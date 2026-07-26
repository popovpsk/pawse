use gpui::{Pixels, px};

/// Width `gpui_component` lays its scrollbar band into, against the container's right
/// edge (`THUMB_ACTIVE_INSET * 2 + THUMB_ACTIVE_WIDTH`; the crate's own constant is
/// private, so check this against `scroll/scrollbar.rs` when bumping the version).
///
/// Reserve it only where the thumb would land on something with an edge of its own —
/// the tag dialog's inputs, whose borders it visibly crossed. Lists deliberately do
/// **not**: the thumb floats over the rows there, which is what Zed does and what this
/// project settled on after trying the alternative.
///
/// Pushing the band outward into a surrounding element's padding is not an option: a
/// negative margin on the scroll container makes the scrollbar vanish.
pub const SCROLLBAR_GUTTER: Pixels = px(16.);
