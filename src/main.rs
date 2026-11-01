//! Myna Captions: pick a PipeWire audio output and read it as live captions
//! in a floating overlay. Speech is transcribed locally by an installed Myna
//! inference backend.

mod above;
mod nodes;
mod overlay;
mod session;
mod transcript;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use nodes::{Kind, Node};
use overlay::{Caption, Style};
use session::Event;
use transcript::Transcript;

const APP_ID: &str = "com.canonical.Myna.Captions";

struct Running {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    window: gtk::Window,
    caption: Caption,
}

struct App {
    runtime: tokio::runtime::Handle,
    x11: bool,
    backends: Vec<(String, PathBuf)>,
    backend: RefCell<usize>,
    language: RefCell<Option<String>>,
    style: RefCell<Style>,
    sessions: RefCell<HashMap<u64, Running>>,
    shown: RefCell<Vec<Node>>,
    list: gtk::ListBox,
}

fn main() -> glib::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut x11 = true;
    let mut socket = None;
    let mut autostart = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wayland" => x11 = false,
            "--socket" => socket = args.next().map(PathBuf::from),
            "--caption" => autostart = args.next().and_then(|s| s.parse::<u64>().ok()),
            _ => {
                eprintln!("usage: myna-captions [--wayland] [--socket PATH] [--caption SERIAL]");
                return glib::ExitCode::FAILURE;
            }
        }
    }
    if x11 {
        // GNOME Wayland gives a client no way to stay above other windows;
        // XWayland and EWMH do.
        gdk::set_allowed_backends("x11,*");
    }

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let handle = runtime.handle().clone();
    std::thread::spawn(move || runtime.block_on(std::future::pending::<()>()));

    let mut backends = session::backends();
    if let Some(socket) = socket {
        backends.insert(0, (socket.display().to_string(), socket));
    }

    // A scripted run must not hand itself to an instance already running.
    let flags = if autostart.is_some() { gtk::gio::ApplicationFlags::NON_UNIQUE } else { Default::default() };
    let application = adw::Application::builder().application_id(APP_ID).flags(flags).build();
    application.connect_activate(move |application| {
        if let Some(window) = application.active_window() {
            window.present();
            return;
        }
        overlay::install_css();
        build(application, handle.clone(), x11, backends.clone(), autostart);
    });
    application.run_with_args::<&str>(&[])
}

fn build(
    application: &adw::Application,
    runtime: tokio::runtime::Handle,
    x11: bool,
    backends: Vec<(String, PathBuf)>,
    autostart: Option<u64>,
) {
    let list = gtk::ListBox::builder().css_classes(["boxed-list"]).selection_mode(gtk::SelectionMode::None).build();
    let app = Rc::new(App {
        runtime,
        x11,
        backends,
        backend: RefCell::new(0),
        language: RefCell::new(None),
        style: RefCell::new(Style::default()),
        sessions: RefCell::new(HashMap::new()),
        shown: RefCell::new(Vec::new()),
        list: list.clone(),
    });

    let engine = adw::PreferencesGroup::builder().title("Engine").build();
    let names: Vec<&str> = app.backends.iter().map(|(name, _)| name.as_str()).collect();
    let backend_row = adw::ComboRow::builder()
        .title("Backend")
        .model(&gtk::StringList::new(&names))
        .build();
    if names.is_empty() {
        backend_row.set_subtitle("No Myna backend is installed");
        backend_row.set_sensitive(false);
    }
    backend_row.connect_selected_notify(glib::clone!(#[weak] app, move |row| {
        *app.backend.borrow_mut() = row.selected() as usize;
    }));
    let language_row = adw::EntryRow::builder().title("Language code (empty for automatic)").build();
    language_row.connect_changed(glib::clone!(#[weak] app, move |row| {
        let text = row.text().trim().to_owned();
        *app.language.borrow_mut() = (!text.is_empty()).then_some(text);
    }));
    engine.add(&backend_row);
    engine.add(&language_row);

    let appearance = adw::PreferencesGroup::builder().title("Appearance").build();
    let font = gtk::FontDialogButton::builder()
        .dialog(&gtk::FontDialog::builder().title("Caption Font").build())
        .font_desc(&app.style.borrow().font)
        .valign(gtk::Align::Center)
        .build();
    font.connect_font_desc_notify(glib::clone!(#[weak] app, move |button| {
        if let Some(font) = button.font_desc() {
            app.restyle(|style| style.font = font);
        }
    }));
    let font_row = adw::ActionRow::builder().title("Font").build();
    font_row.add_suffix(&font);
    font_row.set_activatable_widget(Some(&font));
    let opacity = adw::SpinRow::with_range(0.0, 100.0, 5.0);
    opacity.set_title("Background opacity");
    opacity.set_value((app.style.borrow().background.alpha() * 100.0).round() as f64);
    opacity.connect_value_notify(glib::clone!(#[weak] app, move |row| {
        let alpha = row.value() as f32 / 100.0;
        app.restyle(|style| style.background.set_alpha(alpha));
    }));
    let lines = adw::SpinRow::with_range(1.0, 6.0, 1.0);
    lines.set_title("Lines for new captions");
    lines.set_value(app.style.borrow().lines as f64);
    lines.connect_value_notify(glib::clone!(#[weak] app, move |row| {
        app.style.borrow_mut().lines = row.value() as i32;
    }));
    appearance.add(&font_row);
    appearance.add(&opacity);
    appearance.add(&lines);

    let sources = adw::PreferencesGroup::builder()
        .title("Audio Outputs")
        .description("Pick what to caption. Captions appear in a window you can drag anywhere.")
        .build();
    sources.add(&list);

    let page = adw::PreferencesPage::new();
    page.add(&sources);
    page.add(&engine);
    page.add(&appearance);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Myna Captions")
        .default_width(560)
        .default_height(720)
        .content(&view)
        .build();
    window.present();

    app.refresh();
    let wanted = autostart.and_then(|serial| app.shown.borrow().iter().find(|n| n.serial == serial).cloned());
    match (autostart, wanted) {
        (Some(serial), None) => eprintln!("myna-captions: no audio output has serial {serial}"),
        (_, Some(node)) => app.start(&node),
        _ => {}
    }
    {
        app.shown.borrow_mut().clear();
        app.refresh();
    }
    // The refresh owns the app state for as long as the picker exists;
    // everything else holds it weakly.
    let picker = window.downgrade();
    glib::timeout_add_seconds_local(2, move || {
        if picker.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        app.refresh();
        glib::ControlFlow::Continue
    });
}

