//! Tools the model can call. Each has a JSON-schema definition for Ollama and
//! an async implementation. Risky ones ask the user first.

pub mod calc;
pub mod files;
pub mod location;
pub mod mcp;
pub mod web;

use serde_json::{Value, json};

use crate::ollama::chat::{Event, ToolCall};
use crate::prefs::Prefs;
use crate::store::Source;

/// What a tool needs to run, cloned into the chat task.
#[derive(Clone)]
pub struct Ctx {
    pub prefs: Prefs,
}

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub text: String,
    pub sources: Vec<Source>,
    pub failed: bool,
}

impl Output {
    pub fn text(t: impl Into<String>) -> Output {
        Output { text: t.into(), ..Default::default() }
    }

    pub fn error(t: impl Into<String>) -> Output {
        Output { text: format!("Error: {}", t.into()), failed: true, ..Default::default() }
    }
}

pub struct Info {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Asks before each use.
    pub asks: bool,
}

/// Built-in tools, in the order Settings lists them.
pub const BUILTIN: &[Info] = &[
    Info { name: "web_search", title: "Web search", description: "Look things up on the web and cite sources.", asks: false },
    Info { name: "web_fetch", title: "Read web pages", description: "Open a link and read the page's text.", asks: false },
    Info {
        name: "get_location",
        title: "Location",
        description: "Tell the model roughly where you are, as set under Location.",
        asks: false,
    },
    Info { name: "calculator", title: "Calculator", description: "Exact arithmetic instead of guesses.", asks: false },
    Info {
        name: "read_file",
        title: "Read files",
        description: "Read and list files, only inside the folders you allow below.",
        asks: false,
    },
    Info {
        name: "run_shell",
        title: "Run commands",
        description: "Run shell commands. Asks you before every command.",
        asks: true,
    },
    Info {
        name: "save_memory",
        title: "Save memories",
        description: "Suggest facts to remember across chats. Asks before saving.",
        asks: true,
    },
];

fn def(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": { "type": "object", "properties": props, "required": required },
        }
    })
}

/// Definitions for the enabled tools, plus every tool from running MCP servers.
pub fn definitions(enabled: &[String], location_on: bool) -> Vec<Value> {
    let on = |n: &str| enabled.iter().any(|e| e == n);
    let mut out = Vec::new();
    if on("web_search") {
        out.push(def(
            "web_search",
            "Search the web. Use only when the question needs current events, facts you're unsure of, or anything \
             after your training data; not for greetings or small talk. Cite results by their [number].",
            json!({ "query": { "type": "string", "description": "What to search for" } }),
            &["query"],
        ));
    }
    if on("web_fetch") {
        out.push(def(
            "web_fetch",
            "Download a web page and return its main text.",
            json!({ "url": { "type": "string", "description": "An http or https URL" } }),
            &["url"],
        ));
    }
    if on("get_location") && location_on {
        out.push(def("get_location", "Get the user's current location (city, region, country, coordinates). Use only when the answer depends on \
             where the user is, such as weather or nearby places.", json!({}), &[]));
    }
    if on("calculator") {
        out.push(def(
            "calculator",
            "Evaluate an arithmetic expression. Supports + - * / % ^, parentheses, and sqrt, sin, cos, tan, ln, log, abs, \
             round, floor, ceil, pi, e.",
            json!({ "expression": { "type": "string" } }),
            &["expression"],
        ));
    }
    if on("read_file") {
        out.push(def(
            "read_file",
            "Read a text file on the user's computer. Only allowed folders can be read.",
            json!({ "path": { "type": "string", "description": "Absolute path, or starting with ~/" } }),
            &["path"],
        ));
        out.push(def(
            "list_directory",
            "List the files in a folder on the user's computer. Only allowed folders can be listed.",
            json!({ "path": { "type": "string" } }),
            &["path"],
        ));
    }
    if on("run_shell") {
        out.push(def(
            "run_shell",
            "Run a bash command on the user's Linux computer and return its output. The user approves each command.",
            json!({ "command": { "type": "string" } }),
            &["command"],
        ));
    }
    if on("save_memory") {
        out.push(def(
            "save_memory",
            "Remember a lasting fact or preference about the user for future chats. The user confirms first.",
            json!({ "fact": { "type": "string", "description": "One short sentence" } }),
            &["fact"],
        ));
    }
    out.extend(mcp::definitions());
    out
}

