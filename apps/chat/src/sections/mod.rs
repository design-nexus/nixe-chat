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
    /// The sidebar group it's listed under.
    pub group: &'static str,
    pub icon: &'static str,
    /// One line for the status bar.
    pub description: &'static str,
    pub build: fn() -> gtk::Widget,
}

/// The pages in the sidebar. Settings isn't one: it opens as a card over the window.
pub fn all() -> Vec<Section> {
    vec![
        Section {
            id: "chat",
            title: "Chat",
            group: "Chat",
            icon: "user-available-symbolic",
            description: "",
            build: chat::build,
        },
        Section {
            id: "arena",
            title: "Arena",
            group: "Chat",
            icon: "applications-games-symbolic",
            description: "Two anonymous models answer the same prompt; vote, and the leaderboard keeps score",
            build: arena::build,
        },
        Section {
            id: "stats",
            title: "Stats",
            group: "Chat",
            icon: "chat-stats-symbolic",
            description: "Your chats in numbers, counted from this computer's history",
            build: stats::build,
        },
        Section {
            id: "models",
            title: "Models",
            group: "Models",
            icon: "chat-models-symbolic",
            description: "What's installed on this server, what's loaded, and what's downloading",
            build: models::build,
        },
        Section {
            id: "marketplace",
            title: "Marketplace",
            group: "Models",
            icon: "chat-store-symbolic",
            description: "Models on Hugging Face and in the Ollama library, and which fit this computer",
            build: marketplace::build,
        },
        Section {
            id: "knowledge",
            title: "Knowledge",
            group: "Library",
            icon: "accessories-dictionary-symbolic",
            description: "Collections of your documents; attach one to a chat, or type # in the message box",
            build: knowledge::build,
        },
        Section {
            id: "prompts",
            title: "Prompts",
            group: "Library",
            icon: "utilities-terminal-symbolic",
            description: "Text you use often; type / and its command in any chat to insert it",
            build: prompts::build,
        },
    ]
}
