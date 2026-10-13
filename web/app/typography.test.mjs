import { test } from 'node:test'
import assert from 'node:assert/strict'
import { BODY_PARAGRAPH, markPublisherHeadings } from './typography.js'

const paragraph = (textContent, heading = false) => ({
    textContent, localName: 'p',
    closest: () => heading,
    setAttribute(name, value) { this[name] = value },
    removeAttribute(name) { delete this[name] },
})
const documentOf = paragraphs => {
    paragraphs.forEach((p, i) => { p.previousElementSibling = paragraphs[i - 1] })
    return { querySelectorAll: () => paragraphs }
}

test('only the TOC chapter title and its adjacent number escape body overrides', () => {
    const paragraphs = [paragraph('1'), paragraph('Learning Is Misunderstood'),
        paragraph('EARLY IN HIS CAREER as a pilot, Matt Brown was flying.'),
        paragraph('He reduced altitude and kept an eye on the oil gauge.')]
    markPublisherHeadings(documentOf(paragraphs), [{ label: '1. Learning Is Misunderstood' }])
    assert.deepEqual(paragraphs.map(p => p['data-omaread-display']),
        ['number', 'title', undefined, undefined])
    assert.equal(BODY_PARAGRAPH, 'p:not([data-omaread-display])')
})

test('inherited centering, large fonts, and a header wrapper cannot mark body text as headings', () => {
    const p = paragraph('Ordinary body text.')
    p.style = { textAlign: 'center', fontSize: '32px' }
    p.closest = selector => selector.split(', ').includes('header')
    markPublisherHeadings(documentOf([p]), [{ label: 'Chapter 1. A chapter title' }])
    assert.equal(p['data-omaread-display'], undefined)
})

test('handles nested TOC entries, whitespace, Roman numbers, and semantic headings', () => {
    const paragraphs = [paragraph('II'), paragraph('A\n Chapter Title'),
        paragraph('A semantic heading', true), paragraph('III'), paragraph('Not a title')]
    markPublisherHeadings(documentOf(paragraphs), [
        { label: 'Part I', subitems: [{ label: 'Chapter II. A Chapter Title' }] },
    ])
    assert.deepEqual(paragraphs.map(p => p['data-omaread-display']),
        ['number', 'title', 'heading', undefined, undefined])
})

test('clears old broad classifications, including when a book has no TOC', () => {
    const p = paragraph('Ordinary body text.')
    p['data-omaread-display'] = ''
    markPublisherHeadings(documentOf([p]))
    assert.equal(p['data-omaread-display'], undefined)
})
