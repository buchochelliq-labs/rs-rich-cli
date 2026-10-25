# Noctiluca

<img src="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-cli/main/docs/assets/noctiluca/logo.svg" alt="Noctiluca's logo: a bloom of small terminal windows, linked, glowing in dark water" width="128" align="right">

A terminal web browser on [intuiTUIve](https://crates.io/crates/rs-rich-intuituive).
Chrome loads and runs the page; Noctiluca reads what Chrome laid out and
draws it in cells. Text stays terminal text you can select and copy,
controls are intuiTUIve widgets, and images are media regions.

```
noctiluca https://docs.rs/serde        # an address
noctiluca ./page.html                  # a local file
noctiluca gh rs-rich                   # a keyword and its words
noctiluca --dump text page.html        # the page's text, for scripts
```

It needs a Chrome or Chromium you install (a system Chrome is preferred,
for its codecs). The browser runs with its sandbox on, so Noctiluca runs
as a normal user, never as root.

- **Reading**: the projection (Chrome's geometry snapped to cells) by
  default; `r` switches to reader mode, one readable column.
- **Going places**: `o` opens an address, a keyword or a search; `f`
  shows link hints; `H` and `L` go back and forward; `/` finds in the
  page; `F1` lists every action with its keys and, when one cannot run,
  why.
- **Forms**: fields, check boxes, radios, selects and sliders are
  intuiTUIve widgets. A field's edit is a draft until Enter; if the page
  changed the value meanwhile, it is a conflict, never overwritten.
- **Video**: a playing video plays in its region, from Chrome's own
  frames, within what your terminal can take (at most eight frames a
  second; Kitty, iTerm2, Sixel, then character art when it is short, and
  character art first over SSH). `p` plays or pauses, `[` and `]` seek
  five seconds, `alt+m` mutes, `F4` freezes moving pictures. A paused
  video shows its poster with a play mark. `about:noctiluca` lists the
  codecs your Chrome plays.
- **Sound**: off by default. `--audio` (or `[media] audio = true`) lets
  pages play sound, with Play/Pause, Mute/Unmute and volume controls for
  each video and audio. Sound plays on the machine Chrome runs on, never
  over SSH: run over SSH, it plays on the server.
- **Safety**: everything a page says is cleaned before it reaches the
  terminal; no escape sequence passes. Downloads are refused. The
  profile is temporary unless you keep one (`--profile NAME`).
- **Config**: `noctiluca/config.toml` in your config directory, with
  keywords, the search engine, keys, and per-site settings and keys. See
  [`config.sample.toml`](config.sample.toml).

## Cargo features

- `fake-engine` (off; **test-only**): the binary replays saved page
  snapshots from the directory `NOCTILUCA_FAKE_ENGINE` names instead of
  starting Chrome. The PTY test and the docs tape use it. A normal build
  ignores the variable.

## Limitations

- Noctiluca drives Chrome over DevTools and enables its `Runtime` domain
  (it needs it to hear the page's change pings). Some sites' bot checks
  can notice that, and may treat Noctiluca as automation. Noctiluca does
  not try to hide it.
- Sound plays on the machine Chrome runs on, never over SSH.

The guide, with every key, the config and troubleshooting, is
[Noctiluca's page](https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/)
on the docs site. The design, its phases and what is still to come are in
[`docs/design/browser.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/browser.md).
