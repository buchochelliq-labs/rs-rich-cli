# The docs' fonts

Inter and JetBrains Mono, the faces of Zensical's modern theme, served with
the site so that no page loads anything from Google Fonts (`theme.font:
false` in `mkdocs.yml` and `mkdocs-python.yml`; the `@font-face` rules are
in `../fonts.css`).

| Files | Source | Version | Licence |
|---|---|---|---|
| `inter-{latin,latin-ext}-{400,500,700}-{normal,italic}.woff2` | the npm package [`@fontsource/inter`](https://www.npmjs.com/package/@fontsource/inter) (`files/`), built from [rsms/inter](https://github.com/rsms/inter) | 5.3.0 (`sha512-RofMylZmjlJEfELXeNHFWBRcSs75rGU/6bV2S2jfnvv/3rPXPGe0LgUJTklcHZ9lM4OZmAVFhcJPnACfb91A3g==`) | SIL Open Font License 1.1, [`LICENSE-Inter.txt`](LICENSE-Inter.txt) |
| `jetbrains-mono-{latin,latin-ext}-{400,700}-{normal,italic}.woff2` | the npm package [`@fontsource/jetbrains-mono`](https://www.npmjs.com/package/@fontsource/jetbrains-mono) (`files/`), built from [JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono) | 5.3.0 (`sha512-fqDfB5I9f1p1TV486aUgB9t8zP84P0O1FtQR5Ol9vjwPy+S+EIGlVYm1cvj2W5shcZMTg2nZFdVMoH5wFu8a1A==`) | SIL Open Font License 1.1, [`LICENSE-JetBrainsMono.txt`](LICENSE-JetBrainsMono.txt) |

These are the weights and styles the modern theme's stylesheet uses (Inter
400, 500 and 700; JetBrains Mono 400 and 700; each upright and italic), in
fontsource's Latin and Latin Extended subsets, unchanged. The
`unicode-range` of each rule in `../fonts.css` is fontsource's, so a
browser downloads only the files a page needs. The docs of 0.0.23 and
older, built from their own trees, keep the Roboto faces they shipped with.

To update, `npm pack @fontsource/inter@VERSION @fontsource/jetbrains-mono@VERSION`,
copy the same files from each package's `files/`, take each file's rule
from the package's `<weight>[-italic].css`, and record the version and the
integrity `npm pack --json` prints here.
