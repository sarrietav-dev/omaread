import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readingOrder } from './sections.js'

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
