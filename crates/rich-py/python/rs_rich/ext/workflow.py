"""``rs_rich.ext.workflow``: workflows: command records and runner, task trees, completion summaries (``rich_ext::workflow``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    state_marker,
    state_label,
    ManualClock,
    TaskTree,
    SummaryItem,
    CompletionSummary,
    CommandRecord,
    CommandView,
    run_command,
    WORKFLOW_STATES,
    WORKFLOW_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
STATES = WORKFLOW_STATES
STYLES = WORKFLOW_STYLES

__all__ = [
    "state_marker",
    "state_label",
    "ManualClock",
    "TaskTree",
    "SummaryItem",
    "CompletionSummary",
    "CommandRecord",
    "CommandView",
    "run_command",
    "WORKFLOW_STATES",
    "WORKFLOW_STYLES",
    "STATES",
    "STYLES",
]
