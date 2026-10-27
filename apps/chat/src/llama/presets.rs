//! Model-specific llama.cpp router presets. Preserve user-defined sections and
//! keys while replacing only the settings managed by Chat.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::prefs::{self, ModelSettings};
use crate::{hub, rt};

thread_local! {
    static PENDING: RefCell<BTreeMap<String, u64>> = RefCell::new(BTreeMap::new());
    static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static CURRENT_CONTEXT: RefCell<BTreeMap<String, i64>> = RefCell::new(BTreeMap::new());
    static APPLYING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static WATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn path() -> Result<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("no configuration directory")?;
    Ok(config.join("nixe-chat/llama-models.ini"))
}

fn managed_key(line: &str) -> bool {
    let key = line.split_once('=').map(|(key, _)| key.trim()).unwrap_or("");
    matches!(key, "ctx-size" | "c" | "n-gpu-layers" | "gpu-layers" | "ngl" | "flash-attn" |
        "cache-type-k" | "cache-type-v" | "temp" | "temperature" | "top-p" | "top-k" | "min-p" | "repeat-penalty")
}

fn values(s: &ModelSettings) -> Vec<String> {
    let flash = if s.kv_cache == "f16" { s.flash_attention.as_str() } else { "on" };
    vec![
        format!("ctx-size = {}", s.num_ctx), format!("n-gpu-layers = {}", s.gpu_layers),
        format!("flash-attn = {flash}"), format!("cache-type-k = {}", s.kv_cache),
        format!("cache-type-v = {}", s.kv_cache), format!("temp = {}", s.temperature),
        format!("top-p = {}", s.top_p), format!("top-k = {}", s.top_k),
        format!("min-p = {}", s.min_p), format!("repeat-penalty = {}", s.repeat_penalty),
    ]
}

fn render(existing: &str, models: &BTreeMap<String, ModelSettings>) -> String {
    let mut sections: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    for line in existing.lines() {
        let name = line.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']'));
        if let Some(name) = name {
            sections.push((name.to_string(), vec![line.to_string()]));
        } else {
            sections.last_mut().unwrap().1.push(line.to_string());
        }
    }
    if !sections[0].1.iter().any(|line| line.trim_start().starts_with("version =")) {
        sections[0].1.insert(0, "version = 1".into());
    }
    let mut configured = HashSet::new();
    for (name, lines) in &mut sections {
        let setting = if name == "*" { Some(ModelSettings::default()) } else { models.get(name).cloned() };
        if let Some(setting) = setting {
            lines.retain(|line| !managed_key(line));
            lines.extend(values(&setting));
            configured.insert(name.clone());
        }
    }
    if !configured.contains("*") {
        let mut lines = vec!["[*]".into()];
        lines.extend(values(&ModelSettings::default()));
        sections.push(("*".into(), lines));
    }
    for (name, setting) in models {
        if name.contains(['\n', '\r', '[', ']']) || configured.contains(name) { continue; }
        let mut lines = vec![format!("[{name}]")];
        lines.extend(values(setting));
        sections.push((name.clone(), lines));
    }
    let mut result = sections.into_iter().flat_map(|(_, lines)| lines).collect::<Vec<_>>().join("\n");
    result.push('\n');
    result
}

pub fn sync() -> Result<()> {
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let content = render(&existing, &prefs::get().model_settings);
    if content != existing {
        let temp = path.with_extension("ini.tmp");
        std::fs::write(&temp, content)?;
        std::fs::rename(temp, path)?;
    }
    Ok(())
}

pub fn pending(name: &str) -> bool { PENDING.with(|p| p.borrow().contains_key(name)) }
pub fn runtime_context(name: &str) -> Option<i64> { CURRENT_CONTEXT.with(|c| c.borrow().get(name).copied()) }

pub fn mark_with_context(name: String, current_context: i64) {
    CURRENT_CONTEXT.with(|c| { c.borrow_mut().entry(name.clone()).or_insert(current_context); });
    mark([name]);
}

pub fn mark(names: impl IntoIterator<Item = String>) {
    PENDING.with(|p| for name in names {
        let version = NEXT.with(|next| { let v = next.get(); next.set(v + 1); v });
        p.borrow_mut().insert(name, version);
    });
    apply_pending();
    if !WATCHING.with(|w| w.replace(true)) {
        gtk::glib::timeout_add_seconds_local(5, || {
            if PENDING.with(|p| p.borrow().is_empty()) {
                WATCHING.with(|w| w.set(false));
                gtk::glib::ControlFlow::Break
            } else {
                apply_pending();
                gtk::glib::ControlFlow::Continue
            }
        });
    }
}

pub fn apply_pending() {
    if !crate::llama::api::local_service() || crate::pulls::active_count() > 0 { return; }
    if APPLYING.with(|a| a.replace(true)) { return; }
    let versioned: Vec<(String, u64)> = PENDING.with(|p| p.borrow().iter().map(|(n, v)| (n.clone(), *v)).collect());
    if versioned.is_empty() { APPLYING.with(|a| a.set(false)); return; }
    let names: Vec<String> = versioned.iter().map(|(n, _)| n.clone()).collect();
    let conn = crate::ollama::api::Conn::current();
    let requested = names.clone();
    rt::spawn(async move { crate::llama::api::reload_if_idle(&conn, &requested).await }, move |result| {
        APPLYING.with(|a| a.set(false));
        let applied = matches!(result, Ok(true));
        if applied {
            PENDING.with(|p| {
                for (name, version) in &versioned {
                    let unchanged = p.borrow().get(name) == Some(version);
                    if unchanged {
                        p.borrow_mut().remove(name);
                        CURRENT_CONTEXT.with(|c| { c.borrow_mut().remove(name); });
                    }
                }
            });
            hub::emit(&hub::MODELS);
            crate::models::refresh_running();
        }
        if applied && PENDING.with(|p| !p.borrow().is_empty()) { apply_pending(); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_custom_preset_keys() {
        let mut models = BTreeMap::new();
        models.insert("org/model:Q4_K_M".into(), ModelSettings { num_ctx: 65536, ..Default::default() });
        let text = render("version = 1\n[org/model:Q4_K_M]\nembeddings = true\nctx-size = 8192\n", &models);
        assert!(text.contains("embeddings = true"));
        assert!(text.contains("ctx-size = 65536"));
        assert_eq!(render(&text, &models), text);
    }
}
