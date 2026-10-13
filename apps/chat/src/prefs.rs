//! This app's own preferences (`~/.config/nexus-chat/settings.toml`).
//!
//! The GTK thread owns the live copy. Background work gets a clone.

pub use nixe_theme::ThemeMode;

use serde::{Deserialize, Serialize};

/// An Ollama server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Host {
    pub name: String,
    pub url: String,
    /// Sent as `Authorization: Bearer …` when set (for servers behind a proxy).
    pub key: String,
}

impl Default for Host {
    fn default() -> Self {
        Host { name: "This computer".into(), url: "http://127.0.0.1:11434".into(), key: String::new() }
    }
}

/// A Model Context Protocol server started over stdio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct McpServer {
    pub name: String,
    /// The command line, split on whitespace (quotes are respected).
    pub command: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    // ----- This window -----
    pub mode: ThemeMode,
    pub theme: String,
    pub reduce_motion: bool,
    /// The sidebar shows only icons, whatever the window width.
    pub sidebar_collapsed: bool,
    pub glow: bool,
    pub last_chat: i64,

    // ----- Connection -----
    pub hosts: Vec<Host>,
    pub active_host: usize,

    // ----- Chat defaults -----
    /// The model new chats start with; empty means the last one picked.
    pub new_chat_model: String,
    /// The model last picked in a chat.
    pub last_model: String,
    /// Keep the last picked model in memory until another is picked.
    pub keep_loaded: bool,
    pub system_prompt: String,
    pub user_name: String,
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: i64,
    pub num_ctx: i64,
    /// What to do when a chat outgrows its context: "rolling", "keep_first" or "stop".
    pub overflow: String,
    /// Suggest follow-up questions after a reply.
    pub followups: bool,
    pub keep_alive: String,
    pub think: bool,
    pub auto_title: bool,
    pub enter_sends: bool,

    // ----- Tools -----
    pub tools_enabled: Vec<String>,
    pub allowed_folders: Vec<String>,
    pub max_tool_rounds: u32,

    // ----- Web search -----
    /// duckduckgo, searxng, brave or tavily.
    pub search_backend: String,
    pub searxng_url: String,
    pub brave_key: String,
    pub tavily_key: String,
    pub search_results: u32,
    pub search_fetch_pages: bool,

    // ----- Location -----
    /// off, desktop (GeoClue), ip or manual.
    pub location_mode: String,
    pub manual_location: String,
    pub location_in_prompt: bool,

    // ----- Knowledge -----
    pub embed_model: String,
    pub chunk_size: usize,
    pub rag_top_k: usize,

    // ----- Voice -----
    pub whisper_bin: String,
    pub whisper_model: String,
    pub piper_bin: String,
    pub piper_voice: String,
    pub auto_read: bool,
    pub hands_free: bool,

    // ----- Quick ask -----
    pub quick_model: String,

    // ----- MCP -----
    pub mcp_servers: Vec<McpServer>,

    // ----- Hardware (cached so we never wake a sleeping GPU to read it) -----
    pub vram_bytes: u64,

    // ----- Model updates -----
    pub update_check_day: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            reduce_motion: false,
            sidebar_collapsed: false,
            glow: true,
            last_chat: 0,
            hosts: vec![Host::default()],
            active_host: 0,
            new_chat_model: String::new(),
            last_model: String::new(),
            keep_loaded: true,
            system_prompt: String::new(),
            user_name: String::new(),
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            num_ctx: 8192,
            overflow: "rolling".into(),
            followups: false,
            keep_alive: "5m".into(),
            think: true,
            auto_title: true,
            enter_sends: true,
            tools_enabled: vec!["web_search".into(), "web_fetch".into(), "calculator".into()],
            allowed_folders: Vec::new(),
            max_tool_rounds: 8,
            search_backend: "duckduckgo".into(),
            searxng_url: String::new(),
            brave_key: String::new(),
            tavily_key: String::new(),
            search_results: 5,
            search_fetch_pages: false,
            location_mode: "off".into(),
            manual_location: String::new(),
            location_in_prompt: false,
            embed_model: "nomic-embed-text".into(),
            chunk_size: 1200,
            rag_top_k: 5,
            whisper_bin: "whisper-cli".into(),
            whisper_model: String::new(),
            piper_bin: "piper".into(),
            piper_voice: String::new(),
            auto_read: false,
            hands_free: false,
            quick_model: String::new(),
            mcp_servers: Vec::new(),
            vram_bytes: 0,
            update_check_day: String::new(),
        }
    }
}

impl Prefs {
    pub fn host(&self) -> Host {
        self.hosts.get(self.active_host).or_else(|| self.hosts.first()).cloned().unwrap_or_default()
    }

    pub fn tool_on(&self, name: &str) -> bool {
        self.tools_enabled.iter().any(|t| t == name)
    }
}

nixe_core::prefs_store!(Prefs, nixe_core::prefs::Store::standard_with(fix));

/// There is always at least one host to talk to.
fn fix(p: &mut Prefs) -> bool {
    if p.hosts.is_empty() {
        p.hosts.push(Host::default());
        return true;
    }
    false
}

/// Normalise what people type for a host: add the scheme and default port.
pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.is_empty() {
        return "http://127.0.0.1:11434".into();
    }
    let with_scheme = if h.contains("://") { h.to_string() } else { format!("http://{h}") };
    let after_scheme = with_scheme.split("://").nth(1).unwrap_or("");
    if after_scheme.contains(':') || after_scheme.contains('/') || with_scheme.starts_with("https://") {
        with_scheme
    } else {
        format!("{with_scheme}:11434")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hosts() {
        assert_eq!(normalize_host(""), "http://127.0.0.1:11434");
        assert_eq!(normalize_host("192.168.1.5"), "http://192.168.1.5:11434");
        assert_eq!(normalize_host("http://box:8080/"), "http://box:8080");
        assert_eq!(normalize_host("https://ollama.example.com"), "https://ollama.example.com");
    }

    #[test]
    fn old_files_load_with_defaults() {
        let p: Prefs = toml::from_str("theme = \"nord\"").unwrap();
        assert_eq!(p.theme, "nord");
        assert_eq!(p.search_backend, "duckduckgo");
    }
}
