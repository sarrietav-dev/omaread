import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readingOrder, textEdges } from './sections.js'

// A tiny book: href → spine index, like book.resolveHref.
const resolver = hrefs => href => {
    const index = hrefs.indexOf(href.split('#')[0])
    return index < 0 ? null : { index }
}
const toc = (...entries) => entries.map(([label, href]) => ({ label, href }))

test('a normal book keeps the spine order', () => {
    const hrefs = ['cover.xhtml', 'ch1.xhtml', 'ch2.xhtml']
    const order = readingOrder(toc(['Cover', 'cover.xhtml'], ['One', 'ch1.xhtml'], ['Two', 'ch2.xhtml#top']),
        resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [0, 1, 2])
})

test('the TOC wins over a spine that puts chapter 5 before chapter 1', () => {
    const hrefs = ['part1.xhtml', 'ch5.xhtml', 'ch1.xhtml', 'ch2.xhtml', 'part2.xhtml', 'ch3.xhtml', 'ch4.xhtml']
    const order = readingOrder(toc(
        ['Part I', 'part1.xhtml'],
        ['Chapter 1', 'ch1.xhtml'],
        ['Chapter 2', 'ch2.xhtml'],
        ['Part II', 'part2.xhtml'],
        ['Chapter 3', 'ch3.xhtml'],
        ['Chapter 4', 'ch4.xhtml'],
        ['Chapter 5', 'ch5.xhtml'],
    ), resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [0, 2, 3, 4, 5, 6, 1])
})

test('nested TOC entries only count the section once', () => {
    const hrefs = ['ch1.xhtml', 'ch2.xhtml']
    const order = readingOrder([{
        label: 'One', href: 'ch1.xhtml',
        subitems: [{ label: 'One A', href: 'ch1.xhtml#a' }, { label: 'One B', href: 'ch1.xhtml#b' }],
    }, { label: 'Two', href: 'ch2.xhtml' }], resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [0, 1])
})

test('sections the TOC does not name stay next to their neighbours', () => {
    const hrefs = ['cover.xhtml', 'ch1.xhtml', 'part2.xhtml', 'ch2.xhtml', 'index.xhtml']
    const order = readingOrder(toc(['One', 'ch1.xhtml'], ['Two', 'ch2.xhtml']),
        resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [0, 1, 2, 3, 4])
})

test('a book without a usable TOC falls back to the spine', () => {
    const hrefs = ['ch1.xhtml', 'ch2.xhtml']
    assert.equal(readingOrder(null, resolver(hrefs), hrefs.length), null)
    assert.equal(readingOrder([], resolver(hrefs), hrefs.length), null)
    assert.equal(readingOrder(toc(['One', 'ch1.xhtml'], ['One again', 'ch1.xhtml#a']),
        resolver(hrefs), hrefs.length), null)
})

test('broken TOC hrefs are ignored', () => {
    const hrefs = ['ch1.xhtml', 'ch2.xhtml']
    const order = readingOrder(toc(['Gone', 'missing.xhtml'], ['One', 'ch1.xhtml'], ['Two', 'ch2.xhtml']),
        resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [0, 1])
})

// Make It Stick's shape: the cover is not in the TOC, and the index is.
const frontToBack = () => {
    const hrefs = ['cover.xhtml', 'halftitle.xhtml', 'titlepage.xhtml', 'copyright.xhtml',
        'dedication.xhtml', 'contents.xhtml', 'preface.xhtml', 'ch1.xhtml', 'ch2.xhtml',
        'notes.xhtml', 'reading.xhtml', 'acknowledgments.xhtml', 'index.xhtml']
    const toc = [
        ['Half Title', 'halftitle.xhtml'], ['Title Page', 'titlepage.xhtml'],
        ['Copyright', 'copyright.xhtml'], ['Dedication', 'dedication.xhtml'],
        ['Contents', 'contents.xhtml'], ['Preface', 'preface.xhtml'],
        ['1. One', 'ch1.xhtml'], ['2. Two', 'ch2.xhtml'],
        ['Notes', 'notes.xhtml'], ['Suggested Reading', 'reading.xhtml'],
        ['Acknowledgments', 'acknowledgments.xhtml'], ['Index', 'index.xhtml'],
    ].map(([label, href]) => ({ label, href }))
    return { hrefs, toc }
}

test('the text starts at the preface and ends before the notes', () => {
    const { hrefs, toc } = frontToBack()
    const edges = textEdges(toc, resolver(hrefs), hrefs.map((_, index) => index))
    // The unnamed cover is skipped, and so is every named part after chapter 2.
    assert.deepEqual(edges, { start: 6, end: 8 })
})

test('a TOC that names nothing usable keeps the ends of the book', () => {
    const hrefs = ['cover.xhtml', 'ch1.xhtml', 'index.xhtml']
    assert.deepEqual(textEdges(null, resolver(hrefs), [0, 1, 2]), { start: 0, end: 2 })
    assert.deepEqual(textEdges(toc(['Cover', 'cover.xhtml'], ['Index', 'index.xhtml']),
        resolver(hrefs), [0, 1, 2]), { start: 0, end: 2 })
})

test('the reading order, not the spine, decides the text edges', () => {
    const hrefs = ['ch2.xhtml', 'index.xhtml', 'ch1.xhtml']
    const order = readingOrder(toc(['1. One', 'ch1.xhtml'], ['2. Two', 'ch2.xhtml'], ['Index', 'index.xhtml']),
        resolver(hrefs), hrefs.length)
    assert.deepEqual(order, [2, 0, 1])
    const edges = textEdges(toc(['1. One', 'ch1.xhtml'], ['2. Two', 'ch2.xhtml'], ['Index', 'index.xhtml']),
        resolver(hrefs), order)
    assert.deepEqual(edges, { start: 2, end: 0 })
})
