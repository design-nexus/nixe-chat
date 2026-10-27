//! Settings as a card over the window: the groups listed on the left, the
//! chosen group's options on the right, and a search across all of them.

use crate::sections::settings;
use crate::{widgets, window};
use gtk::prelude::*;
use nixe_ui::settings_card::{self, Spec};

pub use nixe_ui::settings_card::{escape, fit, focus_search, is_open};

/// Each settings group's icon and one-line summary in the dialog's list.
const GROUPS: &[(&str, &str, &str)] = &[
    ("Chat window", "applications-graphics-symbolic", "Theme, glow, motion"),
    ("Connection", "network-server-symbolic", "Model servers"),
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

const SPEC: Spec = Spec { groups: GROUPS, search_text: Some(crate::widgets::search_text) };

pub fn open() {
    let (Some(overlay), Some(window)) = (window::overlay(), window::window()) else { return };
    settings_card::open(&overlay, &window, SPEC, || {
        let page = widgets::page("settings");
        settings::build(&page);
        window::mark_page(page.root.upcast_ref(), window::narrow());
        (page.root.upcast(), page.body)
    });
}
