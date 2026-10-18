"""``rs_rich.tui``: intuiTUIve, the TUI framework (``rs-rich-intuituive``; no Rich counterpart).

The same API as the Rust crate, name for name, so one guide serves both::

    from rs_rich.tui import App, column, label, signal, text

    def build():
        count = signal(0)
        return (
            column([
                text(lambda: f"[b]Count:[/] {count.get()}").panel("Counter"),
                label("[dim]+ adds one · q quits"),
            ])
            .on_key("+", lambda cx: count.update(lambda c: c + 1))
            .on_key("q", lambda cx: cx.quit())
        )

    App(build).run()

- **Nodes**: ``text``, ``label``, ``renderable`` (any renderable), ``leaf``,
  ``column``, ``row``, ``grid``, ``each``, ``switch``, ``list``, ``scroll``
  (and ``scroll_with``, ``scroll_x``, ``scroll_both``, ``scroll_both_with``),
  ``component`` and ``repeating`` (``rs_rich.interact`` components),
  ``Log(...).view()``, ``layers``, ``effect``, ``fill``, and ``widget`` for a
  ``Widget`` subclass. Builders on ``Node`` chain (``.panel``, ``.fixed``,
  ``.on_key``, ``.focus_style``, ``.title_align``, ``.border``, ``.style``,
  ``.text_align``, ``.theme_styles``, ...); ``class_`` is Rust's ``class``.
  ``layout_split`` and ``layout_split_with_spacers`` are Rust's
  ``layout::split`` and ``split_with_spacers``.
- **Widgets**: ``table``, ``table_with`` (``TableOptions``), ``virtual_table``,
  ``virtual_list``, ``tabs``, ``tree``, ``tree_with``, ``tree_lazy``
  (``TreeItem``, ``LazyItem``), ``text_area``, ``calendar``, ``calendar_with``
  (with ``min`` and ``max``), ``time_picker``, ``datetime_picker``, ``split``,
  ``split_with``, ``hsplit``, ``vsplit``, menus (``menu_bar``, ``Menu``,
  ``MenuItem``, ``open_menu``, ``context_menu``).
- **State**: ``signal``, ``memo``, ``watch``, ``every``, ``viewport`` (the
  size the app lays out in); background work with
  ``spawn`` (a thread), ``spawn_async`` (a coroutine on an asyncio loop),
  ``resource`` and ``Proxy``.
- **The app**: ``App`` (themes, stylesheets, the palette and help keys,
  accessibility and linear mode, inline), ``Ctx`` (what handlers get:
  screens, modals, pop-ups, toasts, animations, focus), ``Theme``,
  ``Stylesheet``; ``Driver`` for a loop of your own; ``run`` with a
  ``Script`` for headless tests, and ``assert_screen``,
  ``assert_screen_styled`` and ``assert_snapshot`` for snapshots
  (``UPDATE_SNAPSHOTS=1`` writes them).
- **Elsewhere**: ``serve`` and ``Server`` put apps in a browser
  (``rs-rich-web``); ``terminal`` and ``web_view`` embed a program or a web
  page (``rs-rich-embed``).

The app runs on the Python thread that called ``run()``. A Python exception in
any of its callbacks stops the app and is raised from ``run()`` (or from the
``Driver`` method that was running). Signals, nodes and apps belong to the
thread that made them.
"""

