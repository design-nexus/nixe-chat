//! The main window: a sidebar with pages and the chat list, and a stack of
//! pages built the first time they're shown.

use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::sections::{self, Section};
use crate::{chatlist, hub, models, prefs, pulls, theme, widgets};

struct Ui {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    nav_items: HashMap<&'static str, gtk::Button>,
    pages: HashMap<&'static str, gtk::Widget>,
    sections: Vec<Section>,
    current: &'static str,
    overlay: gtk::Overlay,
}

type NarrowHook = Box<dyn Fn(bool)>;

thread_local! {
    static UI: RefCell<Option<Rc<RefCell<Ui>>>> = const { RefCell::new(None) };
    static NARROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NARROW_HOOKS: RefCell<Vec<NarrowHook>> = const { RefCell::new(Vec::new()) };
}

fn ui() -> Option<Rc<RefCell<Ui>>> {
    UI.with(|u| u.borrow().clone())
}

pub fn window() -> Option<gtk::ApplicationWindow> {
    ui().map(|u| u.borrow().window.clone())
}

pub fn current() -> &'static str {
    ui().map(|u| u.borrow().current).unwrap_or("")
}

pub fn narrow() -> bool {
    NARROW.with(|n| n.get())
}

/// Run `f` now and whenever the window crosses the compact width.
pub fn on_narrow(f: impl Fn(bool) + 'static) {
    f(narrow());
    NARROW_HOOKS.with(|h| h.borrow_mut().push(Box::new(f)));
}

/// Set up the app once (theme, icons, models) without showing a window.
pub fn init() {
    thread_local! { static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    theme::install();
    install_icons();
    models::refresh();
    let p = prefs::get();
    if p.mcp_servers.iter().any(|s| s.enabled) {
        crate::rt::detach(crate::tools::mcp::restart(p.mcp_servers.clone()));
    }
    // Keep reachability fresh: a server that comes up later is noticed.
    glib::timeout_add_seconds_local(20, || {
        if models::online() != Some(true) {
            models::refresh();
        }
        glib::ControlFlow::Continue
    });
    glib::timeout_add_seconds_local_once(8, models::daily_update_check);
}

pub fn present(app: &gtk::Application, section: Option<&str>) {
    if let Some(ui) = ui() {
        let window = ui.borrow().window.clone();
        if let Some(s) = section {
            navigate(s);
        }
        window.present();
        return;
    }
    init();
    build(app);
    let start = section.map(String::from).unwrap_or_else(|| prefs::get().last_section);
    navigate(&start);
    if let Some(out) = std::env::var_os("CHAT_SNAPSHOT") {
        snapshot_and_quit(app, std::path::PathBuf::from(out));
        return;
    }
    if let Some(ui) = ui() {
        ui.borrow().window.present();
    }
    // Developer aid: CHAT_AUTOSEND="question" sends it a few seconds after
    // the window shows, for checking layout with a real compositor and grim.
    if let Ok(text) = std::env::var("CHAT_AUTOSEND") {
        glib::timeout_add_local_once(std::time::Duration::from_millis(3500), move || sections::chat::send_text(&text));
    }
}

/// A few symbolic icons icon themes may lack, and the app icon for
/// running before installation.
fn install_icons() {
    const ICONS: &[(&str, &str)] = &[
        ("chat-models-symbolic.svg", include_str!("../data/icons/chat-models-symbolic.svg")),
        ("chat-store-symbolic.svg", include_str!("../data/icons/chat-store-symbolic.svg")),
        ("chat-stats-symbolic.svg", include_str!("../data/icons/chat-stats-symbolic.svg")),
        ("io.github.design_nexus.Chat.svg", include_str!("../data/io.github.design_nexus.Chat.svg")),
    ];
    let dir = crate::paths::cache_dir().join("icons");
    for (name, svg) in ICONS {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*svg) {
            let _ = crate::cmd::atomic_write(&path, svg);
        }
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
    }
}

