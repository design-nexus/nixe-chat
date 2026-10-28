//! Marketplace: search Hugging Face GGUF repositories and the Ollama library,
//! see which downloads fit this machine, and pull them.

use gtk::prelude::*;
use gtk::{glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::hf::{self, Kind, Sort};
use crate::hw::{self, Fit, Memory};
use crate::markdown::Markdown;
use crate::ollama::library;
use crate::{hub, models, prefs, pulls, rt, util, widgets};

#[derive(Clone, Copy, PartialEq)]
enum Source {
    HuggingFace,
    Ollama,
}

struct Market {
    source: Cell<Source>,
    sort: Cell<Sort>,
    kind: Cell<Kind>,
    query: RefCell<String>,
    stack: gtk::Stack,
    results: gtk::Box,
    detail: gtk::Box,
    hf_controls: gtk::FlowBox,
    memory: Cell<Memory>,
    generation: Cell<u64>,
}

pub fn build() -> gtk::Widget {
    let page = widgets::page("marketplace");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(if prefs::get().reduce_motion { 0 } else { 160 });
    let results = widgets::vbox(6);
    results.add_css_class("market-results");
    let detail = widgets::vbox(10);
    stack.add_named(&results, Some("results"));
    stack.add_named(&detail, Some("detail"));
    // The two filters sit side by side, or one above the other in a narrow window.
    let hf_controls = gtk::FlowBox::new();
    hf_controls.set_selection_mode(gtk::SelectionMode::None);
    hf_controls.set_min_children_per_line(1);
    hf_controls.set_max_children_per_line(2);
    hf_controls.set_column_spacing(10);
    hf_controls.set_row_spacing(8);

    let pr = prefs::get();
    let mem = hw::detect(pr.vram_bytes);
    if mem.vram > 0 && mem.vram != pr.vram_bytes {
        prefs::update(|p| p.vram_bytes = mem.vram);
    }
    let m = Rc::new(Market {
        source: Cell::new(Source::HuggingFace),
        sort: Cell::new(Sort::Trending),
        kind: Cell::new(Kind::Text),
        query: RefCell::new(String::new()),
        stack: stack.clone(),
        results,
        detail,
        hf_controls: hf_controls.clone(),
        memory: Cell::new(mem),
        generation: Cell::new(0),
    });

    let controls = widgets::vbox(10);
    controls.add_css_class("market-controls");
    let top = widgets::hbox(10);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search models"));
    search.add_css_class("settings-search");
    search.set_hexpand(true);
    top.append(&search);
    let m2 = m.clone();
    let src = widgets::segmented(&[("hf", "Hugging Face"), ("ollama", "Ollama library")], "hf", move |v| {
        m2.source.set(if v == "ollama" { Source::Ollama } else { Source::HuggingFace });
        m2.hf_controls.set_visible(v != "ollama");
        search_now(&m2);
    });
    src.set_visible(pr.host().backend == prefs::Backend::Ollama);
    let src2 = src.clone();
    let market = m.clone();
    hub::on(&hub::HOST, &src, move || {
        let ollama = prefs::get().host().backend == prefs::Backend::Ollama;
        src2.set_visible(ollama);
        if !ollama {
            market.source.set(Source::HuggingFace);
            market.hf_controls.set_visible(true);
        }
        search_now(&market);
    });
    top.append(&src);
    controls.append(&top);

    let m2 = m.clone();
    let kind = widgets::segmented(&Kind::ALL.map(|k| (k.label(), k.label())), Kind::Text.label(), move |v| {
        m2.kind.set(Kind::ALL.into_iter().find(|k| k.label() == v).unwrap_or(Kind::All));
        search_now(&m2);
    });
    kind.set_halign(gtk::Align::Start);
    hf_controls.insert(&kind, -1);
    let m2 = m.clone();
    let sort = widgets::segmented(&Sort::ALL.map(|s| (s.label(), s.label())), Sort::Trending.label(), move |v| {
        m2.sort.set(Sort::ALL.into_iter().find(|s| s.label() == v).unwrap_or(Sort::Trending));
        search_now(&m2);
    });
    sort.set_halign(gtk::Align::End);
    hf_controls.insert(&sort, -1);
    let mut c = hf_controls.first_child();
    while let Some(w) = c {
        w.set_focusable(false);
        c = w.next_sibling();
    }
    controls.append(&hf_controls);

    let mem_note = widgets::label("", "dim");
    mem_note.add_css_class("tnum");
    mem_note.add_css_class("small-text");
    mem_note.set_wrap(true);
    let estimate_ctx = if crate::llama::api::local_service() { 8192 } else { pr.num_ctx };
    mem_note.set_text(&if mem.vram > 0 {
        format!(
            "GPU memory {} · RAM {} · new model estimates use {} tokens of context",
            util::size(mem.vram),
            util::size(mem.ram),
            util::count(estimate_ctx as u64)
        )
    } else {
        format!("No dedicated GPU memory found · RAM {} · models run on the CPU", util::size(mem.ram))
    });
    controls.append(&mem_note);
    page.body.append(&controls);
    page.body.append(&stack);

    let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let m2 = m.clone();
    search.connect_search_changed(move |e| {
        *m2.query.borrow_mut() = e.text().trim().to_string();
        if let Some(id) = pending.take() {
            id.remove();
        }
        let (m3, p2) = (m2.clone(), pending.clone());
        pending.set(Some(glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
            p2.set(None);
            search_now(&m3);
        })));
    });

    MARKET.with(|x| *x.borrow_mut() = Some(m.clone()));
    search_now(&m);
    // Installed state and download buttons follow the model list.
    let m2 = m.clone();
    hub::on(&hub::MODELS, &m.detail, move || {
        if m2.stack.visible_child_name().as_deref() == Some("detail")
            && let Some(f) = DETAIL_REFRESH.with(|d| d.borrow().clone())
        {
            f();
        }
    });
    page.root.upcast()
}

