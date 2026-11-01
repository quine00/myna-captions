//! The caption overlay: an undecorated, translucent window that shows the
//! tail of a transcript, laid out by Pango and bottom-anchored like a
//! subtitle. Dragging anywhere moves it; the corner grip resizes it, and its
//! height decides how many lines show.

use std::cell::{Cell, RefCell};

use gtk::prelude::*;
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

const PAD_H: f32 = 24.0;
const PAD_V: f32 = 14.0;
const RADIUS: f32 = 14.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Caption {
        pub text: RefCell<String>,
        pub stable: Cell<usize>,
        pub style: RefCell<Style>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Caption {
        const NAME: &'static str = "MynaCaption";
        type Type = super::Caption;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Caption {}

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
        self.update_property(&[gtk::accessible::Property::Label(&text)]);
        self.imp().text.replace(text);
        self.imp().stable.set(stable);
        self.queue_draw();
    }

    pub fn set_style(&self, style: Style) {
        self.imp().style.replace(style);
        self.queue_resize();
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
window.myna-captions:hover .chrome { opacity: 1; }
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

/// The overlay window around `caption`. `on_close` runs once it closes, by
/// its own button or otherwise.
pub fn window(app: &adw::Application, title: &str, caption: &Caption) -> gtk::Window {
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
        .margin_top(6)
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

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(caption));
    overlay.add_overlay(&close);
    overlay.add_overlay(&grip);
    window.set_child(Some(&gtk::WindowHandle::builder().child(&overlay).build()));

    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    window.add_controller(escape);
    window
}
