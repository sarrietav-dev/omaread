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

const layout = {
    flow: 'paginated',
    gap: 0.06,
    margin: 48,
    maxInlineSize: 720,
    maxColumnCount: 2,
}

const style = {
    lineHeight: 1.5,
    justify: true,
    hyphenate: true,
}

const langOf = el => {
    for (; el; el = el.parentElement) {
        const lang = el.lang || el.getAttributeNS?.('http://www.w3.org/XML/1998/namespace', 'lang')
        if (lang) return lang
    }
}

// Two sheets: the first goes before the book's own CSS (defaults it may
// override), the second after it (the theme, which it may not).
const bookCSS = () => {
    const p = palette
    return [`
    @namespace epub "http://www.idpf.org/2007/ops";
    html {
        line-height: ${style.lineHeight};
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
`, p ? `
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
    p, li, blockquote, dd {
        line-height: ${style.lineHeight};
        text-align: ${style.justify ? 'justify' : 'start'};
        hyphens: ${style.hyphenate ? 'auto' : 'none'};
    }
` : '']
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
    r.setAttribute('flow', layout.flow)
    r.setAttribute('gap', `${layout.gap * 100}%`)
    r.setAttribute('margin', `${layout.margin}px`)
    r.setAttribute('max-inline-size', `${layout.maxInlineSize}px`)
    r.setAttribute('max-column-count', layout.maxColumnCount)
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
    emit({ type: 'book-ready', metadata: metadataOf(book), pageTotal: book.pageList?.findLast(x => !isNaN(parseInt(x.label)))?.label ?? null })
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
    setLayout(l) {
        Object.assign(layout, l)
        applyLayout()
    },
    setStyle(s) {
        Object.assign(style, s)
        view?.renderer?.setStyles?.(bookCSS())
    },
    next: () => view?.next(),
    prev: () => view?.prev(),
    nextSection: () => view?.renderer?.nextSection?.(),
    prevSection: () => view?.renderer?.prevSection?.(),
    first: () => view?.renderer?.firstSection?.(),
    last: () => view?.renderer?.lastSection?.(),
    goTo: target => view?.goTo(target),
    goToFraction: f => view?.goToFraction(f),
    back: () => view?.history.back(),
    forward: () => view?.history.forward(),
}

emit({ type: 'ready' })
