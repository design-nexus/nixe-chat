//! Knowledge: collections of documents that chats can answer from.

use gtk::prelude::*;

use crate::ollama::api::Conn;
use crate::{hub, models, prefs, pulls, rag, rt, store, tools, util, widgets, window};

pub fn build() -> gtk::Widget {
    let page = widgets::page(
        "Knowledge",
        "Collections of your documents. Attach one to a chat, or type # in the message box, and answers draw on it.",
        &[],
    );
    let banner = widgets::vbox(0);
    page.body.append(&banner);

    let g = page.group("New collection");
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Collection name"));
    entry.set_hexpand(true);
    let create = gtk::Button::with_label("Create");
    create.add_css_class("suggested-action");
    create.set_valign(gtk::Align::Center);
    entry.set_valign(gtk::Align::Center);
    let row = widgets::hbox(8);
    row.add_css_class("settings-option");
    row.append(&entry);
    row.append(&create);
    g.add(&row);
    let add = {
        let entry = entry.clone();
        move || {
            let name = entry.text().trim().to_string();
            if name.is_empty() {
                return;
            }
            let model = embed_model();
            if model.is_empty() {
                window::toast("Download an embedding model first");
                return;
            }
            match store::add_kbase(&name, &model) {
                Ok(_) => {
                    entry.set_text("");
                    hub::emit(&hub::LIBRARY);
                }
                Err(e) => window::toast(&format!("{e}")),
            }
        }
    };
    let a2 = add.clone();
    create.connect_clicked(move |_| a2());
    entry.connect_activate(move |_| add());

    let list = widgets::vbox(0);
    page.body.append(&list);

    let refresh = {
        let (banner, list) = (banner.clone(), list.clone());
        move || {
            widgets::clear(&banner);
            if models::online() == Some(true) && models::embedding_models().is_empty() {
                let b = widgets::banner(
                    "<b>Knowledge needs an embedding model.</b> It turns text into numbers that can be searched by meaning.",
                    false,
                );
                let name = prefs::get().embed_model;
                let dl = gtk::Button::with_label(&format!("Download {name}"));
                dl.set_valign(gtk::Align::Center);
                dl.connect_clicked(move |b| {
                    pulls::start(&name);
                    b.set_sensitive(false);
                });
                b.append(&dl);
                banner.append(&b);
            }
            fill(&list);
        }
    };
    refresh();
    let r2 = refresh.clone();
    hub::on(&hub::LIBRARY, &list, r2);
    hub::on(&hub::MODELS, &banner, refresh);
    page.root.upcast()
}

/// The embedding model to use for new collections.
fn embed_model() -> String {
    let want = prefs::get().embed_model;
    let have = models::embedding_models();
    if have.iter().any(|m| *m == want || *m == models::with_tag(&want)) {
        return models::with_tag(&want);
    }
    have.first().cloned().unwrap_or_default()
}

fn fill(list: &gtk::Box) {
    widgets::clear(list);
    let kbs = store::kbases().unwrap_or_default();
    if kbs.is_empty() {
        list.append(&widgets::empty("No collections yet."));
        return;
    }
    for kb in kbs {
        let wrapper = widgets::vbox(6);
        wrapper.add_css_class("settings-group");
        let head = widgets::hbox(8);
        let t = widgets::short_label(&kb.name.to_uppercase(), "group-title", gtk::pango::EllipsizeMode::End);
        t.set_hexpand(true);
        head.append(&t);
        let m = widgets::short_label(&kb.model, "dim", gtk::pango::EllipsizeMode::Middle);
        m.set_max_width_chars(30);
        m.add_css_class("mono");
        m.add_css_class("small-text");
        m.set_tooltip_text(Some("Embedding model"));
        head.append(&m);
        wrapper.append(&head);
        for (id, name, chars) in &kb.docs {
            let id = *id;
            let del = widgets::two_click("Remove", "Click again", move || {
                let _ = store::delete_kdoc(id);
                hub::emit(&hub::LIBRARY);
            });
            let row = widgets::row(name, &format!("<tt>{}</tt> characters", util::count(*chars as u64)), Some(del.upcast_ref()));
            wrapper.append(&row);
        }
        if kb.docs.is_empty() {
            let e = widgets::label("No documents yet.", "dim");
            e.set_margin_start(2);
            wrapper.append(&e);
        }
        let actions = widgets::hbox(8);
        let addb = gtk::Button::with_label("Add documents…");
        let (kid, kmodel) = (kb.id, kb.model.clone());
        addb.connect_clicked(move |_| {
            let m = kmodel.clone();
            widgets::open_files(
                "Add documents",
                &[("Text and PDF", &["*.txt", "*.md", "*.pdf", "*.rst", "*.html", "*.csv", "*.json", "*.*"])],
                move |files| add_documents(kid, &m, files),
            );
        });
        actions.append(&addb);
        let kid = kb.id;
        let del = widgets::two_click("Delete collection", "Click again to delete it and its documents", move || {
            let _ = store::delete_kbase(kid);
            hub::emit(&hub::LIBRARY);
        });
        actions.append(&del);
        wrapper.append(&actions);
        list.append(&wrapper);
    }
}

fn add_documents(kb: i64, model: &str, files: Vec<std::path::PathBuf>) {
    let conn = Conn::current();
    let model = model.to_string();
    let size = prefs::get().chunk_size;
    let n = files.len();
    window::toast(&format!("Reading {n} document{}…", if n == 1 { "" } else { "s" }));
    rt::spawn(
        async move {
            let mut done = 0;
            let mut errors = Vec::new();
            for f in files {
                let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let text = match tokio::task::spawn_blocking(move || tools::files::extract_text(&f)).await {
                    Ok(Ok(t)) => t,
                    Ok(Err(e)) => {
                        errors.push(format!("{name}: {e}"));
                        continue;
                    }
                    Err(e) => {
                        errors.push(format!("{name}: {e}"));
                        continue;
                    }
                };
                let chunks = rag::chunk(&text, size);
                if chunks.is_empty() {
                    errors.push(format!("{name}: no text"));
                    continue;
                }
                match rag::embed_all(&conn, &model, &chunks).await {
                    Ok(vecs) => {
                        let pairs: Vec<(String, Vec<f32>)> = chunks.into_iter().zip(vecs).collect();
                        match store::add_kdoc(kb, &name, &pairs) {
                            Ok(()) => done += 1,
                            Err(e) => errors.push(format!("{name}: {e}")),
                        }
                    }
                    Err(e) => errors.push(format!("{name}: {e:#}")),
                }
            }
            (done, errors)
        },
        |(done, errors): (usize, Vec<String>)| {
            hub::emit(&hub::LIBRARY);
            if errors.is_empty() {
                window::toast(&format!("Added {done} document{}", if done == 1 { "" } else { "s" }));
            } else {
                window::toast(&format!("Added {done}. Problems: {}", util::clip(&errors.join("; "), 200)));
            }
        },
    );
}
