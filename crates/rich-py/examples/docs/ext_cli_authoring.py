"""The pictures on docs/python/ext/cli-authoring.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import cli_doc


# --8<-- [start:spec]
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
# --8<-- [end:spec]


def help_screen(console):
    # --8<-- [start:help]
    console.print(spec.help())
    # --8<-- [end:help]


def usage_error(console):
    # --8<-- [start:error]
    error = spec.unknown("--paralel")
    console.print(error)
    print(error.exit_code, cli_doc.suggest("eu-west", ["eu-west-1", "us-east-2"]))
    # --8<-- [end:error]


def layered(console):
    # --8<-- [start:precedence]
    L = cli_doc.ConfigLayer
    precedence = cli_doc.Precedence([
        L("defaults", {"deploy.parallel": "4"}),
        L("user", {"deploy.parallel": 8}, origin="~/.config/deploy.toml"),
        L("flags", [("deploy.parallel", "2")]),
    ])
    console.print(precedence.explain("deploy.parallel"))
    # --8<-- [end:precedence]


shots = Shots("ext/cli-authoring")
shots.shot("help", help_screen, width=72)
shots.shot("error", usage_error, width=72)
shots.shot("precedence", layered, width=72)
shots.finish()
