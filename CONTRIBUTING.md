# Contributing

Thanks for helping improve Stint!

## Ground rules

1. **Privacy first.** Stint must never send data off the device. PRs that add network access, telemetry, or new kinds of collected data (e.g. URLs, keystrokes, screenshots) will not be accepted, unless they're strictly opt-in, off by default, and discussed in an issue first.
2. **No HTML sinks in the UI.** Use `el(...)` / `textContent`. CI fails on `innerHTML` and similar.
3. **Keep it light.** Please avoid adding a frontend framework or heavy dependencies. Every new Rust crate needs a short justification in the PR.
4. **Validate at the boundary.** New IPC commands must validate all arguments, be listed in `src-tauri/build.rs`, and be granted in `src-tauri/capabilities/main.json`.

## Before opening a PR

```bash
cd src-tauri
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

Then run the app with `npm run dev` and check the screens you touched in both light and dark mode.

## Adding app → category rules

Edit `RULES` in `src-tauri/src/categories.rs` and add a case to the `guesses` test.
