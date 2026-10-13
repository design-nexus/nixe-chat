//! Syntax highlighting for code blocks, as Pango markup. The colours are
//! derived from the theme tokens, so code follows the theme like everything else.

use std::cell::RefCell;
use std::str::FromStr;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::{self, Rgb};
use crate::util::esc;

thread_local! {
    static SYNTAXES: SyntaxSet = SyntaxSet::load_defaults_newlines();
    static THEME: RefCell<Option<(String, Theme)>> = const { RefCell::new(None) };
}

fn color(c: Rgb) -> Color {
    let v = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color { r: v(c.0), g: v(c.1), b: v(c.2), a: 255 }
}

fn build_theme() -> Theme {
    let p = theme::palette();
    let (text, dim, accent, danger) = (Rgb::hex(&p.text), Rgb::hex(&p.dim_text), Rgb::hex(&p.accent), Rgb::hex(&p.danger));
    let item = |sel: &str, c: Rgb, style: Option<FontStyle>| ThemeItem {
        scope: ScopeSelectors::from_str(sel).expect("valid scope selector"),
        style: StyleModifier { foreground: Some(color(c)), background: None, font_style: style },
    };
    Theme {
        name: None,
        author: None,
        settings: ThemeSettings { foreground: Some(color(text)), ..Default::default() },
        scopes: vec![
            item("comment, punctuation.definition.comment", dim, Some(FontStyle::ITALIC)),
            item("keyword, storage, keyword.control, keyword.other", accent, None),
            item("string, constant.character, markup.raw", accent.mix(danger, 0.55), None),
            item("constant.numeric, constant.language, constant.other", danger, None),
            item("entity.name.function, support.function, meta.function-call", accent.mix(text, 0.45), None),
            item("entity.name.type, entity.name.class, support.type, support.class, storage.type", danger.mix(text, 0.4), None),
            item("entity.name.tag, entity.other.attribute-name", accent.mix(text, 0.25), None),
            item("variable.parameter, variable.other", text, None),
            item("keyword.operator, punctuation", text.mix(dim, 0.5), None),
            item("markup.heading", accent, Some(FontStyle::BOLD)),
        ],
    }
}

fn with_theme<T>(f: impl FnOnce(&Theme) -> T) -> T {
    let p = theme::palette();
    let key = format!("{}{}{}{}", p.text, p.dim_text, p.accent, p.danger);
    THEME.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|(k, _)| *k != key) {
            *slot = Some((key, build_theme()));
        }
        f(&slot.as_ref().expect("just built").1)
    })
}

/// Language names people write after ``` that syntect knows by another name.
fn alias(lang: &str) -> &str {
    match lang {
        "sh" | "shell" | "zsh" | "console" => "bash",
        "js" | "jsx" | "ts" | "tsx" | "typescript" | "javascript" | "mjs" => "js",
        "py" | "python3" => "py",
        "rs" => "rs",
        "yml" => "yaml",
        "c++" | "cpp" | "hpp" => "cpp",
        "golang" => "go",
        "md" => "md",
        "toml" | "ini" | "conf" => "ini",
        "svg" | "xml" => "xml",
        "jsonc" | "json5" => "json",
        other => other,
    }
}

/// Highlighted Pango markup for `code`. Unknown languages come back escaped.
pub fn markup(code: &str, lang: &str) -> String {
    let lang = alias(lang.trim().to_ascii_lowercase().as_str()).to_string();
    SYNTAXES.with(|ss| {
        let Some(syntax) = ss.find_syntax_by_token(&lang).or_else(|| ss.find_syntax_by_extension(&lang)) else {
            return esc(code);
        };
        // Very long blocks are slow to highlight; show them plain.
        if code.len() > 60_000 {
            return esc(code);
        }
        with_theme(|theme| {
            let mut h = HighlightLines::new(syntax, theme);
            let mut out = String::with_capacity(code.len() * 2);
            for line in LinesWithEndings::from(code) {
                let Ok(ranges) = h.highlight_line(line, ss) else { return esc(code) };
                for (style, text) in ranges {
                    let c = style.foreground;
                    let mut attrs = format!("foreground=\"#{:02x}{:02x}{:02x}\"", c.r, c.g, c.b);
                    if style.font_style.contains(FontStyle::ITALIC) {
                        attrs.push_str(" style=\"italic\"");
                    }
                    if style.font_style.contains(FontStyle::BOLD) {
                        attrs.push_str(" weight=\"bold\"");
                    }
                    out.push_str(&format!("<span {attrs}>{}</span>", esc(text)));
                }
            }
            out.trim_end_matches('\n').to_string()
        })
    })
}
