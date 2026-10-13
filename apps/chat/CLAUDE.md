# Chat — notes for working on this repo

- GTK4 (gtk4-rs 0.11) + Rust, no libadwaita. Follows `~/Projects/STYLE.md`; `theme.rs`,
  `widgets.rs`, `cmd.rs` and `style.css` started as copies of Tasks (`~/Projects/nexus-tasks`).
  Every color is a `@theme_*` token or one of the derived mixes at the top of `style.css`
  (`@hairline`, `@nav_active`, …); Pango markup that bakes colors (code highlighting, inline
  code) is rebuilt on `hub::THEME`. The window chrome (top bar, sidebar groups, status bar,
  toasts, F1 shortcuts) follows Music (`~/Projects/nexus-music/src/window.rs`).
- Settings is a card over the window (`settings_dialog.rs`, copied from Music), not a page:
  `sections/settings.rs::build` fills one page with every group and the card shows one group
  at a time; `navigate("settings")` opens it. Groups are matched by name in its `GROUPS`.
- Pages have no title: the top bar names the page (the chat's title on the chat page) and
  `Section::description` goes in the status bar (`window::refresh_crumb`/`refresh_note`).
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
  `CHAT_SNAPSHOT_REPO=user/repo`, `CHAT_SNAPSHOT_PULL=name`, `CHAT_SNAPSHOT_ARENA="prompt"`;
  `chat --quick` also honors `CHAT_SNAPSHOT` and `CHAT_SNAPSHOT_SEND`.
- Layout checks: `CHAT_DEBUG_WIDTH=736` (with `CHAT_SNAPSHOT`) prints the innermost widgets
  whose minimum width is over that, i.e. what makes a page wider than an 800 px window.
  Long one-line text (model names, titles, paths) goes through `widgets::short_label`;
  drop-downs ellipsize via `widgets::ellipsize`. The wide layout (sidebar expanded) must also
  fit a half-screen tile (~780 px): if it's wider, GTK keeps the window at its minimum, the
  width never drops under 980 and the window stays cut off. `CHAT_DEBUG_WIDE=1` forces the
  wide layout so `CHAT_DEBUG_WIDTH` can measure it; extras that widen it go behind
  `window::on_roomy`, and side panels call `window::set_needs_room`.
  `CHAT_AUTOSEND="text"` sends a message in a normal (visible) window, for checking with grim.
- Never move a scroll adjustment inside its `changed` handler: that runs during layout and
  GTK drops the move, leaving content drawn at the old offset. Defer it (idle or tick).
