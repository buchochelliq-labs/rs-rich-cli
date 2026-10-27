# The site's fonts

Roboto and Roboto Mono, served with the site so that the page loads no
font from anywhere else (the `@font-face` rules are in `../site.css`).
They are rs-rich-cli's docs fonts (its `docs/assets/fonts/`), a subset of
them: Roboto 400 (upright and italic) and 700, Roboto Mono 400 and 700, in
fontsource's Latin and Latin Extended subsets, unchanged.

| Files | Source | Version | Licence |
|---|---|---|---|
| `roboto-{latin,latin-ext}-*.woff2` | the npm package [`@fontsource/roboto`](https://www.npmjs.com/package/@fontsource/roboto) (`files/`), built from [googlefonts/roboto-classic](https://github.com/googlefonts/roboto-classic) | 5.3.0 | SIL Open Font License 1.1, [`LICENSE-Roboto.txt`](LICENSE-Roboto.txt) |
| `roboto-mono-{latin,latin-ext}-*.woff2` | the npm package [`@fontsource/roboto-mono`](https://www.npmjs.com/package/@fontsource/roboto-mono) (`files/`), built from [googlefonts/robotomono](https://github.com/googlefonts/robotomono) | 5.3.0 | SIL Open Font License 1.1, [`LICENSE-RobotoMono.txt`](LICENSE-RobotoMono.txt) |

To update, copy the same files from rs-rich-cli's `docs/assets/fonts/`.
