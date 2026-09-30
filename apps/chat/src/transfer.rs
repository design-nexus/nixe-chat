//! Export chats as Markdown or JSON, and import them back (also from Open WebUI).

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::store::{self, Message, NewMessage};

pub fn markdown(chat_id: i64) -> Result<String> {
    let chat = store::chat(chat_id)?.context("no such chat")?;
    let all = store::messages(chat_id)?;
    let path = store::path_to(&all, chat.leaf);
    let mut out = format!("# {}\n\n", chat.title);
    for m in path {
        match m.role.as_str() {
            "user" => out.push_str(&format!("**You**\n\n{}\n\n", m.content.trim())),
            "assistant" if !m.content.trim().is_empty() => {
                let who = if m.model.is_empty() { "Assistant".to_string() } else { m.model.clone() };
                out.push_str(&format!("**{who}**\n\n{}\n\n", m.content.trim()));
                let sources = m.sources();
                if !sources.is_empty() {
                    for (i, s) in sources.iter().enumerate() {
                        out.push_str(&format!("[{}]: {} ({})\n", i + 1, s.title, s.url));
                    }
                    out.push('\n');
                }
            }
            "tool" => out.push_str(&format!(
                "> {}: {}\n\n",
                crate::tools::title_for(&m.tool_name),
                crate::util::clip(m.content.trim(), 300).replace('\n', " ")
            )),
            _ => {}
        }
    }
    Ok(out)
}

fn message_json(m: &Message) -> Value {
    json!({
        "id": m.id, "parent": m.parent, "role": m.role, "content": m.content, "thinking": m.thinking,
        "model": m.model, "tool_calls": m.tool_calls, "tool_name": m.tool_name, "extra": m.extra,
        "tokens": m.tokens, "tps": m.tps, "created": m.created,
    })
}

pub fn chat_json(chat_id: i64) -> Result<Value> {
    let c = store::chat(chat_id)?.context("no such chat")?;
    let folder = c.folder.and_then(store::folder).map(|f| f.name);
    Ok(json!({
        "title": c.title, "model": c.model, "system": c.system, "tags": c.tags, "pinned": c.pinned,
        "folder": folder, "created": c.created, "updated": c.updated, "leaf": c.leaf,
        "params": c.params, "schema": c.schema,
        "messages": store::messages(chat_id)?.iter().map(message_json).collect::<Vec<_>>(),
    }))
}

pub fn export_one(chat_id: i64) -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({ "app": "nexus-chat", "version": 1, "chats": [chat_json(chat_id)?] }))?)
}

pub fn export_all() -> Result<String> {
    let mut chats = Vec::new();
    for archived in [false, true] {
        for c in store::chats(archived)? {
            let mut v = chat_json(c.id)?;
            v["archived"] = json!(archived);
            chats.push(v);
        }
    }
    Ok(serde_json::to_string_pretty(&json!({ "app": "nexus-chat", "version": 1, "chats": chats }))?)
}

/// A chat in a neutral shape, ready to be written.
struct Imported {
    title: String,
    model: String,
    system: String,
    tags: Vec<String>,
    archived: bool,
    pinned: bool,
    /// (old id, old parent, role, content, thinking, model)
    messages: Vec<(String, Option<String>, String, String, String, String)>,
    leaf: Option<String>,
}

/// Import a file. Returns how many chats were added.
pub fn import(text: &str) -> Result<usize> {
    let v: Value = serde_json::from_str(text).context("that file isn't JSON")?;
    let chats = if v["app"] == "nexus-chat" {
        parse_ours(&v)
    } else if let Some(list) = v.as_array() {
        list.iter().filter_map(parse_open_webui).collect()
    } else if v.get("chat").is_some() {
        parse_open_webui(&v).into_iter().collect()
    } else {
        bail!("this doesn't look like a chat export");
    };
    let n = chats.len();
    for c in chats {
        write(c)?;
    }
    Ok(n)
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn parse_ours(v: &Value) -> Vec<Imported> {
    v["chats"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Imported {
            title: s(&c["title"]),
            model: s(&c["model"]),
            system: s(&c["system"]),
            tags: c["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
            archived: c["archived"].as_bool().unwrap_or(false),
            pinned: c["pinned"].as_bool().unwrap_or(false),
            messages: c["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some((
                        id_of(&m["id"])?,
                        id_of(&m["parent"]),
                        s(&m["role"]),
                        s(&m["content"]),
                        s(&m["thinking"]),
                        s(&m["model"]),
                    ))
                })
                .collect(),
            leaf: id_of(&c["leaf"]),
        })
        .collect()
}

