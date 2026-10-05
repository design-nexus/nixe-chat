# Chat

A desktop chat client for [Ollama](https://ollama.com), built with GTK 4. It chats with
local models, lets them use tools such as web search, and has a marketplace for finding
and downloading models from Hugging Face and the Ollama library. It takes its colors
from your [Omarchy](https://omarchy.org) theme and fits a half-screen tile.

Chat talks to an Ollama server (on this computer by default). It doesn't run models itself.

## What it does

- **Chat**
  - The model picker sits above the conversation, with a bar under it that pulses
    while the model loads and fills once it's in memory. The model you pick stays
    loaded until you pick another, and new chats start with it (both can be changed in
    Settings).
  - Replies stream in with Markdown, tables, syntax-highlighted code (with a copy
    button), and formulas shown as readable symbols.
  - Thinking models show their reasoning in a collapsible block.
  - Stop a reply with <kbd>Esc</kbd>, regenerate it, or edit a question and send it
    again. Every version is kept; arrows under a message switch between them.
  - **Compare:** answer the same message with up to three models side by side, then
    continue with the answer you like.
  - **Attachments:** images for vision models (drop, paste or attach), and text, code or
    PDF files. Long files are searched for the relevant parts when an embedding model is
    installed.
  - **Screenshot:** <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> lets you drag out an
    area of the screen and attaches it.
  - Per-chat system prompt, temperature, top P, top K, context length, seed and thinking,
    in a side panel.
  - **Structured output:** ask for JSON, optionally matching a JSON schema. JSON replies
    are shown as a tree.
  - **Previews:** SVG code blocks are drawn in the side panel; HTML and Mermaid blocks
    open in your browser.
  - Type <kbd>/</kbd> for saved prompts, <kbd>@</kbd> to switch model, <kbd>#</kbd> to add
    a knowledge collection.
  - Chats get a title automatically, and can be renamed, pinned, tagged, archived,
    exported as Markdown or JSON, and searched by their full text.
  - **Folders:** each folder can set the model, system prompt, tools and knowledge its
    chats start with.
- **Tools** for models that support them:
  - Web search (DuckDuckGo with no setup, or SearXNG, Brave Search or Tavily), with
    numbered citations that link to their sources.
  - Reading web pages, the date and time, and a calculator.
  - Location (off by default): from the desktop location service (GeoClue), roughly from
    your IP address, or a place you type.
  - Reading files, only inside folders you allow.
  - Running shell commands, with your approval each time.
  - Saving memories, with your approval.
  - Tools from [MCP](https://modelcontextprotocol.io) servers that run on this computer.
  - Models without tool support still get web results: the search runs first and the
    results go into the prompt.
- **Memories:** facts about you that every chat knows.
- **Knowledge:** collections of documents, embedded with an Ollama embedding model, that
  chats can answer from.
- **Models:** what's installed, what's loaded (and how much sits on the GPU), with
  unload, copy, delete, details and a model's parameters and template. **Create** makes
  a new model from an installed one with its own system prompt and settings. **Check for
  updates** compares each model with its registry and updates it in one click; this also
  happens once a day.
- **Marketplace:**
  - Search Hugging Face GGUF repositories by trending, downloads, likes or date, and
    filter to text, vision or embedding models.
  - Search the Ollama library and see every tag with its size.
  - Each download is marked **Fits GPU**, **Partial offload** or **Too big** for this
    computer, counting the memory for the context length set in Settings, and the best
    fit is marked **Recommended**.
  - Model cards are shown in full. Downloads queue up with progress, speed and time left.
- **Arena:** two anonymous models answer the same prompt; you vote, and an Elo
  leaderboard keeps score.
- **Stats:** tokens and chats per day, and replies and speed per model.
- **Voice:** dictate with [whisper.cpp](https://github.com/ggml-org/whisper.cpp)
  (<kbd>Ctrl</kbd>+<kbd>Space</kbd>) and hear replies read aloud with
  [Piper](https://github.com/rhasspy/piper). Hands-free mode sends when you stop talking.
- **Quick ask:** `chat --quick` opens a small window for one-off questions, optionally
  about the text you've highlighted or copied. **Open in chat** keeps the exchange.
- **Several servers:** add other computers running Ollama and switch between them from
  the status bar.
- **Import and export:** all chats as one JSON file, and imports from Open WebUI exports.

### Easy on the battery

On hybrid laptops the discrete GPU sleeps when nothing uses it. Chat uses GTK's GL
renderer, which doesn't wake it, and reads NVIDIA memory size with `nvidia-smi` only
while the GPU is already awake.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nexus-chat/main/install.sh | bash
```

This builds with Cargo and installs `chat` to `~/.local/bin`, along with a launcher
entry. Add `-s -- --bind` after `bash` to also print lines that open Quick ask with
Super+Alt+Space:

```lua
o.bind("SUPER + ALT + SPACE", "Quick ask", "chat --quick --selection")
o.window({ title = "^Quick ask$" }, { float = true, center = true, size = { 720, 480 } })
```

To remove it, run the same line with `uninstall.sh` in place of `install.sh`. Add
`-s -- --purge` to also remove its settings and chat history.

You also need [Ollama](https://ollama.com/download), here or on another computer.
Optional: `wl-clipboard` (Quick ask with the selection), `grim` and `slurp`
(screenshots), `poppler` (PDFs), `geoclue` (desktop location), whisper.cpp and Piper
(voice).

## Usage

```
chat [--section ID] [--new] [--quick [--selection | --clipboard]] [--screenshot] [--attach FILE…]
```

| Option | What it does |
| --- | --- |
| `--section ID` | Opens `chat`, `models`, `marketplace`, `arena`, `knowledge`, `prompts`, `stats` or `settings` |
| `--new` | Starts a new chat |
| `--quick` | Opens Quick ask; with `--selection` or `--clipboard`, asks about that text |
| `--screenshot` | Drag out an area of the screen and attach it to a chat |
| `--attach FILE…` | Attaches files to a new message |

| Keys | |
| --- | --- |
| <kbd>Enter</kbd> / <kbd>Shift</kbd>+<kbd>Enter</kbd> | Send / new line (swappable in Settings) |
| <kbd>Esc</kbd> | Stop the reply |
| <kbd>Ctrl</kbd>+<kbd>N</kbd> | New chat |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Search chats (or settings, while Settings is open) |
| <kbd>Ctrl</kbd>+<kbd>Space</kbd> | Dictate |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> | Screenshot into the chat |
| <kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>7</kbd> | Go to a page in the sidebar |
| <kbd>Alt</kbd>+<kbd>←</kbd> | Back to the previous page |
| <kbd>Ctrl</kbd>+<kbd>B</kbd> | Collapse or expand the sidebar |
| <kbd>Ctrl</kbd>+<kbd>,</kbd> | Settings |
| <kbd>F1</kbd> | Show the keyboard shortcuts |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

## Files

| Path | What |
| --- | --- |
| `~/.config/nexus-chat/settings.toml` | Preferences, servers and MCP servers |
| `~/.config/nexus-chat/themes/*.toml` | Your own themes |
| `~/.local/share/nexus-chat/chat.db` | Chats, prompts, memories, knowledge and arena votes (SQLite) |
| `~/.local/share/nexus-chat/attachments/` | Attached images and the text of attached files |

## License

MIT
