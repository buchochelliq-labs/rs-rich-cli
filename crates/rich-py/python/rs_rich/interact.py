"""``rs_rich.interact``: interactive components (``rs-rich-interact``; no Rich counterpart).

Components: ``Select`` and ``MultiSelect`` (fuzzy pickers over ``Item``s whose
values are any Python objects), ``Input`` (one line, masked with
``password=True``), ``TextArea`` (several lines), ``Confirm`` (with
``Choice``s), ``Form``, ``Pager``, ``FilePicker`` (a path), ``ColorPicker`` (a
colour string), ``AssetPicker`` (an emoji, box style or spinner), ``Reorder``
(the items in a new order), and ``DatePicker``, ``TimePicker`` and
``DateTimePicker`` (a ``datetime.date``, ``datetime.time`` or naive
``datetime.datetime``, within ``min`` and ``max``).
Subclass ``Component`` (``render(context)``, ``handle(event)``, ``keymap()``) to
write your own; any object with ``handle(event)`` and ``render(width, height)``
is a component too (``Event``, ``Done``, ``Cancel``, ``Ignored``).

Composition: ``Column``, ``Row`` and ``Stack`` lay children along an axis,
``Split`` puts two side by side or stacked, ``Tabs`` shows one at a time, and
``Layers`` opens ``Layer``s (modals and popovers) over a base. Built-ins, your
own components and containers mix; ``Map`` says what a child's answer means,
``Label`` shows markup, and ``Keymap`` and ``Binding`` declare keys, which
``keymap(component)`` lists.

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
    InteractAssetPicker,
    InteractBinding,
    InteractCancel,
    InteractChoice,
    InteractColorPicker,
    InteractColumn,
    InteractComponent,
    InteractConfirm,
    InteractContainer,
    InteractContext,
    InteractDatePicker,
    InteractDateTimePicker,
    InteractDone,
    InteractError,
    InteractEvent,
    InteractFilePicker,
    InteractForm,
    InteractIgnored,
    InteractInput,
    InteractItem,
    InteractKeymap,
    InteractLabel,
    InteractLayer,
    InteractLayers,
    InteractMap,
    InteractMultiSelect,
    InteractOutcome,
    InteractPager,
    InteractRecord,
    InteractReorder,
    InteractRow,
    InteractScript,
    InteractSelect,
    InteractSplit,
    InteractStack,
    InteractTabs,
    InteractTextArea,
    InteractTimePicker,
    NotInteractive,
    fuzzy_match,
    fuzzy_rank,
    interact_ask,
    interact_degrade,
    interact_headless,
    interact_keymap,
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
TextArea = InteractTextArea
FilePicker = InteractFilePicker
ColorPicker = InteractColorPicker
AssetPicker = InteractAssetPicker
Reorder = InteractReorder
DatePicker = InteractDatePicker
TimePicker = InteractTimePicker
DateTimePicker = InteractDateTimePicker
Event = InteractEvent
Done = InteractDone
Cancel = InteractCancel
Ignored = InteractIgnored
Component = InteractComponent
Context = InteractContext
Keymap = InteractKeymap
Binding = InteractBinding
Label = InteractLabel
Map = InteractMap
Container = InteractContainer
Stack = InteractStack
Column = InteractColumn
Row = InteractRow
Split = InteractSplit
Tabs = InteractTabs
Layer = InteractLayer
Layers = InteractLayers
Script = InteractScript
Outcome = InteractOutcome
Record = InteractRecord
Match = FuzzyMatch
run = interact_run
ask = interact_ask
headless = interact_headless
degrade = interact_degrade
keymap = interact_keymap
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
    "TextArea",
    "FilePicker",
    "ColorPicker",
    "AssetPicker",
    "Reorder",
    "DatePicker",
    "TimePicker",
    "DateTimePicker",
    "Event",
    "Done",
    "Cancel",
    "Ignored",
    "Component",
    "Context",
    "Keymap",
    "Binding",
    "Label",
    "Map",
    "Container",
    "Stack",
    "Column",
    "Row",
    "Split",
    "Tabs",
    "Layer",
    "Layers",
    "Script",
    "Outcome",
    "Record",
    "Match",
    "run",
    "ask",
    "headless",
    "degrade",
    "keymap",
    "fuzzy",
    "rank",
    "InteractError",
    "Cancelled",
    "NotInteractive",
]
