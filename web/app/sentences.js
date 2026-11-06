// Sentence boundaries for read-aloud. foliate-js segments with
// Intl.Segmenter and uses only the offsets, so the segmenter is wrapped here:
//  - Many EPUBs hard-wrap their paragraphs, and the Unicode sentence rules end
//    a sentence at every line feed. Line breaks are shown to the segmenter as
//    spaces (same length, so every offset still lines up with the text).
//  - The rules also end a sentence after "Dr." or "J." when a capital follows.
//    Pieces ending in a known abbreviation or an initial are joined to the next.

const ABBREVIATION = /(?:^|[\s(\[“"'‘])(?:Mr|Mrs|Ms|Dr|Prof|St|Jr|Sr|vs|cf|Figs?|Vols?|pp|approx|e\.g|i\.e)\.$/u
// A lone capital with a period: "Richard T.", "and C.". A bare "I." is the
// pronoun and may well end a sentence.
const INITIAL = /(?:^|[\s(\[])(?!I\.$)\p{Lu}\.$/u

export const endsInAbbreviation = text => {
    const t = text.trimEnd()
    return ABBREVIATION.test(t) || INITIAL.test(t)
}

// {index, segment} items from a segmenter, with abbreviation splits undone.
export function* joinAbbreviations(segments) {
    let pending = null
    for (const s of segments) {
        pending = pending ? { index: pending.index, segment: pending.segment + s.segment } : { index: s.index, segment: s.segment }
        if (!endsInAbbreviation(pending.segment)) {
            yield pending
            pending = null
        }
    }
    if (pending) yield pending
}

export const installSentenceSegmenter = (intl = Intl) => {
    const Native = intl.Segmenter
    intl.Segmenter = class extends Native {
        segment(text) {
            if (this.resolvedOptions().granularity !== 'sentence') return super.segment(text)
            return joinAbbreviations(super.segment(String(text).replace(/[\r\n\u0085\u2028\u2029]/g, ' ')))
        }
    }
    return () => { intl.Segmenter = Native }
}
