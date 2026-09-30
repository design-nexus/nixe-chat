//! Every page in the sidebar, in order.

pub mod arena;
pub mod chat;
pub mod knowledge;
pub mod marketplace;
pub mod models;
pub mod prompts;
pub mod settings;
pub mod stats;

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub build: fn() -> gtk::Widget,
}

pub fn all() -> Vec<Section> {
    vec![
        Section { id: "chat", title: "Chat", icon: "user-available-symbolic", build: chat::build },
        Section { id: "models", title: "Models", icon: "chat-models-symbolic", build: models::build },
        Section { id: "marketplace", title: "Marketplace", icon: "chat-store-symbolic", build: marketplace::build },
        Section { id: "arena", title: "Arena", icon: "applications-games-symbolic", build: arena::build },
        Section { id: "knowledge", title: "Knowledge", icon: "accessories-dictionary-symbolic", build: knowledge::build },
        Section { id: "prompts", title: "Prompts", icon: "utilities-terminal-symbolic", build: prompts::build },
        Section { id: "stats", title: "Stats", icon: "chat-stats-symbolic", build: stats::build },
        Section { id: "settings", title: "Settings", icon: "emblem-system-symbolic", build: settings::build },
    ]
}
