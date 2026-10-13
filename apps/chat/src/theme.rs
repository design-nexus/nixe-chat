//! Theming: the shared Nixe theme engine, given this app's stylesheet.

pub use nixe_theme::*;

use crate::prefs;

/// Install the stylesheet and the current palette, and follow Omarchy live.
pub fn install() {
    nixe_theme::on_change(|| crate::hub::THEME.with(|h| h.emit()));
    nixe_theme::install(Style {
        css: include_str!("style.css"),
        no_glow_css: "",
        extra_colors: None,
        prefs: || {
            let p = prefs::get();
            ThemePrefs { mode: p.mode, theme: p.theme, glow: p.glow, reduce_motion: p.reduce_motion }
        },
    });
}

/// A palette colour mixed with another, as `#rrggbb` for Pango markup.
pub fn mixed(a: &str, b: &str, t: f64) -> String {
    Rgb::hex(a).mix(Rgb::hex(b), t).to_hex()
}
