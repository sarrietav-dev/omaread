# Omaread

A fast, keyboard-first EPUB reader for [Omarchy](https://omarchy.org).

It wears your Omarchy theme and follows it live: colours from `colors.toml`,
sizes and control states from the theme's `shell.toml`, drawn by the same
rules the Omarchy shell uses. Books are laid out by
[foliate-js](https://github.com/johnfactotum/foliate-js), the engine behind
Foliate, inside a WebKitGTK view; everything else is Rust and GTK4.

> Early days. See [PLAN.md](PLAN.md) for where it's going.

## Building

```sh
sudo pacman -S --needed base-devel rust gtk4 libadwaita webkitgtk-6.0
cargo run --release -- path/to/book.epub
```

## Keys

| Keys | Action |
|---|---|
| `j` `l` Space → | next page |
| `k` `h` Shift-Space ← | previous page |
| `J` / `K` | next / previous chapter |
| `gg` / `G` | start / end of the book |
| `Ctrl-o` / `Ctrl-i` | back / forward |
| `i` | toggle the status line |
| `q` | close |

## License

MIT. foliate-js is vendored under `web/foliate-js` with its own MIT license.
