// Link hints: a short label on every link in view, typed to follow it.

const KEYS = 'asdfghjklqwertyuiopzxcvbnm'

// `n` labels, all the same length, so no label is the start of another.
export const hintLabels = n => {
    if (n <= 0) return []
    let length = 1
    while (KEYS.length ** length < n) length++
    const labels = []
    for (let i = 0; i < n; i++) {
        let label = ''
        for (let k = 0, x = i; k < length; k++, x = Math.floor(x / KEYS.length)) label = KEYS[x % KEYS.length] + label
        labels.push(label)
    }
    return labels
}

// The targets whose labels start with what's been typed.
export const matching = (targets, typed) => targets.filter(t => t.label.startsWith(typed))
