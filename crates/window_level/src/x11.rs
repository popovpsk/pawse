use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};
use x11rb::rust_connection::RustConnection;

use crate::{Error, SizeLimits};

const REMOVE: u32 = 0;
const ADD: u32 = 1;
const SOURCE_APPLICATION: u32 = 1;

pub(crate) fn set(window: u32, on: bool) -> Result<(), Error> {
    let (conn, screen) = x11rb::connect(None).map_err(platform)?;
    let state = intern(&conn, b"_NET_WM_STATE")?;
    let above = intern(&conn, b"_NET_WM_STATE_ABOVE")?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .map(|screen| screen.root)
        .ok_or_else(|| Error::Platform("no X11 screen".into()))?;
    let action = if on { ADD } else { REMOVE };
    let event =
        ClientMessageEvent::new(32, window, state, [action, above, 0, SOURCE_APPLICATION, 0]);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .map_err(platform)?;
    conn.flush().map_err(platform)?;
    Ok(())
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<u32, Error> {
    Ok(conn
        .intern_atom(false, name)
        .map_err(platform)?
        .reply()
        .map_err(platform)?
        .atom)
}

fn platform(err: impl std::fmt::Display) -> Error {
    Error::Platform(err.to_string())
}

pub(crate) fn set_size_limits(window: u32, limits: SizeLimits, scale: f32) -> Result<(), Error> {
    let (conn, _) = x11rb::connect(None).map_err(platform)?;
    let mut hints = WmSizeHints::get_normal_hints(&conn, window)
        .map_err(platform)?
        .reply()
        .map_err(platform)?
        .unwrap_or_default();
    let physical = |(w, h): (f32, f32)| ((w * scale) as i32, (h * scale) as i32);
    hints.min_size = Some(physical(limits.min));
    hints.max_size = Some(physical(limits.max));
    hints.set_normal_hints(&conn, window).map_err(platform)?;
    conn.flush().map_err(platform)?;
    Ok(())
}
