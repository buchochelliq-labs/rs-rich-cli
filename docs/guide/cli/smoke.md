# Smoke test

`scripts/smoke_cli.py` runs every `rich` command and render mode once against a
build of the binary and tells you, in about ten seconds, whether any of them is
broken. Use it after building a release candidate, after a change that touches
argument handling or a renderer, or on a new platform.

It is a smoke test, not a snapshot test: it checks that each command runs,
exits with the right code and prints a key piece of its output. Exact output is
covered by the snapshot and golden tests.

## What it covers

The tool writes its own sample files into a temporary directory (the same files
the [walkthrough](walkthrough.md) uses) and runs 86 cases:

| Area | Cases |
|---|---|
| Render modes | `print` (argument and stdin), Markdown (auto and `markdown --hyperlinks`), syntax (auto and `syntax --width`), JSON, CSV (file and stdin), notebook, `jsonl`, `log` (plain, `--log-presentation rich`, stdin), `rule`, `--format auto` on piped JSON, YAML and plain text |
| Structured data | `inspect` with no option, `--select`, `--find`, `--flatten`, `--redact`, `--compare`, and JSON from stdin |
| Text diffs | two files, `--side-by-side`, a patch on stdin, and a failing `--threshold` (exit 5) |
| Art | `image` in blocks, ASCII, quadrants and Braille; `--image-fit cover` with an anchor; `--image-background` with a colour, `default` and `checkerboard`; `--image-color ansi16 --image-dither bayer4x4`, and `atkinson` with `--image-color-distance oklab`; rotation, grayscale and contrast; `gif`; an image `diff` and a failing image gate (exit 5) |
| Escapes | `ansi explain`, `--ansi-inline`, `--sanitize` |
| Viewers | `view` on source with `--search`, Markdown and JSON from stdin; `hex` with a search; `unicode`; `env` with masking; `capture` |
| Layout and export | panel with title, caption, padding and border style; centring with a fixed width and style; `--export-html` (checks the file); a named theme, `--theme-style` and `--theme-file` |
| Workflows | `--pager` (with `PAGER=cat`; redirected output is not paged), `--watch` on two files (renders once when redirected), `--batch --dry-run`, and a real `--batch --jobs 2` (checks both HTML files) |
| Configuration | `config validate`, `config show`, `config explain KEY`, `config reference`, and an invalid config (exit 2) |
| Generated docs | `--help`, `--version`, completions for bash, zsh, fish and PowerShell, `docs markdown`, `docs config`, `docs man` (checks `rich.1`) |
| Diagnostics and CI | `doctor`, `doctor --report json`, `bench compare` with a regression (exit 5), `--report json` success, and exit codes 2, 3 and 4 |
| Demo | `--demo-list`, and `--demo --demo-section core --no-color` |

`python3 scripts/smoke_cli.py --list` prints every case with its command line.

Each case checks:

- the **exit code** (usually `0`; the gate and error cases expect `2`–`5`);
- a **key substring** in stdout, or stderr for reports and warnings;
- that **files it should create** exist (HTML exports, batch outputs, man pages);
- for cases with a screenshot, that the **`--export-svg` file** was written and
  is an SVG.

Nothing needs a terminal or the network: every case runs with redirected
stdio. The environment is pinned so results do not depend on your machine —
`HOME` and the config directory point at an empty folder, `COLUMNS` is fixed
per case, and `NO_COLOR`, `PAGER`, `COLORTERM` and the `RICH_*` overrides are
removed.

**Image and GIF cases are skipped**, not failed, when the binary was built
without the `art` feature. The tool finds out from `rich doctor --report json`.

## Run it locally

```bash
cargo build -p rs-rich-cli
python3 scripts/smoke_cli.py
```

The default binary is `target/debug/rich` (or `$CARGO_TARGET_DIR/debug/rich`).
Point it elsewhere with `--binary`:

```bash
cargo build -p rs-rich-cli --release
python3 scripts/smoke_cli.py --binary target/release/rich
```

A run prints one row per case and a summary, and exits `1` if anything failed:

```text
case                    status     time  command
----------------------  ------  -------  -------
print                   PASS      0.16s  rich print '[bold magenta]Hello[/] from [green]rich[/]!'
print-stdin             PASS      0.16s  rich print -
markdown                PASS      0.23s  rich notes.md
markdown-command        PASS      0.33s  rich markdown notes.md --hyperlinks
…
demo-list               PASS      0.00s  rich --demo-list
demo-core               PASS      0.43s  rich --demo --demo-section core --no-color

75 passed, 0 failed, 0 skipped in 7.4s
```