fn build(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder().application(app).title("Chat").default_width(1180).default_height(820).build();
    window.add_css_class("chat-window");
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_icon_name(Some("io.github.design_nexus.Chat"));

    let sections = sections::all();

    // ----- Sidebar -----
    let nav = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav.add_css_class("settings-navigation");
    nav.set_hexpand(false);
    let mut compact_hide: Vec<gtk::Widget> = Vec::new();

    let head = widgets::hbox(6);
    let heading = widgets::label("CHAT", "menu-heading");
    heading.set_hexpand(true);
    head.append(&heading);
    let new_chat = gtk::Button::from_icon_name("document-new-symbolic");
    new_chat.add_css_class("flat");
    new_chat.add_css_class("new-chat");
    new_chat.set_tooltip_text(Some("New chat (Ctrl+N)"));
    new_chat.connect_clicked(|_| sections::chat::new_chat(None));
    head.append(&new_chat);
    nav.append(&head);
    compact_hide.push(heading.clone().upcast());

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search chats"));
    search.add_css_class("settings-search");
    nav.append(&search);
    compact_hide.push(search.clone().upcast());

    let mut nav_items = HashMap::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for s in &sections {
        let button = gtk::Button::new();
        button.add_css_class("nav-item");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.append(&gtk::Image::from_icon_name(s.icon));
        let l = widgets::label(s.title, "nav-label");
        l.set_hexpand(true);
        compact_hide.push(l.clone().upcast());
        content.append(&l);
        if s.id == "models" {
            // Active downloads.
            let r = widgets::label("", "nav-readout");
            r.add_css_class("mono");
            let r2 = r.clone();
            hub::on(&hub::PULLS, &r, move || {
                let n = pulls::active_count();
                r2.set_text(&if n > 0 { format!("↓{n}") } else { String::new() });
            });
            compact_hide.push(r.clone().upcast());
            content.append(&r);
        }
        button.set_child(Some(&content));
        button.set_tooltip_text(Some(s.title));
        let id = s.id;
        button.connect_clicked(move |_| navigate(id));
        list.append(&button);
        nav_items.insert(s.id, button);
    }
    nav.append(&list);

    // The chat list fills the rest of the sidebar.
    let chats = chatlist::build(&search);
    chats.set_vexpand(true);
    nav.append(&chats);
    compact_hide.push(chats.clone().upcast());
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    spacer.set_visible(false);
    nav.append(&spacer);

    let footer = footer();
    nav.append(&footer);
    compact_hide.push(footer.clone().upcast());

    // ----- Content -----
    let stack = gtk::Stack::new();
    stack.add_css_class("settings-content");
    stack.set_hexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(if prefs::get().reduce_motion { 0 } else { 160 });

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&nav);
    body.append(&stack);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&body));
    window.set_child(Some(&overlay));

    // ----- Keys -----
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let s2 = search.clone();
    let w2 = window.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        match key {
            gdk::Key::f if ctrl => {
                if current() == "settings" {
                    sections::settings::focus_search();
                } else if narrow() {
                    navigate("chat");
                    sections::chat::open_history();
                } else {
                    s2.grab_focus();
                }
                glib::Propagation::Stop
            }
            gdk::Key::n if ctrl => {
                sections::chat::new_chat(None);
                glib::Propagation::Stop
            }
            gdk::Key::q | gdk::Key::w if ctrl => {
                w2.close();
                glib::Propagation::Stop
            }
            gdk::Key::comma if ctrl => {
                navigate("settings");
                glib::Propagation::Stop
            }
            gdk::Key::space if ctrl => {
                if current() == "chat" {
                    sections::chat::toggle_dictation();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            gdk::Key::S if ctrl && shift => {
                sections::chat::screenshot();
                glib::Propagation::Stop
            }
            gdk::Key::Escape if s2.has_focus() || s2.focus_child().is_some() => {
                s2.set_text("");
                glib::Propagation::Proceed
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(keys);

    // Narrow windows (a tiled half-screen) get an icon-only sidebar.
    let apply_width = {
        let nav = nav.clone();
        let spacer = spacer.clone();
        move |w: &gtk::ApplicationWindow| {
            let width = if w.width() > 0 { w.width() } else { w.default_width() };
            let compact = width > 0 && width < 980;
            if compact == nav.has_css_class("compact") && nav.has_css_class("sized") {
                return;
            }
            nav.add_css_class("sized");
            if compact {
                nav.add_css_class("compact");
            } else {
                nav.remove_css_class("compact");
            }
            for wdg in &compact_hide {
                wdg.set_visible(!compact);
            }
            spacer.set_visible(compact);
            set_narrow(compact);
        }
    };
    let aw = apply_width.clone();
    window.connect_default_width_notify(move |w| aw(w));
    let aw = apply_width.clone();
    window.connect_realize(move |w| aw(w));
    let w2 = window.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
        apply_width(&w2);
        glib::ControlFlow::Continue
    });

    let ui = Ui { window, stack, nav_items, pages: HashMap::new(), sections, current: "", overlay };
    UI.with(|u| *u.borrow_mut() = Some(Rc::new(RefCell::new(ui))));
}

/// Server switcher and reachability, above the version.
fn footer() -> gtk::Box {
    let footer = widgets::vbox(6);
    footer.add_css_class("nav-footer");
    let row = widgets::hbox(8);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("status-dot");
    dot.set_valign(gtk::Align::Center);
    let status = widgets::label("Connecting…", "dim");
    status.add_css_class("small-text");
    status.set_hexpand(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&dot);
    row.append(&status);
    footer.append(&row);

    let hosts = gtk::DropDown::from_strings(&[]);
    hosts.add_css_class("host-switcher");
    hosts.set_tooltip_text(Some("Ollama server"));
    footer.append(&hosts);
    let fill_hosts = {
        let hosts = hosts.clone();
        move || {
            let p = prefs::get();
            let names: Vec<String> = p.hosts.iter().map(|h| h.name.clone()).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            hosts.set_model(Some(&gtk::StringList::new(&refs)));
            hosts.set_selected(p.active_host.min(names.len().saturating_sub(1)) as u32);
            hosts.set_visible(names.len() > 1);
        }
    };
    fill_hosts();
    hosts.connect_selected_notify(|d| {
        let i = d.selected() as usize;
        if i != prefs::get().active_host && i < prefs::get().hosts.len() {
            switch_host(i);
        }
    });
    hub::on(&hub::HOST, &hosts, fill_hosts);

    let version = widgets::label(concat!("Chat ", env!("CARGO_PKG_VERSION")), "dim");
    version.add_css_class("small-text");
    footer.append(&version);

    let refresh = {
        let (dot, status) = (dot.clone(), status.clone());
        move || {
            let host = prefs::get().host();
            dot.remove_css_class("on");
            dot.remove_css_class("off");
            match models::online() {
                Some(true) => {
                    dot.add_css_class("on");
                    let n = models::installed().len();
                    status.set_text(&format!("{} · {n} model{}", host.name, if n == 1 { "" } else { "s" }));
                }
                Some(false) => {
                    dot.add_css_class("off");
                    status.set_text(&format!("{} · offline", host.name));
                }
                None => status.set_text("Connecting…"),
            }
            status.set_tooltip_text(Some(&crate::prefs::normalize_host(&host.url)));
        }
    };
    refresh();
    hub::on(&hub::MODELS, &footer, refresh);
    footer
}

pub fn switch_host(i: usize) {
    prefs::update(|p| p.active_host = i);
    hub::emit(&hub::HOST);
    models::refresh();
}

fn set_narrow(narrow: bool) {
    NARROW.with(|n| n.set(narrow));
    let Some(ui) = ui() else { return };
    for page in ui.borrow().pages.values() {
        mark_page(page, narrow);
    }
    NARROW_HOOKS.with(|h| {
        for f in h.borrow().iter() {
            f(narrow);
        }
    });
}

fn mark_page(page: &gtk::Widget, narrow: bool) {
    let body = page.downcast_ref::<gtk::ScrolledWindow>().and_then(|s| s.child()).and_then(|v| v.first_child());
    let target = body.unwrap_or_else(|| page.clone());
    if narrow {
        target.add_css_class("narrow");
    } else {
        target.remove_css_class("narrow");
    }
}

fn ensure_built(id: &'static str) {
    let Some(ui) = ui() else { return };
    if ui.borrow().pages.contains_key(id) {
        return;
    }
    let build = ui.borrow().sections.iter().find(|s| s.id == id).map(|s| s.build);
    let Some(build) = build else { return };
    let page = build();
    mark_page(&page, narrow());
    let stack = ui.borrow().stack.clone();
    stack.add_named(&page, Some(id));
    ui.borrow_mut().pages.insert(id, page);
}

pub fn navigate(id: &str) {
    let Some(ui) = ui() else { return };
    let resolved = {
        let u = ui.borrow();
        u.sections.iter().find(|s| s.id == id).or_else(|| u.sections.first()).map(|s| s.id)
    };
    let Some(id) = resolved else { return };
    ensure_built(id);
    let mut u = ui.borrow_mut();
    if let Some(prev) = u.nav_items.get(u.current) {
        prev.remove_css_class("active");
    }
    if let Some(b) = u.nav_items.get(id) {
        b.add_css_class("active");
    }
    u.stack.set_visible_child_name(id);
    u.current = id;
    drop(u);
    prefs::update(|p| p.last_section = id.to_string());
}

fn toast_box(message: &str) -> gtk::Box {
    let label = gtk::Label::new(Some(message));
    label.set_wrap(true);
    label.set_max_width_chars(70);
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bx.add_css_class("toast");
    bx.append(&label);
    bx.set_halign(gtk::Align::Center);
    bx.set_valign(gtk::Align::End);
    bx
}

/// Show a short message at the bottom of the window.
pub fn toast(message: &str) {
    let Some(ui) = ui() else {
        eprintln!("chat: {message}");
        return;
    };
    let overlay = ui.borrow().overlay.clone();
    let bx = toast_box(message);
    overlay.add_overlay(&bx);
    glib::timeout_add_local_once(std::time::Duration::from_millis(3500), move || {
        overlay.remove_overlay(&bx);
    });
}

/// A toast with a button (Undo, Open, …). It stays a little longer.
pub fn toast_action(message: &str, action: &str, f: impl Fn() + 'static) {
    let Some(ui) = ui() else { return };
    let overlay = ui.borrow().overlay.clone();
    let bx = toast_box(message);
    let b = gtk::Button::with_label(action);
    b.add_css_class("flat");
    b.add_css_class("accent-text");
    let (o2, bx2) = (overlay.clone(), bx.clone());
    b.connect_clicked(move |_| {
        f();
        if bx2.parent().is_some() {
            o2.remove_overlay(&bx2);
        }
    });
    bx.append(&b);
    overlay.add_overlay(&bx);
    glib::timeout_add_local_once(std::time::Duration::from_millis(6000), move || {
        if bx.parent().is_some() {
            overlay.remove_overlay(&bx);
        }
    });
}

fn snapshot_and_quit(app: &gtk::Application, out: std::path::PathBuf) {
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    // Developer aids for checking a whole flow without clicking:
    // CHAT_SNAPSHOT_COMPARE=model answers alongside a second model,
    // CHAT_SNAPSHOT_SEND="question" sends it, CHAT_SNAPSHOT_REPO=user/repo
    // opens a Marketplace page, CHAT_SNAPSHOT_PULL=name starts a download.
    if let Ok(model) = std::env::var("CHAT_SNAPSHOT_COMPARE") {
        sections::chat::set_compare(vec![model]);
    }
    if let Ok(text) = std::env::var("CHAT_SNAPSHOT_SEND") {
        glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || sections::chat::send_text(&text));
    }
    if let Ok(name) = std::env::var("CHAT_SNAPSHOT_PULL") {
        crate::pulls::start(&name);
    }
    if let Ok(repo) = std::env::var("CHAT_SNAPSHOT_REPO") {
        sections::marketplace::open_repo(&repo);
    }
    snapshot_window(app, window.upcast_ref(), out);
}

