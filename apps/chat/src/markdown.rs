//! Markdown to GTK widgets. Each top-level block becomes its own widget and is
//! kept while its source is unchanged, so a streaming reply only rebuilds the
//! block that's still growing.

use gtk::pango;
use gtk::prelude::*;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::cell::RefCell;
use std::rc::Rc;

use crate::store::Source;
use crate::theme;
use crate::util::esc;

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
}

type PreviewFn = Rc<dyn Fn(&str, &str)>;

thread_local! {
    static PREVIEW: RefCell<Option<PreviewFn>> = const { RefCell::new(None) };
}

/// What happens when someone presses Preview on an HTML, SVG or Mermaid block.
pub fn set_preview_handler(f: impl Fn(&str, &str) + 'static) {
    PREVIEW.with(|p| *p.borrow_mut() = Some(Rc::new(f)));
}

pub fn previewable(lang: &str) -> bool {
    matches!(lang, "html" | "svg" | "mermaid" | "xml")
}

pub struct Markdown {
    pub root: gtk::Box,
    blocks: RefCell<Vec<(String, gtk::Widget)>>,
    sources: RefCell<Vec<Source>>,
    text: RefCell<String>,
}

impl Markdown {
    pub fn new() -> Rc<Markdown> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("markdown");
        let md = Rc::new(Markdown {
            root,
            blocks: RefCell::new(Vec::new()),
            sources: RefCell::new(Vec::new()),
            text: RefCell::new(String::new()),
        });
        let weak = Rc::downgrade(&md);
        crate::hub::on(&crate::hub::THEME, &md.root, move || {
            if let Some(md) = weak.upgrade() {
                md.rerender();
            }
        });
        md
    }

    pub fn set_sources(&self, sources: Vec<Source>) {
        if *self.sources.borrow() != sources {
            *self.sources.borrow_mut() = sources;
            self.rerender();
        }
    }

    fn rerender(&self) {
        let text = self.text.borrow().clone();
        for (_, w) in self.blocks.borrow_mut().drain(..) {
            self.root.remove(&w);
        }
        self.set(&text);
    }

    pub fn set(&self, text: &str) {
        *self.text.borrow_mut() = text.to_string();
        let sources: Vec<String> = split_blocks(text);
        let mut blocks = self.blocks.borrow_mut();
        let keep = blocks.iter().zip(&sources).take_while(|((a, _), b)| a == *b).count();
        for (_, w) in blocks.drain(keep..) {
            self.root.remove(&w);
        }
        let cites = self.sources.borrow();
        for src in &sources[keep..] {
            let w = render_block(src, &cites);
            self.root.append(&w);
            blocks.push((src.clone(), w));
        }
    }
}

/// The source text of each top-level block.
pub fn split_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (ev, range) in Parser::new_ext(text, options()).into_offset_iter() {
        match ev {
            Event::Start(_) => {
                if depth == 0 {
                    start = range.start;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.push(text[start..range.end].to_string());
                }
            }
            _ if depth == 0 => out.push(text[range].to_string()),
            _ => {}
        }
    }
    out
}

fn render_block(src: &str, sources: &[Source]) -> gtk::Widget {
    let events: Vec<Event<'static>> = Parser::new_ext(src, options()).map(Event::into_static).collect();
    let bx = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let mut i = 0;
    blocks(&events, &mut i, None, &bx, sources);
    // A single child needs no wrapper.
    if let Some(only) = bx.first_child()
        && only.next_sibling().is_none()
    {
        bx.remove(&only);
        return only;
    }
    bx.upcast()
}

fn is_inline(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. }
            | Tag::Superscript
            | Tag::Subscript
    )
}

fn text_label(markup: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_markup(markup);
    l.set_wrap(true);
    l.set_wrap_mode(pango::WrapMode::WordChar);
    l.set_xalign(0.0);
    l.set_selectable(true);
    l.set_can_focus(false);
    l.add_css_class(class);
    l
}

