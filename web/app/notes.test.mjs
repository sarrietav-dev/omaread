import { test } from 'node:test'
import assert from 'node:assert/strict'
import { isNoteRef, noteBlock, noteText } from './notes.js'

const el = (localName, { type, role, text = '', parent = null, children = [], align } = {}) => ({
    localName, textContent: text, parentElement: parent, children, align,
    getAttributeNS: (_, name) => (name === 'type' ? type ?? null : null),
    getAttribute: name => (name === 'role' ? role ?? null : null),
})
const styleOf = node => ({ verticalAlign: node.align ?? 'baseline' })

test('links marked as notes are notes, backlinks are not', () => {
    assert.equal(isNoteRef(el('a', { type: 'noteref', text: 'see' }), styleOf), true)
    assert.equal(isNoteRef(el('a', { role: 'doc-noteref', text: '3' }), styleOf), true)
    const sup = el('sup')
    assert.equal(isNoteRef(el('a', { type: 'backlink', text: '3', parent: sup }), styleOf), false)
})

test('a raised short marker is a note; a plain or long link is not', () => {
    assert.equal(isNoteRef(el('a', { text: '12', parent: el('sup') }), styleOf), true)
    assert.equal(isNoteRef(el('a', { text: '*', align: 'super' }), styleOf), true)
    const sup = el('sup')
    assert.equal(isNoteRef(el('a', { text: '4', children: [sup] }), styleOf), true)
    assert.equal(isNoteRef(el('a', { text: 'Chapter 2', parent: el('p') }), styleOf), false)
    assert.equal(isNoteRef(el('a', { text: 'a raised sentence', parent: el('sup') }), styleOf), false)
})

test('the note is the block around its anchor', () => {
    const aside = el('aside', { text: '1. A note.' })
    const span = el('span', { parent: aside })
    assert.equal(noteBlock(span), aside)
    assert.equal(noteBlock(aside), aside)
    const body = el('body')
    const anchor = el('a', { parent: body })
    anchor.nextElementSibling = el('p', { text: 'Following.' })
    assert.equal(noteBlock(anchor), anchor.nextElementSibling)
})

test('note text loses its spacing and backlink arrow', () => {
    assert.equal(noteText({ textContent: '  1.  Herbert, Dune,\n p. 4. ↩ ' }), '1. Herbert, Dune, p. 4.')
    assert.equal(noteText(null), '')
})
