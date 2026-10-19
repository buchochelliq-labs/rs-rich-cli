# intuiTUIve gaps: termusic

## Round 1: termusic

[`examples/music.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/music.rs)
rebuilds [termusic](https://github.com/tramhao/termusic), the terminal
music player (tui-realm on ratatui), on intuiTUIve with no framework
change. It needs the crate's `wav` and `art` features. It is 1,556 lines
(after rustfmt): about 560 are the domain (the library scan with
termusic's filters and alphanumeric order, a WAV `LIST`/`INFO` tag reader
through std, an `.lrc` parser with termusic's caption rule, the playlist
and transport state as signals in a `Copy` struct), and the rest the
panes, the search pop-up and the keys. Its tests
([`tests/music.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/tests/music.rs), 508
lines, 17 tests) write a library into a temporary directory for each test
(16-bit WAV files written byte by byte, two with an `INFO` chunk, a
`cover.png`, an `.lrc` file, a fake `.mp3`, a hidden file and a text file),
drive the app with termusic's keys, play on the app's clock
(`wait_for_tasks(true)`, `Driver::advance`) and assert on the rendered
text and, for the current lyric line, a cell's style.

spotify-player was not chosen: it needs a Spotify account and the
network. Its views beyond termusic's are listed at the end, with what they
would need.

What is there, with termusic's layout, strings and default keys:

- **The tree view**: the library on the left third, and on the right the
  playlist over the 3-row progress gauge and the 4-row lyrics pane, the
  footer (`Help: <control+h> Config: <shift+C> Library: <1> …`) on the
  last row. Rounded borders, blue while focused, the highlight in light
  yellow.
- **Library**: a tree of the music folder (folders and the extensions
  termusic plays, hidden files left out, alphanumeric order). `l` adds a
  track (or opens a folder), `L` a folder's tracks, `d` deletes (asked
  first, and the playlist follows), `y`/`p` move a file, Enter opens a
  folder as the root and Backspace goes back up with the old root
  selected, `o`/`a`/`A` cycle, add and remove roots, `/` searches.
- **Playlist**: termusic's table (a marker column with 🚀 on the cursor
  over ► on the playing track, `[ 03:25 ]`, the title in bold, artist,
  album) and its title, `─ Playlist ──┤ Total 2 tracks | 00:10 | Mode: 🔁
  ├─`. `l` plays, `d`/`D` delete one or all, `r` shuffles, `m` cycles the
  loop mode in termusic's order (🔁 → ⮕ → 🔂 → 🔀), `K`/`J` move a track,
  `s` adds 20 random tracks, `S` a random album, `/` searches.
