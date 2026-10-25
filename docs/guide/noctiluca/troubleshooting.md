# Troubleshooting and FAQ

![Noctiluca's lanternfish, puzzled, its terminal lure flickering with a question mark](../../assets/noctiluca/lanternfish-lost.svg){ .tui-mascot width="112" }

First, press **F1**. Every action is there with its key, and an action
that cannot run now says why.

## Starting

**"Chrome cannot run its sandbox as root."** Run Noctiluca as a normal
user. It never turns Chrome's sandbox off.

**No browser found, or the wrong one.** Install Chrome or Chromium, or
name one with `--chrome PATH` or the `RICH_EMBED_CHROME` environment
variable. The order Noctiluca looks in is in
[Getting started](getting-started.md#the-chrome-it-needs).

**A config warning on the status line.** Noctiluca read your config, but
not all of it. `noctiluca keys` prints every warning in full. An unknown
key is only a warning, so the rest of the file still applies; a TOML
syntax error stops Noctiluca with the line.

**My `[keys]` lines do nothing.** One key name Noctiluca does not know
drops every line of `[keys]`. `none` is one of them: to unbind an action,
give it an empty string (`quit = ""`). `noctiluca keys` shows the warning
and the keys in effect.

## Reading

**A site shows a bot check, or refuses me.** Noctiluca drives Chrome over
DevTools, and some bot checks notice that. Noctiluca does not try to hide
it, or to get past bot checks, CAPTCHAs or sign-in walls. See
[Bot checks](privacy.md#bot-checks).

**The projection looks wrong.** The first projection has no fixed or
sticky layer and no paint order, so sticky headers and overlapping boxes
can read oddly. Press `r` for reader mode.

**The projection shows an earlier page.** A known issue in 0.0.22: after
opening a page with `o` or `Ctrl+L` while in reader mode, switching to the
projection can keep showing an earlier page, and reloading does not help.
The address line and reader mode show the right page. Stay in reader mode
for that page, or start Noctiluca again.

**The page stops where it should load more.** Noctiluca scrolls its own
copy of the page, so pages that load more as you scroll (infinite scroll,
lazy images) do not load more yet. Following Chrome's scroll is planned
for 0.0.23.

**Keys type into a field.** A focused text field takes every key you
type. Esc does not leave it: press `Tab` to move to the next widget.

**"Edit conflict: the page changed this value."** The page changed a
field while you were editing it. Your text is kept. `Ctrl+R` in the field
takes the page's value; then edit and press Enter again. See
[Typing in a field](forms.md#typing-in-a-field).

**`O` or `g t` says "One tab for now".** Tabs need several pages in
rs-rich-embed, which is still to come.

## Pictures and sound

**No pictures in the projection.** In 0.0.22, pictures are drawn in
reader mode only; the projection shows a placeholder with the alt text.
Press `r`. Pictures in the projection are planned for 0.0.23.

**Blocky pictures.** By default Noctiluca picks what your terminal can
show, and falls back to half blocks. In a terminal with Kitty graphics,
Sixel or iTerm2 images, name it with `--images` (or `[media] images`).

**No sound.** Sound needs `--audio` (or `[media] audio = true`). It plays
where Chrome runs: over SSH, that is the server. A site table with
`audio = false` keeps that site muted.

**A video shows only its alt text.** Video frames come in 0.0.24. Its
sound plays now, with `--audio`.

## Saving and leaving

**A download did nothing.** Downloads are refused for now. See
[Downloads](privacy.md#downloads).

**"No config file to keep site keys in."** Binding a key on a site writes
to your config file, and Noctiluca found no config directory (no `HOME`,
say). Name a file with `--config PATH`.

**"This page has no site (a local file): site keys need a host."** Site
keys belong to a host. Serve the page from a local server to bind keys on
it.

**`rich-embed-chrome-…` folders in my temporary directory.** A known
issue in 0.0.22: a run's temporary profile is not removed when Noctiluca
quits. See [the browser profile](privacy.md#the-browser-profile).

**`--dump` says the page "did not load in time".** `--dump` waits up to a
minute for the page to load and stay still. A page that never stops
changing (a ticking clock, an animation in the DOM) does not finish.

## FAQ

**Is it a real browser?** Yes: Chrome loads and runs every page, with its
scripts. Noctiluca is the screen and the keyboard.

**Can I sign in to sites?** Fields, check boxes and buttons work, and
`--profile NAME` keeps you signed in between runs. Sign-in forms are not
tested yet; they come in the forms and tables phase.

**Does it work over SSH?** Yes. The page, its text and its pictures reach
your terminal like any other program's. Sound plays on the server.

**Can I use the mouse?** Clicking the page is not wired up yet. Link
hints (`f`) reach every link and control.

**Why does it need Chrome?** A modern page needs a modern engine to run
its scripts and lay it out. Noctiluca reads Chrome's layout instead of
laying pages out itself. See [the design note](../../design/browser.md).

**Will it play DRM video, run extensions, or solve CAPTCHAs?** No, never.
See [what's next](status.md#never).
