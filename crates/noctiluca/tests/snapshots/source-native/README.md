Captured from sandboxed Chromium through public `ChromeBackend::snapshot`, on
2026-11-09, with source viewport80×35 cells /640×630 CSS pixels. The terminal
regression projects this source into40 columns. The local neutral fixture has
nested genuine figures, a40×20 PNG displayed at160×80 and96×48, reversed DOM /
visual grid order, unequal spans and explicit `display: contents` wrappers.

`dom.json` and `ax.json` are the actual capture. `formatting.json` is the optional
`sourceFormatting` member separated verbatim from that same capture; extraction
reattaches it for the tests. These are computed style facts bound to backend IDs,
not style inference from missing boxes. `glow.png` is the exact served resource,
SHA2568370582b782669a08a2aba7bd79e024103ffcd7a8448ad765cdba37439784a89.

The fixture HTML hash is
ca966a77eaa70d913ee2abd01eac4d890b1339dbaa413dedefa1ef69a56605d6.
Original native screenshots, earlier scrollbar-reduced diagnostic capture,
source reference PNG and immutable old CLI remain in external task evidence.
