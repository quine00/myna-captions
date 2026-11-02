//! The caption overlay: an undecorated, translucent window that shows the
//! tail of a transcript, laid out by Pango and bottom-anchored like a
//! subtitle. Dragging anywhere moves it; the corner grip resizes it, and its
//! height decides how many lines show.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use adw::prelude::AnimationExt;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};

#[derive(Clone, Debug)]
pub struct Style {
    pub font: pango::FontDescription,
    pub text: gdk::RGBA,
    pub tentative: gdk::RGBA,
    pub background: gdk::RGBA,
    pub lines: i32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font: pango::FontDescription::from_string("Ubuntu Sans Medium 26"),
            text: gdk::RGBA::new(1.0, 1.0, 1.0, 1.0),
            tentative: gdk::RGBA::new(0.80, 0.84, 0.88, 0.85),
            background: gdk::RGBA::new(0.0, 0.0, 0.0, 0.55),
            lines: 2,
        }
    }
}

/// Captions fade out once nothing new has been said for this long.
const FADE_AFTER: Duration = Duration::from_secs(6);

/// The chrome stays this long after the pointer leaves the overlay...
const LINGER: Duration = Duration::from_secs(1);
/// ...or this long after a press on the handle, time to reach the close
/// button or the grip.
const HOLD: Duration = Duration::from_secs(5);

/// Height of the clear strip above the caption box that holds the handle.
const HANDLE_STRIP: i32 = 28;

const PAD_H: f32 = 24.0;
const PAD_V: f32 = 14.0;
const RADIUS: f32 = 14.0;

mod imp {
    use super::*;

    pub struct Caption {
        pub text: RefCell<String>,
        pub stable: Cell<usize>,
        pub style: RefCell<Style>,
        pub changed: Cell<Instant>,
        pub fade: RefCell<Option<adw::TimedAnimation>>,
    }

