// The page side of Omaread. foliate-js lays the book out; everything else
// (chrome, keys, storage) lives in Rust. Rust calls `globalThis.omaread.*`
// and hears back through the `omaread` message handler, one JSON object per
// message.

import { makeBook } from '../foliate-js/view.js'
import { Overlayer } from '../foliate-js/overlayer.js'
import * as CFI from '../foliate-js/epubcfi.js'
import { BODY_PARAGRAPH, markPublisherHeadings } from './typography.js'
import { colorCSS } from './colors.js'
import { installSentenceSegmenter } from './sentences.js'
import { readingOrder, textEdges } from './sections.js'
import { resolveDocumentViewportUnits } from './viewport-units.js'
import { isNoteRef, noteBlock, noteText } from './notes.js'
import { hintLabels, matching } from './hints.js'

const emit = msg => globalThis.webkit.messageHandlers.omaread.postMessage(JSON.stringify(msg))

const fileInput = document.getElementById('file')

let view = null
let palette = null
// Highlights by CFI ({ value, color }), and bookmark CFIs, as Rust last sent them.
let highlights = new Map()
let bookmarks = []
// The live selection: where it is, so it can be highlighted.
let selection = null
// Bumped to cancel a running search.
let searchToken = 0

// --- Text-to-speech -------------------------------------------------------
// Rust speaks one sentence at a time; the page says what the sentences are
// and shows which one is being read.

const TTS_HIGHLIGHT = 'omaread-tts'
let ttsSession = null
// One block is looked at ahead as soon as a block starts, so its first
// sentence can be synthesized while the current block plays. The peek walks
// back afterwards, so the iterator stays on the block being read.
let ttsAhead = null
// The sentence being read, kept so stopping and starting again resumes there
// instead of at the top of the page.
let ttsRange = null

const ttsClearHighlight = () => {
    for (const { doc } of view?.renderer?.getContents?.() ?? [])
        doc?.defaultView?.CSS?.highlights?.delete(TTS_HIGHLIGHT)
}

const ttsHighlight = range => {
    ttsRange = range.cloneRange()
    view.renderer.scrollToAnchor?.(range, true)
    const win = range.startContainer?.ownerDocument?.defaultView
    if (!win?.CSS?.highlights || !win.Highlight) return
    ttsClearHighlight()
    win.CSS.highlights.set(TTS_HIGHLIGHT, new win.Highlight(range))
}

// foliate-js speaks by these sentences: see sentences.js.
installSentenceSegmenter()

const ttsInit = () => view.initTTS('sentence', ttsHighlight)

// SSML with a <mark> before each sentence → [{ mark, text }].
const sentencesOf = ssml => {
    if (!ssml) return []
    const doc = new DOMParser().parseFromString(ssml, 'application/xml')
    if (doc.querySelector('parsererror')) return []
    const parts = []
    let current = null
    const walk = node => {
        for (const n of node.childNodes) {
            if (n.nodeType === 1 && n.localName === 'mark') {
                current = { mark: n.getAttribute('name'), text: '' }
                parts.push(current)
            } else if (n.nodeType === 3) {
                if (!current) parts.push(current = { mark: null, text: '' })
                current.text += n.nodeValue
            } else if (n.nodeType === 1) walk(n)
        }
    }
    walk(doc.documentElement)
    return parts.map(p => ({ mark: p.mark, text: p.text.replace(/\s+/g, ' ').trim() }))
        .filter(p => /[\p{L}\p{N}]/u.test(p.text))
}

// The last sentence read, if it belongs to the document on screen.
const ttsRangeHere = () =>
    Boolean(ttsRange?.startContainer?.isConnected) &&
    ttsRange.startContainer.ownerDocument === view.renderer?.getContents?.()[0]?.doc

// The range for a stored CFI, going there first when its section is not the
// one on screen.
const ttsRangeOfCfi = async (cfi, session) => {
    let resolved
    try {
        resolved = view.resolveCFI(cfi)
    } catch {
        return null
    }
    if (!resolved || typeof resolved.anchor !== 'function') return null
    const here = () => view.renderer.getContents().find(c => c.index === resolved.index)
    let contents = here()
    if (!contents) {
        try {
            await view.goTo(cfi)
        } catch {
            return null
        }
        if (session !== ttsSession) return null
        contents = here()
    }
    if (!contents) return null
    try {
        return resolved.anchor(contents.doc)
    } catch {
        return null
    }
}

