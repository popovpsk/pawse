use gpui::{Pixels, px};

/// Right-hand gutter a scrolling container must keep clear for its scrollbar.
///
/// `gpui_component` lays the scrollbar band against its container's right edge and
/// reserves this much for it (`THUMB_ACTIVE_INSET * 2 + THUMB_ACTIVE_WIDTH`; the crate's
/// own constant is private, so check this against `scroll/scrollbar.rs` when bumping the
/// version). Nothing stops the band from painting over what is underneath, so content
/// that runs to the edge gets a thumb drawn across it — reserving the band's own width
/// gives the thumb an empty column instead.
///
/// Pushing the band *outward* into a surrounding element's padding is not an
/// alternative: a negative margin on the scroll container makes the scrollbar vanish.
pub const SCROLLBAR_GUTTER: Pixels = px(16.);
