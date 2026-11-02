//! The overlay's X11 behaviour: kept above other windows, and click-through.
//!
//! GNOME Wayland gives no client a way to stay on top, so the app runs on
//! XWayland unless told otherwise (`--wayland`) and asks the window manager
//! here. Clicks pass through the overlay to whatever is under it (video
//! controls, a call's buttons) unless Ctrl is held with the pointer over it,
//! which makes it take input so it can be dragged, resized or closed.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{ClientMessageEvent, ClipOrdering, ConnectionExt, EventMask, KeyButMask};
use x11rb::rust_connection::RustConnection;

/// `_NET_WM_STATE_ADD`.
const ADD: u32 = 1;
const POLL: Duration = Duration::from_millis(100);

type Error = Box<dyn std::error::Error>;

pub fn install(window: &gtk::Window) {
    window.connect_map(|window| {
        let Some(surface) = window.surface().and_downcast::<gdk4_x11::X11Surface>() else { return };
        match X11::connect(surface.xid() as u32) {
            Ok(x11) => {
                if let Err(e) = x11.keep_above() {
                    eprintln!("myna-captions: cannot keep the overlay above: {e}");
                }
                x11.watch(window);
            }
            Err(e) => eprintln!("myna-captions: no X11 connection for the overlay: {e}"),
        }
    });
}

struct X11 {
    conn: RustConnection,
    root: u32,
    window: u32,
}

impl X11 {
    fn connect(window: u32) -> Result<Rc<Self>, Error> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(Rc::new(Self { conn, root, window }))
    }

    fn atom(&self, name: &str) -> Result<u32, Error> {
        Ok(self.conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }

    fn keep_above(&self) -> Result<(), Error> {
        let state = self.atom("_NET_WM_STATE")?;
        let pairs = [
            (self.atom("_NET_WM_STATE_ABOVE")?, self.atom("_NET_WM_STATE_STICKY")?),
            (self.atom("_NET_WM_STATE_SKIP_TASKBAR")?, self.atom("_NET_WM_STATE_SKIP_PAGER")?),
        ];
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        for (first, second) in pairs {
            let event = ClientMessageEvent::new(32, self.window, state, [ADD, first, second, 1, 0]);
            self.conn.send_event(false, self.root, mask, event)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Take input only while Ctrl is held over the window.
    fn watch(self: &Rc<Self>, window: &gtk::Window) {
        let interactive = Cell::new(None);
        let x11 = self.clone();
        let weak = window.downgrade();
        glib::timeout_add_local(POLL, move || {
            if weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            let wanted = x11.ctrl_over_window().unwrap_or(false);
            if interactive.get() != Some(wanted) && x11.set_input(wanted).is_ok() {
                interactive.set(Some(wanted));
            }
            glib::ControlFlow::Continue
        });
    }

    fn ctrl_over_window(&self) -> Result<bool, Error> {
        let pointer = self.conn.query_pointer(self.root)?.reply()?;
        if u16::from(pointer.mask) & u16::from(KeyButMask::CONTROL) == 0 {
            return Ok(false);
        }
        let geometry = self.conn.get_geometry(self.window)?.reply()?;
        let origin = self.conn.translate_coordinates(self.window, self.root, 0, 0)?.reply()?;
        let (x, y) = (pointer.root_x - origin.dst_x, pointer.root_y - origin.dst_y);
        Ok(x >= 0 && y >= 0 && (x as u16) < geometry.width && (y as u16) < geometry.height)
    }

    /// The whole window takes input, or none of it does.
    fn set_input(&self, interactive: bool) -> Result<(), Error> {
        if interactive {
            self.conn.shape_mask(SO::SET, SK::INPUT, self.window, 0, 0, x11rb::NONE)?;
        } else {
            self.conn.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, self.window, 0, 0, &[])?;
        }
        self.conn.flush()?;
        Ok(())
    }
}