/// Render `window` to a PNG (invisibly) after a delay, then quit.
pub fn snapshot_window(app: &gtk::Application, window: &gtk::Window, out: std::path::PathBuf) {
    window.set_opacity(0.01);
    window.set_title(Some("Chat snapshot"));
    window.set_default_size(
        std::env::var("CHAT_SNAPSHOT_W").ok().and_then(|v| v.parse().ok()).unwrap_or(1180),
        std::env::var("CHAT_SNAPSHOT_H").ok().and_then(|v| v.parse().ok()).unwrap_or(820),
    );
    window.present();
    let (app, window) = (app.clone(), window.clone());
    let delay = std::env::var("CHAT_SNAPSHOT_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(2500);
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        if let Some(child) = window.child() {
            let paintable = gtk::WidgetPaintable::new(Some(&child));
            let (w, h) = (child.width(), child.height());
            let snapshot = gtk::Snapshot::new();
            snapshot.append_color(&gdk::RGBA::BLACK, &gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
            paintable.snapshot(&snapshot, w as f64, h as f64);
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer()) {
                let texture = renderer.render_texture(node, None);
                match texture.save_to_png(&out) {
                    Ok(()) => println!("snapshot {w}x{h} -> {}", out.display()),
                    Err(e) => eprintln!("snapshot failed: {e}"),
                }
            }
        }
        app.quit();
    });
}