/// Render block-level events until `until` closes (or the end).
fn blocks(ev: &[Event], i: &mut usize, until: Option<TagEnd>, out: &gtk::Box, sources: &[Source]) {
    while *i < ev.len() {
        match &ev[*i] {
            Event::End(end) if Some(*end) == until => {
                *i += 1;
                return;
            }
            Event::Start(Tag::Paragraph) => {
                *i += 1;
                let m = inline(ev, i, sources);
                skip_end(ev, i, TagEnd::Paragraph);
                if !m.trim().is_empty() {
                    out.append(&text_label(&m, "md-p"));
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let lvl = *level as usize;
                *i += 1;
                let m = inline(ev, i, sources);
                skip_end(ev, i, TagEnd::Heading(*level));
                out.append(&text_label(&m, &format!("md-h{}", lvl.min(4))));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                *i += 1;
                let mut code = String::new();
                while *i < ev.len() {
                    match &ev[*i] {
                        Event::Text(t) => code.push_str(t),
                        Event::End(TagEnd::CodeBlock) => {
                            *i += 1;
                            break;
                        }
                        _ => {}
                    }
                    *i += 1;
                }
                out.append(&code_block(code.trim_end_matches('\n'), &lang));
            }
            Event::Start(Tag::List(start)) => {
                let mut n = *start;
                *i += 1;
                let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
                list.add_css_class("md-list");
                while *i < ev.len() {
                    match &ev[*i] {
                        Event::Start(Tag::Item) => {
                            *i += 1;
                            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                            let marker = match n {
                                Some(k) => {
                                    n = Some(k + 1);
                                    format!("{k}.")
                                }
                                None => "•".into(),
                            };
                            let m = gtk::Label::new(Some(&marker));
                            m.add_css_class("md-marker");
                            m.set_valign(gtk::Align::Start);
                            m.set_yalign(0.0);
                            let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
                            content.set_hexpand(true);
                            blocks(ev, i, Some(TagEnd::Item), &content, sources);
                            row.append(&m);
                            row.append(&content);
                            list.append(&row);
                        }
                        Event::End(TagEnd::List(_)) => {
                            *i += 1;
                            break;
                        }
                        _ => *i += 1,
                    }
                }
                out.append(&list);
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let end = TagEnd::BlockQuote(*kind);
                *i += 1;
                let q = gtk::Box::new(gtk::Orientation::Vertical, 6);
                q.add_css_class("md-quote");
                blocks(ev, i, Some(end), &q, sources);
                out.append(&q);
            }
            Event::Start(Tag::Table(_)) => {
                *i += 1;
                out.append(&table(ev, i, sources));
            }
            Event::Start(Tag::HtmlBlock) => {
                *i += 1;
                let mut html = String::new();
                while *i < ev.len() {
                    match &ev[*i] {
                        Event::Html(t) => html.push_str(t),
                        Event::End(TagEnd::HtmlBlock) => {
                            *i += 1;
                            break;
                        }
                        _ => {}
                    }
                    *i += 1;
                }
                if !html.trim().is_empty() {
                    out.append(&text_label(&esc(html.trim()), "md-html"));
                }
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let label = label.to_string();
                *i += 1;
                let f = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                f.add_css_class("md-footnote");
                let l = gtk::Label::new(Some(&format!("[{label}]")));
                l.set_valign(gtk::Align::Start);
                let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
                blocks(ev, i, Some(TagEnd::FootnoteDefinition), &content, sources);
                f.append(&l);
                f.append(&content);
                out.append(&f);
            }
            Event::Rule => {
                *i += 1;
                let s = gtk::Separator::new(gtk::Orientation::Horizontal);
                s.add_css_class("md-rule");
                out.append(&s);
            }
            Event::Start(tag) if !is_inline(tag) => {
                // Anything else (definition lists, metadata): show its text.
                *i += 1;
            }
            Event::End(_) => *i += 1,
            _ => {
                // Inline content straight inside a block (tight list items).
                let m = inline(ev, i, sources);
                if !m.trim().is_empty() {
                    out.append(&text_label(&m, "md-p"));
                }
            }
        }
    }
}

fn skip_end(ev: &[Event], i: &mut usize, end: TagEnd) {
    if *i < ev.len() && ev[*i] == Event::End(end) {
        *i += 1;
    }
}

fn code_bg() -> String {
    let p = theme::palette();
    theme::mixed(&p.bg, &p.muted, 0.55)
}

/// Collect inline events into Pango markup, stopping before any block
/// boundary (a block start, or the end of the enclosing block).
fn inline(ev: &[Event], i: &mut usize, sources: &[Source]) -> String {
    let mut m = String::new();
    let mut in_link = 0;
    let mut image_alt: Option<String> = None;
    while *i < ev.len() {
        match &ev[*i] {
            Event::Text(t) => {
                if let Some(alt) = image_alt.as_mut() {
                    alt.push_str(t);
                } else if in_link > 0 || sources.is_empty() {
                    m.push_str(&esc(t));
                } else {
                    // The parser splits "[1]" into "[" and "1]"; join runs of text first.
                    let mut run = t.to_string();
                    while let Some(Event::Text(next)) = ev.get(*i + 1) {
                        run.push_str(next);
                        *i += 1;
                    }
                    m.push_str(&cite(&run, sources));
                }
            }
            Event::Code(t) => m.push_str(&format!(
                "<span font_family=\"JetBrains Mono, monospace\" background=\"{}\"> {} </span>",
                code_bg(),
                esc(t)
            )),
            Event::InlineMath(t) => m.push_str(&format!("<i>{}</i>", esc(&math(t)))),
            Event::DisplayMath(t) => m.push_str(&format!("\n<i>{}</i>\n", esc(&math(t.trim())))),
            Event::SoftBreak => m.push(' '),
            Event::HardBreak => m.push('\n'),
            Event::TaskListMarker(done) => m.push_str(if *done { "☑ " } else { "☐ " }),
            Event::FootnoteReference(l) => m.push_str(&format!("<sup>[{}]</sup>", esc(l))),
            Event::InlineHtml(h) => {
                let lower = h.to_ascii_lowercase();
                if lower.starts_with("<br") {
                    m.push('\n');
                } else if !lower.starts_with("</") && !lower.starts_with('<') {
                    m.push_str(&esc(h));
                }
            }
            Event::Start(Tag::Emphasis) => m.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => m.push_str("</i>"),
            Event::Start(Tag::Strong) => m.push_str("<b>"),
            Event::End(TagEnd::Strong) => m.push_str("</b>"),
            Event::Start(Tag::Strikethrough) => m.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => m.push_str("</s>"),
            Event::Start(Tag::Superscript) => m.push_str("<sup>"),
            Event::End(TagEnd::Superscript) => m.push_str("</sup>"),
            Event::Start(Tag::Subscript) => m.push_str("<sub>"),
            Event::End(TagEnd::Subscript) => m.push_str("</sub>"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link += 1;
                m.push_str(&format!("<a href=\"{}\">", esc(dest_url)));
            }
            Event::End(TagEnd::Link) => {
                in_link -= 1;
                m.push_str("</a>");
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image_alt = Some(String::new());
                m.push_str(&format!("<a href=\"{}\">", esc(dest_url)));
            }
            Event::End(TagEnd::Image) => {
                let alt = image_alt.take().unwrap_or_default();
                m.push_str(&format!("🖼 {}</a>", esc(if alt.is_empty() { "image" } else { &alt })));
            }
            Event::Start(_) | Event::End(_) | Event::Rule => break,
            Event::Html(h) => m.push_str(&esc(h)),
        }
        *i += 1;
    }
    m
}

/// Common LaTeX as plain Unicode: `x^2 \times \frac{a}{b}` → `x² × (a)/(b)`.
/// Not a typesetter, but formulas in chat replies become readable.
pub fn math(src: &str) -> String {
    const SYMBOLS: &[(&str, &str)] = &[
        ("\\times", "×"),
        ("\\cdot", "·"),
        ("\\div", "÷"),
        ("\\pm", "±"),
        ("\\mp", "∓"),
        ("\\leq", "≤"),
        ("\\le", "≤"),
        ("\\geq", "≥"),
        ("\\ge", "≥"),
        ("\\neq", "≠"),
        ("\\ne", "≠"),
        ("\\approx", "≈"),
        ("\\equiv", "≡"),
        ("\\infty", "∞"),
        ("\\sum", "Σ"),
        ("\\prod", "Π"),
        ("\\int", "∫"),
        ("\\partial", "∂"),
        ("\\nabla", "∇"),
        ("\\sqrt", "√"),
        ("\\rightarrow", "→"),
        ("\\Rightarrow", "⇒"),
        ("\\leftarrow", "←"),
        ("\\to", "→"),
        ("\\in", "∈"),
        ("\\notin", "∉"),
        ("\\subseteq", "⊆"),
        ("\\subset", "⊂"),
        ("\\cup", "∪"),
        ("\\cap", "∩"),
        ("\\forall", "∀"),
        ("\\exists", "∃"),
        ("\\ldots", "…"),
        ("\\dots", "…"),
        ("\\cdots", "⋯"),
        ("\\degree", "°"),
        ("\\circ", "∘"),
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\Gamma", "Γ"),
        ("\\delta", "δ"),
        ("\\Delta", "Δ"),
        ("\\epsilon", "ε"),
        ("\\varepsilon", "ε"),
        ("\\theta", "θ"),
        ("\\lambda", "λ"),
        ("\\mu", "μ"),
        ("\\pi", "π"),
        ("\\rho", "ρ"),
        ("\\sigma", "σ"),
        ("\\Sigma", "Σ"),
        ("\\tau", "τ"),
        ("\\phi", "φ"),
        ("\\Phi", "Φ"),
        ("\\omega", "ω"),
        ("\\Omega", "Ω"),
        ("\\quad", "  "),
        ("\\qquad", "    "),
        ("\\,", " "),
        (r"\;", " "),
        ("\\!", ""),
        ("\\left", ""),
        ("\\right", ""),
        ("\\{", "{"),
        ("\\}", "}"),
        ("\\%", "%"),
    ];
    let mut s = src.to_string();
    // \frac{a}{b} → (a)/(b), innermost first.
    while let Some(i) = s.find("\\frac{") {
        let Some((num, after_num)) = group(&s[i + 5..]) else { break };
        let Some((den, rest)) = group(after_num) else { break };
        let wrap = |x: &str| if x.chars().count() > 1 { format!("({x})") } else { x.to_string() };
        s = format!("{}{}/{}{}", &s[..i], wrap(num), wrap(den), rest);
    }
    for cmd in ["\\text", "\\mathrm", "\\mathbf", "\\mathit", "\\operatorname"] {
        while let Some(i) = s.find(&format!("{cmd}{{")) {
            let Some((inner, rest)) = group(&s[i + cmd.len()..]) else { break };
            s = format!("{}{}{}", &s[..i], inner, rest);
        }
    }
    // Longest names first so \leq isn't read as \le + q.
    let mut symbols = SYMBOLS.to_vec();
    symbols.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    for (k, v) in symbols {
        s = s.replace(k, v);
    }
    const SUP: &[(char, char)] = &[
        ('0', '⁰'),
        ('1', '¹'),
        ('2', '²'),
        ('3', '³'),
        ('4', '⁴'),
        ('5', '⁵'),
        ('6', '⁶'),
        ('7', '⁷'),
        ('8', '⁸'),
        ('9', '⁹'),
        ('n', 'ⁿ'),
        ('+', '⁺'),
        ('-', '⁻'),
        ('i', 'ⁱ'),
    ];
    let sup = |c: char| SUP.iter().find(|(a, _)| *a == c).map(|(_, b)| *b);
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '^' {
            // ^2 or ^{12}
            let (body, next) = if chars.get(i + 1) == Some(&'{') {
                let end = chars[i + 2..].iter().position(|c| *c == '}').map(|p| i + 2 + p);
                match end {
                    Some(e) => (chars[i + 2..e].to_vec(), e + 1),
                    None => (vec![], i + 1),
                }
            } else {
                (chars.get(i + 1).map(|c| vec![*c]).unwrap_or_default(), i + 2)
            };
            if !body.is_empty() && body.iter().all(|c| sup(*c).is_some()) {
                out.extend(body.iter().filter_map(|c| sup(*c)));
                i = next;
                continue;
            }
        }
        if chars[i] != '{' && chars[i] != '}' {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// `{inner}rest` → (inner, rest), honouring nested braces.
fn group(s: &str) -> Option<(&str, &str)> {
    let s = s.strip_prefix('{')?;
    let mut depth = 1;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Turn `[3]` (or `[1, 4]`) into links to those sources.
fn cite(text: &str, sources: &[Source]) -> String {
    let link = |n: usize| {
        format!("<a href=\"{}\" title=\"{}\"><sup>[{n}]</sup></a>", esc(&sources[n - 1].url), esc(&sources[n - 1].title))
    };
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let close = after.find(']');
        let nums: Option<Vec<usize>> = close.and_then(|c| {
            let inner = &after[..c];
            let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
            let ns: Vec<usize> = parts.iter().filter_map(|p| p.parse().ok()).collect();
            (!inner.is_empty() && ns.len() == parts.len() && ns.iter().all(|n| *n >= 1 && *n <= sources.len())).then_some(ns)
        });
        match (close, nums) {
            (Some(c), Some(ns)) => {
                out.push_str(&esc(&rest[..open]));
                for n in ns {
                    out.push_str(&link(n));
                }
                rest = &after[c + 1..];
            }
            _ => {
                out.push_str(&esc(&rest[..open + 1]));
                rest = after;
            }
        }
    }
    out.push_str(&esc(rest));
    out
}

fn table(ev: &[Event], i: &mut usize, sources: &[Source]) -> gtk::Widget {
    let grid = gtk::Grid::new();
    grid.add_css_class("md-table");
    grid.set_column_spacing(0);
    grid.set_row_spacing(0);
    let (mut row, mut col) = (0, 0);
    let mut head = false;
    while *i < ev.len() {
        match &ev[*i] {
            Event::Start(Tag::TableHead) => {
                head = true;
                col = 0;
                *i += 1;
            }
            Event::End(TagEnd::TableHead) => {
                head = false;
                row += 1;
                *i += 1;
            }
            Event::Start(Tag::TableRow) => {
                col = 0;
                *i += 1;
            }
            Event::End(TagEnd::TableRow) => {
                row += 1;
                *i += 1;
            }
            Event::Start(Tag::TableCell) => {
                *i += 1;
                let m = inline(ev, i, sources);
                skip_end(ev, i, TagEnd::TableCell);
                let markup = if head { format!("<b>{m}</b>") } else { m };
                let l = text_label(&markup, "md-cell");
                if head {
                    l.add_css_class("head");
                }
                l.set_hexpand(true);
                grid.attach(&l, col, row, 1, 1);
                col += 1;
            }
            Event::End(TagEnd::Table) => {
                *i += 1;
                break;
            }
            _ => *i += 1,
        }
    }
    grid.set_hexpand(true);
    grid.upcast()
}

pub fn code_block(code: &str, lang: &str) -> gtk::Widget {
    let bx = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bx.add_css_class("md-code");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    head.add_css_class("md-code-head");
    let l = gtk::Label::new(Some(if lang.is_empty() { "text" } else { lang }));
    l.add_css_class("mono");
    l.add_css_class("dim");
    l.set_hexpand(true);
    l.set_xalign(0.0);
    head.append(&l);
    let lower = lang.to_ascii_lowercase();
    if previewable(&lower) {
        let b = gtk::Button::with_label("Preview");
        b.add_css_class("flat");
        b.add_css_class("small");
        let (c, lg) = (code.to_string(), lower.clone());
        b.connect_clicked(move |_| {
            if let Some(f) = PREVIEW.with(|p| p.borrow().clone()) {
                f(&lg, &c);
            }
        });
        head.append(&b);
    }
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("flat");
    copy.add_css_class("small");
    copy.set_tooltip_text(Some("Copy code"));
    let c = code.to_string();
    copy.connect_clicked(move |b| {
        b.clipboard().set_text(&c);
        crate::window::toast("Copied");
    });
    head.append(&copy);
    bx.append(&head);
    let body = gtk::Label::new(None);
    body.set_markup(&crate::highlight::markup(code, &lower));
    body.set_xalign(0.0);
    body.set_selectable(true);
    body.set_can_focus(false);
    body.add_css_class("md-code-body");
    body.add_css_class("mono");
    let sw = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&body)
        .build();
    bx.append(&sw);
    bx.upcast()
}

/// Markdown reduced to plain sentences (for reading aloud).
pub fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for ev in Parser::new_ext(text, options()) {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                out.push_str("\n(code omitted)\n");
            }
            Event::Text(t) if !in_code => out.push_str(&t),
            Event::Code(t) => out.push_str(&t),
            Event::SoftBreak => out.push(' '),
            Event::HardBreak | Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => out.push('\n'),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_top_level_blocks() {
        let b = split_blocks("# Title\n\nSome *text*.\n\n- a\n- b\n\n```rs\nfn x() {}\n```\n\n---\n");
        assert_eq!(b.len(), 5);
        assert!(b[0].starts_with("# Title"));
        assert!(b[2].contains("- b"));
        assert!(b[3].starts_with("```rs"));
    }

    #[test]
    fn growing_text_keeps_earlier_blocks() {
        let a = split_blocks("Para one.\n\nPara tw");
        let b = split_blocks("Para one.\n\nPara two is longer");
        assert_eq!(a[0], b[0]);
        assert_ne!(a[1], b[1]);
    }

    #[test]
    fn citations_link_to_sources() {
        let s = vec![Source { title: "A".into(), url: "https://a.example/?x=1&y=2".into() }];
        let m = cite("See [1] and [2] or [x].", &s);
        assert!(m.contains("<a href=\"https://a.example/?x=1&amp;y=2\""));
        assert!(m.contains("[2]"));
        assert!(!m.contains("href=\"\""));
        assert!(m.ends_with("or [x]."));
        let m = cite("See [1, 1].", &s);
        assert_eq!(m.matches("<a ").count(), 2);
    }

    #[test]
    fn split_citations_still_link() {
        let s = vec![Source { title: "A".into(), url: "https://a".into() }];
        let ev: Vec<Event> = Parser::new_ext("Rust 1.98 is out [1].", options()).collect();
        let mut i = 1;
        let m = inline(&ev, &mut i, &s);
        assert!(m.contains("<a href=\"https://a\""), "{m}");
    }

    #[test]
    fn inline_markup_is_escaped() {
        let ev: Vec<Event> = Parser::new_ext("a < b **bold** `x<y>`", options()).collect();
        let mut i = 1;
        let m = inline(&ev, &mut i, &[]);
        assert!(m.starts_with("a &lt; b <b>bold</b>"));
        assert!(m.contains("x&lt;y&gt;"));
    }

    #[test]
    fn math_becomes_readable() {
        assert_eq!(math("1234 \\times 5678"), "1234 × 5678");
        assert_eq!(math("x^2 + y^{10} \\leq \\frac{a+b}{2}"), "x² + y¹⁰ ≤ (a+b)/2");
        assert_eq!(math("\\text{speed} = \\frac{d}{t}"), "speed = d/t");
        assert_eq!(math("e^{2i}"), "e²ⁱ");
    }

    #[test]
    fn plain_text_for_speech() {
        let p = plain("# Hi\n\nThis is **bold**.\n\n```\ncode\n```\n");
        assert_eq!(p, "Hi\nThis is bold.\n\n(code omitted)");
    }
}
