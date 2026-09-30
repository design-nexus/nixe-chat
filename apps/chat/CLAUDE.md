# Chat — notes for working on this repo

- GTK4 (gtk4-rs 0.11) + Rust, no libadwaita. Follows `~/Projects/STYLE.md`; `theme.rs`,
  `widgets.rs`, `cmd.rs` and `style.css` started as copies of Tasks (`~/Projects/nexus-tasks`).
  Every colour is a `@theme_*` token; Pango markup that bakes colours (code highlighting,
  inline code) is rebuilt on `hub::THEME`.
- Threads: all network/disk work runs on the tokio runtime in `rt.rs`. `rt::spawn` hands a
  result back to the GTK thread; `rt::stream` delivers a stream of events. Never block the
  GTK thread. `store::with` opens one SQLite connection per thread (WAL), so async code can
  use the store directly.
- Change notification: `hub.rs` (`THEME`, `MODELS`, `CHATS`, `PULLS`, `LIBRARY`, `HOST`).
  Subscribers are tied to a widget and dropped with it.
- Chat flow: `sections/chat/send.rs` stores the user message, creates one assistant row per
  model (compare mode shares `extra.group`), and runs `ollama::chat::run`, which loops
  stream → tool calls → results. Each round after tools is its own assistant row; tool
  results are `role = "tool"` rows. Knowledge lookups and pre-searches (for models without
  tools) are recorded in the reply's `extra.pre`. Messages form a tree (`parent`); a chat's
  `leaf` picks the branch shown.
- Streams outlive the view (`STREAMS` in `sections/chat/mod.rs`): switching chats doesn't stop
  a reply; the live widget is re-attached when the chat is shown again.
- Markdown (`markdown.rs`) keeps one widget per top-level block and only rebuilds blocks
  whose source changed, so streaming stays cheap.
- Don't wake the dGPU: `GSK_RENDERER=ngl` in `main.rs`; `hw.rs` only calls `nvidia-smi`
  while `runtime_status` isn't `suspended`, and caches VRAM in prefs.
- Checks: `cargo clippy -- -D warnings`, `cargo test` (network tests: `cargo test -- --ignored`
  with `OLLAMA_URL` pointing at a server that has `all-minilm`).
- Visual check (quit any running instance first; it's single-instance), ideally with scratch
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME`:
  `CHAT_SNAPSHOT=/tmp/x.png [CHAT_SNAPSHOT_W=… CHAT_SNAPSHOT_H=… CHAT_SNAPSHOT_DELAY=ms] chat --section models`.
  Extra hooks for whole flows: `CHAT_SNAPSHOT_SEND="question"`, `CHAT_SNAPSHOT_COMPARE=model`,
  `CHAT_SNAPSHOT_REPO=user/repo`, `CHAT_SNAPSHOT_PULL=name`; `chat --quick` also honours
  `CHAT_SNAPSHOT` and `CHAT_SNAPSHOT_SEND`.
