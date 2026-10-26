# The docs' fonts

Roboto and Roboto Mono, served with the site so that no page loads anything
from Google Fonts (`theme.font: false` in `mkdocs.yml` and
`mkdocs-python.yml`; the `@font-face` rules are in `../fonts.css`).

| Files | Source | Version | Licence |
|---|---|---|---|
| `roboto-{latin,latin-ext}-{300,400,700}-{normal,italic}.woff2` | the npm package [`@fontsource/roboto`](https://www.npmjs.com/package/@fontsource/roboto) (`files/`), built from [googlefonts/roboto-classic](https://github.com/googlefonts/roboto-classic) | 5.3.0 (`sha512-BapRJOWYP+LZ21zp+wBQjfpPYKRoxc4LspJ/RLuI+HSMBD5u/X4O+ESDrSvEqDSy0rAl7GwBJ+09mdc16cVQ1Q==`) | SIL Open Font License 1.1, [`LICENSE-Roboto.txt`](LICENSE-Roboto.txt) |
| `roboto-mono-{latin,latin-ext}-{400,700}-{normal,italic}.woff2` | the npm package [`@fontsource/roboto-mono`](https://www.npmjs.com/package/@fontsource/roboto-mono) (`files/`), built from [googlefonts/robotomono](https://github.com/googlefonts/robotomono) | 5.3.0 (`sha512-yhwplHMHW9dCk+1pZuERfcJe+K3rosGVZEoK0XktkxRSqQ9qsipSwwiuW2/8Qmtp9lzu3Ixa/ie4BMH3i8CkZQ==`) | SIL Open Font License 1.1, [`LICENSE-RobotoMono.txt`](LICENSE-RobotoMono.txt) |

These are the weights and styles Material for MkDocs asked Google for
(Roboto 300, 400 and 700; Roboto Mono 400 and 700; each upright and
italic), in fontsource's Latin and Latin Extended subsets, unchanged. The
`unicode-range` of each rule in `../fonts.css` is fontsource's, so a
browser downloads only the files a page needs.

To update, `npm pack @fontsource/roboto@VERSION @fontsource/roboto-mono@VERSION`,
copy the same files from each package's `files/`, and record the version
and the integrity `npm pack --json` prints here.
