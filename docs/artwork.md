# EVIF artwork

The website and README use a haunted image-file character derived from the existing
`logo.svg`: violet paper, acid-lime eyes, a folded corner and drifting pixels.
Generated with the built-in image-generation tool on 2026-10-25. The tool did not
expose a model selector. The small website icons are SVG paths.

The two generated PNGs were converted to WebP with `cwebp -q 85`; no content edits
were made during conversion. Final assets: `site/haunted-file.webp` (website hero)
and `site/evif-banner.webp` (README). The website keeps its title and navigation in
HTML; the README banner has descriptive alternative text.

## Hero prompt

> Use case: stylized-concept. Asset type: wide hero illustration for EVIF, an image-format research tool, also used as a README banner. Create a polished spooky editorial illustration, restrained and atmospheric rather than childish: one haunted image-file sheet floating upright above a subtle matrix of tiny square image tiles, with a folded upper-right page corner, two luminous acid-lime eyes and a short row of square pixel teeth. Its silhouette trails into a torn ghost-sheet hem with a few drifting pixel fragments. The sheet has rich dark violet paper material with thin lilac rim light. Deep nearly black aubergine background (#0d0716), low violet spectral mist, precise acid-lime (#d9ff7a) highlights, no orange Halloween palette. Wide landscape composition, ghost on the right half, left half mostly dark calm negative space for HTML typography. Sophisticated scientific-tool identity, tactile engraved paper with cinematic lighting and delicate grain. No words, no letters, no logos, no numbers, no watermark, no charts or fake data. Avoid pumpkins, bats, skulls, gore, and unrelated props. Landscape 3:2 composition.

## README banner prompt

Input reference: the generated hero illustration.

> Create a compact wide README banner based on this illustration. Preserve the same haunted violet image-file character with acid-lime eyes and pixel teeth, folded upper-right corner, mist and dark aubergine background. Recompose to a very wide 3:1 landscape banner: large ghost on the right third, on the left add large beautifully typeset pale-lilac title exactly 'EVIF' and smaller acid-lime subtitle exactly 'THE EVIL IMAGE FORMAT'. Keep all text crisp, generous margins, ghost fully visible with pixel fragments. Dark restrained atmospheric editorial design, no additional words, no watermark, no charts, no pumpkins or bats. Deliver the banner only, no mockup.