- **Now playing**: the gauge's title ` Status: Running | Volume: 30 |
  Speed: 1.0 | Gapless: True ` and its label `00:02    -    00:06`
  (`--:--` stopped), rich-ext's `FillGauge` (ratatui's `Gauge`) under it.
  Space pauses or starts, `n`/`N` next and previous, `+`/`-` volume by 5,
  `0` restarts, `f`/`b` seek 5 s, Ctrl+F/Ctrl+B the speed, Ctrl+G gapless.
  The end of a track goes on by the loop mode: the same track, the next
  (round), the next until the last, or a random one.
- **Lyrics**: an `.lrc` beside the track (`[mm:ss.xx]`, several stamps a
  line, `[offset:]`), the pane titled ` Lyrics of Artist - Title `, the
  current line lit and kept in view as the clock moves, with termusic's
  two seconds of lead; `F`/`B` move them a second; `Stopped.` and `No
  lyrics available.` as termusic says them.
- **The cover**: the first `jpg`/`jpeg`/`png` in the track's folder (not
  `artist.*`), drawn in half blocks over the playlist's bottom right
  corner, where termusic puts it; Ctrl+Shift+arrows move it, Ctrl+Shift+
  PgUp/PgDn size it, Ctrl+Shift+End hides it.
- **Search** (`/`): termusic's general search, a box 65% by 68% with
  ` Search for: (support * and ?) ` over the results (`idx`, `File name`
  for the library; `Duration`, `Artist`, `Title` for the playlist), `*`
  and `?` wildcards without case; Enter in the box goes to the results,
  Enter there locates the item (the tree opens to it), `l` loads or plays.
- **Help** (Ctrl+H, by section), **quit** (`q`/Esc, asked), **save
  playlist** (Ctrl+S, an `.m3u` in the root), errors as notifications.

Playback is simulated. intuiTUIve's `audio` plugin captures but has no
output, so the transport is a 100 ms `every` on the app's clock, scaled by
the speed. A WAV is opened with `audio::wav` for its length and to check
that it decodes; other formats are listed and queued, and playing one
says `No decoder for .mp3 files`. Tags come from a WAV's `INFO` chunk
(parsed with std, since hound skips it), else from the file name
(`Artist - Title`). Volume, speed and gapless are shown and change the
clock (speed), but there is nothing to apply them to.

Left out: sound itself, decoders other than WAV, ID3/Vorbis/MP4 tags and
embedded covers and lyrics, the database view (`2`, SQLite of tags), the
podcast view (`3`, RSS and downloads), YouTube search and download, the
tag editor (`t`), the config editor (`C`), the sort pop-up (`,`), lyric
download and the lyric frames switch (`T`), themes and the config file,
MPRIS and Discord, the client/server split (termusic's TUI talks to a
`termusic-server` over gRPC), and the cover through Kitty, Sixel or
ueberzug. `2`, `3`, `C`, `t`, `s` (library) and `,` say they are not in
this clone.

What is bespoke, and why:

- **No widget of its own.** Every pane is a core node: `tree_with`,
  `table_with`, `list`, `renderable(FillGauge)`, `image_of`, `layers`,
  `input`, `cx.modal`, `cx.confirm`, `cx.prompt`, `cx.notify`, `cx.help`.
- **Titles that change** (`titled`, 10 lines): a panel's title is fixed
  when it is built, so each pane is a `layers` of the panel with an empty
  title and a `text` placed on its top row (`place(align, Start)`,
  `inset(0, 1, 0, 1)`). It looks as termusic's titles do, but it is not
  the panel's title (not its accessible name, not in `border.focused`).
- **The data source**: the RIFF `INFO` reader (40 lines) and the `.lrc`
  parser (40) are what a tag plugin and a lyrics plugin would hold; the
  wildcard matcher (15) is termusic's search rule.

### New gaps and bugs

| Gap or bug | Needed for | API sketch or repro | Where |
|---|---|---|---|
| No audio output: `audio` opens inputs only (`audio::input`, `audio::open`, `devices` lists capture devices) | playing anything: termusic's whole point; the clone simulates the clock | plugin (`audio`): `audio::output(OutputOptions::new().device(id).rate(r).channels(c))` taking `Matrix` buffers, `audio::output_devices()`; and a player on it: `audio::player(source)` with `play`, `pause`, `seek(d)`, `volume(f)`, `speed(f)`, `gapless`, a `position()` signal, an `on_end` hook | plugin, over the core transport below |
| No transport on the app's clock: `audio::play` and `latest_iter` play an iterator at a pace but cannot pause, seek or change rate, and a handler cannot read the clock (`Ctx` has no `now()`; only `DrawCx::now`) | the position, pause, seek, speed and the end of a track; the clone adds 100 ms a tick and checks the end itself | core: `let t = transport(length)`: `t.position()` (a signal), `play`/`pause`/`seek(d)`/`rate(f)`, `t.on_end(\|cx\| ..)`, on the app's clock (a test advances it); and `cx.now()` | core (generic: media, slideshows, replays, animations) |
| Decoders: only WAV (hound) | mp3, flac, ogg, opus, m4a; symphonia is not in the lockfile | plugin: `audio::decode(path) -> Decoded { rate, channels, duration, frames(): Iterator<Matrix>, seek(d) }`, `audio::wav` given `seek` too | plugin |
| No tag reader | ID3v2, Vorbis comments, MP4 atoms, FLAC: title, artist, album, duration, the embedded cover and lyrics (termusic uses lofty); the clone parses WAV `INFO` itself and falls back on the file name | plugin: `tags::read(path) -> Tags { title, artist, album, track, duration, cover: Option<Bytes>, lyrics: Option<String> }`, and `tags::write` for a tag editor | plugin (none in core: a domain) |
| An image from bytes: `image`, `image_with` and `image_of` take a path | an embedded cover (bytes from a tag), a cover fetched over the network (spotify-player) | `image_from(\|\| Option<Arc<[u8]>>, view)`, or `ImageSource::{Path, Bytes}` in `image_of` | plugin (`art`) |
| A tree has no options: no highlight symbol, no style per row, no way to say which keys it takes (Enter and Space toggle) | termusic's 🦄 symbol; Enter as "open as root" and Space as pause needed `previews_keys` on the pane and on the root | `tree_with(..).symbol("🦄")`, `.row_style(\|path\| ..)`, `TreeOptions::toggle_keys("")`, the list options a tree could share | core |
| A tree is selected and expanded by index paths, with no key and no reveal | locating a search result (the clone walks the entries for the indices, inserts every ancestor's path into `expanded`, and resets both after each rescan); `tree_lazy` has keys but cannot reveal a key (open its ancestors) or reload a level after a delete | `tree_keyed(items, key_of, selected: Signal<Option<K>>)` with `.reveal(&key)` and `.reload(&key)`; `tree_lazy(..).reveal(key)` | core |
| A table has no marker column or per-row symbol | termusic's first column (🚀 on the cursor, ► on the playing track); the clone puts them in a cell, so its rows closure reads the cursor and every move rebuilds every row | `TableOptions::marker(\|i\| Option<markup>)` (as a list's `.marker`), with `.symbol(..)` drawn over it | core |
| No seekable progress bar | a click or drag on the gauge to seek (termusic has none, but every GUI player and spotify-player's progress does); `renderable(FillGauge)` has no idea where it was clicked | `gauge(\|\| ratio).label(\|\| ..).on_seek(\|ratio, cx\| ..)`, or a `slider(value)` node | core |
| A list cannot keep its selection centred | synced lyrics read best with the current line in the middle (spotify-player's lyrics page, every karaoke view); the clone keeps it in view only | `list(..).scroll_padding(Center)` or `.keep_centered()` | core |
| The help lists only the focused node's path | termusic's help is one table of every section (Global, Library, Playlist, …) wherever the focus is; with the playlist focused, the library's keys are not listed | `HelpOptions::new().whole_app(true)`: every described binding of the screen, by section | core |
| No percentage offsets in layers: `inset` is cells | termusic places the cover by percentages of the terminal (x 100%, y 77%, width 20%) | `.inset_percent(top, right, bottom, left)` or `.place_at(x%, y%)` | core (low) |
| No media keys, MPRIS or now-playing integration | termusic's and spotify-player's MPRIS (play/pause from the desktop, the lock screen) | plugin: `mpris(app_name)` on D-Bus (zbus, not in the lockfile), feeding commands; core shape: named commands from outside the terminal reaching bindings, `cx.run_command("Pause current track")` / `App::commands(source)` | plugin + core |

### Friction with the existing APIs

- **Placement belongs to the `layers` child.** The cover's `switch` (to
  rebuild when it moves or hides) had `.place(..)` on the node it built;
  the `switch` itself took the whole rectangle and blanked the playlist
  under it. Moving `.place` and `.inset` onto the `switch` fixed it. A
  debug warning for `place` on a node whose parent is not `layers` would
  have said so.
- **Panel titles that follow state** (already listed in bottom's round 1):
  three of termusic's four panes change their titles every second. The
  `titled` overlay works and looks right; `panel_with(|| ..)` would make
  it the panel's own title.
- **Two `previews_keys`.** The root previews keys so Space pauses rather
  than toggling a tree folder, and the library pane previews them so Enter
  opens a folder as the root rather than toggling it.
- **A theme style per pane** is `theme_styles([("selected", ..),
  ("tree.selected", ..)])` on each widget: it works, but a tree, a table
  and a list each look their styles up under different names.
- **The confirm is Yazi's.** termusic's quit and delete boxes are a
  30x3 radio (`Yes  No`); `cx.confirm` draws Yazi's box with `[Y]es (N)o`.
  A `ConfirmOptions::compact()` would port small yes/no boxes.
- **A WAV is read twice when queued** (hound for its length, std for its
  `INFO` chunk) and opened again when played. With an output it would be
  read as it plays, but `audio::wav` has no `seek`, so `f`/`b` would have
  to read again from the start.

### Components that would help

| Component | What it would replace or enable in music.rs | API sketch | Where | Value |
|---|---|---|---|---|
| An audio player: output, decoders, transport | the simulated clock (`tick`, `seek`, `next`'s end check), sound itself; termusic's three backends (symphonia/rodio, mpv, GStreamer), spotify-player's librespot sink | core: `transport(length)` (position signal, play/pause/seek/rate, `on_end`, on the app's clock); plugin (`audio`): `output(..)`, `decode(path)`, `player(decoded, output)` with volume, speed and gapless | core + plugin | high |
| A tag reader | `riff_info`, `Track::read`'s name parsing; embedded covers and lyrics | plugin: `tags::read(path)`, `tags::write(path, tags)` (lofty's shape) | plugin | high |
| A playback queue | the playlist half of `Player` (current index kept through removes, moves and shuffles; next and previous by loop mode; random picks) | core: `queue::<T>()` with `current()`, `next(Repeat::All \| One \| Off \| Shuffle)`, `previous`, `remove`, `swap`, `shuffle(seed)`, `play(i)` (generic: playlists, slideshows, a task runner's queue) | core | medium |
| A keyed tree with reveal | `Entry::locate`, `locate_path`, the `expanded` bookkeeping, the rescans that reset the selection | `tree_keyed(..)` with `.reveal(key)`, `.symbol(..)`, `.cursor_by(key)` as a list's; termusic's library and every file tree (Yazi's, gitui's) | core | high |
| A time-synced list | `lyric_lines`, the `memo`/`watch` pair that follows the position, `Lyrics::index_at` | core: `follow_time(cues: \|\| Vec<Duration>, position: Signal<Duration>) -> Memo<Option<usize>>` and `list(..).keep_centered()` (subtitles, transcripts, logs by time); plugin: `lrc::parse(text)` with offsets and several languages | core + plugin | medium |
| A seekable gauge | `renderable(FillGauge)` and the label; seeking by click | `gauge(ratio).label(..).on_seek(..)` (ratatui's `Gauge`, Bubbles' `progress`, Textual's `ProgressBar`) | core | medium |
| A search pop-up over a table | `search`: the modal, the input, the filtered rows memo, Enter to the results, the Esc | `finder_table(columns, rows, matcher, on_pick)`: the core `fuzzy_list` with columns and a matcher of your own (wildcards, substrings, fuzzy) | core | medium |
| Images from bytes, and Kitty/Sixel cells | an embedded or fetched cover; a sharp cover where the terminal can show pixels (termusic's `cover` feature, spotify-player's) | `image_from(bytes)`; the paint step passing rs-rich-art's Kitty/Sixel placements through (already noted in the guide) | plugin (`art`) + core | medium |
| Whole-app help | the help with the playlist's keys while the library is focused | `HelpOptions::whole_app(true)` | core | medium |
| MPRIS and media keys | play/pause/next from the desktop, the now-playing widget of a desktop | plugin: `mpris(..)` (D-Bus); core: named commands from outside | plugin + core | medium |
| m3u playlists | `save_playlist`, and loading one (termusic's database view lists them) | plugin: `playlist::read_m3u(path)`, `write_m3u(path, paths)` | plugin | low |

### What spotify-player would add

spotify-player (ratatui) has views termusic does not, each needing more
than this clone used: a **library page** (playlists, followed artists,
saved albums in three panes), a **search page** (tracks, albums, artists
and playlists, four result panes filled as you type), a **context page**
(an album's, artist's or playlist's header over its track table, top
tracks and related artists for an artist), a **browse page** (categories,
then their playlists), a **lyrics page**, a **queue pop-up**, a **device
picker** (Spotify Connect devices) and a **command palette** of its
actions. It streams through librespot, shows the cover through Kitty,
Sixel or iTerm2, sends desktop notifications on a track change, and
exposes MPRIS. On intuiTUIve those need, beyond the gaps above: an HTTP
client plugin with OAuth (the Web API), panes of results that fill as
requests return (the core `resource_for` per pane, with a loading state),
`image_from(bytes)` for covers fetched over the network, and an output
device picker (`audio::picker` lists inputs only).
