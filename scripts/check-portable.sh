#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Type-checks the OS-independent crates for Windows and macOS from a Linux machine, so a
# portability regression is caught before CI (or a Windows/macOS box) sees it.
# Needs: rustup targets (installed below) and, for phone-core's bundled SQLite on Windows, a
# MinGW cross compiler (Fedora: mingw64-gcc, Debian: gcc-mingw-w64-x86-64).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
rustup target add x86_64-pc-windows-gnu x86_64-apple-darwin >/dev/null
echo "== phone-obex (pure Rust): Windows, macOS"
cargo check -p phone-obex --target x86_64-pc-windows-gnu
cargo check -p phone-obex --target x86_64-apple-darwin
echo "== phone-core + tests: Windows"
cargo check -p phone-core --tests --target x86_64-pc-windows-gnu
echo "(phone-core on macOS needs a macOS C toolchain, e.g. osxcross; CI covers it on a macOS runner.)"
