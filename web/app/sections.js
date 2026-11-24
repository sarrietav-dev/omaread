// Reading order for chapter navigation.
//
// foliate-js steps through the spine, but a book's spine can disagree with its
// table of contents: a converted EPUB may put chapter 5's file between Part I
// and chapter 1 while the NCX has the right sequence. The TOC is what the
// reader sees, so J/L and text-to-speech walk the TOC order instead.
//
// Sections the TOC never names (cover pages, part dividers, untitled front
// matter) stay where the spine put them: they ride along with the section
// before them, so no page becomes unreachable.

// toc: foliate's nested TOC ({ label, href, subitems }).
// resolveHref: href → { index } for the book, or null.
// count: number of spine sections.
// Returns spine indices in reading order, or null when the TOC cannot order
// the book (no TOC, or every entry points at the same section).
export const readingOrder = (toc, resolveHref, count) => {
    if (!count || typeof resolveHref !== 'function') return null
    const position = new Map()
    const walk = items => {
        for (const item of items ?? []) {
            const index = item?.href ? resolveHref(item.href)?.index : undefined
            if (Number.isInteger(index) && index >= 0 && index < count && !position.has(index))
                position.set(index, position.size)
            walk(item?.subitems)
        }
    }
    walk(toc)
    if (position.size < 2) return null

    // Rows in TOC order, each carrying the unlisted sections that sit before
    // it in the spine. Anything before the first TOC entry opens the book.
    const leading = []
    const rows = []
    let unlisted = leading
    for (let index = 0; index < count; index++) {
        if (position.has(index)) {
            rows.push({ index, before: unlisted })
            unlisted = []
        } else unlisted.push(index)
    }
    rows.sort((a, b) => position.get(a.index) - position.get(b.index))

    const order = []
    for (const { index, before } of rows) order.push(...before, index)
    // Trailing sections after the last TOC entry close the book.
    order.push(...unlisted)
    return order
}
