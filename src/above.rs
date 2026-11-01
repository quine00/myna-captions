//! Keep the overlay above other windows. Only X11 has a way to ask: on
//! GNOME Wayland no client protocol lets a window stay on top, so there the
//! app runs on XWayland unless told otherwise (`--wayland`).

use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};

/// `_NET_WM_STATE_ADD`.
const ADD: u32 = 1;

pub fn keep_above(window: &gtk::Window) {
    window.connect_map(|window| {
        let Some(surface) = window.surface().and_downcast::<gdk4_x11::X11Surface>() else { return };
        if let Err(e) = request(surface.xid() as u32) {
            eprintln!("myna-captions: cannot keep the overlay above: {e}");
        }
    });
}

fn request(window: u32) -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &str| -> Result<u32, Box<dyn std::error::Error>> {
        Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    };
    let state = atom("_NET_WM_STATE")?;
    let skip = [atom("_NET_WM_STATE_SKIP_TASKBAR")?, atom("_NET_WM_STATE_SKIP_PAGER")?];
    for (first, second) in [(atom("_NET_WM_STATE_ABOVE")?, atom("_NET_WM_STATE_STICKY")?), (skip[0], skip[1])] {
        let event = ClientMessageEvent::new(32, window, state, [ADD, first, second, 1, 0]);
        conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?;
    }
    conn.flush()?;
    Ok(())
}