from ._native import (
    TUI_DEFAULT_MAX_SESSIONS,
    TUI_DEFAULT_SCROLLBACK,
    TUI_XTERM_VERSION,
    InteractScript,
    TuiAccessNode,
    TuiAccessState,
    TuiAnnouncement,
    TuiApp,
    TuiAxis,
    TuiBorder,
    TuiCanvas,
    TuiCellView,
    TuiColumn,
    TuiCtx,
    TuiDrawCx,
    TuiDriver,
    TuiEdge,
    TuiEasing,
    TuiEventCx,
    TuiExitStatus,
    TuiFrameStats,
    TuiLazyItem,
    TuiLoad,
    TuiLog,
    TuiMeasureCx,
    TuiMemo,
    TuiMenu,
    TuiMenuItem,
    TuiMouse,
    TuiNode,
    TuiNodeTiming,
    TuiOrder,
    TuiOverflow,
    TuiPlacement,
    TuiProgramEngine,
    TuiProxy,
    TuiRect,
    TuiReplayHandle,
    TuiReplayHost,
    TuiResource,
    TuiRole,
    TuiRun,
    TuiScrollCx,
    TuiServer,
    TuiServerHandle,
    TuiShadow,
    TuiSheetError,
    TuiSides,
    TuiSignal,
    TuiSize,
    TuiStylesheet,
    TuiTableOptions,
    TuiTask,
    TuiTerminalPane,
    TuiTextAlign,
    TuiTheme,
    TuiTraceEntry,
    TuiTrack,
    TuiTreeItem,
    TuiWebHandle,
    TuiWebView,
    TuiWidget,
    TuiWidgetEvent,
    tui_assert_screen,
    tui_assert_screen_styled,
    tui_assert_snapshot,
    tui_calendar,
    tui_calendar_with,
    tui_datetime_picker,
    tui_column,
    tui_component,
    tui_context_menu,
    tui_each,
    tui_effect,
    tui_every,
    tui_fill,
    tui_grid,
    tui_hsplit,
    tui_label,
    tui_layers,
    tui_layout_split,
    tui_layout_split_with_spacers,
    tui_leaf,
    tui_list,
    tui_memo,
    tui_menu_bar,
    tui_open_menu,
    tui_renderable,
    tui_repeating,
    tui_resource,
    tui_row,
    tui_run,
    tui_scroll,
    tui_scroll_both,
    tui_scroll_both_with,
    tui_scroll_with,
    tui_scroll_x,
    tui_serve,
    tui_signal,
    tui_spawn,
    tui_spawn_async,
    tui_split,
    tui_split_with,
    tui_switch,
    tui_table,
    tui_table_with,
    tui_tabs,
    tui_terminal,
    tui_terminal_with,
    tui_text,
    tui_text_area,
    tui_time_picker,
    tui_tree,
    tui_tree_lazy,
    tui_tree_with,
    tui_virtual_list,
    tui_virtual_table,
    tui_viewport,
    tui_vsplit,
    tui_watch,
    tui_web_view,
    tui_widget,
)

# The Rust names. The flat native module prefixes them, as other areas do.
App = TuiApp
Ctx = TuiCtx
Driver = TuiDriver
Run = TuiRun
Script = InteractScript
Theme = TuiTheme
Stylesheet = TuiStylesheet
SheetError = TuiSheetError
Node = TuiNode
Size = TuiSize
Rect = TuiRect
Axis = TuiAxis
Easing = TuiEasing
Placement = TuiPlacement
Order = TuiOrder
Role = TuiRole
Border = TuiBorder
Shadow = TuiShadow
Edge = TuiEdge
Sides = TuiSides
TextAlign = TuiTextAlign
Overflow = TuiOverflow
Track = TuiTrack
Mouse = TuiMouse
Signal = TuiSignal
Memo = TuiMemo
Log = TuiLog
Task = TuiTask
Load = TuiLoad
Resource = TuiResource
Proxy = TuiProxy
Column = TuiColumn
TableOptions = TuiTableOptions
TreeItem = TuiTreeItem
LazyItem = TuiLazyItem
Menu = TuiMenu
MenuItem = TuiMenuItem
Widget = TuiWidget
DrawCx = TuiDrawCx
Canvas = TuiCanvas
CellView = TuiCellView
EventCx = TuiEventCx
MeasureCx = TuiMeasureCx
ScrollCx = TuiScrollCx
WidgetEvent = TuiWidgetEvent
AccessNode = TuiAccessNode
AccessState = TuiAccessState
Announcement = TuiAnnouncement
FrameStats = TuiFrameStats
TraceEntry = TuiTraceEntry
NodeTiming = TuiNodeTiming
Server = TuiServer
ServerHandle = TuiServerHandle
ExitStatus = TuiExitStatus
ReplayHost = TuiReplayHost
ReplayHandle = TuiReplayHandle
TerminalPane = TuiTerminalPane
ProgramEngine = TuiProgramEngine
WebView = TuiWebView
WebHandle = TuiWebHandle
XTERM_VERSION = TUI_XTERM_VERSION
DEFAULT_MAX_SESSIONS = TUI_DEFAULT_MAX_SESSIONS
DEFAULT_SCROLLBACK = TUI_DEFAULT_SCROLLBACK