pub fn title_for(name: &str) -> String {
    match name {
        "list_directory" => return "List folder".into(),
        // Older chats may have called this before the date moved into the prompt.
        "get_datetime" => return "Date and time".into(),
        _ => {}
    }
    if let Some(i) = BUILTIN.iter().find(|i| i.name == name) {
        return i.title.into();
    }
    mcp::title_for(name).unwrap_or_else(|| name.replace('_', " "))
}

/// Ask the user; false when they decline or the window goes away.
async fn approve(tx: &async_channel::Sender<Event>, title: String, detail: String) -> bool {
    let (reply, rx) = tokio::sync::oneshot::channel();
    if tx.send(Event::Approval { title, detail, reply }).await.is_err() {
        return false;
    }
    rx.await.unwrap_or(false)
}

pub async fn call(call: &ToolCall, ctx: &Ctx, tx: &async_channel::Sender<Event>) -> Output {
    let arg = |k: &str| call.arguments[k].as_str().unwrap_or("").trim().to_string();
    let result = match call.name.as_str() {
        "web_search" => web::search(&arg("query"), &ctx.prefs).await,
        "web_fetch" => web::fetch(&arg("url")).await,
        "get_location" => location::lookup(&ctx.prefs).await.map(|l| Output::text(l.describe())),
        "get_datetime" => Ok(Output::text(datetime())),
        "calculator" => Ok(match calc::eval(&arg("expression")) {
            Ok(v) => Output::text(calc::format(v)),
            Err(e) => Output::error(e),
        }),
        "read_file" => files::read(&arg("path"), &ctx.prefs.allowed_folders),
        "list_directory" => files::list(&arg("path"), &ctx.prefs.allowed_folders),
        "run_shell" => {
            let command = arg("command");
            if approve(tx, "Run this command?".into(), command.clone()).await {
                files::shell(&command).await
            } else {
                Ok(Output::error("The user declined to run this command."))
            }
        }
        "save_memory" => {
            let fact = arg("fact");
            if fact.is_empty() {
                Ok(Output::error("No fact given."))
            } else if approve(tx, "Remember this?".into(), fact.clone()).await {
                crate::store::add_memory(&fact).map(|_| Output::text("Saved."))
            } else {
                Ok(Output::text("The user chose not to save this."))
            }
        }
        name if mcp::owns(name) => mcp::call(name, &call.arguments).await,
        other => Ok(Output::error(format!("There is no tool called {other}."))),
    };
    result.unwrap_or_else(|e| Output::error(format!("{e:#}")))
}

pub fn datetime() -> String {
    let now = chrono::Local::now();
    let tz = std::env::var("TZ").ok().or_else(|| {
        std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().split("zoneinfo/").last().unwrap_or("").to_string())
    });
    format!(
        "{} ({}), time zone {} (UTC{})",
        now.format("%A, %B %-d, %Y, %H:%M"),
        now.format("%Y-%m-%d"),
        tz.unwrap_or_else(|| "local".into()),
        now.format("%:z")
    )
}

/// The line every system prompt starts with, so models know the date without a tool.
pub fn now_line() -> String {
    format!("Current date and time: {}.", datetime())
}

/// Fill `{{CURRENT_DATE}}` and friends in a system prompt.
pub fn expand_variables(text: &str, prefs: &Prefs, location: Option<&str>, clipboard: Option<&str>) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let now = chrono::Local::now();
    text.replace("{{CURRENT_DATE}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{CURRENT_TIME}}", &now.format("%H:%M").to_string())
        .replace("{{CURRENT_WEEKDAY}}", &now.format("%A").to_string())
        .replace("{{CURRENT_DATETIME}}", &now.format("%Y-%m-%d %H:%M").to_string())
        .replace("{{USER_NAME}}", if prefs.user_name.is_empty() { "the user" } else { &prefs.user_name })
        .replace("{{USER_LOCATION}}", location.unwrap_or("unknown"))
        .replace("{{CLIPBOARD}}", clipboard.unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_tools_are_offered() {
        let defs = definitions(&["calculator".into(), "get_location".into()], false);
        let names: Vec<&str> = defs.iter().map(|d| d["function"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["calculator"]);
        let defs = definitions(&["read_file".into()], false);
        assert_eq!(defs.len(), 2);
    }

    #[test]
    fn variables() {
        let p = Prefs { user_name: "Sam".into(), ..Default::default() };
        let out = expand_variables("Hi {{USER_NAME}} in {{USER_LOCATION}}", &p, Some("Oslo"), None);
        assert_eq!(out, "Hi Sam in Oslo");
        assert!(!expand_variables("{{CURRENT_DATE}}", &p, None, None).contains("{{"));
    }
}
