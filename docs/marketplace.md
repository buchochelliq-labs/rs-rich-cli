# Marketplace

Find plugins, micro packs and themes for `rich`. This page lists what the
[plugin marketplace](https://github.com/buchochelliq-labs/rs-rich-marketplace)
holds, and gives the command that installs each package. In a terminal,
`rich plugin search` and `rich plugin browse` show the same list
([In the terminal](#mkt-terminal)). The marketplace also has its own site:
[buchochelliq-labs.github.io/rs-rich-marketplace](https://buchochelliq-labs.github.io/rs-rich-marketplace/).

<div id="mkt-app" class="mkt" data-base="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/" data-stats="https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json" data-tiers="design/marketplace/#tiers" markdown>

<noscript>
This page needs JavaScript to read the marketplace. Without it, the
listings are in the marketplace repository's
<a href="https://github.com/buchochelliq-labs/rs-rich-marketplace/tree/main/packages">packages folder</a>,
and <code>rich plugin search</code> lists them in a terminal.
</noscript>

</div>

## How to read a package { #mkt-reading }

**Which packages are rs-rich's own.** A package marked
**✓ Official · by rs-rich** is built and published by rs-rich, from
[its repository](https://github.com/buchochelliq-labs/rs-rich-cli). The
mark comes only from the listing's `tier`, which only a maintainer can set,
and the marketplace refuses `official` for a package whose repository is
not rs-rich's. Every other package is someone else's: **reviewed** means a
maintainer read its source and rebuilt the same file; **community** means
the marketplace's checks passed and nothing more, so read its source and
permissions first. A package's own name, description and authors are its
author's words; "Authored by" names them in quotes when they claim to be
rs-rich. [The tiers](design/marketplace.md#tiers) say more.

**Kinds.** A *WASM plugin* runs in a sandbox with no files, network, clock
or processes. A *micro pack* and a *theme* are data. A *crate* is native
code: it is never installed by `rich`, so its command is the `cargo add`
line for your own program.

## Trust { #mkt-trust }

This page only lists packages: it reads the marketplace's `index.json` and
each listing in your browser, checks each listing's SHA-256 against the
index, and sends nothing else anywhere. It does not check the index's
signature. `rich plugin install` does: it verifies the index's minisign
signature with the keys compiled into `rich`, then the listing's hash, then
the downloaded file's size and SHA-256, before it writes anything.

Download counts and ratings come from the marketplace's `stats.json`,
refreshed nightly. They are not part of the signed index, and `rich` never
reads them. A rating is the 👍 and 👎 reactions on the package's GitHub
discussion, and its reviews are that discussion's comments; **Read or write
reviews on GitHub** opens it. A count shows as "—" where nothing was
counted, such as a version published before counting began.

## In the terminal { #mkt-terminal }

```bash
rich plugin search                          # every package: kind, tier, version, author
rich plugin search rot13                    # every word must match: id, name, description,
                                            # keywords, authors or capabilities
rich plugin search --kind theme --tier official
rich plugin info textcase                   # who publishes it, every version, permissions
rich plugin install textcase                # shows what it is and asks first
rich plugin browse                          # all of it in a selector
```

<div class="tape-player" data-cast="../media/tapes/plugin-search/plugin-search.cast" data-poster="npt:0:6">
  <img src="../media/tapes/plugin-search/plugin-search.gif" alt="rich plugin search filtering by tier and kind, rich plugin info, and rich plugin browse, against a fixture marketplace">
</div>

[Tape](tapes/plugin-search.tape) · [Cast](media/tapes/plugin-search/plugin-search.cast) ·
[GIF](media/tapes/plugin-search/plugin-search.gif)

See [Install plugins from the marketplace](cli.md#install-plugins-from-the-marketplace-0022)
for every `rich plugin` command, and [Extending](PLUGINS.md#rich-plugin-installing-from-the-marketplace-0022)
for how the marketplace works and how to publish a package.
