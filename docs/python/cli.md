# The command line

The wheel ships the `rich` command line of this repository (the Rust port of
[`rich-cli`](https://github.com/Textualize/rich-cli)), with every command and
option the `rich` binary has:

```bash
python -m rs_rich README.md
rich-rs data.json --panel rounded
rich-rs --help
```

The console script is `rich-rs`, because `rich` is `rich-cli`'s command. Both
forms run the same code as the `rich` binary built from the same source, so
their output, errors and exit statuses are the binary's, byte for byte (the
tests compare them). See the [CLI reference](../cli-reference.md) for the commands and
options.

Python `rich` has no command line of its own; this page is about `rich-cli`'s.

## From a script

The command line writes to the process's standard output and error and reads
its standard input, so a script runs it as a child process and captures what
it prints:

```python
import subprocess
import sys

result = subprocess.run(
    [sys.executable, "-m", "rs_rich", "--print", "[bold]Hello[/] from [i]rs_rich[/]"],
    capture_output=True,
    text=True,
)
print(result.stdout, end="")
print("exit status", result.returncode)
```

```text
Hello from rs_rich
exit status 0
```

Output that is not a terminal is plain text; `FORCE_COLOR=1` in the child's
environment keeps the colour. Input comes on standard input with `-`:

```python
result = subprocess.run(
    [sys.executable, "-m", "rs_rich", "--json", "-", "--width", "30"],
    input='{"name": "rich", "values": [1, 2.5, true, null]}',
    capture_output=True,
    text=True,
)
print(result.stdout, end="")
```

```text
{
  "name": "rich",
  "values": [
    1,
    2.5,
    true,
    null
  ]
}
```

Errors go to standard error, with the binary's exit statuses (2 for a usage
error, 3 for input that cannot be read):

```python
result = subprocess.run(
    [sys.executable, "-m", "rs_rich", "--no-such-option"],
    capture_output=True,
    text=True,
)
print(result.stderr, end="")
print("exit status", result.returncode)
```

```text
rich: unknown option "--no-such-option" (try --help)
exit status 2
```

## `rs_rich.cli.main`

```text
rs_rich.cli.main(argv=None) -> int
```

Runs the command line and returns its exit status. It does not call
`sys.exit`; `python -m rs_rich` and `rich-rs` do that with its result.

- **`main()`**, with no `argv`, is the entry point: the arguments are
  `sys.argv[1:]`, and the command line runs in this process with the GIL
  released. Python's `sys.stdout` and `sys.stderr` are flushed before and
  after, so output interleaves in order. For the run, Ctrl-C has its default
  action, ending the process as it ends the binary; Python's handler is put
  back afterwards.
- **`main(argv)`**, with a list of argument strings (without the program
  name), is the call for a running program. The command line runs in a child
  `python -m rs_rich` process that shares this one's standard input, output
  and error, so the output lands in the same place. It is a child because the
  command line keeps process-wide state no library call should leave behind:
  `--batch`, `--watch` and the demo install a Ctrl-C handler that replaces
  Python's and can be installed only once per process (a config file can turn
  those modes on without any flag), and a few error paths end the process.

Neither form captures output: to capture it, run the command line with
`subprocess` as above.

The build includes the default features of the `rich` binary: URL fetching
(`rich-rs https://…`), images and GIFs, and Mermaid diagrams drawn as text.
The `lumis` highlighter and the `mmdc` Mermaid backend are not in the wheel's
command line.