// Start at the block that holds the top of the visible page. foliate's
// own `from` throws when nothing qualifies (a cover with no text, or the
// last page of a section), so this may come back empty.
const ttsFrom = range => {
    if (!range) return null
    const start = range.cloneRange()
    start.setEnd(start.startContainer, start.startOffset)
    try {
        return view.tts.from(start)
    } catch {
        return null
    }
}

// Sections in the order the TOC lists them; null when the book's TOC cannot
// say (then foliate-js's spine order is all there is). A book's spine can be
// wrong: this one file has chapter 5 between Part I and chapter 1.
const chapterOrder = () => {
    const { book } = view ?? {}
    if (!book?.sections) return null
    const order = readingOrder(book.toc, href => book.resolveHref?.(href), book.sections.length)
    // Sections marked linear="no" are not part of the reading sequence.
    return order?.filter(index => book.sections[index]?.linear !== 'no') ?? null
}

// Everything the reader can step through, in reading order.
const sectionsOf = () => {
    const sections = view?.book?.sections ?? []
    const order = chapterOrder() ?? sections.map((_, index) => index)
    return order.filter(index => sections[index]?.linear !== 'no')
}

// Where the text starts and ends, for opening a book and for `gg` / `G`.
const textEdgesHere = () => textEdges(view?.book?.toc, href => view?.book?.resolveHref?.(href), sectionsOf())

// One section forward (+1) or back (-1); false when there is nowhere to go.
const stepChapter = async dir => {
    const r = view?.renderer
    if (!r) return false
    const before = r.getContents?.()[0]?.index
    const order = chapterOrder()
    const at = order ? order.indexOf(before) : -1
    if (at < 0) await (dir > 0 ? r.nextSection?.() : r.prevSection?.())
    else {
        const index = order[at + dir]
        if (index == null) return false
        await r.goTo({ index })
    }
    const after = r.getContents?.()[0]?.index
    return after != null && after !== before
}

// Into the next chapter; false at the end of the book.
const ttsNextSection = async () => {
    if (!await stepChapter(1)) return false
    await ttsInit()
    return true
}

// Send the block's sentences, skipping blocks with nothing to say.
const ttsEmit = async (ssml, session) => {
    if (session !== ttsSession) return
    let parts = sentencesOf(ssml)
    for (let guard = 0; !parts.length && guard < 200; guard++) {
        const next = view.tts.next()
        if (next == null) {
            if (!await ttsNextSection()) {
                if (session === ttsSession) emit({ type: 'tts-end', session })
                return
            }
            if (session !== ttsSession) return
            parts = sentencesOf(view.tts.start())
        } else parts = sentencesOf(next)
    }
    if (session !== ttsSession) return
    if (!parts.length) return emit({ type: 'tts-end', session })
    emit({ type: 'tts-block', session, sentences: parts })
}
let pendingOpen = {}

// Mirrors ReaderPrefs in Rust; replaced wholesale by setPrefs.
let prefs = {
    font_family: null,
    font_size: 18,
    line_height: 1.5,
    justify: true,
    hyphenate: true,
    margin: 48,
    max_width: 720,
    columns: 0,
    flow: 'paginated',
    publisher: 'keep',
}

const cssString = s => `"${String(s).replace(/["\\\n]/g, '')}"`

const langOf = el => {
    for (; el; el = el.parentElement) {
        const lang = el.lang || el.getAttributeNS?.('http://www.w3.org/XML/1998/namespace', 'lang')
        if (lang) return lang
    }
}

