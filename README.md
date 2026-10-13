# Omaread

A fast, keyboard-first EPUB reader for [Omarchy](https://omarchy.org).

It wears your Omarchy theme and follows it live: colours from `colors.toml`,
sizes and control states from the theme's `shell.toml`, drawn by the same
rules the Omarchy shell uses. Books are laid out by
[foliate-js](https://github.com/johnfactotum/foliate-js), the engine behind
Foliate, inside a WebKitGTK view; everything else is Rust and GTK4.

> Early days. See [PLAN.md](PLAN.md) for where it's going.

## Installing

```sh
sudo pacman -S --needed base-devel rust gtk4 libadwaita webkitgtk-6.0
bin/install            # builds and installs to ~/.local
bin/install --default  # …and makes Omaread the app for EPUB files
```

Omaread reads the EPUBs under `~/Documents/Books`; each folder is a shelf.
Other folders go in `~/.config/omaread/config.toml`:

```toml
[library]
folders = ["~/Documents/Books", "/mnt/nas/books"]
```

## Keys

Library

| Keys | Action |
|---|---|
| `h` `j` `k` `l` / arrows | move |
| Enter | open |
| `/` | filter the whole library (Esc clears) |
| `o` | cycle sort: recent, title, author, progress, added |
| Tab / Shift-Tab | next / previous shelf |
| `gg` / `G` | first / last book |
| `gr` | rescan |
| `q` | quit |

Reading

| Keys | Action |
|---|---|
| `j` `l` Space → | next page |
| `k` `h` Shift-Space ← | previous page |
| `J` / `K` | next / previous chapter |
| `gg` / `G` | start / end of the book |
| `Ctrl-o` / `Ctrl-i` | back / forward |
| `t` | table of contents (`j`/`k` move, `h`/`l` fold, Enter go) |
| `:` | command line: `:42%`, `:p 120`, `:ch 7` |
| `,` | reading settings (`j`/`k` move, `h`/`l` change) |
| `+` / `-` | bigger / smaller text |
| `T` | cycle theme: Omarchy, light, dark, sepia |
| `i` | toggle the status line |
| `q` / Esc | back to the library |

Reading settings (theme, font, size, line height, justification,
hyphenation, margins, width, columns, pages or scroll, how much of the
publisher's styling to keep) apply to all books, or to one book when you
switch "Applies to" to "this book". Any installed Omarchy theme can be a
book's theme.

For hyphenation, install the dictionaries: `sudo pacman -S hyphen-en hyphen-es`.

Your place in every book is saved as you read. The status line shows the
chapter, time left in the chapter and the book (learned from your own reading
pace), the page and how far through you are.

## License

MIT. foliate-js is vendored under `web/foliate-js` with its own MIT license.
