//! What's installed on the active server, and what each model can do. Pages
//! read this cache and subscribe to `hub::MODELS` for changes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ollama::api::{self, Conn, Installed, ModelInfo, Running};
use crate::{hub, prefs, rt};

#[derive(Default)]
struct State {
    installed: Vec<Installed>,
    running: Vec<Running>,
    info: HashMap<String, ModelInfo>,
    /// None until the first check finishes.
    online: Option<bool>,
    error: String,
    updates: HashSet<String>,
    refreshing: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn installed() -> Vec<Installed> {
    STATE.with(|s| s.borrow().installed.clone())
}

/// Installed models that can chat (not embedding-only).
pub fn chat_models() -> Vec<String> {
    STATE.with(|s| {
        let s = s.borrow();
        s.installed
            .iter()
            .filter(|m| s.info.get(&m.name).is_none_or(|i| !i.has("embedding") || i.has("completion")))
            .map(|m| m.name.clone())
            .collect()
    })
}

pub fn embedding_models() -> Vec<String> {
    STATE.with(|s| {
        let s = s.borrow();
        s.installed.iter().filter(|m| s.info.get(&m.name).is_some_and(|i| i.has("embedding"))).map(|m| m.name.clone()).collect()
    })
}

pub fn running() -> Vec<Running> {
    STATE.with(|s| s.borrow().running.clone())
}

pub fn info(name: &str) -> Option<ModelInfo> {
    STATE.with(|s| s.borrow().info.get(name).cloned())
}

pub fn has(name: &str, cap: &str) -> bool {
    info(name).is_some_and(|i| i.has(cap))
}

pub fn online() -> Option<bool> {
    STATE.with(|s| s.borrow().online)
}

pub fn error() -> String {
    STATE.with(|s| s.borrow().error.clone())
}

pub fn is_installed(name: &str) -> bool {
    let want = with_tag(name);
    STATE.with(|s| s.borrow().installed.iter().any(|m| m.name == want || m.name.eq_ignore_ascii_case(&want)))
}

/// `llama3` → `llama3:latest`, the way Ollama lists it.
pub fn with_tag(name: &str) -> String {
    let last = name.rsplit('/').next().unwrap_or(name);
    if last.contains(':') { name.to_string() } else { format!("{name}:latest") }
}

pub fn has_update(name: &str) -> bool {
    STATE.with(|s| s.borrow().updates.contains(name))
}

pub fn updates() -> Vec<String> {
    STATE.with(|s| s.borrow().updates.iter().cloned().collect())
}

/// The model a new chat starts with: the saved default if it's installed, else the first.
pub fn default_model() -> String {
    let want = prefs::get().default_model;
    let chat = chat_models();
    if !want.is_empty() && chat.contains(&want) {
        return want;
    }
    chat.first().cloned().unwrap_or(want)
}

/// Re-read the model list and each model's capabilities.
pub fn refresh() {
    if STATE.with(|s| std::mem::replace(&mut s.borrow_mut().refreshing, true)) {
        return;
    }
    let conn = Conn::current();
    rt::spawn(
        async move {
            let tags = api::tags(&conn).await;
            let running = api::running(&conn).await.unwrap_or_default();
            let mut infos = Vec::new();
            if let Ok(list) = &tags {
                let futs = list.iter().map(|m| {
                    let conn = conn.clone();
                    let name = m.name.clone();
                    async move { (name.clone(), api::show(&conn, &name).await) }
                });
                infos = futures_util::future::join_all(futs).await;
            }
            (tags, running, infos)
        },
        |(tags, running, infos)| {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.refreshing = false;
                match tags {
                    Ok(list) => {
                        s.online = Some(true);
                        s.error.clear();
                        s.installed = list;
                    }
                    Err(e) => {
                        s.online = Some(false);
                        s.error = api::short_error(&format!("{e:#}"));
                        s.installed.clear();
                    }
                }
                s.running = running;
                for (name, info) in infos {
                    if let Ok(i) = info {
                        s.info.insert(name, i);
                    }
                }
                let names: HashSet<String> = s.installed.iter().map(|m| m.name.clone()).collect();
                s.updates.retain(|n| names.contains(n));
            });
            hub::emit(&hub::MODELS);
        },
    );
}

/// Just the running list (the Models page polls this).
pub fn refresh_running() {
    let conn = Conn::current();
    rt::spawn(async move { api::running(&conn).await }, |r| {
        if let Ok(list) = r {
            let changed = STATE.with(|s| {
                let mut s = s.borrow_mut();
                let changed = s
                    .running
                    .iter()
                    .map(|m| (&m.name, m.expires.as_str()))
                    .ne(list.iter().map(|m| (&m.name, m.expires.as_str())));
                s.running = list;
                changed
            });
            if changed {
                hub::emit(&hub::MODELS);
            }
        }
    });
}

/// Compare every installed model with its registry. `done` gets how many can update.
pub fn check_updates(done: impl FnOnce(usize) + 'static) {
    let list: Vec<(String, String)> = installed().into_iter().map(|m| (m.name, m.digest)).collect();
    rt::spawn(
        async move {
            let futs = list.into_iter().map(|(name, digest)| async move {
                let r = api::update_available(&name, &digest).await;
                (name, r)
            });
            futures_util::future::join_all(futs).await
        },
        move |results| {
            let found: HashSet<String> = results.into_iter().filter_map(|(n, r)| r.ok().filter(|b| *b).map(|_| n)).collect();
            let n = found.len();
            STATE.with(|s| s.borrow_mut().updates = found);
            prefs::update(|p| p.update_check_day = chrono::Local::now().format("%Y-%m-%d").to_string());
            hub::emit(&hub::MODELS);
            done(n);
        },
    );
}

/// Once a day, quietly.
pub fn daily_update_check() {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if prefs::get().update_check_day != today && online() == Some(true) && !installed().is_empty() {
        check_updates(|n| {
            if n > 0 {
                crate::window::toast(&format!("{n} model update{} available", if n == 1 { "" } else { "s" }));
            }
        });
    }
}

pub fn clear_update(name: &str) {
    STATE.with(|s| s.borrow_mut().updates.remove(name));
}
