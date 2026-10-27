# Noctiluca on real sites

The other pages of this guide show Noctiluca on the crate's own test
pages, which never change. This page shows it on sites people use every
day.

These pictures were recorded on **27 October 2026** with **Noctiluca
0.0.2** (the 0.0.23 cohort) driving **Chrome for Testing 153.0.8010.12**,
headless, with its sandbox on, at 120×40 cells. Live sites change daily,
so this is a snapshot: what you see today will differ, for better or
worse. Each section names its tape, so you can record it again.

Every picture is a real terminal frame, recorded by
[`rich record`](../../recordings.md) from a tape in
[`docs/tapes/live/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/docs/tapes/live).
CI does not run these tapes or check their screenshots. See
[Recording them again](#recording-them-again).

## Wikipedia

[`real-wikipedia.tape`](../../tapes/live/real-wikipedia.tape) opens
[Noctiluca scintillans](https://en.wikipedia.org/wiki/Noctiluca_scintillans)
with `--images kitty`, shows link hints (`f`), then switches to reader
mode (`r`) and finds a sentence (`/`).

![Noctiluca's reader mode on the Wikipedia article: a paragraph with the found words highlighted, then the article's photograph of a Noctiluca cell drawn as Kitty graphics, with its caption](../../media/tapes/live/real-wikipedia/wikipedia-reader.png)

*Reader mode with Kitty graphics, after `/` found "There are 2 colours".
The article's photograph of a single cell is drawn in place, with its
caption below it.*

| The projection | Link hints |
|---|---|
| ![The Wikipedia article in the projection, with the logo top left and that day's fundraising banner over the article](../../media/tapes/live/real-wikipedia/wikipedia-projection.png) | ![The same view with a yellow hint label before every link and button](../../media/tapes/live/real-wikipedia/wikipedia-hints.png) |
| The projection (the default view), with Wikipedia's logo in Kitty graphics. On that day a fundraising banner covered the top of the article, and Noctiluca shows it where Chrome put it. | `f` puts a label on every link and control; typing the label follows it. |

![An animation: the Wikipedia article in the projection, link hints, then reader mode and a find](../../media/tapes/live/real-wikipedia/real-wikipedia.gif)

## Hacker News

[`real-hackernews.tape`](../../tapes/live/real-hackernews.tape) opens
the [Hacker News](https://news.ycombinator.com) front page. The page is
laid out with tables nested in tables, which Noctiluca reads as blocks of
text rather than as a data table. No `--images` is given, so the
recording terminal's default, half blocks, is used.

| The front page | Link hints |
|---|---|
| ![The Hacker News front page in the projection: the orange header's links, then numbered stories, each with its points, author, age and comment count](../../media/tapes/live/real-hackernews/hackernews.png) | ![The front page with a hint label before every story, user, and comments link](../../media/tapes/live/real-hackernews/hackernews-hints.png) |
| The projection: each story's title and site, with its points, author and comments under it. | `f` labels every link: a story, its site, its author, its comments. |

## MDN

[`real-mdn.tape`](../../tapes/live/real-mdn.tape) opens MDN's
[`Array.prototype.map()`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/map)
reference with `--images sixel`, then reader mode, and finds an example
(`/const numbers`).

| The projection | Reader mode, at an example |
|---|---|
| ![MDN's map() page in the projection: the title, the Baseline badge drawn in Sixel, the summary, and the interactive example's line numbers and first line of code](../../media/tapes/live/real-mdn/mdn-projection.png) | ![Reader mode on the same page: code examples in a monospaced block with a rule down their left side, the found words highlighted](../../media/tapes/live/real-mdn/mdn-reader.png) |
| The projection with Sixel pictures: the heading, the "Baseline" badge, and the start of the interactive example. MDN's layout leaves wide gaps at this width. | Reader mode: code blocks with a rule at their left, headings as links, and the match from `/` highlighted. |

## The Guardian

[`real-guardian.tape`](../../tapes/live/real-guardian.tape) opens the
[international front page](https://www.theguardian.com/international)
with `--images sixel` and pages down once (`Space`).

![The Guardian's front page in the projection: section labels in red, headlines in bold, timestamps, and grey boxes where the pictures go, each with its caption](../../media/tapes/live/real-guardian/guardian-front.png)

*The projection, one page down: the stories in Chrome's columns, with
section labels in their colours. The pictures had not arrived when this
was recorded and show as grey boxes with their captions (see
[What did not work](#what-did-not-work)).*

## GitHub

[`real-github.tape`](../../tapes/live/real-github.tape) opens
[this project's repository](https://github.com/buchochelliq-labs/rs-rich-cli)
in reader mode (`--reader`) and finds the README's "Install and try".

![The rs-rich-cli repository's README in reader mode: the "Install and try" heading highlighted, shell commands and a Rust snippet in code blocks, and the release notes as a bulleted list](../../media/tapes/live/real-github/github-readme.png)

*Reader mode on the README: headings, code blocks and lists, with the
match highlighted. GitHub's stylesheets were not reachable from the
recording machine, so the projection was unstyled; reader mode does not
need them.*

## YouTube

[`real-youtube.tape`](../../tapes/live/real-youtube.tape) opens a watch
page, the Blender Foundation's
[Big Buck Bunny](https://www.youtube.com/watch?v=aqz-KE-bpKQ), with
`--images kitty`, then the about page (`?`).

| The watch page | What Chrome can play |
|---|---|
| ![YouTube's watch page with "Sign in to confirm you're not a bot" where the player would be, and the video's title, channel and buttons below](../../media/tapes/live/real-youtube/youtube-watch.png) | ![Noctiluca's about overlay: its version, the browser's version, the terminal adaptations, and a table of codecs, with H.264, AV1, VP9 and AAC marked as playable](../../media/tapes/live/real-youtube/youtube-codecs.png) |
| YouTube answered the recording machine with its "Sign in to confirm you're not a bot" wall in place of the player. Noctiluca shows the wall and does not try to get past it ([Privacy and security](privacy.md)). The title, channel and buttons below still show. | `?` opens the about page. This Chrome can play H.264, AV1, VP9 and AAC, so the video would play on a machine YouTube lets through. |

For a video playing in place, see the
[recorded watch page](media.md#pictures-and-video) on the media page.

## Twitch

[`real-twitch.tape`](../../tapes/live/real-twitch.tape) opens the
[category directory](https://www.twitch.tv/directory) in half blocks
(`--images ansi`), then the
[Astronomiemuseum's live channel](https://www.twitch.tv/astronomiemuseum),
which streams a meteor radar, with `--images kitty`, and presses `p` to
play.

| The directory | A live channel |
|---|---|
| ![Twitch's category directory in half blocks: the followed-channels sidebar's avatars, category icons, and large category covers drawn as coarse coloured cells](../../media/tapes/live/real-twitch/twitch-directory.png) | ![A Twitch channel page in Kitty graphics: the player showing Twitch's question-mark error over the stream's poster, the chat panel on the right, and LIVE with the viewer count below](../../media/tapes/live/real-twitch/twitch-channel.png) |
| The directory with half blocks, which work in any terminal: two pixels to a cell, so the covers are coarse. | The channel in Kitty graphics, after `p`. The status bar reads `PAUSED 0:00 / live`. The stream did not play: Chrome can decode H.264, but the recording machine could not reach Twitch's video hosts, so the player shows Twitch's error over the channel's poster. |

## BBC News

The BBC was on the list, but it could not be recorded. Outside the UK,
`www.bbc.co.uk/news` redirects to `www.bbc.com`, and the recording
machine's network did not allow that host, so Chrome showed its own
"This site can't be reached" page. Noctiluca drew that page as it was.

## What did not work

Recording these pages found four bugs, fixed in the same change:

- **Pages built of tables lost their text.** Hacker News's front page
  showed only its footer: an outer layout table's cells held the stories'
  table, and folding a cell into one line dropped it. A table with a table
  inside it, or with `role="presentation"`, is now read as blocks.
- **Footers drawn at the top.** On MDN and GitHub, and with Wikipedia's
  reference back-links, text whose box had no size counted as being at
  the page's top-left corner, so the projection drew it first. Boxes with
  no area no longer count.
- **The Guardian read as one paragraph.** A script appending to `<html>`
  made the whole page look like a live log. The page's root is no longer
  taken for one.
- **A crash when finding text.** Finding text after a non-ASCII
  character, such as in a reader-mode code block, panicked. Find now
  highlights by byte offset.

Still open:

- **The Guardian's pictures** stayed grey boxes in both views, and
  each picture showed twice (the picture and its caption box).
- **MDN at 120 columns** leaves large vertical gaps, and narrow columns
  (Wikipedia's account links, MDN's "Learn more") are squeezed to one
  letter wide.
- **Twitch's sidebar** repeats its tooltip, "Use the Right Arrow Key to
  show more information.", once for each avatar.
- **YouTube** could not be shown playing from this machine (the bot wall),
  and Twitch could not reach its video hosts.

## Recording them again

Build the release binaries, then run each tape as a normal user (Chrome's
sandbox does not run as root, and Noctiluca never turns it off), with a
Chrome or Chromium on `PATH` under one of the names Noctiluca looks for:

```sh
cargo build --release -p rs-rich-cli -p noctiluca --bins --locked
PATH=/path/to/chrome-dir:$PATH target/release/rich record \
  --bin-dir target/release --output docs/media/tapes/live \
  docs/tapes/live/real-wikipedia.tape
```

Each recording's `provenance.json` records its date, the sites it
visited, and the Chrome version. Look at every screenshot before you
commit it: a live page can show a cookie wall, a banner or a bot check
on any day.

---

The pages shown belong to their owners; their content and trademarks are
theirs. Shown to demonstrate Noctiluca.
