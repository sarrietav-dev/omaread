// The theme's colors, laid over the book's. Books style for a white page: a
// tinted box (`aside.tip { background: #f6f1fd !important }`) that leaves its
// text color to the reader turns into light text on a light box under a dark
// theme. So the overrides sit in a cascade layer: an !important rule in a
// layer beats every unlayered !important rule in the book, whatever its
// specificity. Only !important rules belong in the layer; a normal one there
// loses to any book rule at all, so those stay outside.
export const COLOR_LAYER = 'omaread-colors'

export const colorCSS = (p, ttsHighlight) => `
    @layer ${COLOR_LAYER} {
        html, body {
            color: ${p.fg} !important;
            background: none !important;
        }
        body * {
            color: inherit !important;
            border-color: color-mix(in srgb, currentColor 35%, transparent) !important;
            background-color: transparent !important;
        }
        pre, pre *, code, kbd, samp, tt {
            background-color: ${p.surface} !important;
        }
        a:any-link, a:any-link * {
            color: ${p.link} !important;
        }
        svg, img { background-color: transparent !important; }
    }
    html {
        color-scheme: ${p.mode === 'dark' ? 'dark' : 'light'};
    }
    a:any-link, a:any-link * {
        text-decoration-color: color-mix(in srgb, currentColor 40%, transparent);
        text-underline-offset: .12em;
    }
    ::selection { background: ${p.selection}; }
    ::highlight(${ttsHighlight}) { background-color: color-mix(in srgb, ${p.link} 30%, transparent); color: inherit; }
`