impl App {
    fn restyle(&self, change: impl FnOnce(&mut Style)) {
        change(&mut self.style.borrow_mut());
        let style = self.style.borrow().clone();
        for running in self.sessions.borrow().values() {
            let mut kept = style.clone();
            kept.lines = running.caption.lines();
            running.caption.set_style(kept);
        }
    }

    /// Rebuild the output list when the graph changed.
    fn refresh(self: &Rc<Self>) {
        let nodes = nodes::list();
        if *self.shown.borrow() == nodes {
            return;
        }
        self.list.remove_all();
        if nodes.is_empty() {
            self.list.append(&adw::ActionRow::builder().title("No audio outputs").build());
        }
        for node in &nodes {
            self.list.append(&self.row(node));
        }
        self.shown.replace(nodes);
    }

    fn row(self: &Rc<Self>, node: &Node) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&node.title))
            .subtitle(glib::markup_escape_text(&node.subtitle))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(match node.kind {
            Kind::Stream => "applications-multimedia-symbolic",
            Kind::Sink => "audio-speakers-symbolic",
        }));
        if node.running {
            row.add_suffix(&gtk::Label::builder().label("Playing").css_classes(["dim-label", "caption"]).build());
        }
        let toggle = gtk::ToggleButton::builder()
            .label("Caption")
            .valign(gtk::Align::Center)
            .active(self.sessions.borrow().contains_key(&node.serial))
            .build();
        let node = node.clone();
        toggle.connect_toggled(glib::clone!(#[weak(rename_to = app)] self, move |toggle| {
            if toggle.is_active() {
                if !app.sessions.borrow().contains_key(&node.serial) {
                    app.start(&node);
                }
            } else if let Some(running) = app.sessions.borrow().get(&node.serial) {
                running.window.close();
            }
        }));
        row.add_suffix(&toggle);
        row.set_activatable_widget(Some(&toggle));
        row
    }

    fn start(self: &Rc<Self>, node: &Node) {
        let Some((_, socket)) = self.backends.get(*self.backend.borrow()).cloned() else {
            eprintln!("myna-captions: no backend to transcribe with");
            return;
        };
        let application = self.list.root().and_downcast::<gtk::Window>().and_then(|w| w.application());
        let Some(application) = application.and_downcast::<adw::Application>() else {
            eprintln!("myna-captions: the picker has no application yet");
            return;
        };

        let caption = Caption::new(self.style.borrow().clone());
        let window = overlay::window(&application, &node.title, &caption);
        if self.x11 {
            above::keep_above(&window);
        }

        let (events_tx, events_rx) = async_channel::unbounded();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let spec = session::Spec {
            socket,
            serial: node.serial,
            kind: node.kind.clone(),
            language: self.language.borrow().clone(),
        };
        self.runtime.spawn(session::run(spec, events_tx, stop_rx));

        let serial = node.serial;
        window.connect_close_request(glib::clone!(#[weak(rename_to = app)] self, #[upgrade_or] glib::Propagation::Proceed, move |_| {
            if let Some(mut running) = app.sessions.borrow_mut().remove(&serial) {
                if let Some(stop) = running.stop.take() {
                    let _ = stop.send(());
                }
            }
            app.shown.borrow_mut().clear();
            app.refresh();
            glib::Propagation::Proceed
        }));

        glib::spawn_future_local(glib::clone!(#[weak] caption, async move {
            let mut transcript = Transcript::default();
            let trace = std::env::var_os("MYNA_CAPTIONS_TRACE").is_some();
            let born = std::time::Instant::now();
            while let Ok(event) = events_rx.recv().await {
                if trace {
                    eprintln!("[{:6.2}] {event:?}", born.elapsed().as_secs_f32());
                }
                match event {
                    Event::Status(state) => {
                        if transcript.display().0.is_empty() {
                            let waiting = if state == "loading" { "Loading the speech model…" } else { "" };
                            caption.set_text(waiting.to_owned(), 0);
                        }
                        continue;
                    }
                    Event::Delta { item, text, unstable } => transcript.delta(&item, &text, unstable),
                    Event::Completed { item, text } => transcript.completed(&item, &text),
                    Event::Ended(None) => break,
                    Event::Ended(Some(error)) => {
                        caption.set_text(format!("Captions stopped: {error}"), 0);
                        break;
                    }
                }
                let (text, stable) = transcript.display();
                caption.set_text(text, stable);
            }
        }));

        window.present();
        self.sessions.borrow_mut().insert(serial, Running { stop: Some(stop_tx), window, caption });
    }
}