// Two sheets: the first goes before the book's own CSS (defaults it may
// override), the second after it (what it may not).
const bookCSS = () => {
    const p = palette
    const family = prefs.font_family ? `${cssString(prefs.font_family)}, serif` : null
    const strong = prefs.publisher !== 'keep'
    const all = prefs.publisher === 'all'
    const imp = all ? ' !important' : ''
    const notCode = ':not(pre, pre *, code, kbd, samp, tt, var)'
    // Our alignment and spacing are defaults the book may override, unless
    // the reader asked for everything ("all"), where they come after its CSS.
    const bodyRule = `
    ${BODY_PARAGRAPH}, li, blockquote, dd {
        line-height: ${prefs.line_height}${imp};
        text-align: ${prefs.justify ? 'justify' : 'start'}${imp};
        hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
        -webkit-hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
    }`
    return [`
    @namespace epub "http://www.idpf.org/2007/ops";
    ${all ? '' : bodyRule}
    html {
        font-size: ${prefs.font_size}px;
        line-height: ${prefs.line_height};
        ${family ? `font-family: ${family};` : ''}
        hanging-punctuation: allow-end last;
        orphans: 2;
        widows: 2;
    }
    [align="left"] { text-align: left; }
    [align="right"] { text-align: right; }
    [align="center"] { text-align: center; }
    [align="justify"] { text-align: justify; }
    :is(hgroup, header) p { text-align: unset; hyphens: unset; }
    h1, h2, h3, h4, h5, h6, hgroup, th, p[data-omaread-display] { text-wrap: balance; }
    pre { white-space: pre-wrap !important; tab-size: 4; }
    aside[epub|type~="footnote"] { display: none; }
`, `
    ${strong ? `
    html { font-size: ${prefs.font_size}px !important; }
    body { font-size: 1rem !important; }
    ${family ? `body, body *${notCode} { font-family: ${family} !important; }` : ''}
    /* The chosen size wins on body text too, not just html/body: books that
       set their own per-element size (0.8em, 12pt, …) would otherwise shrink
       past it. Headings and captions keep their relative scale. */
    ${BODY_PARAGRAPH}, li, blockquote, dd, dt, td, th { font-size: 1rem !important; }
    ` : ''}
    ${all ? `${bodyRule}
    p[data-omaread-display="title"], p[data-omaread-display="number"] {
        text-align: center !important;
    }` : ''}
    ${p ? colorCSS(p, TTS_HIGHLIGHT) : ''}
`]
}

const redrawHighlights = () => {
    for (const a of highlights.values()) view?.addAnnotation(a).catch?.(() => {})
}

const applyPalette = () => {
    if (!palette) return
    redrawHighlights()
    const root = document.documentElement.style
    root.setProperty('--bg', palette.bg)
    root.setProperty('--fg', palette.fg)
    root.setProperty('--blend', palette.mode === 'dark' ? 'screen' : 'multiply')
    view?.renderer?.setStyles?.(bookCSS())
}

const applyLayout = () => {
    const r = view?.renderer
    if (!r) return
    const scrolled = prefs.flow === 'scrolled'
    r.setAttribute('flow', scrolled ? 'scrolled' : 'paginated')
    // In scrolled mode, glide page-to-page (j/k) instead of jumping. Leave
    // paginated turns instant so they stay one-frame and holding j is snappy.
    if (scrolled) r.setAttribute('animated', '')
    else r.removeAttribute('animated')
    r.setAttribute('gap', '6%')
    r.setAttribute('margin', `${prefs.margin}px`)
    r.setAttribute('max-inline-size', `${prefs.max_width}px`)
    r.setAttribute('max-column-count', prefs.columns || 2)
    r.setStyles?.(bookCSS())
}

const LINE_STYLES = ['underline', 'squiggly', 'strikethrough']
const HIGHLIGHT_NAMES = ['red', 'yellow', 'green', 'blue', 'magenta']

const colorOf = name => {
    const i = HIGHLIGHT_NAMES.indexOf(name)
    return palette?.highlights?.[i >= 0 ? i : 1] ?? 'yellow'
}

// Where a range is on screen, in the page's coordinates (iframes are
// transformed by the paginator, so map through the frame).
const rectOf = range => {
    const frame = range.startContainer?.ownerDocument?.defaultView?.frameElement
    const f = frame?.getBoundingClientRect() ?? { left: 0, top: 0 }
    const m = frame ? getComputedStyle(frame).transform.match(/matrix\((.+)\)/) : null
    const [sx, , , sy] = m?.[1]?.split(/\s*,\s*/).map(Number) ?? [1, 0, 0, 1]
    const rects = Array.from(range.getClientRects())
    if (!rects.length) return null
    const first = rects[0], last = rects.at(-1)
    const left = Math.min(...rects.map(r => r.left)) * sx + f.left
    const right = Math.max(...rects.map(r => r.right)) * sx + f.left
    return {
        x: Math.round(left), y: Math.round(first.top * sy + f.top),
        width: Math.round(right - left), height: Math.round((last.bottom - first.top) * sy),
    }
}

