import { test } from 'node:test'
import assert from 'node:assert/strict'
import { COLOR_LAYER, colorCSS } from './colors.js'

const palette = {
    mode: 'dark', bg: '#1d2021', fg: '#d5c4a1', link: '#83a598',
    surface: '#282828', selection: '#504945',
}
const css = colorCSS(palette, 'omaread-tts')

// The layer's body, by matching braces from its opening one.
const layerOf = css => {
    const start = css.indexOf('{', css.indexOf(`@layer ${COLOR_LAYER}`))
    let depth = 0
    for (let i = start; i < css.length; i++) {
        if (css[i] === '{') depth++
        else if (css[i] === '}' && --depth === 0) return { inside: css.slice(start + 1, i), outside: css.slice(0, start) + css.slice(i + 1) }
    }
    throw new Error('unclosed layer')
}
const declarations = css => [...css.matchAll(/\{([^{}]*)\}/g)]
    .flatMap(([, body]) => body.split(';')).map(d => d.trim()).filter(Boolean)

test('the color overrides sit in a cascade layer, so a specific !important book rule cannot beat them', () => {
    const { inside } = layerOf(css)
    const decls = declarations(inside)
    assert.ok(decls.includes(`color: ${palette.fg} !important`))
    assert.ok(decls.includes('color: inherit !important'))
    assert.ok(decls.includes('background-color: transparent !important'))
    assert.ok(decls.includes(`color: ${palette.link} !important`))
})

test('every rule in the layer is !important: a normal layered rule loses to any book rule', () => {
    for (const d of declarations(layerOf(css).inside)) assert.match(d, /!important$/, d)
})

test('nothing outside the layer forces a color or background on book elements', () => {
    for (const d of declarations(layerOf(css).outside)) assert.doesNotMatch(d, /!important/, d)
})
