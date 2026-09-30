#!/bin/bash
# Remove Chat.
#
# Options:
#   --purge   also remove its settings and chat history
set -euo pipefail

purge=false
[[ ${1:-} == "--purge" ]] && purge=true

say() { printf '\033[1;34m::\033[0m %s\n' "$*"; }

pkill -x chat 2>/dev/null || true
rm -f "$HOME/.local/bin/chat" \
  "$HOME/.local/share/applications/io.github.design_nexus.Chat.desktop" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.design_nexus.Chat.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/nexus-chat"
say "Removed the app."

if [[ $purge == true ]]; then
  rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/nexus-chat" "${XDG_DATA_HOME:-$HOME/.local/share}/nexus-chat"
  say "Removed its settings and chat history. Models stay in Ollama."
fi