signal = tui_signal
memo = tui_memo
viewport = tui_viewport
watch = tui_watch
every = tui_every
spawn = tui_spawn
spawn_async = tui_spawn_async
resource = tui_resource
text = tui_text
label = tui_label
renderable = tui_renderable
leaf = tui_leaf
column = tui_column
row = tui_row
grid = tui_grid
each = tui_each
switch = tui_switch
list = tui_list  # noqa: A001 - the Rust name; import it by name, not with *
scroll = tui_scroll
scroll_with = tui_scroll_with
scroll_x = tui_scroll_x
scroll_both = tui_scroll_both
scroll_both_with = tui_scroll_both_with
component = tui_component
repeating = tui_repeating
widget = tui_widget
layers = tui_layers
effect = tui_effect
fill = tui_fill
layout_split = tui_layout_split
layout_split_with_spacers = tui_layout_split_with_spacers
table = tui_table
table_with = tui_table_with
virtual_table = tui_virtual_table
virtual_list = tui_virtual_list
tabs = tui_tabs
tree = tui_tree
tree_with = tui_tree_with
tree_lazy = tui_tree_lazy
text_area = tui_text_area
calendar = tui_calendar
calendar_with = tui_calendar_with
time_picker = tui_time_picker
datetime_picker = tui_datetime_picker
split = tui_split
split_with = tui_split_with
hsplit = tui_hsplit
vsplit = tui_vsplit
menu_bar = tui_menu_bar
open_menu = tui_open_menu
context_menu = tui_context_menu
run = tui_run
serve = tui_serve
terminal = tui_terminal
terminal_with = tui_terminal_with
web_view = tui_web_view
assert_screen = tui_assert_screen
assert_screen_styled = tui_assert_screen_styled
assert_snapshot = tui_assert_snapshot


async def _awaited(awaitable):
    """``spawn_async``'s wrapper for an awaitable that is not a coroutine."""
    return await awaitable


__all__ = [
    "App",
    "Ctx",
    "Driver",
    "Run",
    "Script",
    "Theme",
    "Stylesheet",
    "SheetError",
    "Node",
    "Size",
    "Rect",
    "Axis",
    "Easing",
    "Placement",
    "Order",
    "Role",
    "Border",
    "Shadow",
    "Edge",
    "Sides",
    "TextAlign",
    "Overflow",
    "Track",
    "Mouse",
    "Signal",
    "Memo",
    "Log",
    "Task",
    "Load",
    "Resource",
    "Proxy",
    "Column",
    "TableOptions",
    "TreeItem",
    "LazyItem",
    "Menu",
    "MenuItem",
    "Widget",
    "DrawCx",
    "Canvas",
    "CellView",
    "EventCx",
    "MeasureCx",
    "ScrollCx",
    "WidgetEvent",
    "AccessNode",
    "AccessState",
    "Announcement",
    "FrameStats",
    "TraceEntry",
    "NodeTiming",
    "Server",
    "ServerHandle",
    "ExitStatus",
    "ReplayHost",
    "ReplayHandle",
    "TerminalPane",
    "ProgramEngine",
    "WebView",
    "WebHandle",
    "XTERM_VERSION",
    "DEFAULT_MAX_SESSIONS",
    "DEFAULT_SCROLLBACK",
    "signal",
    "memo",
    "viewport",
    "watch",
    "every",
    "spawn",
    "spawn_async",
    "resource",
    "text",
    "label",
    "renderable",
    "leaf",
    "column",
    "row",
    "grid",
    "each",
    "switch",
    "list",
    "scroll",
    "scroll_with",
    "scroll_x",
    "scroll_both",
    "scroll_both_with",
    "component",
    "repeating",
    "widget",
    "layers",
    "effect",
    "fill",
    "layout_split",
    "layout_split_with_spacers",
    "table",
    "table_with",
    "virtual_table",
    "virtual_list",
    "tabs",
    "tree",
    "tree_with",
    "tree_lazy",
    "text_area",
    "calendar",
    "calendar_with",
    "time_picker",
    "datetime_picker",
    "split",
    "split_with",
    "hsplit",
    "vsplit",
    "menu_bar",
    "open_menu",
    "context_menu",
    "run",
    "serve",
    "terminal",
    "terminal_with",
    "web_view",
    "assert_screen",
    "assert_screen_styled",
    "assert_snapshot",
]