    impl Default for Caption {
        fn default() -> Self {
            Self {
                text: RefCell::default(),
                stable: Cell::default(),
                style: RefCell::default(),
                changed: Cell::new(Instant::now()),
                fade: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Caption {
        const NAME: &'static str = "MynaCaption";
        type Type = super::Caption;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Caption {
        fn constructed(&self) {
            self.parent_constructed();
            let widget = self.obj();
            glib::timeout_add_local(
                Duration::from_millis(250),
                glib::clone!(#[weak] widget, #[upgrade_or] glib::ControlFlow::Break, move || {
                    widget.fade_if_quiet();
                    glib::ControlFlow::Continue
                }),
            );
        }
    }

    impl WidgetImpl for Caption {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                return (240, 960, -1, -1);
            }
            let line = self.obj().line_height();
            let minimum = (line + 2.0 * PAD_V).ceil() as i32;
            let natural = (line * self.style.borrow().lines as f32 + 2.0 * PAD_V).ceil() as i32;
            (minimum, natural, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let (w, h) = (widget.width() as f32, widget.height() as f32);
            let style = self.style.borrow();
            let bounds = graphene::Rect::new(0.0, 0.0, w, h);
            snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(bounds, RADIUS));
            snapshot.append_color(&style.background, &bounds);

            let text = self.text.borrow();
            let (shown, stable) = if text.is_empty() { ("Listening…", 0) } else { (text.as_str(), self.stable.get()) };
            let layout = widget.create_pango_layout(Some(shown));
            layout.set_font_description(Some(&style.font));
            layout.set_width(((w - 2.0 * PAD_H).max(1.0) * pango::SCALE as f32) as i32);
            layout.set_wrap(pango::WrapMode::WordChar);
            layout.set_alignment(pango::Alignment::Center);
            if stable < shown.len() {
                let attrs = pango::AttrList::new();
                let c = &style.tentative;
                let scale = |v: f32| (v * 65535.0) as u16;
                let mut color = pango::AttrColor::new_foreground(scale(c.red()), scale(c.green()), scale(c.blue()));
                color.set_start_index(stable as u32);
                color.set_end_index(shown.len() as u32);
                attrs.insert(color);
                layout.set_attributes(Some(&attrs));
            }

            // Whole lines only, bottom-anchored: the newest words sit on the
            // last line and older ones scroll up out of the clip.
            let line = widget.line_height();
            let visible = ((h - 2.0 * PAD_V) / line).floor().max(1.0) * line;
            let text_height = layout.pixel_size().1 as f32;
            let top = h - PAD_V - visible;
            snapshot.push_clip(&graphene::Rect::new(0.0, top, w, visible));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(PAD_H, top + visible - text_height));
            snapshot.push_shadow(&[gsk::Shadow::new(gdk::RGBA::new(0.0, 0.0, 0.0, 0.8), 0.0, 1.5, 3.0)]);
            snapshot.append_layout(&layout, &style.text);
            snapshot.pop();
            snapshot.restore();
            snapshot.pop();
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct Caption(ObjectSubclass<imp::Caption>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Caption {
    pub fn new(style: Style) -> Self {
        let caption: Self = glib::Object::new();
        caption.imp().style.replace(style);
        caption
    }

    pub fn set_text(&self, text: String, stable: usize) {
        let imp = self.imp();
        if *imp.text.borrow() == text && imp.stable.get() == stable {
            return;
        }
        self.update_property(&[gtk::accessible::Property::Label(&text)]);
        imp.text.replace(text);
        imp.stable.set(stable);
        self.wake();
        self.queue_draw();
    }

    /// Fade the caption away after a quiet spell, as a subtitle disappears
    /// when nobody is talking. New words bring it straight back.
    fn fade_if_quiet(&self) {
        let imp = self.imp();
        if self.opacity() < 1.0 || imp.fade.borrow().is_some() || imp.changed.get().elapsed() < FADE_AFTER {
            return;
        }
        let target = adw::PropertyAnimationTarget::new(self, "opacity");
        let fade = adw::TimedAnimation::new(self, 1.0, 0.0, 800, target);
        fade.connect_done(glib::clone!(#[weak(rename_to = caption)] self, move |_| {
            caption.imp().fade.take();
        }));
        fade.play();
        imp.fade.replace(Some(fade));
    }

    /// Restyle, and show the result: a caption faded for quiet would
    /// otherwise take the change invisibly, which reads as ignored.
    pub fn set_style(&self, style: Style) {
        self.imp().style.replace(style);
        self.wake();
        self.queue_resize();
        self.queue_draw();
    }

    /// Bring a faded caption back for another quiet spell.
    fn wake(&self) {
        let imp = self.imp();
        imp.changed.set(Instant::now());
        if let Some(fade) = imp.fade.take() {
            fade.reset();
        }
        self.set_opacity(1.0);
    }

    pub fn lines(&self) -> i32 {
        self.imp().style.borrow().lines
    }

    fn line_height(&self) -> f32 {
        let layout = self.create_pango_layout(Some("Ág"));
        layout.set_font_description(Some(&self.imp().style.borrow().font));
        layout.pixel_size().1 as f32
    }
}

const CSS: &str = "
window.myna-captions { background: none; }
window.myna-captions .chrome { opacity: 0; transition: opacity 200ms; }
window.myna-captions.revealed .chrome { opacity: 1; }
window.myna-captions .handle { padding: 2px 8px; border-radius: 6px; font-size: smaller; opacity: .45;
  transition: opacity 200ms; }
window.myna-captions.revealed .handle { opacity: 1; }
window.myna-captions .grip { min-width: 18px; min-height: 18px; margin: 4px;
  border-right: 3px solid alpha(white, .7); border-bottom: 3px solid alpha(white, .7);
  border-bottom-right-radius: 8px; }
";

pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("a display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// The overlay window around `caption`, and its handle: a pill naming the
/// source, the one part that always takes the pointer, so the rest can let
/// clicks through.
pub fn window(app: &adw::Application, title: &str, caption: &Caption) -> (gtk::Window, gtk::Widget, Rc<Reveal>) {
    let source = gtk::Box::builder()
        .spacing(6)
        .css_classes(["osd", "handle"])
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_start(10)
        .tooltip_text("Drag to move")
        .build();
    source.append(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));
    source.append(&gtk::Label::new(Some(title)));
    let window = gtk::Window::builder()
        .application(app)
        .title(format!("Captions – {title}"))
        .decorated(false)
        .resizable(true)
        .css_classes(["myna-captions"])
        .build();

    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .css_classes(["osd", "circular", "chrome"])
        .tooltip_text("Stop captions")
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(HANDLE_STRIP + 6)
        .margin_end(6)
        .build();
    close.connect_clicked(glib::clone!(#[weak] window, move |_| window.close()));

    let grip = gtk::Box::builder()
        .css_classes(["grip", "chrome"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .cursor(&gdk::Cursor::from_name("se-resize", None).expect("cursor"))
        .build();
    let press = gtk::GestureClick::new();
    press.connect_pressed(glib::clone!(#[weak] window, #[weak] grip, move |gesture, _, x, y| {
        let Some(toplevel) = window.surface().and_downcast::<gdk::Toplevel>() else { return };
        let Some(point) = grip.compute_point(&window, &graphene::Point::new(x as f32, y as f32)) else { return };
        if let Some(device) = gesture.device() {
            toplevel.begin_resize(
                gdk::SurfaceEdge::SouthEast,
                Some(&device),
                gesture.current_button() as i32,
                point.x() as f64,
                point.y() as f64,
                gesture.current_event_time(),
            );
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
    }));
    grip.add_controller(press);

    // The handle sits in a clear strip above the caption box, never on the
    // text; the strip passes clicks through like the rest.
    caption.set_margin_top(HANDLE_STRIP);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(caption));
    overlay.add_overlay(&source);
    overlay.add_overlay(&close);
    overlay.add_overlay(&grip);
    // Moving is the handle's alone (above.rs): gtk::WindowHandle's window-manager
    // move does nothing for this XWayland window under Mutter.
    window.set_child(Some(&overlay));

    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    window.add_controller(escape);
    let reveal = Reveal::install(&window, source.upcast_ref());
    (window, source.upcast(), reveal)
}

type Listener = Box<dyn Fn(bool)>;

/// Whether the overlay shows its chrome (close button, grip) and takes
/// input. It follows the pointer, but lingers after it leaves and holds
/// after a press on the handle: crossing events around a click are not to
/// be trusted, and the controls must outlast the trip to them.
pub struct Reveal {
    window: glib::WeakRef<gtk::Window>,
    shown: Cell<bool>,
    inside: Cell<bool>,
    hold_until: Cell<Instant>,
    timer: RefCell<Option<glib::SourceId>>,
    listeners: RefCell<Vec<Listener>>,
}

impl Reveal {
    fn install(window: &gtk::Window, handle: &gtk::Widget) -> Rc<Self> {
        let reveal = Rc::new(Self {
            window: window.downgrade(),
            shown: Cell::new(false),
            inside: Cell::new(false),
            hold_until: Cell::new(Instant::now()),
            timer: RefCell::default(),
            listeners: RefCell::default(),
        });
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(glib::clone!(#[weak] reveal, move |_, _, _| reveal.pointer(true)));
        motion.connect_motion(glib::clone!(#[weak] reveal, move |_, _, _| reveal.pointer(true)));
        motion.connect_leave(glib::clone!(#[weak] reveal, move |_| reveal.pointer(false)));
        window.add_controller(motion);
        // Capture phase: the drag gesture claims the press.
        let press = gtk::EventControllerLegacy::new();
        press.set_propagation_phase(gtk::PropagationPhase::Capture);
        press.connect_event(glib::clone!(#[weak] reveal, #[upgrade_or] glib::Propagation::Proceed, move |_, event| {
            if matches!(event.event_type(), gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease) {
                reveal.hold();
            }
            glib::Propagation::Proceed
        }));
        handle.add_controller(press);
        reveal
    }

    /// Calls `f` with every change, and now with the current state.
    pub fn connect(&self, f: impl Fn(bool) + 'static) {
        f(self.shown.get());
        self.listeners.borrow_mut().push(Box::new(f));
    }

    fn pointer(self: &Rc<Self>, inside: bool) {
        if inside {
            self.inside.set(true);
            self.cancel();
            self.set(true);
        } else if self.inside.replace(false) {
            self.conceal_later();
        }
    }

    fn hold(self: &Rc<Self>) {
        self.hold_until.set(Instant::now() + HOLD);
        self.set(true);
        self.conceal_later();
    }

    fn conceal_later(self: &Rc<Self>) {
        self.cancel();
        let wait = self.hold_until.get().saturating_duration_since(Instant::now()).max(LINGER);
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(wait, move || {
            let Some(reveal) = weak.upgrade() else { return };
            reveal.timer.take();
            if !reveal.inside.get() {
                reveal.set(false);
            }
        });
        self.timer.replace(Some(id));
    }

    fn cancel(&self) {
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }

    fn set(&self, shown: bool) {
        if shown == self.shown.get() {
            return;
        }
        self.shown.set(shown);
        if let Some(window) = self.window.upgrade() {
            if shown { window.add_css_class("revealed") } else { window.remove_css_class("revealed") }
        }
        for f in self.listeners.borrow().iter() {
            f(shown);
        }
    }
}