// Which bookmarks fall on the page in view.
const visibleBookmarks = cfi => {
    if (!cfi || !bookmarks.length) return []
    try {
        const start = CFI.collapse(cfi), end = CFI.collapse(cfi, true)
        return bookmarks.filter(b => CFI.compare(b, start) >= 0 && CFI.compare(b, end) <= 0)
    } catch { return [] }
}

// The paragraph a selection sits in (with the one before it when it's
// short), for lookups that read around the words.
const passageOf = range => {
    const blockOf = node => {
        let el = node?.nodeType === 1 ? node : node?.parentElement
        while (el && !/^(p|li|blockquote|dd|dt|h[1-6]|pre|td|figcaption)$/i.test(el.localName ?? '')
            && el.localName !== 'body') el = el.parentElement
        return el?.localName === 'body' ? null : el
    }
    const block = blockOf(range.startContainer)
    if (!block) return ''
    let text = block.textContent.replace(/\s+/g, ' ').trim()
    const before = block.previousElementSibling
    if (text.length < 400 && before) text = `${before.textContent.replace(/\s+/g, ' ').trim()}\n\n${text}`
    return text.slice(-2400)
}

// Tell Rust about the selection in `doc` (section `index`), or that there
// is none. The mouse and visual mode both end here.
const reportSelection = (doc, index) => {
    const sel = doc.getSelection()
    if (!sel.rangeCount || sel.isCollapsed) {
        if (selection) emit({ type: 'selection-cleared' })
        selection = null
        return false
    }
    const range = sel.getRangeAt(0)
    const cfi = view.getCFI(index, range)
    selection = { index, range, cfi }
    emit({
        type: 'selection',
        text: sel.toString(),
        cfi,
        lang: langOf(range.commonAncestorContainer?.parentElement ?? range.commonAncestorContainer) ?? null,
        chapter: view.getProgressOf(index, range)?.tocItem?.label?.trim() ?? '',
        fraction: view.getSectionFractions()[index] ?? 0,
        rect: rectOf(range),
        context: passageOf(range),
    })
    return true
}

// --- Visual mode ------------------------------------------------------------
// Selecting from the keyboard: start on the first word of the page, then
// grow or shrink the selection by characters, words, sentences, lines or
// paragraphs (the browser's own Selection.modify), from either end.

let visual = null // { doc, index }

const visualSelection = () => visual?.doc?.getSelection()

// Keep the moving end on screen: turn the page when it walks off it.
const visualFollow = () => {
    const sel = visualSelection()
    if (!sel?.rangeCount) return
    const focus = visual.doc.createRange()
    focus.setStart(sel.focusNode, sel.focusOffset)
    const rect = rectOf(focus) ?? (() => {
        // A collapsed range can have no rects; measure its node instead.
        const r = visual.doc.createRange()
        r.selectNodeContents(sel.focusNode.nodeType === 1 ? sel.focusNode : sel.focusNode.parentNode)
        return rectOf(r)
    })()
    if (!rect) return
    if (rect.x < 0 || rect.x > innerWidth || rect.y < 0 || rect.y > innerHeight)
        view.renderer.scrollToAnchor?.(focus)
}

const visualText = () => {
    const text = visualSelection()?.toString() ?? ''
    return text.replace(/\s+/g, ' ').trim()
}

const visualReport = () => emit({ type: 'visual', active: Boolean(visual), text: visual ? visualText() : '' })

// Read the note a link points at and hand it to Rust; jump there instead
// when it can't be read.
const showNote = async (book, href, a) => {
    try {
        const target = book.resolveHref(href)
        const doc = target ? await book.sections[target.index]?.createDocument?.() : null
        const anchor = doc ? target.anchor(doc) : null
        const text = noteText(noteBlock(anchor?.nodeType === 1 ? anchor : null))
        if (!text) return view.goTo(href)
        const range = a.ownerDocument.createRange()
        range.selectNode(a)
        emit({ type: 'footnote', href, text, rect: rectOf(range) })
    } catch (e) {
        console.error('footnote', e)
        view.goTo(href)
    }
}

