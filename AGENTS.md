# AGENTS.md

This file provides guidance to AI Agent when working with code in this repository.

Omaread is a keyboard-first EPUB reader for Omarchy (Rust + GTK4/libadwaita + WebKitGTK; books are laid out by vendored foliate-js). `README.md` covers user-facing behavior and keys; `PLAN.md` holds the design decisions and milestones.

## Commands

```sh
cargo build                     # dev build (dependencies are optimized at opt-level 2)
cargo run -- [book.epub]        # run the app; no arg opens the library
cargo test                      # Rust tests (mostly in omaread-core)
cargo test -p omaread-core epub # one crate / name filter
node --test web/app/             # JS tests (typography.test.mjs, node:test)
bin/omaread-install [--default] # release build + install to ~/.local (+ default EPUB handler)
```

CLI subcommands handled in `main.rs` before GTK starts: `scan`, `status [--json]`, `check-library [--json]` (opens every EPUB, smoke test), `lookup`, `import-foliate`.

System deps (Arch): `gtk4 libadwaita webkitgtk-6.0`.

## Architecture

- **`crates/omaread-core`**: GTK-free logic, unit-tested: EPUB metadata/covers, library index + SQLite (`db`), annotations, config/keymap TOML, Omarchy theme tokens → stylesheet (`theme`, `style`), lookup providers, SSIP speech client, reading-speed model, fuzzy matching.
- **`crates/omaread`**: the GTK app. `window.rs` (the big one) owns a `gtk::Overlay`: base is the library grid or the reader; every panel (`toc_view`, `search_view`, `settings_view`, `annotations_view`, `lookup_view`, `voice_view`, `palette`, `help_view`) is a native overlay drawn over the book. `keys.rs` holds the vim-style keymap, with per-context actions that `config.toml` can rebind.
- **Web page ↔ Rust bridge**: `web/app/reader.js` (+ `typography.js`) runs foliate-js inside a `webkit6::WebView`. Rust → page via `reader.rs` calling `globalThis.omaread.<fn>()`; page → Rust via JSON messages parsed into `reader::Event` (serde tag = `type`, kebab-case). Adding an event means changing both sides. The web view only renders the book; all chrome is native.
- **Assets**: `assets.rs` serves `web/` over the `omaread://app/…` scheme using rust-embed. In debug builds it reads from disk (edit JS, reload, no rebuild); release embeds it. foliate-js is vendored in `web/foliate-js` (don't edit; pinned upstream copy). Books are served to WebKit by path, never copied or extracted.
- **Theme**: colors come from `~/.local/state/omarchy/current/theme/{colors,shell}.toml` and are watched live (`notify`); `theming.rs` applies them to GTK CSS. Annotations/progress are stored as CFIs so the engine is replaceable.
- Single instance via GApplication: a second `omaread book.epub` opens in the running window.

## Debug-build helpers

- `OMAREAD_APP_ID=<id>` runs a debug build beside the installed app.
- Debug-only GAction hooks (see `install_debug_actions` in `main.rs`): `debug-key` (inject keys), `debug-js` (evaluate in page), `debug-snapshot` (render window to PNG); invoke via `gdbus call … org.gtk.Actions.Activate`.
- `OMAREAD_TTS_MUTE` silences TTS; `OMAREAD_THEME_HOME` substitutes `$HOME` for theme lookup.
