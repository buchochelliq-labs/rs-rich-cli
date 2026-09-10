# Remaining UAT reports checked for 0.0.3

Checked 2026-09-10 against runtime head `baf277c`, with the subsequent CSV review
repair tested separately. These are findings to triage, not additional fixes
claimed by this PR. Issue titles alone overstate how much remains.

## #60 — Windows pager fallback remains suspect

The fallback now exists: `MANPAGER`, then `PAGER`, then a platform default. A Linux
PTY probe with both variables unset successfully invoked a temporary `less`
program and delivered `pager probe\n` with exit 0.

The Windows branch selects `more`, while the report identifies `more.com`.
Rust's [Command documentation](https://doc.rust-lang.org/std/process/struct.Command.html#method.new)
requires non-`.exe` executable extensions explicitly. Therefore the present
Windows default is still expected to miss `more.com`; this is source-confirmed
reasoning, not a native Windows execution test. Change that command and add a
Windows smoke test before closing #60. Help also still understates the existing
fallback as “no pager, no paging”.

## #62 — mostly polish and upstream conventions

| Finding | Current result |
|---|---|
| Duplicate bad-image error | Fixed: one error for `--diff valid.png notes.md`. Dependency error still has awkward `."md"` punctuation. |
| Plain image diagnostic | Still prints the low-level UTF-8 error before the useful image hint. |
| UTF-16 | Still unreadable; both BOM and headerless fixtures can exit 0 with NUL/replacement characters. Pinned CLI opens files as UTF-8 with `errors="replace"`, so encoding support/diagnostics must respect that default contract. |
| Missing resource | Markdown reads empty stdin and emits zero bytes; print emits a newline; CSV stdin without a sniffable delimiter exits 1. Terminal stdin can wait silently. |
| Trailing whitespace | Still present, but rich 15.0.0 itself pads Markdown and Syntax output; a blanket trimming fix would break parity. |
| Syntax trailing blank row | Matches actual rich 15.0.0 for code ending in a newline. |
| Auto Sixel downgrade | Fixed: piped auto mode emits its ASCII diagnostic, including RICH_SIXEL=1. |
| CSV delimiter sniffing | Fixed and documented; regression covers semicolon/tab/pipe inputs. |
| Exit codes | Documented as 0 success / 1 failure. |
| Repeated scalar flags | Last value still wins, matching upstream; document the behavior. |

Encoding semantics were checked against the pinned
[rich-cli 1.8.1 source](https://github.com/Textualize/rich-cli/blob/v1.8.1/src/rich_cli/__main__.py).
Rendering comparisons used an isolated Python rich 15.0.0 environment.

## #65 — confirmed remaining behavior and documentation gaps

- Notebook alignment and panel flags, and demo alignment/panel flags, are still
  silently ignored. Rule alignment and decorators now work; GIF decorators are
  rejected explicitly.
- A mode with terminal stdin and no resource waits silently (PTY probe ended
  after one second). Make the input behavior clear without breaking piped stdin.
- Rule and panel title markup still renders tags literally; Python interprets it.
- Diff HTML exported while stdout is redirected still uses ASCII (zero half-block
  glyphs in the halo fixture export), contrary to the documented colour-export
  expectation. Export rendering still uses stdout capability.
- COLUMNS and the absence of FORCE_COLOR support are not explained in help;
  default GIF repeat count is unstated. Troubleshooting incorrectly claims no
  arguments prints help; the actual behavior is the capability demo.

Already fixed: option terminator `--`, JSON/CSV/syntax/rule alignment, notebook
traceback newlines, deep JSON, GIF decorator validation and general exit-code docs.

Verified upstream behavior to retain: `-p -` reads stdin; repeated `-o` uses the
last value; `--no-color` does not strip HTML export styles; Markdown rules/tables,
truncated panel titles and narrow fitted rule panels follow upstream conventions.
GIF block rendering is a feature request, separate from these correctness repairs.

## New #108 review findings

The CSV stream is now exposed via `LineRenderable` in `protocol.rs`, replacing
the inherent Table hook. Early-closing consumers such as `head` now produce exit 0
and no error diagnostic. A regression test failed on the previous implementation
and passes after the fix; it sends 10,000 rows and closes stdout after one byte.
An independent reviewer repeated that process check and found no blocking issue.
