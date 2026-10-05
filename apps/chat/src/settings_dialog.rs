//! Settings as a card over the window: the groups listed on the left, the
//! chosen group's options on the right, and a search across all of them.

use crate::sections::settings;
use crate::{cmd, paths, widgets, window};
use gtk::prelude::*;
use std::cell::RefCell;

/// Each settings group's icon and one-line summary in the dialog's list.
const GROUPS: &[(&str, &str, &str)] = &[
    ("Chat window", "applications-graphics-symbolic", "Theme, glow, motion"),
    ("Connection", "network-server-symbolic", "Ollama servers"),
    ("Chat defaults", "user-available-symbolic", "Model, prompt, sending"),
    ("Tools", "applications-utilities-symbolic", "What models can use"),
    ("Web search", "system-search-symbolic", "Search engine, results"),
    ("Location", "find-location-symbolic", "Where you are"),
    ("Memories", "user-bookmarks-symbolic", "Facts every chat knows"),
    ("Knowledge", "accessories-dictionary-symbolic", "Embedding, chunks"),
    ("Voice", "audio-input-microphone-symbolic", "Dictation, reading aloud"),
    ("Quick ask", "edit-find-symbolic", "The small window"),
    ("MCP servers", "network-workgroup-symbolic", "Tools from other programs"),
    ("Keyboard", "input-keyboard-symbolic", "Shortcuts"),
    ("Data", "drive-harddisk-symbolic", "Import, export, delete"),
];

struct Group {
    name: String,
    wrapper: gtk::Widget,
    button: gtk::Button,
}

struct Dialog {
    veil: gtk::Box,
    card: gtk::Box,
    title: gtk::Label,
    search: gtk::SearchEntry,
    empty: gtk::Label,
    groups: Vec<Group>,
    current: String,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

pub fn open() {
    let Some(overlay) = window::overlay() else { return };
    if DIALOG.with(|d| d.borrow().is_none()) {
        let dialog = build();
        overlay.add_overlay(&dialog.veil);
        DIALOG.with(|d| *d.borrow_mut() = Some(dialog));
    }
    let first = DIALOG.with(|d| {
        let d = d.borrow();
        let d = d.as_ref()?;
        d.veil.set_visible(true);
        Some(if d.current.is_empty() { d.groups.first().map(|g| g.name.clone()).unwrap_or_default() } else { d.current.clone() })
    });
    if let Some(w) = window::window() {
        fit(&w);
    }
    if let Some(name) = first {
        select(&name);
    }
}

pub fn is_open() -> bool {
    DIALOG.with(|d| d.borrow().as_ref().is_some_and(|d| d.veil.is_visible()))
}

pub fn close() {
    DIALOG.with(|d| {
        if let Some(d) = d.borrow().as_ref() {
            d.veil.set_visible(false);
        }
    });
}

/// Esc clears the search first, then closes.
pub fn escape() {
    let search = DIALOG.with(|d| d.borrow().as_ref().map(|d| d.search.clone()));
    match search {
        Some(s) if !s.text().is_empty() => s.set_text(""),
        _ => close(),
    }
}

pub fn focus_search() {
    if let Some(s) = DIALOG.with(|d| d.borrow().as_ref().map(|d| d.search.clone())) {
        s.grab_focus();
    }
}

/// Size the card to the window: as large as the window allows, up to a limit.
pub fn fit(window: &gtk::ApplicationWindow) {
    let (w, h) = (window.width(), window.height());
    if w <= 0 || h <= 0 {
        return;
    }
    let size = ((w - 48).clamp(0, 1040), (h - 48).clamp(0, 720));
    DIALOG.with(|d| {
        if let Some(d) = d.borrow().as_ref()
            && d.card.size_request() != size
        {
            d.card.set_size_request(size.0, size.1);
            // A tiled half-screen window gets a slimmer list and less padding.
            if size.0 < 900 {
                d.card.add_css_class("narrow");
            } else {
                d.card.remove_css_class("narrow");
            }
        }
    });
}

fn build() -> Dialog {
    let page = widgets::page("settings");
    settings::build(&page);
    window::mark_page(page.root.upcast_ref(), window::narrow());

    let veil = gtk::Box::new(gtk::Orientation::Vertical, 0);
    veil.add_css_class("settings-veil");
    veil.set_hexpand(true);
    veil.set_vexpand(true);
    veil.set_visible(false);
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    card.add_css_class("settings-dialog");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_vexpand(true);
    card.set_overflow(gtk::Overflow::Hidden);
    veil.append(&card);

    // A click on the dimmed window around the card closes it.
    let click = gtk::GestureClick::new();
    let c = card.clone();
    click.connect_pressed(move |g, _, x, y| {
        let Some(veil) = g.widget() else { return };
        let inside = c.compute_bounds(&veil).is_some_and(|b| b.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)));
        if !inside {
            close();
        }
    });
    veil.add_controller(click);

    // ----- The list of groups -----
    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.add_css_class("settings-dialog-side");
    side.append(&widgets::label("SETTINGS", "dialog-heading"));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search settings"));
    search.add_css_class("settings-search");
    side.append(&search);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);

    let mut groups = Vec::new();
    let mut child = page.body.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        if !w.has_css_class("settings-group") {
            continue;
        }
        let name = w.widget_name().to_string();
        let (icon, summary) =
            GROUPS.iter().find(|g| g.0 == name).map(|g| (g.1, g.2)).unwrap_or(("emblem-system-symbolic", ""));
        let button = gtk::Button::new();
        button.add_css_class("dialog-nav");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let image = gtk::Image::from_icon_name(icon);
        image.set_valign(gtk::Align::Start);
        content.append(&image);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.append(&widgets::label(&name, "dialog-nav-title"));
        if !summary.is_empty() {
            let s = widgets::label(summary, "dialog-nav-summary");
            s.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&s);
        }
        content.append(&text);
        button.set_child(Some(&content));
        let n = name.clone();
        button.connect_clicked(move |_| {
            let search = DIALOG.with(|d| d.borrow().as_ref().map(|d| d.search.clone()));
            if let Some(s) = search {
                s.set_text("");
            }
            select(&n);
        });
        list.append(&button);
        groups.push(Group { name, wrapper: w, button });
    }
    let list_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    side.append(&list_scroll);
    card.append(&side);

    // ----- The chosen group -----
    let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main.add_css_class("settings-dialog-main");
    main.set_hexpand(true);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    head.add_css_class("dialog-head");
    let title = widgets::label("", "dialog-title");
    title.set_hexpand(true);
    head.append(&title);
    let file = paths::prefs_file();
    let edit = widgets::bar_button("text-editor-symbolic", &format!("Open {}", paths::pretty(&file)));
    edit.connect_clicked(move |_| cmd::open_in_editor(&file));
    head.append(&edit);
    let x = widgets::bar_button("window-close-symbolic", "Close (Esc)");
    x.connect_clicked(|_| close());
    head.append(&x);
    main.append(&head);
    let empty = widgets::label("", "dialog-empty");
    empty.set_xalign(0.5);
    empty.set_vexpand(true);
    empty.set_visible(false);
    main.append(&empty);
    main.append(&page.root);
    card.append(&main);

    search.connect_search_changed(|e| on_search(&e.text()));
    Dialog { veil, card, title, search, empty, groups, current: String::new() }
}

