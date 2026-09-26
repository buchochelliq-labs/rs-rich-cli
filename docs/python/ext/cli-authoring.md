# CLI authoring

Module: `rs_rich.ext.cli_doc` (Rust: `rich_ext::cli_doc`). Describe a
command once, as a `CommandSpec`, and get its help screen, usage errors with
suggestions, shell completions, Markdown and man pages, and a configuration
reference from it.

```python
from rs_rich.console import Console
from rs_rich.ext import cli_doc

console = Console(width=72)
A = cli_doc.ArgSpec
spec = cli_doc.CommandSpec(
    "deploy",
    version="2.1.0",
    about="Ship a build to one or more regions",
    args=[
        A.positional("artifact", value="file", required=True, help="The build archive to upload"),
        A.option("region", short="r", value_name="NAME", multiple=True, env="DEPLOY_REGION",
                 config_key="deploy.region", help="Region to deploy to; repeat for several"),
        A.option("parallel", short="j", value_name="N", default=4, config_key="deploy.parallel",
                 help="Upload this many files at once"),
        A.flag("dry-run", short="n", help="Show the plan without uploading"),
    ],
    subcommands=[cli_doc.CommandSpec("rollback", about="Restore the previous release")],
    examples=[("deploy build.tar -r eu-west-1", "Deploy to one region")],
)
console.print(spec.help())
```

```text
Usage: deploy [OPTIONS] <ARTIFACT> [COMMAND]

Ship a build to one or more regions

Options:
  -r, --region <NAME>...  Region to deploy to; repeat for several [env:
                          DEPLOY_REGION] [config: deploy.region]
  -j, --parallel <N>      Upload this many files at once [default: 4]
                          [config: deploy.parallel]
  -n, --dry-run           Show the plan without uploading

Arguments:
  <ARTIFACT>              The build archive to upload

Commands:
  rollback                Restore the previous release

Examples:
  Deploy to one region
    $ deploy build.tar -r eu-west-1
```

`HelpView(spec)` is the same help as a renderable you can configure;
`help(long=True, path=["rollback"])` shows a subcommand.

## Usage errors

`spec.unknown(argument)` builds a `CliError` with suggestions for a
mistyped argument; the other kinds have constructors such as
`CliError.invalid_value` and `CliError.missing_required`. A `CliError`
renders as a diagnostic and knows its exit code.

```python
error = spec.unknown("--paralel")
console.print(error)
print(error.exit_code, cli_doc.suggest("eu-west", ["eu-west-1", "us-east-2"]))
```

```text
error: unexpected argument '--paralel'
note: usage: deploy [OPTIONS] <ARTIFACT> [COMMAND]
help: a similar argument exists: '--parallel'
2 ['eu-west-1']
```

## Completions and reference docs

`completion(shell)` writes a completion script for each of
`COMPLETION_SHELLS`; `to_markdown()`, `markdown()` and `to_man()` write
reference pages.

```python
print(cli_doc.COMPLETION_SHELLS)
print(spec.to_markdown().splitlines()[0])
print(spec.to_man(date="2026-09-25").splitlines()[0])
```

```text
['bash', 'zsh', 'fish', 'powershell']
# deploy
.TH "DEPLOY" "1" "2026-09-25" "deploy 2.1.0" "User Commands"
```

## Configuration

`ConfigReference.from_spec` documents every setting the arguments'
`config_key`s name, plus `ConfigEntry`s you add. `Precedence` explains where
each value came from when settings are layered (defaults, files,
environment, flags).

```python
L = cli_doc.ConfigLayer
precedence = cli_doc.Precedence([
    L("defaults", {"deploy.parallel": "4"}),
    L("user", {"deploy.parallel": 8}, origin="~/.config/deploy.toml"),
    L("flags", [("deploy.parallel", "2")]),
])
console.print(precedence.explain("deploy.parallel"))
```

```text
deploy.parallel = 2
  ✔ flags                         2  effective
  ✗ user (~/.config/deploy.toml)  8  overridden by flags
  ✗ defaults                      4  overridden by user
```