// --- Link hints -------------------------------------------------------------
// `f` labels every link on screen; typing a label follows it, through the
// same click handler the mouse uses (notes open in their card, links out
// go to the browser).

let hints = null // { targets: [{ label, el, mark }], typed }

// What can be hinted, in reading order, with where it is on screen.
const hintTargets = () => {
    const found = []
    for (const { doc } of view?.renderer?.getContents?.() ?? []) {
        for (const el of doc.querySelectorAll('a[href]')) {
            const range = doc.createRange()
            range.selectNodeContents(el)
            const rect = rectOf(range)
            if (!rect || rect.width <= 0 || rect.height <= 0) continue
            if (rect.x + rect.width < 0 || rect.x > innerWidth || rect.y + rect.height < 0 || rect.y > innerHeight) continue
            found.push({ el, rect })
        }
    }
    return found
}

const hintsClear = () => {
    for (const t of hints?.targets ?? []) t.mark.remove()
    hints = null
}

const hintsReport = () => emit({ type: 'hints', active: Boolean(hints) })

const hintsShow = () => {
    for (const t of hints.targets) {
        const on = t.label.startsWith(hints.typed)
        t.mark.style.display = on ? '' : 'none'
        t.mark.textContent = t.label
        t.mark.dataset.typed = hints.typed.length
    }
}

const metadataOf = book => {
    const text = x => !x ? '' : typeof x === 'string' ? x : x[Object.keys(x)[0]] ?? ''
    const names = x => [x ?? []].flat().map(a => text(typeof a === 'string' ? a : a?.name)).filter(Boolean)
    const m = book.metadata ?? {}
    return {
        title: text(m.title),
        authors: names(m.author),
        language: [m.language ?? []].flat()[0] ?? null,
        identifier: m.identifier ?? null,
    }
}

// Just what the TOC panel needs, and nothing that can't cross to Rust.
const tocOf = (items, depth = 0) => depth > 8 || !Array.isArray(items) ? [] : items
    .filter(item => item && typeof item === 'object')
    .map(item => ({
        label: String(item.label ?? '').trim().replace(/\s+/g, ' ') || '…',
        href: typeof item.href === 'string' ? item.href : null,
        children: tocOf(item.subitems, depth + 1),
    }))

const open = async file => {
    view?.close()
    view?.remove()
    const book = await makeBook(file)
    view = document.createElement('foliate-view')
    document.body.append(view)

    view.addEventListener('relocate', e => {
        const { fraction, section, location, time, tocItem, pageItem, cfi } = e.detail
        emit({
            type: 'relocate',
            fraction,
            cfi,
            section: section ?? null,
            location: location ?? null,
            time: time ?? null,
            tocLabel: tocItem?.label?.trim() ?? null,
            tocHref: tocItem?.href ?? null,
            bookmarks: visibleBookmarks(cfi),
            pageStart: (() => { try { return cfi ? CFI.collapse(cfi) : null } catch { return null } })(),
            pageLabel: pageItem?.label ?? null,
        })
    })
    // A link to a footnote shows the note in a card instead of jumping.
    view.addEventListener('link', e => {
        const { a, href } = e.detail
        const styleOf = el => a.ownerDocument?.defaultView?.getComputedStyle(el)
        if (!isNoteRef(a, styleOf)) return
        e.preventDefault()
        showNote(book, href, a)
    })
    view.addEventListener('external-link', e => {
        e.preventDefault()
        emit({ type: 'external-link', href: e.detail.href })
    })
    view.addEventListener('load', ({ detail: { doc, index } }) => {
        // Before the first layout: in scrolled flow a vh-sized element would
        // otherwise grow the iframe that defines the vh (endless blank page).
        resolveDocumentViewportUnits(doc, innerWidth, innerHeight)
        markPublisherHeadings(doc, book.toc)
        doc.addEventListener('pointerup', () => setTimeout(() => {
            if (visual) return
            reportSelection(doc, index)
        }))
    })
    view.addEventListener('create-overlay', ({ detail: { index } }) => {
        for (const a of highlights.values()) {
            try {
                if (view.resolveCFI(a.value).index === index) view.addAnnotation(a)
            } catch {}
        }
    })
    view.addEventListener('draw-annotation', ({ detail: { draw, annotation, doc, range } }) => {
        if (LINE_STYLES.includes(annotation.color)) {
            const node = range.startContainer
            const el = node.nodeType === 1 ? node : node.parentElement
            draw(Overlayer[annotation.color], { writingMode: doc.defaultView.getComputedStyle(el).writingMode })
        } else draw(Overlayer.highlight, { color: colorOf(annotation.color) })
    })
    view.addEventListener('show-annotation', ({ detail: { value, range } }) => {
        emit({ type: 'annotation-click', cfi: value, rect: rectOf(range) })
    })

    await view.open(book)
    applyLayout()
    // With no saved position, open where the text starts instead of on the
    // cover or the title page.
    await view.init({ lastLocation: pendingOpen.lastLocation ?? textEdgesHere().start, showTextStart: false })
    emit({
        type: 'book-ready',
        metadata: metadataOf(book),
        pageTotal: book.pageList?.findLast(x => !isNaN(parseInt(x.label)))?.label ?? null,
        toc: tocOf(book.toc),
    })
}

