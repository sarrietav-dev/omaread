// Viewport units inside the book's own inline <style> blocks and style=""
// attributes. foliate-js rewrites vh/vw only in external stylesheets, so a
// book that sizes something with them (a 99vh figure image, say) keeps them.
// In scrolled flow that feeds back into the iframe: the element grows with the
// viewport, the viewport grows to fit the content, and the reader ends up with
// tens of thousands of pixels of empty page. Resolve them against the reader
// before the first layout, as foliate-js does for its stylesheets.

const VIEWPORT_UNIT = /(\d*\.?\d+)(vh|vw)/gi

export const resolveViewportUnits = (css, width, height) =>
    String(css).replace(VIEWPORT_UNIT, (_, n, unit) =>
        `${parseFloat(n) * (unit.toLowerCase() === 'vh' ? height : width) / 100}px`)

export const resolveDocumentViewportUnits = (doc, width, height) => {
    for (const el of doc.querySelectorAll('style')) {
        if (el.textContent) el.textContent = resolveViewportUnits(el.textContent, width, height)
    }
    for (const el of doc.querySelectorAll('[style]')) {
        const css = el.getAttribute('style')
        if (css) el.setAttribute('style', resolveViewportUnits(css, width, height))
    }
}
