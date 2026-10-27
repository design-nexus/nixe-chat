#!/usr/bin/env bash
set -euo pipefail

tag=prism-b10826-c9c34bc
asset=llama-prism-b10826-c9c34bc-bin-linux-cuda-13.3-x64.tar.gz
digest=43e203be88b8c2fbebb24d518078c3ff02df742970d89f739e9dc23646c6ff2d
libdir="$HOME/.local/lib/nixe-chat/llama.cpp"
unitdir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

if [[ ! -x $libdir/llama-server || ! -f $libdir/.release || $(cat "$libdir/.release") != "$tag" ]]; then
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  if [[ -n ${NIXE_LLAMA_ARCHIVE:-} ]]; then
    cp "$NIXE_LLAMA_ARCHIVE" "$work/$asset"
  else
    curl -fL --retry 3 -o "$work/$asset" "https://github.com/PrismML-Eng/llama.cpp/releases/download/$tag/$asset"
  fi
  printf '%s  %s\n' "$digest" "$work/$asset" | sha256sum -c -
  tar -xzf "$work/$asset" -C "$work"
  install -d "$libdir"
  cp -a "$work/llama-prism-b10826-c9c34bc/"*.so* "$libdir/"
  install -m 755 "$work/llama-prism-b10826-c9c34bc/llama-server" "$libdir/llama-server"
  printf '%s\n' "$tag" > "$libdir/.release"
fi

install -m 755 "$here/scripts/llama-server-launch" "$libdir/llama-server-launch"

install -d "$unitdir"
install -d "${XDG_CONFIG_HOME:-$HOME/.config}/nixe-chat"
preset="${XDG_CONFIG_HOME:-$HOME/.config}/nixe-chat/llama-models.ini"
[[ -f $preset ]] || printf 'version = 1\n' > "$preset"
if ! grep -q '^\[\*\]' "$preset"; then
  context=$(sed -nE 's/^num_ctx[[:space:]]*=[[:space:]]*([0-9]+).*/\1/p' "${XDG_CONFIG_HOME:-$HOME/.config}/nixe-chat/settings.toml" 2>/dev/null | head -1 || true)
  [[ $context =~ ^[0-9]+$ ]] || context=8192
  printf '\n[*]\nctx-size = %s\nn-gpu-layers = auto\nflash-attn = auto\n' "$context" >> "$preset"
fi
install -m 644 "$here/data/nixe-llama-server.service" "$unitdir/nixe-llama-server.service"
systemctl --user daemon-reload
