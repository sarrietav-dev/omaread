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
bin/omaread-install            # builds and installs to ~/.local
bin/omaread-install --default  # …and makes Omaread the app for EPUB files
```

Or from the AUR: `yay -S omaread-git` (the PKGBUILD is in `packaging/aur/`).

Omaread reads the EPUBs under `~/Documents/Books`; each folder is a shelf.
Other folders go in `~/.config/omaread/config.toml`:

```toml
[library]
folders = ["~/Documents/Books", "/mnt/nas/books"]
```

## Keys

Press `?` anywhere for this list on screen. Any binding can be changed in
`config.toml` — the app re-reads it as you edit.

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
| `:` | command palette |
| `q` | quit |

Reading

| Keys | Action |
|---|---|
| `j` `l` Space → | next page |
| `k` `h` Shift-Space ← | previous page |
| `J` `K` / `H` `L` | next / previous chapter |
| `gg` / `G` | start / end of the book |
| `Ctrl-o` / `Ctrl-i` | back / forward |
| `t` | table of contents (`j`/`k` move, `h`/`l` fold, Enter go) |
| `/` | search the book (Alt+c case, Alt+w whole words, Alt+d accents) |
| `n` / `N` | next / previous match (Esc clears) |
| `:` | command palette: every command, fuzzy; also `:42%`, `:p 120`, `:ch 7`, `:export` |
| select text | `h` highlight, `1`–`5` color, `a` note, `y` copy, `d` dictionary, `w` Wikipedia, `t` translate |
| click a highlight | `a` note, `1`–`5` recolor, `x` delete |
| `s` | read aloud / pause (`]`/`[` next/previous sentence, `<`/`>` speed, `V` voice, Esc stop); on a selection, read from there |
| `m` | bookmark this page (again to remove) |
| `A` / `'` | highlights / bookmarks (`j`/`k`, Enter go, `a` note, `dd` delete) |
| `:export` | highlights and notes to `~/Documents/Omaread/<title>.md` (`:export json` too) |
| `,` | reading settings (`j`/`k` move, `h`/`l` change) |
| `+` / `-` | bigger / smaller text |
| `T` | cycle theme: Omarchy, light, dark, sepia |
| `i` | toggle the status line |
| `q` / Esc | back to the library |

Rebinding, in `~/.config/omaread/config.toml`. Actions are named; a listed
action keeps only the keys you give it:

```toml
[keymap.reader]
next_page = ["j", "l", "space"]
prev_page = ["k", "h"]
speak = "p"

[keymap.library]
open = ["enter", "o"]
```

Key names read as they do above (`j`, `G`, `space`, `ctrl+o`, `enter`,
`shift+tab`, `page_down`). Unknown contexts, actions and keys are ignored
with a note on stderr.

Reading settings (theme, font, size, line height, justification,
hyphenation, margins, width, columns, pages or scroll, how much of the
publisher's styling to keep) apply to all books, or to one book when you
switch "Applies to" to "this book". Any installed Omarchy theme can be a
book's theme.

For hyphenation, install the dictionaries: `sudo pacman -S hyphen-en hyphen-es`.

Your place in every book is saved as you read.

Lookups come from Wiktionary, Wikipedia and Google Translate (Spanish ⇄
English, picked from the text). Answers are cached for a month. Providers
are configurable, including local programs (run without a shell) and web
searches:

```toml
[[lookup.provider]]
name = "Dictionary"
kind = "builtin.wiktionary"
key = "d"

[[lookup.provider]]
name = "Translate"
kind = "builtin.translate"
key = "t"
pair = ["es", "en"]

[[lookup.provider]]
name = "Local"
kind = "command"
key = "D"
command = ["sdcv", "-n", "--", "{text}"]

[[lookup.provider]]
name = "Search"
kind = "url"
key = "g"
url = "https://duckduckgo.com/?q={text}"
```

Commands run without a shell, and a selection never becomes an option
(leading dashes are dropped); end your options with `--` where the program
supports it. `omaread lookup d goroutine` prints what the card would show.
`omaread status` lists what you're reading, most recent first; `omaread
status --json` gives the same with chapter, progress and time left, for a
bar or a script. `omaread check-library` opens every book in the library
and reports the ones that won't (`--json` for machines); it's the smoke
test for a pile of EPUBs.

Reading aloud goes through speech-dispatcher (so your piper voices work),
one sentence at a time with the sentence highlighted; it turns pages and
moves through chapters on its own. Voice and speed are remembered.

Coming from Foliate? On first run Omaread brings over your reading
positions, highlights, notes and bookmarks (`omaread import-foliate` does it
again; nothing already in Omaread is overwritten). The status line shows the
chapter, time left in the chapter and the book (learned from your own reading
pace), the page and how far through you are.

## License

MIT. foliate-js is vendored under `web/foliate-js` with its own MIT license.
