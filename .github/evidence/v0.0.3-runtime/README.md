# Runtime evidence for #98, #59, #72 and #74

Actual CLI output, generated on 2026-09-10. SVGs are produced by the Rust CLI's
own `--export-svg`; JPGs are browser screenshots of those SVGs. They are not
mocked terminal output. Before binary is the preserved release-readiness baseline;
after binary is this runtime change, debug build. COLUMNS=72, TERM=xterm-256color,
NO_COLOR unset. Export forces a recorded colour console.

Commands (from the repository root, substituting the before/after binary):

```sh
rich --json .github/evidence/v0.0.3-runtime/numbers.json --export-svg numbers-after.svg
rich --markdown .github/evidence/v0.0.3-runtime/images.md --export-svg markdown-after.svg
rich --gif crates/rich-art/examples/assets/ball.gif --loop 0 --width 24 --no-color > gif-piped.txt
rich gif-piped.txt --export-svg gif-piped.svg
```

The GIF image is the actual piped first-frame text, displayed through the CLI's
plain-file SVG exporter because GIF animation intentionally cannot be exported.
`gif-stderr.txt` contains its downgrade diagnostic. This infinite-loop command
was also checked with a five-second process timeout and emitted no ESC bytes.

`numbers-before/after` exposes the large-integer precision repair. Markdown
before/after shows adjacent badge images sharing one cell's row, plus quote/HTML
spacing. Source fixtures and plain output accompany every SVG.

`csv-memory.txt` records same-host peak-RSS measurements via POSIX wait4.
`verification.txt` records the default/all-feature/no-default/MSRV gates.

Independent agents reviewed JSON, Console empty output, GIF/Live and CSV.
Their confirmed findings (missing Write import, empty Text/CSV regression) were
fixed and covered by regression tests. A pre-existing TERM=dumb live-terminal
interactivity difference remains outside the pipe-redirection fix.
