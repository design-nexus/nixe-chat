//! Theming. The whole stylesheet is written against the semantic colour
//! tokens (`@theme_bg`, `@theme_accent`, …); a theme is just a value for each.

use crate::{paths, prefs};
use gtk::{gdk, glib};
use serde::Deserialize;
use std::cell::RefCell;
use std::time::SystemTime;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Palette {
    pub bg: String,
    pub surface: String,
    pub text: String,
    pub dim_text: String,
    pub accent: String,
    pub border: String,
    pub muted: String,
    pub highlight: String,
    pub danger: String,
    pub glow: String,
    pub shadow: String,
    #[serde(default)]
    pub light: bool,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub palette: Palette,
}

macro_rules! palette {
    ($bg:expr, $surface:expr, $text:expr, $dim:expr, $accent:expr, $border:expr, $muted:expr,
     $highlight:expr, $danger:expr, $glow:expr, $shadow:expr, $light:expr) => {
        Palette {
            bg: $bg.into(),
            surface: $surface.into(),
            text: $text.into(),
            dim_text: $dim.into(),
            accent: $accent.into(),
            border: $border.into(),
            muted: $muted.into(),
            highlight: $highlight.into(),
            danger: $danger.into(),
            glow: $glow.into(),
            shadow: $shadow.into(),
            light: $light,
        }
    };
}

fn theme(id: &str, name: &str, palette: Palette) -> Theme {
    Theme { id: id.into(), name: name.into(), palette }
}

pub fn bundled() -> Vec<Theme> {
    vec![
        theme(
            "dracula",
            "Dracula",
            palette!(
                "#282a36", "#21222c", "#f8f8f2", "#6272a4", "#bd93f9", "#44475a", "#44475a", "#343746", "#ff5555", "#bd93f9",
                "#191a21", false
            ),
        ),
        theme(
            "catppuccin-mocha",
            "Catppuccin Mocha",
            palette!(
                "#1e1e2e", "#181825", "#cdd6f4", "#7f849c", "#89b4fa", "#313244", "#45475a", "#313244", "#f38ba8", "#89b4fa",
                "#11111b", false
            ),
        ),
        theme(
            "catppuccin-macchiato",
            "Catppuccin Macchiato",
            palette!(
                "#24273a", "#1e2030", "#cad3f5", "#8087a2", "#8aadf4", "#363a4f", "#494d64", "#363a4f", "#ed8796", "#8aadf4",
                "#181926", false
            ),
        ),
        theme(
            "catppuccin-frappe",
            "Catppuccin Frappé",
            palette!(
                "#303446", "#292c3c", "#c6d0f5", "#838ba7", "#8caaee", "#414559", "#51576d", "#414559", "#e78284", "#8caaee",
                "#232634", false
            ),
        ),
        theme(
            "catppuccin-latte",
            "Catppuccin Latte",
            palette!(
                "#eff1f5", "#e6e9ef", "#4c4f69", "#8c8fa1", "#1e66f5", "#ccd0da", "#bcc0cc", "#dce0e8", "#d20f39", "#1e66f5",
                "#9ca0b0", true
            ),
        ),
        theme(
            "tokyo-night",
            "Tokyo Night",
            palette!(
                "#1a1b26", "#16161e", "#c0caf5", "#565f89", "#7aa2f7", "#292e42", "#3b4261", "#283457", "#f7768e", "#7aa2f7",
                "#0f0f14", false
            ),
        ),
        theme(
            "tokyo-night-storm",
            "Tokyo Night Storm",
            palette!(
                "#24283b", "#1f2335", "#c0caf5", "#565f89", "#7aa2f7", "#292e42", "#3b4261", "#2e3c64", "#f7768e", "#7aa2f7",
                "#1b1e2d", false
            ),
        ),
        theme(
            "tokyo-night-moon",
            "Tokyo Night Moon",
            palette!(
                "#222436", "#1e2030", "#c8d3f5", "#636da6", "#82aaff", "#2f334d", "#3b4261", "#2d3f76", "#ff757f", "#82aaff",
                "#191a2a", false
            ),
        ),
        theme(
            "one-dark-pro",
            "One Dark Pro",
            palette!(
                "#282c34", "#21252b", "#abb2bf", "#5c6370", "#61afef", "#3e4451", "#4b5263", "#2c313a", "#e06c75", "#61afef",
                "#181a1f", false
            ),
        ),
        theme(
            "nord",
            "Nord",
            palette!(
                "#2e3440", "#292e39", "#eceff4", "#7b88a1", "#88c0d0", "#3b4252", "#434c5e", "#3b4252", "#bf616a", "#88c0d0",
                "#242933", false
            ),
        ),
        theme(
            "gruvbox-dark",
            "Gruvbox Dark",
            palette!(
                "#282828", "#1d2021", "#ebdbb2", "#928374", "#fabd2f", "#3c3836", "#504945", "#3c3836", "#fb4934", "#fabd2f",
                "#141414", false
            ),
        ),
        theme(
            "rose-pine",
            "Rosé Pine",
            palette!(
                "#191724", "#1f1d2e", "#e0def4", "#6e6a86", "#c4a7e7", "#26233a", "#403d52", "#26233a", "#eb6f92", "#c4a7e7",
                "#111019", false
            ),
        ),
        theme(
            "everforest",
            "Everforest",
            palette!(
                "#2d353b", "#272e33", "#d3c6aa", "#859289", "#a7c080", "#3d484d", "#475258", "#3d484d", "#e67e80", "#a7c080",
                "#1e2326", false
            ),
        ),
        theme(
            "solarized-dark",
            "Solarized Dark",
            palette!(
                "#002b36", "#00232c", "#93a1a1", "#586e75", "#268bd2", "#073642", "#0f4a56", "#073642", "#dc322f", "#268bd2",
                "#001e26", false
            ),
        ),
        theme(
            "kanagawa",
            "Kanagawa",
            palette!(
                "#1f1f28", "#16161d", "#dcd7ba", "#727169", "#7e9cd8", "#2a2a37", "#363646", "#2d4f67", "#e82424", "#7e9cd8",
                "#0d0c0c", false
            ),
        ),
    ]
}

