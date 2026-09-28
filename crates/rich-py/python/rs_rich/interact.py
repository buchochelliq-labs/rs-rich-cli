"""``rs_rich.interact``: interactive components (``rs-rich-interact``; no Rich counterpart).

Components: ``Select`` and ``MultiSelect`` (fuzzy pickers over ``Item``s whose
values are any Python objects), ``Input`` (one line, masked with
``password=True``), ``Confirm`` (with ``Choice``s), ``Form`` and ``Pager``.
Any object with ``handle(event)`` and ``render(width, height)`` is a component
too (``Event``, ``Done``, ``Cancel``).

Drivers:

- ``ask(component, **options)`` (or ``component.ask()``) runs it on the
  terminal and returns the answer; Escape raises ``Cancelled`` and Ctrl+C
  ``KeyboardInterrupt``. ``run`` returns the ``Outcome`` instead.
- Without a terminal the ``fallback`` decides: ``"prompt"`` asks line by line,
  ``"default"`` returns the component's default, ``"error"`` raises
  ``NotInteractive``.
- ``headless(component, script)`` runs it with scripted keys and returns a
  ``Record`` of the painted frames; ``degrade(component, answers)`` runs the
  line-prompt fallback with scripted answers. Tests use these.

``fuzzy`` and ``rank`` are the pickers' matcher.
"""

from ._native import (
    Cancelled,
    FuzzyMatch,
    InteractAction,
    InteractCancel,
    InteractChoice,
    InteractConfirm,
    InteractDone,
    InteractError,
    InteractEvent,
    InteractForm,
    InteractInput,
    InteractItem,
    InteractMultiSelect,
    InteractOutcome,
    InteractPager,
    InteractRecord,
    InteractScript,
    InteractSelect,
    NotInteractive,
    fuzzy_match,
    fuzzy_rank,
    interact_ask,
    interact_degrade,
    interact_headless,
    interact_run,
)

# The Rust names. The flat native module prefixes those other areas use.
Item = InteractItem
Action = InteractAction
Select = InteractSelect
MultiSelect = InteractMultiSelect
Input = InteractInput
Choice = InteractChoice
Confirm = InteractConfirm
Form = InteractForm
Pager = InteractPager
Event = InteractEvent
Done = InteractDone
Cancel = InteractCancel
Script = InteractScript
Outcome = InteractOutcome
Record = InteractRecord
Match = FuzzyMatch
run = interact_run
ask = interact_ask
headless = interact_headless
degrade = interact_degrade
fuzzy = fuzzy_match
rank = fuzzy_rank

__all__ = [
    "Item",
    "Action",
    "Select",
    "MultiSelect",
    "Input",
    "Choice",
    "Confirm",
    "Form",
    "Pager",
    "Event",
    "Done",
    "Cancel",
    "Script",
    "Outcome",
    "Record",
    "Match",
    "run",
    "ask",
    "headless",
    "degrade",
    "fuzzy",
    "rank",
    "InteractError",
    "Cancelled",
    "NotInteractive",
]
