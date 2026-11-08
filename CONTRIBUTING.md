# Contributing

Build and test:

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

- Keep `phone-obex` and `phone-core` free of OS-specific code: they are tested on Windows and macOS in CI, and ports depend on them.
- No `unwrap()`/`expect()` on I/O, D-Bus or user-input paths; return errors and let the service turn them into notices.
- Never lock `Handle::state()` twice in one expression: the guard is not reentrant.
- UI work can run without a phone: `cargo run -p phone-app --features demo -- --demo`.
- Add SPDX headers (`GPL-3.0-or-later`) to new files.