#[derive(Deserialize)]
struct CustomFile {
    name: String,
    #[serde(flatten)]
    palette: Palette,
}

/// User themes in `~/.config/nexus-chat/themes/*.toml`.
pub fn custom() -> Vec<Theme> {
    let Ok(entries) = std::fs::read_dir(paths::custom_themes_dir()) else { return vec![] };
    let mut themes: Vec<Theme> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let file: CustomFile = toml::from_str(&text).ok()?;
            let id = format!("custom:{}", e.path().file_stem()?.to_string_lossy());
            Some(Theme { id, name: file.name, palette: file.palette })
        })
        .collect();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

pub fn all() -> Vec<Theme> {
    let mut themes = bundled();
    themes.extend(custom());
    themes
}

/// Map Omarchy Quattro's `colors.toml` onto the semantic tokens.
pub fn from_omarchy_colors(text: &str) -> Option<Palette> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let get = |key: &str| table.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let background = get("background")?;
    let foreground = get("foreground")?;
    let accent = get("accent").or_else(|| get("blue"))?;
    let light = table.get("mode").and_then(|v| v.as_str()) == Some("light");
    Some(Palette {
        surface: get("dark_background").unwrap_or_else(|| background.clone()),
        dim_text: get("dark_foreground").or_else(|| get("muted")).unwrap_or_else(|| foreground.clone()),
        border: get("lighter_background").or_else(|| get("selection")).unwrap_or_else(|| foreground.clone()),
        muted: get("muted").or_else(|| get("lighter_background")).unwrap_or_else(|| background.clone()),
        highlight: get("selection").or_else(|| get("lighter_background")).unwrap_or_else(|| background.clone()),
        danger: get("red").unwrap_or_else(|| "#e06c75".into()),
        shadow: get("darker_background").unwrap_or_else(|| "#000000".into()),
        glow: accent.clone(),
        bg: background,
        text: foreground,
        accent,
        light,
    })
}

pub fn omarchy_palette() -> Option<Palette> {
    from_omarchy_colors(&std::fs::read_to_string(paths::omarchy_colors()).ok()?)
}

pub fn omarchy_available() -> bool {
    omarchy_palette().is_some()
}

/// The palette that should be showing right now according to the prefs.
pub fn current_palette() -> Palette {
    let prefs = prefs::get();
    if prefs.mode == prefs::ThemeMode::Omarchy
        && let Some(p) = omarchy_palette()
    {
        return p;
    }
    let themes = all();
    themes
        .iter()
        .find(|t| t.id == prefs.theme)
        .or_else(|| themes.iter().find(|t| t.id == "tokyo-night"))
        .map(|t| t.palette.clone())
        .expect("bundled themes are never empty")
}

pub fn palette_css(p: &Palette) -> String {
    format!(
        "@define-color theme_bg {};\n@define-color theme_surface {};\n@define-color theme_text {};\n\
         @define-color theme_dim_text {};\n@define-color theme_accent {};\n@define-color theme_border {};\n\
         @define-color theme_muted {};\n@define-color theme_highlight {};\n@define-color theme_danger {};\n\
         @define-color theme_glow {};\n@define-color theme_shadow {};\n\
         @define-color theme_on_accent {};\n",
        p.bg,
        p.surface,
        p.text,
        p.dim_text,
        p.accent,
        p.border,
        p.muted,
        p.highlight,
        p.danger,
        p.glow,
        p.shadow,
        if p.light { "#ffffff" } else { &p.bg },
    )
}

const STYLE: &str = include_str!("style.css");

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static ACTIVE: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

