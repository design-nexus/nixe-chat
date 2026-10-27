//! Chat's tools on the control interface (`nyx_core::control`), for Nyx Assistant: find and
//! read conversations, open one, and send a message (the model's answer is read back with
//! `chat.read` once it has finished). Nothing deletes or archives.

use crate::sections::chat;
use crate::store;
use nyx_core::control::text;
use serde_json::{json, Value};

pub fn tools() -> Value {
    json!([
        {"name": "chat.list", "description": "The most recent conversations in Chat: id, title, model.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "chat.search", "description": "Find Chat conversations whose title or messages match the words.", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}},
        {"name": "chat.read", "description": "Read a Chat conversation (by id; the open one if none): its last messages.", "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}}},
        {"name": "chat.open", "description": "Show a conversation in the Chat window.", "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}},
        {"name": "chat.send", "description": "Send a message in Chat, in a new conversation or the open one; read the answer later with chat.read.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}, "new": {"type": "boolean"}}, "required": ["text"]}},
    ])
}

fn summary(c: &store::Chat) -> Value {
    json!({"id": c.id, "title": c.title, "model": c.model, "updated": c.updated})
}

fn failed(e: anyhow::Error) -> String {
    format!("{e:#}")
}

pub fn handle(name: &str, args: Value) -> Result<Value, String> {
    match name {
        "chat.list" => {
            let chats = store::chats(false).map_err(failed)?;
            Ok(json!({"chats": chats.iter().filter(|c| !c.temporary).take(20).map(summary).collect::<Vec<_>>()}))
        }
        "chat.search" => {
            let hits = store::search(&text(&args, "query")?).map_err(failed)?;
            Ok(json!({"chats": hits.iter().take(20).map(|h| json!({"id": h.chat, "title": h.title, "snippet": h.snippet})).collect::<Vec<_>>(), "untrusted_content": true}))
        }
        "chat.read" => {
            let id = args["id"].as_i64().or_else(chat::current_id).ok_or("no conversation is open; give an id")?;
            let c = store::chat(id).map_err(failed)?.ok_or("no conversation with that id")?;
            let all = store::messages(id).map_err(failed)?;
            let shown = store::path_to(&all, c.leaf);
            let said: Vec<&store::Message> = shown.iter().filter(|m| m.role == "user" || m.role == "assistant").collect();
            // The last 30.
            let messages: Vec<Value> = said[said.len().saturating_sub(30)..]
                .iter()
                .map(|m| json!({"role": m.role, "model": m.model, "text": m.content}))
                .collect();
            Ok(json!({"chat": summary(&c), "answering": chat::streaming(Some(id)), "messages": messages, "untrusted_content": true}))
        }
        "chat.open" => {
            let id = args["id"].as_i64().ok_or("id is required")?;
            store::chat(id).map_err(failed)?.ok_or("no conversation with that id")?;
            chat::open(id);
            Ok(json!({"opened": id}))
        }
        "chat.send" => {
            let message = text(&args, "text")?;
            if message.trim().is_empty() || message.len() > 50_000 {
                return Err("text must be 1 to 50000 bytes".into());
            }
            if args["new"] == true || chat::current_id().is_none() {
                chat::new_chat(None);
            }
            chat::send_text(&message);
            Ok(json!({"sent": true, "chat": chat::current_id()}))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}
