import { test } from 'node:test'
import assert from 'node:assert/strict'
import { hintLabels, matching } from './hints.js'

test('a few links get one letter each, from the home row', () => {
    assert.deepEqual(hintLabels(3), ['a', 's', 'd'])
    assert.deepEqual(hintLabels(0), [])
})

test('many links get labels of one length, none a prefix of another', () => {
    const labels = hintLabels(30)
    assert.equal(new Set(labels).size, 30)
    assert.ok(labels.every(l => l.length === 2))
    assert.deepEqual(labels.slice(0, 2), ['aa', 'as'])
})

test('typing narrows the targets', () => {
    const targets = hintLabels(30).map(label => ({ label }))
    assert.equal(matching(targets, 'a').length, 26)
    assert.deepEqual(matching(targets, 'as').map(t => t.label), ['as'])
    assert.equal(matching(targets, 'zz').length, 0)
})
