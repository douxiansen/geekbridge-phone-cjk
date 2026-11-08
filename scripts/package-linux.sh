#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds the Linux release tarball: dist/geekbridge-phone-<version>-linux-<arch>.tar.gz
#   ./scripts/package-linux.sh [BINARY]      (default: target/release/geekbridge-phone)
# The version comes from the workspace Cargo.toml. The tarball holds the program, the per-user
# installer and the desktop files; the in-app updater reads bin/geekbridge-phone from it.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || { echo "cannot read the version from Cargo.toml" >&2; exit 1; }
arch="$(uname -m)"
binary="${1:-target/release/geekbridge-phone}"
[[ -x "$binary" ]] || { echo "no program at $binary (cargo build --release -p phone-app)" >&2; exit 66; }

name="geekbridge-phone-${version}-linux-${arch}"
stage="dist/$name"
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/scripts" "$stage/packaging/linux"
install -m 0755 "$binary" "$stage/bin/geekbridge-phone"
install -m 0755 scripts/install.sh "$stage/scripts/install.sh"
cp packaging/linux/* "$stage/packaging/linux/"
cp README.md LICENSE CHANGELOG.md "$stage/"

# Reproducible-ish: fixed owner, sorted names, and the commit time for mtimes.
mtime="@$(git log -1 --format=%ct 2>/dev/null || date +%s)"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="$mtime" -C dist -czf "dist/$name.tar.gz" "$name"
rm -rf "$stage"
echo "dist/$name.tar.gz"
