// The page side of Omaread. foliate-js lays the book out; everything else
// (chrome, keys, storage) lives in Rust. Rust calls `globalThis.omaread.*`
// and hears back through the `omaread` message handler, one JSON object per
// message.

import { makeBook } from '../foliate-js/view.js'

const emit = msg => globalThis.webkit.messageHandlers.omaread.postMessage(JSON.stringify(msg))

const fileInput = document.getElementById('file')

let view = null
let palette = null
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
    h1, h2, h3, h4, h5, h6, hgroup, th { text-wrap: balance; }
    pre { white-space: pre-wrap !important; tab-size: 4; }
    aside[epub|type~="footnote"] { display: none; }
`, `
    ${strong ? `
    html { font-size: ${prefs.font_size}px !important; }
    body { font-size: 1rem !important; }
    ${family ? `body, body *${notCode} { font-family: ${family} !important; }` : ''}
    ` : ''}
    p, li, blockquote, dd {
        line-height: ${prefs.line_height}${imp};
        text-align: ${prefs.justify ? 'justify' : 'start'}${imp};
        hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
        -webkit-hyphens: ${prefs.hyphenate ? 'auto' : 'manual'}${imp};
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
    svg, img { background-color: transparent !important; }
    ` : ''}
`]
}

const applyPalette = () => {
    if (!palette) return
    const root = document.documentElement.style
    root.setProperty('--bg', palette.bg)
    root.setProperty('--fg', palette.fg)
    root.setProperty('--blend', palette.mode === 'dark' ? 'screen' : 'multiply')
    view?.renderer?.setStyles?.(bookCSS())
}

const applyLayout = () => {
    const r = view?.renderer
    if (!r) return
    r.setAttribute('flow', prefs.flow === 'scrolled' ? 'scrolled' : 'paginated')
    r.setAttribute('gap', '6%')
    r.setAttribute('margin', `${prefs.margin}px`)
    r.setAttribute('max-inline-size', `${prefs.max_width}px`)
    r.setAttribute('max-column-count', prefs.columns || 2)
    r.setStyles?.(bookCSS())
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
            pageLabel: pageItem?.label ?? null,
        })
    })
    view.addEventListener('external-link', e => {
        e.preventDefault()
        emit({ type: 'external-link', href: e.detail.href })
    })
    view.addEventListener('load', ({ detail: { doc } }) => {
        doc.addEventListener('pointerup', () => {
            const sel = doc.getSelection()
            if (!sel.rangeCount || sel.isCollapsed) return
            emit({ type: 'selection', text: sel.toString(), lang: langOf(sel.anchorNode?.parentElement) ?? null })
        })
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
