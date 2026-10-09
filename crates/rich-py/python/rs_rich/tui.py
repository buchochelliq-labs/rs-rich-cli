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
  ``Log(...).view()``, and ``widget`` for a ``Widget`` subclass. Builders on
  ``Node`` chain (``.panel``, ``.fixed``, ``.on_key``, ``.focus_style``, ...);
  ``class_`` is Rust's ``class``.
- **Widgets**: ``table``, ``table_with`` (``TableOptions``), ``virtual_table``,
  ``virtual_list``, ``tabs``, ``tree``, ``tree_with``, ``tree_lazy``
  (``TreeItem``, ``LazyItem``), ``calendar``, ``calendar_with``, ``split``,
  ``split_with``, ``hsplit``, ``vsplit``, menus (``menu_bar``, ``Menu``,
  ``MenuItem``, ``open_menu``, ``context_menu``).
- **State**: ``signal``, ``memo``, ``watch``, ``every``; background work with
  ``spawn`` (a thread), ``spawn_async`` (a coroutine on an asyncio loop),
  ``resource`` and ``Proxy``.
- **The app**: ``App`` (themes, stylesheets, the palette and help keys,
  accessibility and linear mode, inline), ``Ctx`` (what handlers get:
  screens, modals, pop-ups, toasts, animations, focus), ``Theme``,
  ``Stylesheet``; ``Driver`` for a loop of your own; ``run`` with a
  ``Script`` for headless tests.
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
    TuiCanvas,
    TuiColumn,
    TuiCtx,
    TuiDrawCx,
    TuiDriver,
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
    TuiOrder,
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
    TuiSheetError,
    TuiSignal,
    TuiSize,
    TuiStylesheet,
    TuiTableOptions,
    TuiTask,
    TuiTerminalPane,
    TuiTheme,
    TuiTreeItem,
    TuiWebHandle,
    TuiWebView,
    TuiWidget,
    TuiWidgetEvent,
    tui_calendar,
    tui_calendar_with,
    tui_column,
    tui_component,
    tui_context_menu,
    tui_each,
    tui_every,
    tui_grid,
    tui_hsplit,
    tui_label,
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
    tui_tree,
    tui_tree_lazy,
    tui_tree_with,
    tui_virtual_list,
    tui_virtual_table,
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
EventCx = TuiEventCx
MeasureCx = TuiMeasureCx
ScrollCx = TuiScrollCx
WidgetEvent = TuiWidgetEvent
AccessNode = TuiAccessNode
AccessState = TuiAccessState
Announcement = TuiAnnouncement
FrameStats = TuiFrameStats
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
table = tui_table
table_with = tui_table_with
virtual_table = tui_virtual_table
virtual_list = tui_virtual_list
tabs = tui_tabs
tree = tui_tree
tree_with = tui_tree_with
tree_lazy = tui_tree_lazy
calendar = tui_calendar
calendar_with = tui_calendar_with
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
    "EventCx",
    "MeasureCx",
    "ScrollCx",
    "WidgetEvent",
    "AccessNode",
    "AccessState",
    "Announcement",
    "FrameStats",
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
    "table",
    "table_with",
    "virtual_table",
    "virtual_list",
    "tabs",
    "tree",
    "tree_with",
    "tree_lazy",
    "calendar",
    "calendar_with",
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
]
