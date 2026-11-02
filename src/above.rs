//! The overlay's X11 behaviour: kept above other windows, and click-through.
//!
//! GNOME Wayland gives no client a way to stay on top, so the app runs on
//! XWayland unless told otherwise (`--wayland`) and asks the window manager
//! here. Clicks pass through the overlay to whatever is under it (video
//! controls, a call's buttons), except on its handle; while the overlay is
//! revealed (`overlay::Reveal`) the whole of it takes input, so it can be
//! dragged, resized or closed.
//!
//! Only the input region and ordinary crossing events are relied on: an X
//! client under XWayland learns nothing of the pointer or the modifiers
//! while the pointer is over a Wayland window, so watching for a held key
//! from a window that takes no input cannot work there.

use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{ClientMessageEvent, ClipOrdering, ConfigureWindowAux, ConnectionExt, EventMask, Rectangle};
use x11rb::rust_connection::RustConnection;

/// `_NET_WM_STATE_ADD`.
const ADD: u32 = 1;

type Error = Box<dyn std::error::Error>;

pub fn install(window: &gtk::Window, handle: &gtk::Widget, reveal: &std::rc::Rc<crate::overlay::Reveal>) {
    let (handle, reveal) = (handle.clone(), reveal.clone());
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
        // Whether the region was set: not before the handle is laid out.
        let apply = {
            let (x11, window, handle) = (x11.clone(), window.downgrade(), handle.downgrade());
            move |interactive: bool| -> bool {
                let (Some(window), Some(handle)) = (window.upgrade(), handle.upgrade()) else { return true };
                let region = if interactive {
                    None
                } else {
                    match handle_rect(&window, &handle) {
                        Some(rect) => Some(rect),
                        None => return false,
                    }
                };
                trace(&format!("input {:?}", region.map(|r| (r.x, r.y, r.width, r.height))));
                if let Err(e) = x11.set_input(region) {
                    eprintln!("myna-captions: cannot set the overlay's input region: {e}");
                }
                true
            }
        };
        // Claim the region on the first frame the handle has a size in.
        let initial = apply.clone();
        window.add_tick_callback(move |_, _| {
            if initial(false) { gtk::glib::ControlFlow::Break } else { gtk::glib::ControlFlow::Continue }
        });
        x11.drag_from(&handle);
        if tracing() {
            let legacy = gtk::EventControllerLegacy::new();
            legacy.set_propagation_phase(gtk::PropagationPhase::Capture);
            legacy.connect_event(|_, event| {
                use gtk::gdk::EventType::*;
                if matches!(event.event_type(), ButtonPress | ButtonRelease | EnterNotify | LeaveNotify) {
                    trace(&format!("{:?} at {:?}", event.event_type(), event.position()));
                }
                gtk::glib::Propagation::Proceed
            });
            window.add_controller(legacy);
        }
        reveal.connect(move |shown| {
            apply(shown);
        });
    });
}

fn tracing() -> bool {
    std::env::var_os("MYNA_CAPTIONS_TRACE").is_some()
}

/// Overlay pointer diagnostics, under `MYNA_CAPTIONS_TRACE`.
fn trace(what: &str) {
    if tracing() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("{:.2} overlay {what}", t.as_secs_f64() % 1000.0);
    }
}

/// The handle's box in the window's X pixels.
fn handle_rect(window: &gtk::Window, handle: &gtk::Widget) -> Option<Rectangle> {
    if handle.width() < 1 || handle.height() < 1 {
        return None; // not laid out yet
    }
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

    /// Move the window with drags on `handle`, by configuring it ourselves:
    /// under Mutter an XWayland window's `_NET_WM_MOVERESIZE` request (what
    /// GTK's begin_move sends) moved nothing, measured in a nested Shell.
    /// The button's implicit grab keeps the pointer reported to us while it
    /// is held, wherever it goes.
    fn drag_from(self: &std::rc::Rc<Self>, handle: &gtk::Widget) {
        let drag = gtk::GestureDrag::new();
        let start = std::rc::Rc::new(std::cell::Cell::new(None::<(i32, i32, i32, i32)>));
        drag.connect_drag_begin({
            let (x11, start) = (self.clone(), start.clone());
            move |gesture, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                start.set(x11.origin_and_pointer().ok());
                trace(&format!("drag from {:?}", start.get()));
            }
        });
        drag.connect_drag_update({
            let (x11, start) = (self.clone(), start.clone());
            move |_, _, _| {
                let Some((wx, wy, px, py)) = start.get() else { return };
                if let Ok((_, _, nx, ny)) = x11.origin_and_pointer() {
                    let r = x11.move_to(wx + nx - px, wy + ny - py);
                    trace(&format!("drag to {},{} {r:?}", wx + nx - px, wy + ny - py));
                }
            }
        });
        drag.connect_drag_end(move |_, _, _| start.set(None));
        handle.add_controller(drag);
    }

    /// The window's frame origin and the pointer, in root coordinates.
    fn origin_and_pointer(&self) -> Result<(i32, i32, i32, i32), Error> {
        let pointer = self.conn.query_pointer(self.root)?.reply()?;
        let origin = self.conn.translate_coordinates(self.window, self.root, 0, 0)?.reply()?;
        Ok((origin.dst_x.into(), origin.dst_y.into(), pointer.root_x.into(), pointer.root_y.into()))
    }

    fn move_to(&self, x: i32, y: i32) -> Result<(), Error> {
        self.conn.configure_window(self.window, &ConfigureWindowAux::new().x(x).y(y))?;
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
