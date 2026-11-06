import { test } from 'node:test'
import assert from 'node:assert/strict'
import { endsInAbbreviation, installSentenceSegmenter } from './sentences.js'

const sentences = text => {
    const out = []
    for (const { index, segment } of new Intl.Segmenter('en', { granularity: 'sentence' }).segment(text)) {
        assert.equal(text.slice(index, index + segment.length).replace(/\s/g, ' '), segment)
        out.push(segment.trim())
    }
    return out
}

test('hard-wrapped lines and abbreviations', async t => {
    const restore = installSentenceSegmenter()
    t.after(restore)

    await t.test('line breaks do not end a sentence', () => {
        assert.deepEqual(sentences('With apps, software as a\nservice (SaaS), and clouds.\nNext one.'),
            ['With apps, software as a service (SaaS), and clouds.', 'Next one.'])
    })
    await t.test('titles and initials are joined to what follows', () => {
        assert.deepEqual(sentences('Ask Dr. Smith. He knows.'), ['Ask Dr. Smith.', 'He knows.'])
        assert.deepEqual(sentences('Mr. Jones left. See Fig. Two for it.'), ['Mr. Jones left.', 'See Fig. Two for it.'])
        assert.deepEqual(sentences('[1] Richard T. Kouzes, Gordon A. Anderson, and Deborah K. Gracio. Next.'),
            ['[1] Richard T. Kouzes, Gordon A. Anderson, and Deborah K. Gracio.', 'Next.'])
        assert.deepEqual(sentences('[5] Edgar F. Codd, S. B. Codd, and C. T. Salley. Done.'),
            ['[5] Edgar F. Codd, S. B. Codd, and C. T. Salley.', 'Done.'])
    })
    await t.test('real sentence ends stay', () => {
        assert.deepEqual(sentences('We pay salaries, etc. As databases grew, so did it.'),
            ['We pay salaries, etc.', 'As databases grew, so did it.'])
        assert.deepEqual(sentences('So did I. Then we left.'), ['So did I.', 'Then we left.'])
    })
    await t.test('word segmentation is untouched', () => {
        const words = [...new Intl.Segmenter('en', { granularity: 'word' }).segment('a b')]
        assert.equal(words.length, 3)
    })
})

test('endsInAbbreviation', () => {
    assert.ok(endsInAbbreviation('Ask Dr.'))
    assert.ok(!endsInAbbreviation('It is done.'))
    assert.ok(!endsInAbbreviation('Plan etc.'))
})
