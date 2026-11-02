//! The overlay's X11 behaviour: kept above other windows, and click-through.
//!
//! GNOME Wayland gives no client a way to stay on top, so the app runs on
//! XWayland unless told otherwise (`--wayland`) and asks the window manager
//! here. Clicks pass through the overlay to whatever is under it (video
//! controls, a call's buttons), except on its handle; pointing at the handle
//! makes the whole overlay take input until the pointer leaves it, so it can
//! be dragged, resized or closed.
//!
//! Only the input region and ordinary crossing events are relied on: an X
//! client under XWayland learns nothing of the pointer or the modifiers
//! while the pointer is over a Wayland window, so watching for a held key
//! from a window that takes no input cannot work there.

use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{ClientMessageEvent, ClipOrdering, ConnectionExt, EventMask, Rectangle};
use x11rb::rust_connection::RustConnection;

/// `_NET_WM_STATE_ADD`.
const ADD: u32 = 1;

type Error = Box<dyn std::error::Error>;

pub fn install(window: &gtk::Window, handle: &gtk::Widget) {
    let handle = handle.clone();
    window.connect_map(move |window| {
        let Some(surface) = window.surface().and_downcast::<gdk4_x11::X11Surface>() else { return };
        let x11 = match X11::connect(surface.xid() as u32) {
            Ok(x11) => std::rc::Rc::new(x11),
            Err(e) => {
                eprintln!("myna-captions: no X11 connection for the overlay: {e}");
                return;
            }
        };
        if let Err(e) = x11.keep_above() {
            eprintln!("myna-captions: cannot keep the overlay above: {e}");
        }
        let apply = {
            let (x11, window, handle) = (x11.clone(), window.downgrade(), handle.downgrade());
            move |interactive: bool| {
                let (Some(window), Some(handle)) = (window.upgrade(), handle.upgrade()) else { return };
                let region = if interactive { None } else { handle_rect(&window, &handle) };
                if let Err(e) = x11.set_input(region) {
                    eprintln!("myna-captions: cannot set the overlay's input region: {e}");
                }
            }
        };
        // GTK lays the handle out after the map; claim the region once it has.
        let initial = apply.clone();
        gtk::glib::idle_add_local_once(move || initial(false));
        let motion = gtk::EventControllerMotion::new();
        motion.connect_contains_pointer_notify(move |motion| apply(motion.contains_pointer()));
        window.add_controller(motion);
    });
}

/// The handle's box in the window's X pixels.
fn handle_rect(window: &gtk::Window, handle: &gtk::Widget) -> Option<Rectangle> {
    let bounds = handle.compute_bounds(window)?;
    let scale = window.scale_factor() as f32;
    Some(Rectangle {
        x: (bounds.x() * scale).floor() as i16,
        y: (bounds.y() * scale).floor() as i16,
        width: (bounds.width() * scale).ceil() as u16,
        height: (bounds.height() * scale).ceil() as u16,
    })
}

struct X11 {
    conn: RustConnection,
    root: u32,
    window: u32,
}

impl X11 {
    fn connect(window: u32) -> Result<Self, Error> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self { conn, root, window })
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

    /// `None` lets the whole window take input; a rectangle confines it there.
    fn set_input(&self, region: Option<Rectangle>) -> Result<(), Error> {
        match region {
            None => self.conn.shape_mask(SO::SET, SK::INPUT, self.window, 0, 0, x11rb::NONE)?,
            Some(rect) => {
                self.conn.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, self.window, 0, 0, &[rect])?
            }
        };
        self.conn.flush()?;
        Ok(())
    }
}
