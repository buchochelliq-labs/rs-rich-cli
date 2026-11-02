# Write a plugin

This page takes you from nothing to a plugin anyone can install with
`rich plugin install`: a WASM module, made from the project's template,
built, tried with the `rich` command, then listed on the
[marketplace](../marketplace.md).

**No one outside the project has written one yet.** If you do, yours is the
first community plugin: open a listing pull request (step 6) and say so on
the issue tracker. A plugin that is small, useful and well tested is the
best kind of first one.

There are two kinds of plugin, and this page is about the first:

- **A WASM plugin** runs in a sandbox. It imports nothing, so it cannot read
  files, open connections, see the clock or start processes, and every call
  runs in a fresh instance with a budget of fuel and memory. That is why it
  is the kind the marketplace installs. It exchanges text with its host and
  can add four kinds of capability: text transforms, highlighters, and
  Markdown fences drawn as markup or as ANSI text.
- **A Rust crate** implements the `Plugin` trait from `rs-rich-plugin-api`
  and can add anything the registry holds: themes, box styles, renderers,
  code highlighters, actions and interactive components. It is native code,
  so the marketplace lists it but never installs it; a user adds it with
  `cargo add`. See [Extending](../PLUGINS.md#writing-a-plugin-0012).

## 1. What you need

```bash
rustup target add wasm32-unknown-unknown   # the target the module is built for
cargo install cargo-generate               # 0.23.4 or later
cargo install rs-rich-cli                  # `rich`; it loads WASM plugins by default
```

## 2. Make the project

```bash
cargo generate --git https://github.com/buchochelliq-labs/rs-rich-cli \
  templates/wasm-plugin --name my-plugin
cd my-plugin
```

The project name is the plugin's id: its manifest's `name`, and its
listing's `id` on the marketplace. It follows the plugin name rule:
lowercase letters, digits, `-`, `_` and `.`, starting with a letter or digit,
at most 64 bytes.

The maintainer will also make a GitHub template repository from
[`templates/wasm-plugin/`](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/templates/wasm-plugin);
once it exists, its "Use this template" button gives the same project
without cargo-generate (rename the package in `Cargo.toml` to your id).

What you get:

| File | What it is |
|---|---|
| `src/lib.rs` | The plugin: its `MANIFEST`, the `call` that dispatches each capability, and the capabilities themselves, with unit tests |
| `src/abi.rs` | The four exports the host calls. It needs no changes |
| `Cargo.toml` | A `cdylib` with no dependencies, and a release profile for small, reproducible modules |
| `.cargo/config.toml` | `cargo wasm`, the release build for `wasm32-unknown-unknown` |
| `.github/workflows/release.yml` | Tests every push; on a `v*` tag, builds the module and attaches it to the GitHub release |
| `README.md` | These steps, short |

The template has no dependencies, and that matters: a crate that imports
anything (WASI, `wasm-bindgen`, a clock, randomness) makes the host refuse
the module. The standard library alone imports nothing on
`wasm32-unknown-unknown`.

## 3. Write the plugin

The template's plugin adds three capabilities: a `title-case` transform, a
`todo` highlighter and a `callout` fence. Its manifest says so, one line per
capability, and `call` runs capability number *n*, the *n*th line:

```rust
pub const MANIFEST: &str = concat!(
    "rich-plugin-abi 1.0\n",
    "name ",
    env!("CARGO_PKG_NAME"),
    "\n",
    "version ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "description ",
    env!("CARGO_PKG_DESCRIPTION"),
    "\n",
    "capability transform title-case\n",
    "capability highlighter todo\n",
    "capability fence-markup callout\n",
);

pub fn call(capability: u32, input: &str, width: u32) -> Result<String, String> {
    match capability {
        0 => Ok(title_case(input)),
        1 => Ok(todo_spans(input)),
        2 => Ok(callout(input, width)),
        _ => Err(format!("unknown capability {capability}")),
    }
}
```

Replace them with yours. Each kind is given text and returns text:

| Kind | Given | Returns | Used by |
|---|---|---|---|
| `transform` | plain text | plain text | `--transform NAME` |
| `highlighter` | plain text | one `START END STYLE` line per span: byte offsets on character boundaries, and any style `rich` parses | every printed text, as rich's own highlighters are |
| `fence-markup` | a Markdown fence's code | `rich` console markup | a ```` ```NAME ```` fence in `rich -m` |
| `fence-ansi` | a Markdown fence's code | text with ANSI SGR styling | the same; its listing declares `ansi-output` |

Every call is also given the width available, in cells. An `Err` is shown
to the user as your plugin's error.

What the host enforces, so you know where the edges are:

- **Its output is sanitized.** Terminal controls are made visible, a
  highlighter's styles and markup lose any hyperlink, and only `fence-ansi`
  keeps SGR codes. Escape user text before you put it in markup (the
  template's `escape` does).
- **Limits per call** (`WasmLimits::default()`): 50 million units of fuel
  (roughly, instructions), 64 MiB of memory, and 4 MiB of output. A module
  file is at most 16 MiB.
- **No state between calls.** Each call runs in a fresh instance, so a
  `static` starts from scratch every time, and memory is never freed.
- **Names** follow the plugin name rule, and two plugins may not register
  the same capability under the same name: pick names that will not clash.

## 4. Build and test

```bash
cargo test     # the unit tests, natively
cargo wasm     # target/wasm32-unknown-unknown/release/my_plugin.wasm
```

Unit-test the capabilities as ordinary Rust functions; `cargo test` runs on
your machine, not in WASM. The module itself is what step 5 checks.

## 5. Try it with `rich`

`--plugin PATH` loads a module for one run, with the same sandbox and checks
an install uses:

```bash
wasm=target/wasm32-unknown-unknown/release/my_plugin.wasm
rich plugins info my-plugin --plugin "$wasm"   # what the host read: version, ABI, capabilities
rich --plugin "$wasm" --transform title-case -p "hello there"
rich --plugin "$wasm" -p "TODO: ship it"        # the highlighter marks TODO
printf '```callout\nBack up first.\n```\n' > note.md
rich --plugin "$wasm" -m note.md
```

A module the host refuses says why: an import, a missing export, a
manifest that does not parse, an ABI it does not support or a capability
kind it does not know. To load it on every run instead, list it in your
own config (a project's `./rich.toml` may not list plugins):

```toml
# ~/.config/rich/config.toml
[defaults]
plugins = ["plugins/my_plugin.wasm"]   # relative to this file
```

## 6. Publish it on the marketplace

The marketplace stores where your module is and what it hashes to, never
the module itself: you host the file, on your repository's releases. A
listing is one JSON file, `packages/ID.json`, in
[rs-rich-marketplace](https://github.com/buchochelliq-labs/rs-rich-marketplace);
its format is in [the listing format](../design/marketplace.md).

1. **Release the module.** Tag the version (`git tag v0.1.0 && git push
   --tags`). The template's workflow builds `my-plugin.wasm` and attaches
   it, with its SHA-256, to the release, at
   `https://github.com/OWNER/REPO/releases/download/v0.1.0/my-plugin.wasm`.
2. **Write the listing** with `rich plugin publish`. It needs no network:
   it loads your local copy in the sandbox, takes the id, version, ABI and
   capabilities from the manifest, computes the SHA-256 and size, and reads
   the listing back with the marketplace client's own parser:

    ```bash
    rich plugin publish my-plugin.wasm \
      --url https://github.com/OWNER/REPO/releases/download/v0.1.0/my-plugin.wasm \
      --source-revision "$(git rev-parse v0.1.0)" \
      --license "MIT OR Apache-2.0" --repository https://github.com/OWNER/REPO
    ```

    Publish the very file you uploaded, so the hash matches. It writes
    `packages/my-plugin.json` (or `--output FILE`); `--author`,
    `--keyword`, `--homepage` and `--name` fill in the rest. For a later
    version, `--listing packages/my-plugin.json` adds it to the existing
    listing, newest first.
3. **Open a pull request** to rs-rich-marketplace that adds the file. Its
   `check-listings` workflow validates the listing, downloads your module
   and checks its size and SHA-256, then installs it with `rich` from a copy
   of the marketplace signed with a throwaway key. A name that looks like an
   official one, or uses "official" or "rs-rich", waits for a maintainer.
4. **When it merges,** the marketplace's signing workflow rebuilds and signs
   `index.json`, and the plugin installs anywhere:

    ```bash
    rich plugin search my-plugin
    rich plugin install my-plugin
    ```

A new listing is in the `community` tier: checked by the marketplace's CI
only. The `reviewed` tier ("Approved") means a maintainer read your source
and rebuilt the same file from `source_revision`, which is why the template
builds reproducibly: keep `Cargo.lock` committed, and say in your README
which Rust version built the release. See [Tiers](../design/marketplace.md#tiers).

## 7. Versions

Two numbers matter, and they are separate:

- **Your plugin's version** is the package's: the manifest reads it from
  `Cargo.toml`, and the listing's version must equal it. A listed version
  never changes, so bump it for every release. To withdraw one, a later
  pull request marks it `yanked`, with a reason.
- **The ABI**, the `rich-plugin-abi 1.0` line, is what a host checks. It is
  `rich_plugin_api::abi::ABI_MAJOR.ABI_MINOR`, and your listing's
  `requires.abi`. A host loads a plugin of its own major; a newer minor adds
  things (a new capability kind, say) that an older host refuses only when
  you use them. If the major ever changes, rebuild against the new ABI and
  list a new version; `rich plugin install` picks the newest version this
  host loads, so users on older hosts keep the old one.

`PLUGIN_API_VERSION` is a third number, for Rust crate plugins only: it
versions the Rust traits, and a WASM plugin never sees it.

## See also

- [Extending](../PLUGINS.md): the whole plugin system, and crate plugins.
- [The listing format](../design/marketplace.md): every key of a listing,
  tiers, signing and yanking.
- [Plugin loading](../design/plugin-loading.md): why runtime plugins are
  text in, text out.
- [`examples/wasm/shout.wat`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-plugin-api/examples/wasm/shout.wat):
  the same ABI written by hand, in WebAssembly's text format.