fn parse_open_webui(item: &Value) -> Option<Imported> {
    let chat = &item["chat"];
    let title = if s(&item["title"]).is_empty() { s(&chat["title"]) } else { s(&item["title"]) };
    let mut messages = Vec::new();
    if let Some(map) = chat["history"]["messages"].as_object() {
        for (key, m) in map {
            messages.push((
                id_of(&m["id"]).unwrap_or_else(|| key.clone()),
                id_of(&m["parentId"]),
                s(&m["role"]),
                s(&m["content"]),
                String::new(),
                s(&m["model"]),
            ));
        }
        // Parents must be written before children; timestamps order them.
        messages.sort_by_key(|m| map.get(&m.0).and_then(|x| x["timestamp"].as_i64()).unwrap_or(0));
    } else if let Some(list) = chat["messages"].as_array() {
        let mut prev: Option<String> = None;
        for (i, m) in list.iter().enumerate() {
            let id = id_of(&m["id"]).unwrap_or_else(|| i.to_string());
            messages.push((id.clone(), prev.clone(), s(&m["role"]), s(&m["content"]), String::new(), s(&m["model"])));
            prev = Some(id);
        }
    }
    if messages.is_empty() {
        return None;
    }
    let model = chat["models"].as_array().and_then(|m| m.first()).map(s).unwrap_or_default();
    Some(Imported {
        title: if title.is_empty() { "Imported chat".into() } else { title },
        model,
        system: s(&chat["system"]),
        tags: chat["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
        archived: item["archived"].as_bool().unwrap_or(false),
        pinned: item["pinned"].as_bool().unwrap_or(false),
        leaf: id_of(&chat["history"]["currentId"]),
        messages,
    })
}

fn write(c: Imported) -> Result<()> {
    let host = crate::prefs::get().host().name;
    let id = store::create_chat(&c.model, None, &host)?;
    let mut map: HashMap<String, i64> = HashMap::new();
    let mut last = 0;
    // Write in an order where every parent comes first.
    let mut pending = c.messages;
    let mut guard = 0;
    while !pending.is_empty() && guard < 10_000 {
        guard += 1;
        let before = pending.len();
        pending.retain(|(old, parent, role, content, thinking, model)| {
            let parent_new = match parent {
                Some(p) => match map.get(p) {
                    Some(n) => Some(*n),
                    None => return true,
                },
                None => None,
            };
            let Ok(new) = store::add_message(NewMessage {
                chat: id,
                parent: parent_new,
                role,
                content,
                model,
                images: &[],
                files: &[],
                extra: Value::Null,
            }) else {
                return false;
            };
            if !thinking.is_empty()
                && let Ok(Some(mut m)) = store::message(new)
            {
                m.thinking = thinking.clone();
                let _ = store::finish_message(&m);
            }
            map.insert(old.clone(), new);
            last = new;
            false
        });
        if pending.len() == before {
            // Orphans: attach them at the root.
            for m in pending.iter_mut() {
                m.1 = None;
            }
        }
    }
    let mut chat = store::chat(id)?.context("chat vanished")?;
    chat.title = c.title;
    chat.system = c.system;
    chat.tags = c.tags;
    chat.archived = c.archived;
    chat.pinned = c.pinned;
    chat.leaf = c.leaf.and_then(|l| map.get(&l).copied()).unwrap_or(last);
    store::save_chat(&chat)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_our_format_and_reads_open_webui() {
        store::tests::scratch();
        let c = store::create_chat("m", None, "local").unwrap();
        let u = store::add_message(NewMessage {
            chat: c,
            parent: None,
            role: "user",
            content: "hi",
            model: "",
            images: &[],
            files: &[],
            extra: Value::Null,
        })
        .unwrap();
        store::add_message(NewMessage {
            chat: c,
            parent: Some(u),
            role: "assistant",
            content: "hello",
            model: "m",
            images: &[],
            files: &[],
            extra: Value::Null,
        })
        .unwrap();
        let text = export_one(c).unwrap();
        assert_eq!(import(&text).unwrap(), 1);
        let chats = store::chats(false).unwrap();
        assert_eq!(chats.len(), 2);
        let md = markdown(chats[0].id).unwrap();
        assert!(md.contains("**You**\n\nhi"));
        assert!(md.contains("hello"));

        let owui = r#"[{"title":"From OWUI","chat":{"models":["llama3"],"history":{"currentId":"b","messages":{
            "a":{"id":"a","parentId":null,"role":"user","content":"q","timestamp":1},
            "b":{"id":"b","parentId":"a","role":"assistant","content":"r","model":"llama3","timestamp":2}}}}}]"#;
        assert_eq!(import(owui).unwrap(), 1);
        let imported = store::chats(false).unwrap().into_iter().find(|c| c.title == "From OWUI").unwrap();
        let path = store::path_to(&store::messages(imported.id).unwrap(), imported.leaf);
        assert_eq!(path.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["q", "r"]);
    }
}
