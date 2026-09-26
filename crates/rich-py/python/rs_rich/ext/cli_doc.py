"""``rs_rich.ext.cli_doc``: command help, errors, completions, docs and config from one description (``rich_ext::cli_doc``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    ArgSpec,
    CommandSpec,
    HelpView,
    MarkdownReference,
    CompletionCatalog,
    CliError,
    suggest,
    ConfigEntry,
    ConfigReference,
    ConfigLayer,
    Precedence,
    PrecedenceExplanation,
    HELP_STYLES,
    HELP_STACK_BELOW,
    COMPLETION_SHELLS,
)

# The Rust names, where the flat native module needed a longer one.
Layer = ConfigLayer
STYLES = HELP_STYLES
STACK_BELOW = HELP_STACK_BELOW
SHELLS = COMPLETION_SHELLS

__all__ = [
    "ArgSpec",
    "CommandSpec",
    "HelpView",
    "MarkdownReference",
    "CompletionCatalog",
    "CliError",
    "suggest",
    "ConfigEntry",
    "ConfigReference",
    "ConfigLayer",
    "Precedence",
    "PrecedenceExplanation",
    "HELP_STYLES",
    "HELP_STACK_BELOW",
    "COMPLETION_SHELLS",
    "Layer",
    "STYLES",
    "STACK_BELOW",
    "SHELLS",
]
