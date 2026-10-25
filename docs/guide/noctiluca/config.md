# Configuration

Noctiluca reads one TOML file: `noctiluca/config.toml` in your config
directory.

| System | The file |
|---|---|
| Linux and other Unix | `$XDG_CONFIG_HOME/noctiluca/config.toml`, else `~/.config/noctiluca/config.toml` |
| macOS | `~/Library/Application Support/noctiluca/config.toml` |
| Windows | `%APPDATA%\noctiluca\config.toml` |

`--config PATH` reads another file. With no file, Noctiluca uses its
defaults; a file named with `--config` must exist.

A TOML syntax error stops Noctiluca with the line. Anything else it does
not understand (an unknown table or key, a value of the wrong kind) is a
warning, never an error, so an old config still opens. The first warnings
show on the status line when it starts; `noctiluca keys` prints them all.

The crate has a documented sample with every table,
[`config.sample.toml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/noctiluca/config.sample.toml).

```toml
[startup]
home = "https://docs.rs"
view = "projection"

[search]
url = "https://html.duckduckgo.com/html/?q={}"

[shortcuts]
gh = "https://github.com/search?q={}"
wiki = { search = "https://en.wikipedia.org/w/index.php?search={}", home = "https://en.wikipedia.org" }

[keys]
preset = "vim"
hints = "f ctrl+g"

[media]
audio = false
images = "auto"

[profile]
# name = "work"

[site."github.com"]
view = "reader"
keyword = "s"

[site."github.com".keys]
n = "scroll.down"
"]" = { role = "link", name = "Next" }
```

The command line wins over the file: `--reader` over `[startup] view`,
`--images` over `[media] images`, `--profile` over `[profile] name`, and
`--audio` turns sound on even when the file says `false`.

## `[startup]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `home` | string | none: an empty page | The page to open when none is named on the command line, and when the address bar is left empty |
| `view` | `"projection"`, `"reader"` or `"raster"` | `"projection"` | The first view. `raster` starts the debugging view |

## `[search]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `url` | string with `{}` | `"https://html.duckduckgo.com/html/?q={}"` | The search engine for words the address bar does not read as an address or a keyword. `{}` takes the words, escaped. An address without `{}` is a warning, and the default stays |

## `[shortcuts]`

Keywords, over the built-ins (`rs`, `crates`, `mdn`, `gh`). Each key is a
keyword, and its value is one of:

| Value | Meaning |
|---|---|
| a string with `{}` | A search template. The keyword alone opens the template's site (its scheme and host) |
| a string without `{}` | An address the keyword opens, with or without words |
| `{ search = "…", home = "…" }` | A template and a home of their own. Either may be left out: with no `home`, the template's site; with no `search`, the keyword opens `home` |
| `false` | Removes a built-in keyword |

See [Keywords](keys.md#keywords).

## `[keys]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `preset` | `"vim"` or `"plain"` | `"vim"` | The default key set |
| any action id | string of key names | the preset's | The keys for that action, as alternatives separated by spaces. An empty string unbinds it |

`noctiluca keys` lists the ids. A key name Noctiluca does not know drops
every line of `[keys]` (see [Your own keys](keys.md#your-own-keys)).

## `[media]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `audio` | boolean | `false` | Let pages play sound. It plays on the machine Chrome runs on |
| `fps` | integer, 1 to 60 | `8` | The media frame cap. Read, and kept for video in 0.0.24: it changes nothing in 0.0.22 |
| `images` | `"auto"`, `"ansi"`, `"kitty"`, `"sixel"`, `"iterm2"` or `"none"` | `"auto"` | How pictures reach the terminal. See [Images](media.md#how-pictures-reach-your-terminal) |

## `[profile]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string: letters, digits, `-` and `_`, up to 64 | none: a temporary profile each run | Keep a browser profile under this name, with its cookies and storage, between runs. See [the browser profile](privacy.md#the-browser-profile) |

## `[site."…"]`: per-site settings

A `[site."PATTERN"]` table applies to the pages it matches. The pattern is
a host (`github.com`), a wildcard (`*.example.com`: the domain and every
host under it) or an address prefix (`"https://example.com/docs/"`). When
several match, the most specific wins: a prefix, then a host, then a
wildcard, and a longer one over a shorter. Only one table applies to a
page.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `view` | `"projection"` or `"reader"` | the current view | The view a page of this site opens in (`raster` is ignored here) |
| `images` | boolean | `true` | `false` shows no pictures on this site, only alt text |
| `audio` | boolean | sound as the run started | `false` keeps this site's media muted. `true` cannot turn sound on in a run that started without it |
| `fps` | integer, 1 to 60 | the `[media]` cap | A frame cap for this site. Read, and kept for video in 0.0.24: it changes nothing in 0.0.22 |
| `width` | integer, 20 to 1000 | the terminal's width | The width in cells Chrome lays this site's pages out at |
| `keyword` | string, one word | none | `KEYWORD WORDS` in the address bar searches this site (`site:HOST WORDS` with your search engine) |
| `keys` | table | none | Keys for this site only. See below |

The settings apply when a new page of the site opens.

### `[site."…".keys]`

Each key is one key name, and its value is what it runs:

| Value | Meaning |
|---|---|
| `"ACTION"` | An action, by its id: `n = "scroll.down"` |
| `{ role = "…", name = "…" }` | The element with this accessible role and name, looked up when the key is pressed. `nth = 2` (counting from 1) picks the second of several with the name. No other key is allowed: no selector, no script |
| a list of those | Each in turn: `x = ["top", { role = "link", name = "Home" }]` |

Roles are the accessibility tree's: `link`, `button`, `textbox`,
`searchbox`, `checkbox`, `switch`, `radio`, `combobox`, `listbox`,
`slider`, `spinbutton`, `tab` and `menuitem`. `Ctrl+K` in Noctiluca
writes these lines for you, keeping the file's comments and layout. See
[Keys on one site](keys.md#keys-on-one-site).

## Other files

Noctiluca keeps its data in its data directory: `$XDG_DATA_HOME/noctiluca`
(else `~/.local/share/noctiluca`) on Linux, `~/Library/Application
Support/noctiluca` on macOS, `%LOCALAPPDATA%\noctiluca` on Windows.

- `bookmarks.toml`: your [bookmarks](reading.md#bookmarks), written only
  when you add or remove one.
- `profiles/NAME/`: a [kept profile](privacy.md#the-browser-profile).
