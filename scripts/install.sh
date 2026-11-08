#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs GeekBridge Phone for the current user (no root):
#   ./scripts/install.sh [--binary PATH] [--no-service]    install
#   ./scripts/install.sh --uninstall                       remove (keeps your data)
# Run it from a release tarball (bin/geekbridge-phone next to it) or a source
# checkout after `cargo build --release -p phone-app`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${HOME}/.local"
bindir="$prefix/bin"
appdir="$prefix/share/applications"
icondir="$prefix/share/icons/hicolor/scalable/apps"
unitdir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
binary="" service=1 uninstall=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) binary="$2"; shift 2;;
    --no-service) service=0; shift;;
    --uninstall) uninstall=1; shift;;
    -h|--help) sed -n '2,8p' "${BASH_SOURCE[0]}"; exit 0;;
    *) echo "unknown argument: $1" >&2; exit 64;;
  esac
done

have_systemd_user() { command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1; }

if [[ $uninstall -eq 1 ]]; then
  if have_systemd_user; then systemctl --user disable --now geekbridge-phone.service 2>/dev/null || true; fi
  rm -f "$bindir/geekbridge-phone" "$appdir/dev.geekbridge.Phone.desktop" \
        "$icondir/dev.geekbridge.Phone.svg" "$unitdir/geekbridge-phone.service"
  have_systemd_user && systemctl --user daemon-reload || true
  echo "Removed. Your settings and history in ~/.local/share/geekbridge-phone were kept."
  exit 0
fi

if [[ -z "$binary" ]]; then
  for c in "$here/bin/geekbridge-phone" "$here/target/release/geekbridge-phone"; do
    [[ -x "$c" ]] && { binary="$c"; break; }
  done
fi
[[ -x "$binary" ]] || { echo "No geekbridge-phone binary found. Build one with: cargo build --release -p phone-app" >&2; exit 66; }

for d in "$bindir" "$appdir" "$icondir" "$unitdir"; do mkdir -p "$d"; done
install -m 0755 "$binary" "$bindir/geekbridge-phone"
install -m 0644 "$here/packaging/linux/dev.geekbridge.Phone.desktop" "$appdir/dev.geekbridge.Phone.desktop"
install -m 0644 "$here/packaging/linux/dev.geekbridge.Phone.svg" "$icondir/dev.geekbridge.Phone.svg"
sed "s|@BINDIR@|$bindir|" "$here/packaging/linux/geekbridge-phone.service" > "$unitdir/geekbridge-phone.service"
chmod 0644 "$unitdir/geekbridge-phone.service"

if [[ $service -eq 1 ]] && have_systemd_user; then
  systemctl --user daemon-reload
  systemctl --user enable --now geekbridge-phone.service
  echo "Started in the background (systemd user service)."
fi
command -v update-desktop-database >/dev/null && update-desktop-database -q "$appdir" 2>/dev/null || true
case ":$PATH:" in *":$bindir:"*) ;; *) echo "Note: $bindir is not on your PATH.";; esac
echo "Installed. Run 'geekbridge-phone' or open GeekBridge Phone from your launcher."