fileInput.addEventListener('change', () => {
    const file = fileInput.files[0]
    fileInput.value = ''
    if (!file) return
    open(file).catch(e => emit({ type: 'book-error', message: String(e?.message ?? e), stack: e?.stack ?? '' }))
})

globalThis.omaread = {
    // Asks the app for the file through the page's file picker, which Rust
    // answers with the path. WebKit then reads it lazily from disk.
    open(opts = {}) {
        pendingOpen = opts
        fileInput.click()
    },
    setPalette(p) {
        palette = p
        applyPalette()
    },
    setPrefs(p) {
        prefs = { ...prefs, ...p }
        applyLayout()
    },
    next: () => view?.next(),
    prev: () => view?.prev(),
    nextSection: () => stepChapter(1),
    prevSection: () => stepChapter(-1),
    first: () => view?.renderer?.goTo?.({ index: textEdgesHere().start }),
    last: () => view?.renderer?.goTo?.({ index: textEdgesHere().end, anchor: 1 }),
    goTo: target => view?.goTo(target),
    setHighlights(list) {
        for (const a of highlights.values()) view?.deleteAnnotation(a)
        highlights = new Map(list.map(a => [a.cfi, { value: a.cfi, color: a.color }]))
        redrawHighlights()
    },
    addHighlight({ cfi, color }) {
        const a = { value: cfi, color }
        highlights.set(cfi, a)
        view?.addAnnotation(a)
        omaread.clearSelection()
    },
    removeHighlight(cfi) {
        const a = highlights.get(cfi)
        highlights.delete(cfi)
        if (a) view?.deleteAnnotation(a)
    },
    setBookmarks(list) {
        bookmarks = list
    },
    clearSelection() {
        selection?.range?.startContainer?.ownerDocument?.getSelection()?.removeAllRanges()
        selection = null
    },
    async ttsStart({ fromSelection = false, session, resumeCfi = null } = {}) {
        ttsSession = session
        ttsAhead = null
        try {
            // After a stop, resume at the sentence last read. A fresh open has
            // no such position, so it starts at the saved reading location. Do
            // this before initTTS: finding the CFI may move to another section.
            let range = fromSelection ? selection?.range : null
            if (!range && resumeCfi) range = await ttsRangeOfCfi(resumeCfi, session)
            if (session !== ttsSession) return
            await ttsInit()
            if (session !== ttsSession) return
            if (!range && !fromSelection && ttsRangeHere()) range = ttsRange
            if (!range) range = view.lastLocation?.range
            // A cover or an image-only page has nowhere to start: fall back to
            // the section's first block, and ttsEmit will walk on from there.
            const ssml = ttsFrom(range) ?? view.tts.start()
            omaread.clearSelection()
            await ttsEmit(ssml, session)
        } catch (e) {
            // Never leave the player waiting for words that aren't coming.
            if (session === ttsSession) {
                console.error('ttsStart', e)
                emit({ type: 'tts-end', session })
            }
        }
    },
    // Rust has started reading the block it peeked; make that block current,
    // which also gives it back its sentence ranges for the highlight.
    ttsConsumeAhead() {
        if (!ttsAhead) return
        ttsAhead = null
        view.tts.next()
    },
    ttsPeek() {
        const session = ttsSession
        if (session == null || ttsAhead) return
        // Skip blocks with nothing to say, but never cross into the next
        // section here: that would move the page before it is being read.
        let ssml = null
        let steps = 0
        for (; steps < 200; steps++) {
            const next = view.tts.next()
            if (next == null) break
            ssml = next
            if (sentencesOf(next).length) {
                steps++
                break
            }
        }
        // foliate-js keeps the ranges for the block it built last. Walking
        // back to the block being read keeps its marks highlightable; the
        // peeked block is rebuilt when it is consumed.
        for (let i = 0; i < steps; i++) view.tts.prev()
        if (session !== ttsSession || ssml == null) return
        const sentences = sentencesOf(ssml)
        if (!sentences.length) return
        ttsAhead = sentences
        emit({ type: 'tts-ahead', session, sentences })
    },
    async ttsNext() {
        const session = ttsSession
        if (session == null) return
        ttsAhead = null
        await ttsInit()
        if (session !== ttsSession) return
        const ssml = view.tts.next()
        if (ssml == null) {
            if (!await ttsNextSection()) {
                if (session === ttsSession) emit({ type: 'tts-end', session })
                return
            }
            if (session !== ttsSession) return
            return ttsEmit(view.tts.start(), session)
        }
        await ttsEmit(ssml, session)
    },
    async ttsPrev() {
        const session = ttsSession
        if (session == null) return
        ttsAhead = null
        await ttsInit()
        if (session !== ttsSession) return
        await ttsEmit(view.tts.prev() ?? view.tts.start(), session)
    },
    ttsMark(mark) {
        if (mark == null) return
        view?.tts?.setMark(mark)
        // Tell Rust where the sentence is, so a stop can resume here.
        if (!ttsRange?.startContainer?.isConnected) return
        const index = view.renderer.getContents().find(c => c.doc === ttsRange.startContainer.ownerDocument)?.index
        if (index == null) return
        emit({ type: 'tts-position', cfi: view.getCFI(index, ttsRange) })
    },
    ttsStop() {
        ttsSession = null
        ttsAhead = null
        ttsClearHighlight()
    },
    // Search the whole book, streaming results by chapter. A new search (or
    // clearSearch) cancels the one running.
    search({ query, matchCase = false, matchWholeWords = false, matchDiacritics = false }) {
        const token = ++searchToken
        view?.clearSearch()
        if (!view || !query?.trim()) return
        ;(async () => {
            let count = 0, lastProgress = 0
            const opts = { query, matchCase, matchWholeWords, matchDiacritics, drawOptions: { color: palette?.link } }
            for await (const r of view.search(opts)) {
                if (token !== searchToken) return
                if (r === 'done') break
                if (r.progress != null) {
                    if (r.progress - lastProgress >= 0.05) {
                        lastProgress = r.progress
                        emit({ type: 'search-progress', progress: r.progress })
                    }
                    continue
                }
                const items = (r.subitems ?? []).map(({ cfi, excerpt }) => ({
                    cfi, pre: excerpt?.pre ?? '', hit: excerpt?.match ?? '', post: excerpt?.post ?? '',
                }))
                count += items.length
                if (items.length) emit({ type: 'search-results', chapter: r.label?.trim() ?? '', items })
            }
            if (token === searchToken) emit({ type: 'search-done', count })
        })().catch(e => emit({ type: 'search-done', count: 0, error: String(e?.message ?? e) }))
    },
    clearSearch() {
        searchToken++
        view?.clearSearch()
    },
    // For tests: select the n-th paragraph on the page in view, as a mouse would.
    testSelect(n = 0, words = 6) {
        const { doc } = view.renderer.getContents()[0]
        const p = Array.from(doc.querySelectorAll('p')).filter(p => {
            const r = p.getBoundingClientRect()
            return p.textContent.trim().split(/\s+/).length > words && r.width > 0
        })[n]
        if (!p) return false
        const walker = doc.createTreeWalker(p, NodeFilter.SHOW_TEXT)
        const node = walker.nextNode()
        const range = doc.createRange()
        range.setStart(node, 0)
        range.setEnd(node, Math.min(node.length, node.textContent.split(' ').slice(0, words).join(' ').length))
        doc.getSelection().removeAllRanges()
        doc.getSelection().addRange(range)
        doc.dispatchEvent(new PointerEvent('pointerup'))
        return true
    },
    // Chapter and position for CFIs stored without them (imports).
    async locate(cfis) {
        const fractions = view?.getSectionFractions() ?? []
        const items = []
        for (const cfi of cfis) {
            try {
                const { index } = view.resolveCFI(cfi)
                const toc = await view.getTOCItemOf(cfi)
                items.push({ cfi, chapter: toc?.label?.trim() ?? '', fraction: fractions[index] ?? 0 })
            } catch {}
        }
        emit({ type: 'located', items })
    },
    goToPage: label => {
        const item = view?.book.pageList?.find(p => String(p.label).trim() === String(label))
        if (item) return view.goTo(item.href)
        emit({ type: 'notice', message: view?.book.pageList?.length ? `No page ${label} in this book` : 'This book has no page numbers; try a percentage' })
    },
    goToFraction: f => view?.goToFraction(f),
    hintsStart() {
        hintsClear()
        const found = hintTargets()
        if (!found.length) {
            emit({ type: 'notice', message: 'No links on this page' })
            return hintsReport()
        }
        const labels = hintLabels(found.length)
        // Solid colours: with Glassy Windows the page background is transparent.
        const fg = palette?.surface ?? '#ffffff', bg = palette?.fg ?? '#101315'
        const targets = found.map(({ el, rect }, i) => {
            const mark = document.createElement('div')
            mark.className = 'omaread-hint'
            Object.assign(mark.style, {
                position: 'fixed', left: `${Math.max(0, rect.x - 4)}px`, top: `${Math.max(0, rect.y - 4)}px`,
                zIndex: 10, padding: '0 3px', font: 'bold 12px monospace', lineHeight: '16px',
                color: fg, background: bg, pointerEvents: 'none', textTransform: 'lowercase',
            })
            document.body.append(mark)
            return { label: labels[i], el, mark }
        })
        hints = { targets, typed: '' }
        hintsShow()
        hintsReport()
    },
    hintsKey(key) {
        if (!hints) return
        const typed = hints.typed + String(key).toLowerCase()
        const left = matching(hints.targets, typed)
        if (!left.length) {
            hintsClear()
            return hintsReport()
        }
        if (left.length === 1 && left[0].label === typed) {
            const { el } = left[0]
            hintsClear()
            hintsReport()
            el.click()
            return
        }
        hints.typed = typed
        hintsShow()
    },
    hintsCancel() {
        hintsClear()
        hintsReport()
    },
    visualStart() {
        const range = view?.lastLocation?.range
        const doc = range?.startContainer?.ownerDocument
        const index = view?.renderer?.getContents?.().find(c => c.doc === doc)?.index
        if (!doc || index == null) return emit({ type: 'notice', message: 'Nothing on this page to select' })
        const sel = doc.getSelection()
        sel.removeAllRanges()
        const start = range.cloneRange()
        start.collapse(true)
        sel.addRange(start)
        // From the start of the page to the end of its first word.
        sel.modify('move', 'forward', 'word')
        sel.modify('move', 'backward', 'word')
        sel.modify('extend', 'forward', 'word')
        visual = { doc, index }
        visualReport()
    },
    visualMove({ unit, forward }) {
        const sel = visualSelection()
        if (!sel?.rangeCount) return
        sel.modify('extend', forward ? 'forward' : 'backward', unit)
        visualFollow()
        visualReport()
    },
    // The other end of the selection becomes the one that moves.
    visualSwap() {
        const sel = visualSelection()
        if (!sel?.rangeCount) return
        sel.setBaseAndExtent(sel.focusNode, sel.focusOffset, sel.anchorNode, sel.anchorOffset)
        visualFollow()
        visualReport()
    },
    // Done: the selection becomes a normal one, with its card.
    visualDone() {
        if (!visual) return
        const { doc, index } = visual
        visual = null
        if (!reportSelection(doc, index)) visualReport()
    },
    visualCancel() {
        visualSelection()?.removeAllRanges()
        visual = null
        visualReport()
    },
    back: () => view?.history.back(),
    forward: () => view?.history.forward(),
}

emit({ type: 'ready' })
