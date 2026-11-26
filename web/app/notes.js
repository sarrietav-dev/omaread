// Footnotes and endnotes: which links point at one, and the note's text,
// so it can be shown in a card instead of jumping away from the page.
// The rules follow foliate-js's footnotes.js (not exported there).

const OPS = 'http://www.idpf.org/2007/ops'
const words = s => new Set(String(s ?? '').split(/\s+/).filter(Boolean))
const typesOf = el => words(el?.getAttributeNS?.(OPS, 'type') ?? el?.getAttribute?.('epub:type'))
const rolesOf = el => words(el?.getAttribute?.('role'))

const REF_TYPES = ['noteref', 'biblioref', 'glossref']
const REF_ROLES = ['doc-noteref', 'doc-biblioref', 'doc-glossref']

// Raised like a note number: <sup>, or styled up.
const isSuper = (el, styleOf) => {
    if (!el) return false
    if (el.localName === 'sup') return true
    const align = styleOf?.(el)?.verticalAlign ?? ''
    return align === 'super' || align === 'top' || align === 'text-top' || /^\d/.test(align)
}

// A link to a note: marked as one, or a raised number that isn't a backlink.
export const isNoteRef = (a, styleOf) => {
    const types = typesOf(a), roles = rolesOf(a)
    if (REF_TYPES.some(t => types.has(t)) || REF_ROLES.some(r => roles.has(r))) return true
    if (types.has('backlink') || roles.has('doc-backlink')) return false
    const text = String(a?.textContent ?? '').trim()
    // Raised, and short like a marker ("12", "*", "†", "a"); not a raised sentence.
    if (!text || text.length > 4) return false
    return isSuper(a, styleOf)
        || (a.children?.length === 1 && isSuper(a.children[0], styleOf))
        || isSuper(a.parentElement, styleOf)
}

const INLINE = new Set(['a', 'span', 'sup', 'sub', 'em', 'strong', 'i', 'b', 'small', 'big'])

// The note around the anchor a link points at: the anchor itself when it's a
// block (aside, li, p), else the block holding it.
export const noteBlock = el => {
    let node = el
    while (node && INLINE.has(node.localName)) {
        if (!node.parentElement) break
        node = node.parentElement
    }
    if (!node || node.localName === 'body') {
        const next = el?.nextElementSibling
        return next && !INLINE.has(next.localName) ? next : null
    }
    return node
}

// The note as plain text, without the arrow that links back.
export const noteText = el => String(el?.textContent ?? '')
    .replace(/\s+/g, ' ')
    .replace(/\s*[↩↑⤴︎]+\s*$/u, '')
    .trim()
    .slice(0, 4000)