/// Show one group on its own.
fn select(name: &str) {
    DIALOG.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return };
        d.current = name.to_string();
        d.title.set_text(name);
        for g in &d.groups {
            let on = g.name == name;
            g.wrapper.set_visible(on);
            set_heading(&g.wrapper, false);
            if on {
                g.button.add_css_class("active");
            } else {
                g.button.remove_css_class("active");
            }
        }
    });
}

/// A group's own heading shows only in search results, where several groups
/// share the page.
fn set_heading(wrapper: &gtk::Widget, visible: bool) {
    if let Some(first) = wrapper.first_child().filter(|w| w.has_css_class("group-title")) {
        first.set_visible(visible);
    }
}

fn on_search(text: &str) {
    let q = text.trim().to_lowercase();
    if q.is_empty() {
        let current = DIALOG.with(|d| {
            let d = d.borrow();
            let d = d.as_ref()?;
            for g in &d.groups {
                for_each_row(&g.wrapper, &mut |row| {
                    if row.has_css_class("search-hidden") {
                        row.remove_css_class("search-hidden");
                        row.set_visible(true);
                    }
                });
                g.button.set_visible(true);
            }
            d.empty.set_visible(false);
            Some(d.current.clone())
        });
        if let Some(c) = current {
            select(&c);
        }
        return;
    }
    DIALOG.with(|d| {
        let d = d.borrow();
        let Some(d) = d.as_ref() else { return };
        let mut total = 0;
        for g in &d.groups {
            let whole = g.name.to_lowercase().contains(&q);
            let mut hits = 0;
            for_each_row(&g.wrapper, &mut |row| {
                let text = widgets::search_text(row).unwrap_or_else(|| text_of(row).to_lowercase());
                let hit = whole || q.split_whitespace().all(|t| text.contains(t));
                if hit && row.has_css_class("search-hidden") {
                    row.remove_css_class("search-hidden");
                    row.set_visible(true);
                } else if !hit && row.is_visible() {
                    row.add_css_class("search-hidden");
                    row.set_visible(false);
                }
                if hit && row.is_visible() {
                    hits += 1;
                }
            });
            g.wrapper.set_visible(hits > 0);
            set_heading(&g.wrapper, true);
            g.button.set_visible(hits > 0);
            g.button.remove_css_class("active");
            total += hits;
        }
        d.title.set_text("Search");
        d.empty.set_text(&format!("No settings match “{}”", text.trim()));
        d.empty.set_visible(total == 0);
    });
}

fn for_each_row(w: &gtk::Widget, f: &mut dyn FnMut(&gtk::Widget)) {
    let mut child = w.first_child();
    while let Some(c) = child {
        if c.has_css_class("settings-option") {
            f(&c);
        } else {
            for_each_row(&c, f);
        }
        child = c.next_sibling();
    }
}

/// Every label's text in a row, for matching.
fn text_of(w: &gtk::Widget) -> String {
    let mut out = String::new();
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        out.push_str(&l.text());
        out.push(' ');
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        out.push_str(&text_of(&c));
        child = c.next_sibling();
    }
    out
}
