// Some EPUBs use paragraphs for chapter titles. Identify them by the book's
// table of contents, never by inherited alignment or font size.
export const BODY_PARAGRAPH = 'p:not([data-omaread-display])'

const normalize = text => String(text ?? '').replace(/\s+/g, ' ').trim().toLocaleLowerCase()
const withoutNumber = text => text.replace(/^(?:(?:chapter|part)\s+)?(?:\d+|[ivxlcdm]+)[.\s:–—-]+/i, '')
const chapterNumber = /^(?:(?:chapter|part)\s+)?(?:\d+|[ivxlcdm]+)[.]?$/i

export const markPublisherHeadings = (doc, toc = []) => {
    const titles = new Set()
    const collect = items => {
        for (const item of items) {
            const label = normalize(item.label)
            if (label) {
                titles.add(label)
                titles.add(withoutNumber(label))
            }
            collect(item.subitems ?? [])
        }
    }
    collect(toc)
    const paragraphs = Array.from(doc.querySelectorAll('p'))
    for (const p of paragraphs) p.removeAttribute('data-omaread-display')
    for (const p of paragraphs) {
        if (p.closest('h1, h2, h3, h4, h5, h6, hgroup, [role="heading"]'))
            p.setAttribute('data-omaread-display', 'heading')
        const text = normalize(p.textContent)
        if (!text || !titles.has(text)) continue
        p.setAttribute('data-omaread-display', 'title')
        const previous = p.previousElementSibling
        if (previous?.localName === 'p' && chapterNumber.test(normalize(previous.textContent)))
            previous.setAttribute('data-omaread-display', 'number')
    }
}