/// The palette currently on screen (cached; graphs read it on every frame).
pub fn palette() -> Palette {
    ACTIVE.with(|a| a.borrow_mut().get_or_insert_with(current_palette).clone())
}

/// An RGB colour in 0..1, for drawing with cairo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

impl Rgb {
    pub fn hex(s: &str) -> Rgb {
        let h = s.trim().trim_start_matches('#');
        let v = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).unwrap_or(0) as f64 / 255.0;
        if h.len() == 3 {
            let c = |i: usize| h.get(i..i + 1).and_then(|x| u8::from_str_radix(x, 16).ok()).unwrap_or(0) as f64 / 15.0;
            return Rgb(c(0), c(1), c(2));
        }
        Rgb(v(0), v(2), v(4))
    }

    pub fn to_hex(self) -> String {
        let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", c(self.0), c(self.1), c(self.2))
    }

    /// `mix(self, other, t)`, as in GTK CSS.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        Rgb(self.0 + (other.0 - self.0) * t, self.1 + (other.1 - self.1) * t, self.2 + (other.2 - self.2) * t)
    }
}

/// Install the stylesheet and the current palette on the default display.
pub fn install() {
    let display = gdk::Display::default().expect("no display");
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    PROVIDER.with(|p| *p.borrow_mut() = Some(provider));
    apply();
    watch_omarchy();
}

/// Re-read prefs and restyle live. The palette and the stylesheet are loaded as
/// one sheet so every `@theme_*` reference resolves against the new colours.
pub fn apply() {
    let palette = current_palette();
    ACTIVE.with(|a| *a.borrow_mut() = Some(palette.clone()));
    let mut css = palette_css(&palette);
    css.push_str(STYLE);
    if !prefs::get().glow {
        css.push_str(
            "\n.settings-option.search-hit, .theme-card.selected, .toast, .stat-tile.selected { box-shadow: 0 0 0 1px alpha(@theme_accent, 0.6); }\n\
             window.chat-window scale > trough > slider { box-shadow: 0 0 0 1px alpha(@theme_accent, 0.72); }\n\
             .composer:focus-within, .chat-item.active, .quant-row.recommended { box-shadow: 0 0 0 1px alpha(@theme_accent, 0.6); }\n",
        );
    }
    PROVIDER.with(|p| {
        if let Some(provider) = p.borrow().as_ref() {
            provider.load_from_string(&css);
        }
    });
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(!palette.light);
        settings.set_gtk_enable_animations(!prefs::get().reduce_motion);
    }
    // Markup baked with palette colours (code, inline code) is rebuilt.
    crate::hub::THEME.with(|h| h.emit());
}

/// A palette colour mixed with another, as `#rrggbb` for Pango markup.
pub fn mixed(a: &str, b: &str, t: f64) -> String {
    Rgb::hex(a).mix(Rgb::hex(b), t).to_hex()
}

fn stamp() -> Option<(SystemTime, String)> {
    let colors = paths::omarchy_colors();
    let modified = std::fs::metadata(&colors).and_then(|m| m.modified()).ok()?;
    let name = std::fs::read_to_string(paths::state_home().join("omarchy/current/theme.name")).unwrap_or_default();
    Some((modified, name))
}

/// Omarchy swaps the theme by rewriting its state directory; a light poll is the
/// most robust way to notice every variant of that.
fn watch_omarchy() {
    let last = RefCell::new(stamp());
    glib::timeout_add_seconds_local(1, move || {
        let now = stamp();
        if now != *last.borrow() {
            *last.borrow_mut() = now;
            if prefs::get().mode == prefs::ThemeMode::Omarchy {
                apply();
            }
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_omarchy_colors() {
        let text = r##"
            mode = "dark"
            accent = "#89b4fa"
            selection = "#45475a"
            muted = "#585b70"
            background = "#1e1e2e"
            dark_background = "#161622"
            darker_background = "#101019"
            lighter_background = "#313244"
            foreground = "#cdd6f4"
            dark_foreground = "#6c7086"
            red = "#f38ba8"
        "##;
        let p = from_omarchy_colors(text).unwrap();
        assert_eq!(p.bg, "#1e1e2e");
        assert_eq!(p.surface, "#161622");
        assert_eq!(p.text, "#cdd6f4");
        assert_eq!(p.dim_text, "#6c7086");
        assert_eq!(p.accent, "#89b4fa");
        assert_eq!(p.border, "#313244");
        assert_eq!(p.highlight, "#45475a");
        assert_eq!(p.danger, "#f38ba8");
        assert_eq!(p.glow, "#89b4fa");
        assert!(!p.light);
    }

    #[test]
    fn rejects_incomplete_colors() {
        assert!(from_omarchy_colors("accent = \"#fff\"").is_none());
    }

    #[test]
    fn bundled_ids_are_unique() {
        let themes = bundled();
        let mut ids: Vec<_> = themes.iter().map(|t| t.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), themes.len());
    }
}
