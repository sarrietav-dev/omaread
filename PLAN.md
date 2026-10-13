# Omaread — plan

A fast, reliable, keyboard-first EPUB reader for Omarchy. Looks and feels like
Flea and the other Oma* apps, follows the Omarchy theme live, and reads the
library in `~/Documents/Books`.

## Decisions so far

| Topic | Decision |
|---|---|
| Name | `omaread` (app id `io.github.sebas.Omaread`) |
| Stack | Rust + GTK4 (+ libadwaita, restyled), like Omacharts. WebKitGTK 6 renders the book page only, like Omafeed. |
| Book engine | [foliate-js](https://github.com/johnfactotum/foliate-js) (MIT), vendored at a pinned commit, bundled into the binary as a GResource |
| UI style | Overlay-only: the book fills the window, and every panel (TOC, search, lookup, annotations, settings, palette) is an overlay drawn over it. Library is a full-screen cover grid. |
| Look | Omarchy shell design tokens (`Style.qml` + theme `shell.toml` + `colors.toml`) ported to GTK CSS: square corners, Omarchy font, hairline borders, 4/8/18/22% fill states, accent focus ring |
| Keys | Vim-style everywhere, `?` shows all bindings, `:` command palette |
| Library | Cover grid over `~/Documents/Books` (EPUB only; PDFs/DjVu ignored); top-level folders become shelves |
| Lookups | Pluggable providers in config; defaults: Wiktionary, Wikipedia, translate |
| TTS | speech-dispatcher over SSIP (your piper module + voices) |
| Future | AI features: designed in via the command registry, provider system, and CLI (see the last section) |
| Translate | Spanish ⇄ English: text detected as Spanish goes to English, anything else goes to Spanish |
| Reading mode | Paginated by default (auto columns: 2 when wide); scrolled supported and switchable per book |
| Book fonts | Publisher fonts by default, with a serif fallback; overridable globally and per book |
| Bar widget | No plugin. The app ships `omaread status --json` (current book, chapter, progress, time left) for a bar to read |

## Architecture

```
┌──────────────────────────── GTK4 window (Rust) ───────────────────────────┐
│ gtk::Overlay                                                              │
│  ├─ base: Library grid (gtk::GridView, virtualized)  or  Reader           │
│  │        Reader = webkit6::WebView ── reader.html + foliate-js           │
│  └─ overlays (native GTK, Omarchy-styled): palette, TOC, search,          │
│     lookup card, annotation editor, settings, TTS bar, status line, help  │
└───────────────────────────────────────────────────────────────────────────┘
        ▲ typed JSON messages (script message handler / evaluate_javascript)
        ▼
 omaread-core (pure Rust, no GTK, unit-tested)
   library index · SQLite · EPUB metadata/covers · theme tokens · config
   lookup providers · TTS (SSIP) · reading-speed model · Foliate import
```

* **The app's own page and foliate-js are served from the binary** over
  `omaread://app/…`. Books reach the page the way Foliate does it: the page
  asks for a file, and Rust answers with the path. WebKit then reads the
  EPUB lazily from disk, so nothing is copied or extracted.
* **The web view only renders the book.** Selection, position, search hits
  and TTS ranges go to Rust as events, and Rust draws all the chrome natively.
  The book's CSS only needs a few things from the theme: page background and
  foreground, link and selection colors, and highlight colors.
* **One reader-engine boundary** (`ReaderEngine` trait: `open`, `goto`,
  `next/prev`, `set_style`, `search`, `tts_*`, events). Everything we store
  (progress, highlights, bookmarks) is a CFI, which doesn't depend on the
  engine, so the engine could be replaced later without losing data.
* **Single instance:** `omaread file.epub` from Flea, `xdg-open` or a terminal
  opens in the running window (GApplication, same as Omacharts).

### Workspace

```
Cargo.toml                    workspace
crates/omaread-core/          library, db, epub, theme, config, providers, tts, import
crates/omaread/               GTK app: window, library, reader, overlays, keymap, bridge
web/                          reader.html, bridge.js, foliate-js (git subtree, pinned)
data/                         .desktop, icon, MIME association, GResource xml
bin/omaread-install, packaging/aur/   like Omacharts
```

Main crates: `gtk4`, `libadwaita`, `webkit6`, `rusqlite` (bundled),
`zip`, `quick-xml`, `image`, `rayon`, `notify`, `serde`/`toml`,
`ssip-client-async`, `ureq` (lookups), `clap`.

## Omarchy look and theme hot reload

* **Sources**
  * `~/.local/state/omarchy/current/theme/colors.toml`: palette and `mode`.
  * `~/.local/state/omarchy/current/theme/shell.toml`: `[font] base-size`,
    `[spacing]`, `[controls]` overrides.
  * The Omarchy font as resolved by fontconfig.
* **Token port**
  * `Style.qml`'s rules are reimplemented in `omaread-core::theme`. That
    covers the fill/border alphas per state, the spacing ladder
    (`xxs…huge`, scaled with font), `cornerRadius: 0`, the control height
    and padding, and the hairline width.
  * They are turned into one GTK CSS stylesheet plus libadwaita named colors
    (Omacharts' approach), with contrast lifting for muted text (Flea's
    approach).
  * Unit tests check the generated stylesheet against the values `Style.qml`
    produces for a few themes.
* **Hot reload:** fingerprint `colors.toml` and `shell.toml` (path, mtime,
  size) on a ~1 s timer, which survives Omarchy's directory swap the way
  Omacharts does. Add an inotify watch on the parent directory for an instant
  reaction, with the timer as a backstop. On change:
  * swap the GTK CSS provider;
  * push new CSS variables into the book (no reflow unless font metrics
    changed).
* **Theme modes:** `system` (follow Omarchy), `light`, `dark` (built-in
  Omarchy-like palettes), `custom` (any palette in config). Each can be
  overridden per book, e.g. sepia for one novel.
* **Font:** chrome always uses the Omarchy font at Omarchy sizes. Book text
  defaults to a readable serif and can be changed (see Typography).

## Library

* **Scan:** walk `~/Documents/Books` recursively and pick up `*.epub` only
  (270 books today). Each folder becomes a shelf, and nested folders become
  nested shelves. Any extra library roots can be added in config.
* **Metadata:** parse `container.xml` and the OPF in Rust with
  `zip` + `quick-xml`, in parallel with rayon. Store title, authors, series,
  language, identifier, cover, and page-list presence.
* **Cache:** keyed by `(path, size, mtime)`, so a rescan of 270 books with
  no changes takes milliseconds. Cover thumbnails are cached in
  `~/.cache/omaread/covers/` at 2× grid size.
* **Live updates:** a `notify` watch on the roots picks up books as they are
  added, moved or removed.
* **Book identity:** the EPUB `dc:identifier`, falling back to a content hash.
  Moving or renaming a file keeps its progress and annotations.
* **Grid view:**
  * Covers with a progress bar under each.
  * Sort by recent, title, author, progress or added.
  * `/` filters across title and author.
  * Shelves are listed in a rail; tab cycles through them.
  * A "Continue reading" row sits at the top.
* **Foliate import:** read `~/.local/share/com.github.johnfactotum.Foliate/*.json`
  (31 books, 2 with annotations) once on first run, or with
  `omaread import-foliate`. Brings over the last position, bookmarks and
  annotations, matched by identifier.

## Reader features

### Layout and typography (global defaults + per-book overrides)

* Font family (picker with live preview; "publisher default" option), font
  size, line height, paragraph spacing.
* Justification on/off, hyphenation on/off (WebKit `hyphens: auto`, which
  follows the book's `lang`).
* Margins (horizontal and vertical), max column width, number of columns
  (auto, 1 or 2), paginated or scrolled.
* Override publisher styles (off, fonts only, or everything).
* Settings changes apply live, and the reading position is kept across
  reflow (CFI anchored).

### Navigation and position

* **TOC overlay:** a tree with the current section marked, j/k to move,
  Enter to jump, and folding.
* **Status line:** a thin strip at the bottom, hidden or shown with `i`. Its
  slots can be configured and show:
  * Identifier: the current section/chapter label from the TOC.
  * Page: the publisher's page-list when the book has one, otherwise
    computed pages (Foliate-style "locations"), as `p. 42 / 310`.
  * Overall progress: % of the book.
  * Time left in the section and time left in the book.
* **Reading-speed model:** characters per minute, learned from your page
  turns (outliers dropped, default ~250 wpm until there's enough data). It's
  stored per user and optionally per language.
* **History:** a back/forward jump list (`Ctrl-o` / `Ctrl-i`), like Vim.
* **Go to:** `:goto 42%`, `:goto p 120`, `:goto chapter 7`.

### Annotations, highlights, bookmarks

* **Highlights:** select text, then press `h` (or pick a color with `1`–`5`
  from the theme's red/yellow/green/blue/magenta). Highlights are drawn by
  foliate-js's overlayer and recolored on theme change.
* **Notes:** `a` on a selection or a highlight opens the note editor overlay.
* **Bookmarks:** `m` toggles a bookmark on the current page; `'` lists them.
* **Annotations overlay:** everything grouped by chapter, filterable, Enter
  jumps to the item, `dd` deletes.
* **Export:** Markdown or JSON (`:export`), and through the CLI.

### Search

* `/` opens the search overlay. It searches the whole book in the
  background, streams results grouped by chapter with context, and shows hit
  markers in the text.
* `n`/`N` move to the next/previous hit and Esc clears.
* Options: case and whole words, plus diacritics-insensitive matching
  (foliate-js supports it).

### Lookup: dictionary, Wikipedia, translate

* Select a word or phrase, then press `d` (dictionary), `w` (Wikipedia) or
  `t` (translate). A lookup card overlay opens next to the selection, with
  tabs for each provider.
* Providers are defined in `config.toml`:

```toml
[[lookup.provider]]
name = "Wiktionary"; kind = "builtin.wiktionary"; key = "d"
[[lookup.provider]]
name = "Wikipedia";  kind = "builtin.wikipedia";  key = "w"; lang = "auto"
[[lookup.provider]]
name = "Translate";  kind = "builtin.google-translate"; key = "t"; pair = ["es", "en"]  # auto: es→en, otherwise →es
[[lookup.provider]]
name = "Local dict"; kind = "command"; key = "D"; command = ["sdcv", "-n", "--json", "{text}"]
[[lookup.provider]]
name = "Browser";    kind = "url"; key = "W"; url = "https://duckduckgo.com/?q={text}"
```

  Provider kinds are `builtin.*`, `command` (stdout as text or JSON), `url`
  (opens in the browser) and, later, `llm`.
* Results are cached in SQLite, and requests run off the UI thread with
  timeouts.

### Text-to-speech

* Rust talks to speech-dispatcher over SSIP, which reaches your piper module
  and voices.
* foliate-js's TTS module turns the page into SSML with `<mark>`s. Rust sends
  it in chunks, reads the index-mark events back, and the book highlights the
  sentence being spoken and turns pages automatically.
* Controls: `s` play/pause, `]s`/`[s` next/previous sentence, voice/rate/pitch
  in the TTS bar overlay, and start from the selection or from the top of the
  page.
* Stops cleanly on window close or book change, and survives the
  speech-dispatcher daemon restarting.

## Keybindings (default, remappable in config)

| Context | Keys |
|---|---|
| Reader | `j`/`l`/Space next page · `k`/`h`/Shift-Space prev · `J`/`K` next/prev chapter · `gg`/`G` start/end · `Ctrl-o`/`Ctrl-i` back/forward |
| Panels | `t` TOC · `/` search · `n`/`N` hits · `'` bookmarks · `A` annotations · `i` status line · `,` settings · `:` palette · `?` help · `q`/Esc close panel / back to library |
| Selection | `h` highlight · `1`–`5` color · `a` note · `y` copy · `d`/`w`/`t` lookup · `s` speak from here |
| Page | `m` bookmark · `+`/`-`/`=` font size · `[c`/`]c` columns · `T` cycle theme (system/light/dark) · `F` fullscreen |
| Library | `hjkl` move · Enter open · `/` filter · Tab shelves · `o` sort · `gr` rescan |

Later: a keyboard **visual mode** (`v`) to select text word-by-word and
sentence-by-sentence without the mouse.

## Storage

* `~/.config/omaread/config.toml`: settings, keymap and providers. Hot
  reloaded too, and every option is also in the settings overlay.
* `~/.local/share/omaread/omaread.db` (SQLite, WAL): books, shelves,
  progress, bookmarks, annotations, per-book overrides, reading stats, lookup
  cache. Schema migrations ship in the binary.
* `~/.cache/omaread/`: cover thumbnails.

## Performance and reliability targets

* Cold start to the library grid in under 200 ms (metadata comes from
  SQLite; covers load lazily in a virtualized grid).
* Open a book in under 400 ms; page turn in one frame (foliate-js translates
  columns and doesn't re-layout).
* Progress is saved on every page turn (debounced) and on close. A crash
  never loses more than the current page.
* Corrupt or DRM'd EPUBs are shown as such in the library and never take down
  the app. Every web view message is validated in Rust (serde).
* Tests:
  * Core logic is unit-tested: OPF parsing, tokens, speed model, providers,
    import.
  * Opening every EPUB in `~/Documents/Books` is a smoke test
    (`cargo run -- check-library`).

## Milestones

1. **Skeleton:** workspace, GTK window, WebKit + foliate-js over the URI
   scheme, open an EPUB from the CLI, page with `j`/`k`, Omarchy tokens with
   live reload.
2. **Library:** scan and cache, cover grid, shelves, filter and sort, single
   instance, `.desktop` + MIME so Flea opens EPUBs in Omaread.
3. **Navigation:** TOC overlay, status line (identifier/page/%), time left
   with the speed model, jump history, `:goto`, persisting progress.
4. **Typography and themes:** settings overlay, per-book overrides,
   light/dark/custom themes.
5. **Annotations:** highlights, notes, bookmarks, annotations overlay,
   export, Foliate import.
6. **Search** and the **command palette**.
7. **Lookups:** provider system, the three defaults, `command` and `url`
   kinds.
8. **TTS:** SSIP client, SSML marks, sentence highlight, TTS bar.
9. **Polish:** help overlay, keymap config, library smoke test,
   `bin/omaread-install` + PKGBUILD.

## Hooks for future AI features

* Every action is a named command in one registry. Keys, the palette, the
  selection menu and the CLI all call the same registry, so an AI action is
  just a new command.
* `llm` lookup provider kind: "explain this passage", "summarize chapter so
  far", "who is this character", fed with selection/section text and the
  book's metadata.
* Agent-ready CLI (like Omacharts):
  * `omaread list --json`, `omaread annotations <book> --json`,
    `omaread open <book> --at <cfi>`.
  * `omaread text <book> --section 3` gives clean chapter text for
    summaries and search.
