//! Theming: the shared Nyx theme engine, given this app's stylesheet.

pub use nyx_theme::*;

use crate::prefs;

/// Install the stylesheet and the current palette, and follow Omarchy live.
pub fn install() {
    nyx_theme::on_change(|| crate::hub::THEME.with(|h| h.emit()));
    nyx_theme::install(Style {
        window_class: "chat-window",
        base_skip: &[
            "window.@APP@ popover > contents",
            "window.@APP@ popover listview > row, window.@APP@ popover list > row",
            "window.@APP@ scale > trough > slider, window.@APP@ scale.marks-before > trough > slider, window.@APP@ scale.marks-after > trough > slider",
            ".theme-card",
            ".theme-card.selected",
            ".theme-card picture",
            ".banner",
            ".toast",
            ".dialog-card",
            ".key-cap",
            "window.@APP@ .segmented button.segment",
            ".settings-navigation.compact .nav-group",
            ".settings-dialog",
        ],
        css: include_str!("style.css"),
        no_glow_css: "",
        extra_colors: None,
        prefs: || {
            let p = prefs::get();
            ThemePrefs { mode: p.mode, theme: p.theme, glow: p.glow, reduce_motion: p.reduce_motion }
        },
        set_theme: Some(|mode, theme| {
            prefs::update(|p| {
                p.mode = mode;
                p.theme = theme;
            })
        }),
    });
}

/// A palette colour mixed with another, as `#rrggbb` for Pango markup.
pub fn mixed(a: &str, b: &str, t: f64) -> String {
    Rgb::hex(a).mix(Rgb::hex(b), t).to_hex()
}
