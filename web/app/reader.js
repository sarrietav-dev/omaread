// The page side of Omaread. foliate-js lays the book out; everything else
// (chrome, keys, storage) lives in Rust. Rust calls `globalThis.omaread.*`
// and hears back through the `omaread` message handler, one JSON object per
// message.

import { makeBook } from '../foliate-js/view.js'
import { Overlayer } from '../foliate-js/overlayer.js'
import * as CFI from '../foliate-js/epubcfi.js'
import { BODY_PARAGRAPH, markPublisherHeadings } from './typography.js'

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

const ttsClearHighlight = () => {
    for (const { doc } of view?.renderer?.getContents?.() ?? [])
        doc?.defaultView?.CSS?.highlights?.delete(TTS_HIGHLIGHT)
}

const ttsHighlight = range => {
    view.renderer.scrollToAnchor?.(range, true)
    const win = range.startContainer?.ownerDocument?.defaultView
    if (!win?.CSS?.highlights || !win.Highlight) return
    ttsClearHighlight()
    win.CSS.highlights.set(TTS_HIGHLIGHT, new win.Highlight(range))
}

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

// Into the next chapter; false at the end of the book.
const ttsNextSection = async () => {
    const r = view.renderer
    const before = r.getContents?.()[0]?.index
    await r.nextSection?.()
    if (r.getContents?.()[0]?.index === before) return false
    await ttsInit()
    return true
}

// Send the block's sentences, skipping blocks with nothing to say.
const ttsEmit = async ssml => {
    let parts = sentencesOf(ssml)
    for (let guard = 0; !parts.length && guard < 200; guard++) {
        const next = view.tts.next()
        if (next == null) {
            if (!await ttsNextSection()) return emit({ type: 'tts-end' })
            parts = sentencesOf(view.tts.start())
        } else parts = sentencesOf(next)
    }
    if (!parts.length) return emit({ type: 'tts-end' })
    emit({ type: 'tts-block', sentences: parts })
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
    return [`
    @namespace epub "http://www.idpf.org/2007/ops";
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
    ${BODY_PARAGRAPH}, li, blockquote, dd {
        line-height: ${prefs.line_height}${imp};
        text-align: ${prefs.justify ? 'justify' : 'start'}${imp};
        hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
        -webkit-hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
    }
    p[data-omaread-display="title"], p[data-omaread-display="number"] {
        text-align: center !important;
    }
    ${p ? `
    html {
        color-scheme: ${p.mode === 'dark' ? 'dark' : 'light'};
    }
    html, body {
        color: ${p.fg} !important;
        background: none !important;
    }
    body * {
        color: inherit !important;
        border-color: color-mix(in srgb, currentColor 35%, transparent) !important;
        background-color: transparent !important;
    }
    pre, pre *, code, kbd, samp, tt {
        background-color: ${p.surface} !important;
    }
    a:any-link, a:any-link * {
        color: ${p.link} !important;
        text-decoration-color: color-mix(in srgb, currentColor 40%, transparent);
        text-underline-offset: .12em;
    }
    ::selection { background: ${p.selection}; }
    ::highlight(${TTS_HIGHLIGHT}) { background-color: color-mix(in srgb, ${p.link} 30%, transparent); color: inherit; }
    svg, img { background-color: transparent !important; }
    ` : ''}
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
    view.addEventListener('external-link', e => {
        e.preventDefault()
        emit({ type: 'external-link', href: e.detail.href })
    })
    view.addEventListener('load', ({ detail: { doc, index } }) => {
        markPublisherHeadings(doc, book.toc)
        doc.addEventListener('pointerup', () => setTimeout(() => {
            const sel = doc.getSelection()
            if (!sel.rangeCount || sel.isCollapsed) {
                if (selection) emit({ type: 'selection-cleared' })
                selection = null
                return
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
            })
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
    await view.init({ lastLocation: pendingOpen.lastLocation, showTextStart: !pendingOpen.lastLocation })
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
    nextSection: () => view?.renderer?.nextSection?.(),
    prevSection: () => view?.renderer?.prevSection?.(),
    first: () => view?.renderer?.firstSection?.(),
    last: () => view?.renderer?.lastSection?.(),
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
    async ttsStart({ fromSelection = false } = {}) {
        await ttsInit()
        const range = fromSelection ? selection?.range : null
        const ssml = range ? view.tts.from(range) : view.tts.start()
        omaread.clearSelection()
        await ttsEmit(ssml)
    },
    async ttsNext() {
        await ttsInit()
        const ssml = view.tts.next()
        if (ssml == null) {
            if (!await ttsNextSection()) return emit({ type: 'tts-end' })
            return ttsEmit(view.tts.start())
        }
        await ttsEmit(ssml)
    },
    async ttsPrev() {
        await ttsInit()
        await ttsEmit(view.tts.prev() ?? view.tts.start())
    },
    ttsMark(mark) {
        if (mark != null) view?.tts?.setMark(mark)
    },
    ttsStop() {
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
    back: () => view?.history.back(),
    forward: () => view?.history.forward(),
}

emit({ type: 'ready' })