A failure explains itself under its row:

```text
jsonl                   FAIL      0.00s  rich jsonl events.jsonl
                                         ↳ exit 2, expected 0; stdout lacks '"database unreachable"'; stderr: rich: …
```

| Option | Effect |
|---|---|
| `--binary PATH` | The binary to test. |
| `-k TEXT` | Only cases whose name contains TEXT; repeat for more (`-k inspect -k diff`). |
| `--list` | List the cases and exit. |
| `--json` | Print `{"binary", "art", "summary", "results": [...]}` instead of the table. Each result has `name`, `command`, `status` (`pass`, `fail` or `skip`), `exit_code`, `seconds` and `detail`. |
| `--keep` | Keep the temporary directory and print its path, to rerun a failing command by hand. |
| `--timeout SEC` | Per-case limit (default 60). A case that hangs fails. |
| `--fixtures DIR` | Only write the sample files into DIR, then exit. The walkthrough uses this. |
| `--screenshots DIR` | Also write each case's `cli_*.svg` screenshot into DIR (see below). |

The tool's own tests are in `scripts/test_smoke_cli.py`. They check the fixture
generators and case table, and run the full smoke test when a built binary
exists (skipping otherwise):

```bash
python3 scripts/test_smoke_cli.py
RICH_BINARY=target/release/rich python3 scripts/test_smoke_cli.py
```

## Run it in CI

After the binary is built, one step is enough; the job fails on a non-zero exit:

```yaml
- name: Smoke-test every CLI command
  run: |
    cargo build -p rs-rich-cli --locked
    python3 scripts/smoke_cli.py --binary target/debug/rich
```

To keep the results as an artifact, use `--json`:

```yaml
- run: python3 scripts/smoke_cli.py --binary target/debug/rich --json > smoke.json
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: smoke-results
    path: smoke.json
```

For a lean build without the `art` feature, the image and GIF cases report
`SKIP` instead of failing:

```bash
cargo build -p rs-rich-cli --no-default-features
python3 scripts/smoke_cli.py
```

The tool needs only Python 3.9+ and its standard library; it draws the PNG and
GIF fixtures itself.

## Regenerate the guide's screenshots

Every `cli_*.svg` in the [walkthrough](walkthrough.md) comes from a smoke case
that carries a `shot` name. `--screenshots DIR` runs the cases as usual and
also exports each of those screenshots into DIR with `--export-svg`, at the
case's fixed width:

```bash
cargo build -p rs-rich-cli
python3 scripts/smoke_cli.py --screenshots docs/media/guide
```

The output is deterministic, so a regenerated screenshot only shows up in
`git diff` when the rendering really changed. Combine it with `-k` to refresh
a few: `--screenshots docs/media/guide -k inspect`.

## Add a case

Cases live in the `CASES` list in `scripts/smoke_cli.py`. A new command or
option usually needs one line:

```python
Case("inspect-depth", ["inspect", "deploy.yaml", "--max-depth", "1"], "servers"),
```

`Case(name, args, expect, …)` fields:

| Field | Meaning |
|---|---|
| `name` | Unique, short, used by `-k`. |
| `args` | Arguments after `rich`. Paths are relative to the fixture directory. |
| `expect` | A substring the output must contain (`""` for none). |
| `code` | Expected exit code (default `0`). |
| `stream` | `"stdout"` (default) or `"stderr"` for where `expect` is looked for. |
| `stdin` | Text piped to the command; otherwise stdin is empty. |
| `art` | `True` if it needs the `art` feature; the case is skipped without it. |
| `creates` | Files the command must write, relative to the fixture directory. |
| `env` | Extra environment variables for this case. |
| `shot`, `width` | Export a `cli_<shot>.svg` screenshot at `width` columns. |

If the case needs a new input file, add it to `write_fixtures`. Keep fixtures
small and deterministic, and do not use the network. Then run the case and the
tool's tests:

```bash
python3 scripts/smoke_cli.py -k inspect-depth
python3 scripts/test_smoke_cli.py
```

If you add a `shot`, regenerate it with `--screenshots docs/media/guide` and
reference it from the page as `../../media/guide/cli_<shot>.svg`.

## See also

- [Walkthrough](walkthrough.md) — the same commands, explained
- [CLI reference](../../cli-reference.md) — every option
- `scripts/snapshot_cli.py` — byte-exact output snapshots for chosen cases
