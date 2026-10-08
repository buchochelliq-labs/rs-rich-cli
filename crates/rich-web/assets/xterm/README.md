# Vendored xterm.js

The browser page rs-rich-web serves draws the terminal with
[xterm.js](https://github.com/xtermjs/xterm.js). These files are copied
unchanged from the npm packages below and embedded in the crate
(`include_str!`), so a running server never fetches anything from a CDN.

| File | Package | Path in the package |
|---|---|---|
| `xterm.js` | `@xterm/xterm` 6.0.0 | `lib/xterm.js` (UMD build) |
| `xterm.css` | `@xterm/xterm` 6.0.0 | `css/xterm.css` |
| `LICENSE-xterm` | `@xterm/xterm` 6.0.0 | `LICENSE` |
| `addon-fit.js` | `@xterm/addon-fit` 0.11.0 | `lib/addon-fit.js` (UMD build) |
| `LICENSE-addon-fit` | `@xterm/addon-fit` 0.11.0 | `LICENSE` |

Sources, with the integrity the npm registry publishes for each tarball (both
checked when the files were copied):

- <https://registry.npmjs.org/@xterm/xterm/-/xterm-6.0.0.tgz>
  `sha512-TQwDdQGtwwDt+2cgKDLn0IRaSxYu1tSUjgKarSDkUM0ZNiSRXFpjxEsvc/Zgc5kq5omJ+V0a8/kIM2WD3sMOYg==`
- <https://registry.npmjs.org/@xterm/addon-fit/-/addon-fit-0.11.0.tgz>
  `sha512-jYcgT6xtVYhnhgxh3QgYDnnNMYTcf8ElbxxFzX0IZo+vabQqSPAjC3c1wJrKB5E19VwQei89QCiZZP86DCPF7g==`

Both are MIT licensed; their licence files are kept beside them and ship in
the crate.

## Updating

1. Download the new tarballs from the registry and check them against the
   `dist.integrity` that `https://registry.npmjs.org/@xterm/xterm` lists for
   the version.
2. Copy the files in the table above over these.
3. Put each file's SHA-256 in `PINNED` in `crates/rich-web/build.rs`: the
   build fails until the list matches the files.
4. Update the versions here, in the crate README and in
   `docs/guide/intuituive/web.md`, and try the page in a browser
   (`cargo run -p rs-rich-web --example serve`).
