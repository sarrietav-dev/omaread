import { test } from 'node:test'
import assert from 'node:assert/strict'
import { resolveViewportUnits, resolveDocumentViewportUnits } from './viewport-units.js'

test('vh and vw become pixels of the reader', () => {
    assert.equal(resolveViewportUnits('img{height:99vh}', 1000, 800), 'img{height:792px}')
    assert.equal(resolveViewportUnits('img{width:50vw}', 1000, 800), 'img{width:500px}')
    assert.equal(resolveViewportUnits('p{margin:1.5vh 0}', 1000, 100), 'p{margin:1.5px 0}')
})

test('other units and plain values are untouched', () => {
    const css = 'p{margin:1em;width:100%;padding:2px;line-height:1.5}'
    assert.equal(resolveViewportUnits(css, 1000, 800), css)
    assert.equal(resolveViewportUnits('p{width:100vmax}', 1000, 800), 'p{width:100vmax}')
})

test('resolves units in style blocks and style attributes', () => {
    const style = { textContent: '@page{margin:1.5em}#c img{height:99vh}' }
    const inline = { value: 'height:50vh;width:10vw', getAttribute() { return this.value }, setAttribute(_, v) { this.value = v } }
    const doc = { querySelectorAll: sel => sel === 'style' ? [style] : [inline] }
    resolveDocumentViewportUnits(doc, 900, 400)
    assert.equal(style.textContent, '@page{margin:1.5em}#c img{height:396px}')
    assert.equal(inline.value, 'height:200px;width:90px')
})