thread_local! {
    static DETAIL_REFRESH: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

thread_local! {
    static MARKET: RefCell<Option<Rc<Market>>> = const { RefCell::new(None) };
}

fn loading(b: &gtk::Box) {
    widgets::clear(b);
    let sp = gtk::Spinner::new();
    sp.start();
    sp.set_margin_top(30);
    b.append(&sp);
}

fn search_now(m: &Rc<Market>) {
    m.stack.set_visible_child_name("results");
    loading(&m.results);
    let this_search = m.generation.get() + 1;
    m.generation.set(this_search);
    let q = m.query.borrow().clone();
    let m2 = m.clone();
    match m.source.get() {
        Source::HuggingFace => {
            let (sort, kind) = (m.sort.get(), m.kind.get());
            rt::spawn(async move { hf::search(&q, sort, kind).await }, move |r| {
                if m2.generation.get() != this_search {
                    return;
                }
                widgets::clear(&m2.results);
                match r {
                    Ok(list) if list.is_empty() => m2.results.append(&widgets::empty("No GGUF models match.")),
                    Ok(list) => {
                        for model in list {
                            m2.results.append(&hf_card(&m2, model));
                        }
                    }
                    Err(e) => {
                        m2.results.append(&widgets::banner(&util::esc(&format!("Couldn't reach Hugging Face: {e:#}")), true))
                    }
                }
            });
        }
        Source::Ollama => {
            rt::spawn(async move { library::search(&q).await }, move |r| {
                if m2.generation.get() != this_search {
                    return;
                }
                widgets::clear(&m2.results);
                match r {
                    Ok(list) if list.is_empty() => m2.results.append(&widgets::empty("Nothing in the Ollama library matches.")),
                    Ok(list) => {
                        for e in list {
                            m2.results.append(&ollama_card(&m2, e));
                        }
                    }
                    Err(e) => m2.results.append(&widgets::banner(&util::esc(&format!("Couldn't reach ollama.com: {e:#}")), true)),
                }
            });
        }
    }
}

fn card(title: &str, desc: &str, tags: &[String]) -> (gtk::Button, gtk::Box) {
    let b = gtk::Button::new();
    b.add_css_class("market-card");
    let v = widgets::vbox(4);
    let head = widgets::hbox(8);
    let t = widgets::label(title, "settings-option-title");
    t.set_ellipsize(pango::EllipsizeMode::Middle);
    t.set_hexpand(true);
    head.append(&t);
    let right = widgets::hbox(6);
    head.append(&right);
    v.append(&head);
    if !desc.is_empty() {
        let d = gtk::Label::new(None);
        d.set_markup(desc);
        d.add_css_class("settings-option-description");
        d.set_xalign(0.0);
        d.set_wrap(true);
        d.set_lines(2);
        d.set_ellipsize(pango::EllipsizeMode::End);
        v.append(&d);
    }
    if !tags.is_empty() {
        // Wraps onto more lines rather than widening the page.
        let row = gtk::FlowBox::new();
        row.set_selection_mode(gtk::SelectionMode::None);
        row.set_column_spacing(4);
        row.set_row_spacing(4);
        row.set_max_children_per_line(8);
        row.set_can_target(false);
        for t in tags.iter().take(8) {
            row.insert(&widgets::tag(t, ""), -1);
        }
        v.append(&row);
    }
    b.set_child(Some(&v));
    (b, right)
}

fn hf_card(m: &Rc<Market>, model: hf::Model) -> gtk::Widget {
    let updated = util::parse_time(&model.last_modified).map(util::ago).unwrap_or_default();
    let mut desc = format!("<tt>↓ {}  ♥ {}</tt>", util::count(model.downloads), util::count(model.likes));
    if !updated.is_empty() {
        desc.push_str(&format!("  ·  updated {}", util::esc(&updated)));
    }
    let (b, right) = card(&model.id, &desc, &model.tags);
    if !model.pipeline.is_empty() {
        right.append(&widgets::tag(&model.pipeline.replace('-', " "), "accent"));
    }
    let m2 = m.clone();
    b.connect_clicked(move |_| hf_detail(&m2, model.clone()));
    b.upcast()
}

fn back_row(m: &Rc<Market>, title: &str) -> gtk::Box {
    let row = widgets::hbox(10);
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("flat");
    back.set_tooltip_text(Some("Back to results"));
    let m2 = m.clone();
    back.connect_clicked(move |_| {
        m2.stack.set_visible_child_name("results");
        DETAIL_REFRESH.with(|d| *d.borrow_mut() = None);
    });
    row.append(&back);
    let t = widgets::label(title, "market-detail-title");
    t.set_ellipsize(pango::EllipsizeMode::Middle);
    t.set_hexpand(true);
    row.append(&t);
    row
}

/// Download / Installed / Downloading, for one pullable name.
fn pull_button(name: &str) -> gtk::Widget {
    if models::is_installed(name) {
        return widgets::tag("Installed", "accent").upcast();
    }
    let b = gtk::Button::with_label("Download");
    b.add_css_class("suggested-action");
    let n = name.to_string();
    let update = {
        let b = b.clone();
        let n = n.clone();
        move || {
            if let Some(p) = pulls::all().into_iter().find(|p| p.name == n && p.active()) {
                b.set_label(&pulls::brief(&p));
                b.set_sensitive(false);
            } else {
                b.set_label("Download");
                b.set_sensitive(true);
            }
        }
    };
    update();
    hub::on(&hub::PULLS, &b, update);
    b.connect_clicked(move |b| {
        pulls::start(&n);
        b.set_sensitive(false);
        crate::window::toast_action(&format!("Downloading {n}"), "Show", || crate::window::navigate("models"));
    });
    b.upcast()
}

fn fit_tag(size: u64, ctx: u64, kv: &str, mem: Memory) -> gtk::Label {
    if size == 0 { return widgets::tag("Size unknown", ""); }
    let fit = hw::fit_with_kv(size, ctx, kv, mem);
    let t = widgets::tag(
        fit.label(),
        match fit {
            Fit::Gpu => "accent",
            Fit::Partial => "",
            Fit::TooBig => "danger",
        },
    );
    t.set_tooltip_text(Some(&format!(
        "About {} at {} tokens, {} KV cache",
        util::size(hw::needed_with_kv(size, ctx, kv)),
        util::count(ctx),
        kv.to_uppercase()
    )));
    t
}

fn hf_detail(m: &Rc<Market>, model: hf::Model) {
    let d = &m.detail;
    widgets::clear(d);
    d.append(&back_row(m, &model.id));
    let links = widgets::hbox(10);
    let link = gtk::LinkButton::with_label(&format!("https://huggingface.co/{}", model.id), "Open on Hugging Face");
    links.append(&link);
    d.append(&links);
    let files_box = widgets::vbox(6);
    d.append(&files_box);
    loading(&files_box);
    let readme_box = widgets::vbox(0);
    d.append(&readme_box);
    m.stack.set_visible_child_name("detail");

    let repo = model.id.clone();
    let readme_repo = model.id.clone();
    let (m2, fb) = (m.clone(), files_box.clone());
    rt::spawn(async move { hf::detail(&repo).await }, move |r| {
        let detail = match r {
            Ok(d) => d,
            Err(e) => {
                widgets::clear(&fb);
                fb.append(&widgets::banner(&util::esc(&format!("{e:#}")), true));
                return;
            }
        };
        let detail = Rc::new(detail);
        let (m3, fb2, repo2) = (m2.clone(), fb.clone(), model.id.clone());
        let draw: Rc<dyn Fn()> = Rc::new(move || draw_files(&m3, &fb2, &repo2, &detail));
        draw();
        DETAIL_REFRESH.with(|dr| *dr.borrow_mut() = Some(draw.clone()));
        let d2 = draw.clone();
        hub::on(&hub::PULLS, &fb, move || d2());
    });

    let rb = readme_box.clone();
    rt::spawn(async move { hf::readme(&readme_repo).await }, move |r| {
        if let Ok(text) = r
            && !text.trim().is_empty()
        {
            rb.append(&widgets::label("MODEL CARD", "group-title"));
            let card = widgets::vbox(0);
            card.add_css_class("readme-card");
            let md = Markdown::new();
            md.set(&util::clip(&text, 20_000));
            card.append(&md.root);
            unsafe { card.set_data("markdown", md) };
            rb.append(&card);
        }
    });
}

fn draw_files(m: &Rc<Market>, b: &gtk::Box, repo: &str, d: &hf::Detail) {
    widgets::clear(b);
    let facts = widgets::hbox(6);
    if d.params > 0 {
        facts.append(&widgets::tag(&format!("{} parameters", util::count(d.params)), ""));
    }
    if !d.architecture.is_empty() {
        facts.append(&widgets::tag(&d.architecture, ""));
    }
    if d.context_length > 0 {
        facts.append(&widgets::tag(&format!("{} context", util::count(d.context_length)), ""));
    }
    if d.tools {
        facts.append(&widgets::tag("Tools", "accent"));
    }
    if !d.license.is_empty() {
        facts.append(&widgets::tag(&d.license, ""));
    }
    if !d.base_model.is_empty() {
        facts.append(&widgets::tag(&format!("from {}", d.base_model), ""));
    }
    b.append(&facts);
    b.append(&widgets::label("DOWNLOADS", "group-title"));
    if d.files.is_empty() {
        b.append(&widgets::empty("This repository has no GGUF files."));
        return;
    }
    let mem = m.memory.get();
    let pairs: Vec<(String, u64)> = d.files.iter().map(|f| (f.quant.clone(), f.size)).collect();
    let settings = |quant: &str| {
        if crate::llama::api::local_service() { prefs::for_model(&hf_ref(repo, quant)) }
        else { prefs::ModelSettings { num_ctx: prefs::get().num_ctx, ..Default::default() } }
    };
    let best = hw::recommend_with(&pairs, |i, size| {
        let s = settings(&d.files[i].quant);
        hw::fit_with_kv(size, s.num_ctx as u64, &s.kv_cache, mem)
    });
    if let Some(f) = best.and_then(|i| d.files.get(i)) {
        let s = settings(&f.quant);
        let fit = hw::fit_with_kv(f.size, s.num_ctx as u64, &s.kv_cache, mem);
        let why = match fit {
            Fit::Gpu => "estimated to fit in GPU memory",
            _ => "estimated to fit with some CPU offload",
        };
        let row = widgets::row(
            &format!("Recommended: {}", f.quant),
            &format!("<tt>{}</tt> · {why}.", util::size(f.size)),
            Some(&pull_button(&hf_ref(repo, &f.quant))),
        );
        row.add_css_class("quant-row");
        row.add_css_class("recommended");
        b.append(&row);
    } else if d.files.iter().all(|f| f.size > 0) {
        b.append(&widgets::banner("These files may exceed available memory at the estimated context size. You can still download them.", true));
    }
    for (i, f) in d.files.iter().enumerate() {
        let name = hf_ref(repo, &f.quant);
        let row = widgets::hbox(10);
        row.add_css_class("settings-option");
        row.add_css_class("quant-row");
        let q = widgets::label(&f.quant, "quant-name");
        q.add_css_class("mono");
        q.set_width_chars(10);
        row.append(&q);
        let s = widgets::label(&util::size(f.size), "tnum");
        s.add_css_class("dim");
        s.set_width_chars(9);
        row.append(&s);
        let tags = widgets::hbox(6);
        tags.set_hexpand(true);
        let s = settings(&f.quant);
        tags.append(&fit_tag(f.size, s.num_ctx as u64, &s.kv_cache, mem));
        if Some(i) == best {
            tags.append(&widgets::tag("Recommended", "accent"));
            row.add_css_class("recommended");
        }
        row.append(&tags);
        let pb = pull_button(&name);
        pb.set_valign(gtk::Align::Center);
        row.append(&pb);
        row.set_tooltip_text(Some(&name));
        b.append(&row);
    }
    let note = widgets::label("Q4_K_M is a good balance of size and quality; Q8_0 is close to the original.", "dim");
    note.add_css_class("small-text");
    b.append(&note);
}

fn hf_ref(repo: &str, quant: &str) -> String {
    if prefs::get().host().backend == prefs::Backend::LlamaCpp {
        format!("{repo}:{quant}")
    } else {
        hf::ollama_ref(repo, quant)
    }
}

fn ollama_card(m: &Rc<Market>, e: library::Entry) -> gtk::Widget {
    let mut desc = util::esc(&e.description);
    if !e.pulls.is_empty() {
        desc.push_str(&format!("\n<tt>↓ {}</tt>  ·  updated {}", util::esc(&e.pulls), util::esc(&e.updated)));
    }
    let mut tags = e.capabilities.clone();
    tags.extend(e.sizes.iter().cloned());
    let (b, _) = card(&e.name, &desc, &tags);
    let m2 = m.clone();
    b.connect_clicked(move |_| ollama_detail(&m2, e.clone()));
    b.upcast()
}

fn ollama_detail(m: &Rc<Market>, e: library::Entry) {
    let d = &m.detail;
    widgets::clear(d);
    d.append(&back_row(m, &e.name));
    let desc = widgets::label(&e.description, "dim");
    desc.set_wrap(true);
    d.append(&desc);
    let link = gtk::LinkButton::with_label(&format!("https://ollama.com/library/{}", e.name), "Open on ollama.com");
    link.set_halign(gtk::Align::Start);
    d.append(&link);
    let list = widgets::vbox(6);
    d.append(&list);
    loading(&list);
    m.stack.set_visible_child_name("detail");
    let name = e.name.clone();
    let (m2, l2) = (m.clone(), list.clone());
    rt::spawn(async move { library::tags(&name).await }, move |r| {
        let tags = match r {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => {
                // Fall back to the sizes from the search result.
                e.sizes.iter().map(|s| library::Tag { name: s.clone(), size: String::new(), context: String::new() }).collect()
            }
            Err(err) => {
                widgets::clear(&l2);
                l2.append(&widgets::banner(&util::esc(&format!("{err:#}")), true));
                return;
            }
        };
        let tags = Rc::new(tags);
        let (m3, l3, n3) = (m2.clone(), l2.clone(), e.name.clone());
        let draw: Rc<dyn Fn()> = Rc::new(move || draw_tags(&m3, &l3, &n3, &tags));
        draw();
        DETAIL_REFRESH.with(|dr| *dr.borrow_mut() = Some(draw.clone()));
        let d2 = draw.clone();
        hub::on(&hub::PULLS, &l2, move || d2());
    });
}

fn parse_size(s: &str) -> u64 {
    let t = s.trim();
    let (num, mult) = if let Some(n) = t.strip_suffix("GB") {
        (n, 1e9)
    } else if let Some(n) = t.strip_suffix("MB") {
        (n, 1e6)
    } else if let Some(n) = t.strip_suffix("TB") {
        (n, 1e12)
    } else {
        return 0;
    };
    (num.trim().parse::<f64>().unwrap_or(0.0) * mult) as u64
}

fn draw_tags(m: &Rc<Market>, b: &gtk::Box, model: &str, tags: &[library::Tag]) {
    widgets::clear(b);
    b.append(&widgets::label("VERSIONS", "group-title"));
    let mem = m.memory.get();
    let ctx = prefs::get().num_ctx as u64;
    // The plain sizes (8b, 70b) first; quantisation variants after.
    let mut sorted: Vec<&library::Tag> = tags.iter().collect();
    sorted.sort_by_key(|t| (t.name.contains('-'), parse_size(&t.size)));
    let pairs: Vec<(String, u64)> = sorted
        .iter()
        .map(|t| (if t.name.contains('-') { t.name.to_uppercase() } else { "Q4_K_M".into() }, parse_size(&t.size)))
        .collect();
    let best = hw::recommend(&pairs, ctx, mem);
    for (i, t) in sorted.iter().enumerate() {
        let name = format!("{model}:{}", t.name);
        let row = widgets::hbox(10);
        row.add_css_class("settings-option");
        row.add_css_class("quant-row");
        let q = widgets::label(&t.name, "quant-name");
        q.add_css_class("mono");
        q.set_width_chars(16);
        q.set_ellipsize(pango::EllipsizeMode::End);
        row.append(&q);
        let s = widgets::label(&t.size, "tnum");
        s.add_css_class("dim");
        s.set_width_chars(8);
        row.append(&s);
        let tagbox = widgets::hbox(6);
        tagbox.set_hexpand(true);
        let bytes = parse_size(&t.size);
        if bytes > 0 {
            tagbox.append(&fit_tag(bytes, ctx, "f16", mem));
        }
        if !t.context.is_empty() {
            tagbox.append(&widgets::tag(&format!("{} context", t.context), ""));
        }
        if Some(i) == best {
            tagbox.append(&widgets::tag("Recommended", "accent"));
            row.add_css_class("recommended");
        }
        row.append(&tagbox);
        let pb = pull_button(&name);
        pb.set_valign(gtk::Align::Center);
        row.append(&pb);
        b.append(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_sizes() {
        assert_eq!(parse_size("5.2GB"), 5_200_000_000);
        assert_eq!(parse_size("523MB"), 523_000_000);
        assert_eq!(parse_size("?"), 0);
    }
}
