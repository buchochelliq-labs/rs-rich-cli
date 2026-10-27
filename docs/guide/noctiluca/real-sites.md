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

## BBC News

[`real-bbc.tape`](../../tapes/live/real-bbc.tape) opens the
[BBC News front page](https://www.bbc.com/news) with `--images kitty`
and pages down once (`Space`), then opens
[an article](https://www.bbc.com/news/articles/ckm23y3mg1yyo) in reader
mode (`--reader`) and pages down.

| The front page | One page down |
|---|---|
| ![BBC News's front page in the projection: the menu and Subscribe buttons, the lead story's headline, and its photograph in Kitty graphics with the caption under it](../../media/tapes/live/real-bbc/bbc-front.png) | ![Further down: two columns of stories, each photograph drawn where the page puts it, with headlines, summaries, times and section links beside and under them](../../media/tapes/live/real-bbc/bbc-front-more.png) |
| The projection: the lead story's photograph in Kitty graphics, where Chrome placed it, with an `Image:` caption from its alt text. | One page down (`Space`): stories in the page's columns, each with its picture. |

| An article in reader mode | Further in |
|---|---|
| ![A BBC article in reader mode: the headline, the byline, Share and Save as buttons, and the opening paragraphs in one column](../../media/tapes/live/real-bbc/bbc-article.png) | ![The same article further down: paragraphs, then a photograph in Kitty graphics with its alt text above and the agency's credit below](../../media/tapes/live/real-bbc/bbc-article-more.png) |
| Reader mode (`--reader`): one readable column, the page's buttons as widgets. | One page down: the article's photograph in place, with its alt text and credit. |

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
| The projection (the default view), with Wikipedia's logo in Kitty graphics and the account links at the top right. On that day a fundraising banner covered the top of the article, and Noctiluca shows it where Chrome put it. | `f` puts a label on every link and control; typing the label follows it. |

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
| The projection: each rank beside its story's title, from 1 to 30, with the points, author and comments under it. | `f` labels every link: a story, its site, its author, its comments. |

## MDN

[`real-mdn.tape`](../../tapes/live/real-mdn.tape) opens MDN's
[`Array.prototype.map()`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/map)
reference with `--images sixel`, then reader mode, and finds an example
(`/const numbers`).

| The projection | Reader mode, at an example |
|---|---|
| ![MDN's map() page in the projection: the header and breadcrumbs, the table of contents on the left, the title, the Baseline badge drawn in Sixel, the summary, and the interactive example's line numbers and first line of code](../../media/tapes/live/real-mdn/mdn-projection.png) | ![Reader mode on the same page: code examples in a monospaced block with a rule down their left side, the found words highlighted](../../media/tapes/live/real-mdn/mdn-reader.png) |
| The projection with Sixel pictures: the breadcrumbs, the article's contents at the left, the heading, the "Baseline" badge, and the start of the interactive example. MDN's layout leaves wide gaps at this width. | Reader mode: code blocks with a rule at their left, headings as links, and the match from `/` highlighted. |

## The Guardian

[`real-guardian.tape`](../../tapes/live/real-guardian.tape) opens the
[international front page](https://www.theguardian.com/international)
with `--images sixel`, pages down once (`Space`), then switches to reader
mode (`r`) and pages down again.

| The projection | Reader mode |
|---|---|
| ![The Guardian's front page in the projection: the lead story's headline in bold with its section in red, a column of picture captions beside it, then a row of linked stories and a video's poster with its play mark](../../media/tapes/live/real-guardian/guardian-front.png) | ![The front page in reader mode: the section links, the edition banner, and the first story's photograph in Sixel with its alt text above it](../../media/tapes/live/real-guardian/guardian-reader.png) |
| The projection, one page down: stories in Chrome's columns, section labels in their colours, and a video's poster with its play mark. The Guardian lays its picture captions out in a narrow column, which is cut short at this width. | Reader mode: the pictures arrive (The Guardian serves AVIF; Noctiluca shows Chrome's own pixels of it). |

## GitHub

[`real-github.tape`](../../tapes/live/real-github.tape) opens
[this project's repository](https://github.com/buchochelliq-labs/rs-rich-cli)
with `--images kitty`, then switches to reader mode (`r`) and finds the
README's "Install and try" (`/`).

| The projection | The README in reader mode |
|---|---|
| ![The repository page in the projection: the repository navigation, the branch and Code buttons, the latest commit, the file list with commit messages and dates, and the About panel with topics, releases, contributors' avatars and languages](../../media/tapes/live/real-github/github-projection.png) | ![The README in reader mode: the "Install and try" heading highlighted, shell commands and a Rust snippet in code blocks, and the release notes as a bulleted list](../../media/tapes/live/real-github/github-readme.png) |
| The projection with GitHub's styles: the file list beside the About panel, contributors' avatars in Kitty graphics. | Reader mode: headings, code blocks and lists, the match from `/` highlighted. |

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
which streams a meteor radar around the clock, with `--images kitty`.
Twitch starts the stream itself, muted, so no key is pressed.

| The directory | A live channel |
|---|---|
| ![Twitch's category directory in half blocks: the followed-channels sidebar's avatars, category icons, and large category covers drawn as coarse coloured cells](../../media/tapes/live/real-twitch/twitch-directory.png) | ![A Twitch channel page in Kitty graphics: the stream's frame (a radar spectrogram and a photograph of the observatory) in the player, an image of a question mark above it, the chat panel on the right, and LIVE with the viewer count](../../media/tapes/live/real-twitch/twitch-channel.png) |
| The directory with half blocks, which work in any terminal: two pixels to a cell, so the covers are coarse. | The channel in Kitty graphics, its live stream playing in the player's region: the status bar reads `PLAYING 0:13 / live`, and the photograph in the stream changes from frame to frame (see the animation). The purple question mark above it is an image the page draws over the player. |

![An animation: Twitch's directory in half blocks, then the channel page with its live stream playing](../../media/tapes/live/real-twitch/real-twitch.gif)

## What did not work

Recording these pages found bugs, fixed in the same change, each with a
test on a page reduced from the site:

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
- **Hacker News's ranks out of line.** From rank 10, "10." wrapped its
  period onto a row of its own and every rank after it sat one row off
  its title. A table row's cells now start on one terminal row, rows keep
  their order, and a box too narrow for its longest word grows into free
  cells beside it.
- **Columns one letter wide.** Snapping a grid to equal columns clamped
  boxes past an item's new width to its last column (Wikipedia's account
  links, MDN's "Learn more"). They now keep their place, scaled to the
  item's width.
- **The Guardian's pictures never came, and their alt text showed
  twice.** The Guardian serves AVIF to Chrome, which Noctiluca's decoder
  does not read; the picture failed quietly. A picture in a format it does
  not read now comes from Chrome's own pixels of its box. The alt text is
  shown once, in the `Image:` caption.

Still open:

- **The Guardian's projection** draws pictures that are still loading
  (far down the page) as placeholders, and its narrow caption column cuts
  captions short at 120 columns.
- **MDN at 120 columns** leaves large vertical gaps. One recording caught
  MDN's header before its layout settled, drawn over the example; taking
  it again gave the picture above.
- **Twitch's sidebar** repeats its tooltip, "Use the Right Arrow Key to
  show more information.", once for each avatar.
- **YouTube** answers the recording machine with its bot wall, so no video
  is shown playing there.

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
