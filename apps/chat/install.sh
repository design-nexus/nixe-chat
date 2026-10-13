#!/bin/bash
# Install Chat, a desktop client for Ollama.
#
# From a clone, ./install.sh builds from source and installs to ~/.local.
#
# Options:
#   --bind     also print Hyprland lines for Quick ask to paste into ~/.config/hypr/bindings.lua
set -euo pipefail

REPO="design-nexus/nexus-chat"

bind=false
for arg in "$@"; do
  case "$arg" in
    --bind) bind=true ;;
    -h | --help)
      sed -n '2,7p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

say() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m::\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31m::\033[0m %s\n' "$*" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Piped into bash there is no script file, so fetch the source.
script_dir=""
if [[ -n ${BASH_SOURCE[0]:-} && -f ${BASH_SOURCE[0]} ]]; then
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
src="$script_dir"
if [[ -z $src || ! -f $src/Cargo.toml ]]; then
  command -v git >/dev/null || die "git is needed to fetch the source."
  git clone --depth 1 "https://github.com/$REPO.git" "$work/src"
  src="$work/src"
fi

if ! command -v cargo >/dev/null; then
  if command -v pacman >/dev/null; then
    say "Installing Rust and GTK 4 build dependencies"
    sudo pacman -S --needed --noconfirm rust gtk4 pkgconf gcc
  else
    die "Rust (cargo) is needed to build Chat. Install it and run this again."
  fi
fi

say "Building Chat (a few minutes the first time)"
(cd "$src" && cargo build --release --locked)

bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"

say "Installing to ~/.local"
install -m 755 "$src/target/release/chat" "$bin/chat"
install -m 644 "$src/data/io.github.design_nexus.Chat.desktop" "$apps/"
install -m 644 "$src/data/io.github.design_nexus.Chat.svg" "$icons/"
update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

command -v ollama >/dev/null || warn "Ollama isn't installed. Get it from https://ollama.com/download, or point Chat at another computer in Settings."

# Optional helpers: name, package, what it's for.
while IFS='|' read -r tool pkg what; do
  command -v "$tool" >/dev/null || warn "Optional: $tool ($pkg) for $what."
done <<'LIST'
wl-paste|wl-clipboard|Quick ask with the selection or clipboard
grim|grim|screenshots into a chat
slurp|slurp|choosing the screenshot area
pdftotext|poppler|reading attached PDFs
pw-record|pipewire|dictation and reading aloud
whisper-cli|whisper.cpp|dictation
piper|piper-tts|reading replies aloud
LIST

case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH; launch Chat from the app launcher, or add it to PATH." ;;
esac

if [[ $bind == true ]]; then
  say "For Quick ask on Super+Alt+Space, add these to ~/.config/hypr/bindings.lua:"
  echo '  o.bind("SUPER + ALT + SPACE", "Quick ask", "chat --quick --selection")'
  echo '  o.window({ title = "^Quick ask$" }, { float = true, center = true, size = { 720, 480 } })'
fi

say "Done. Open Chat from the app launcher, or run: chat"
