"""Type stubs for the compiled ``rs_rich._native`` module.

Every public class of ``rs_rich`` is defined here and re-exported under Rich's
module paths (``rs_rich.console.Console`` and so on). See docs/python/.

Each area appends its stubs to its own ``# --- area: <name> ---`` section
only, so areas never edit the same lines.
"""

from typing import (
    IO,
    Any,
    Callable,
    Dict,
    Iterable,
    Iterator,
    List,
    Literal,
    NamedTuple,
    Optional,
    Protocol,
    Tuple,
    Union,
)

__version__: str

JustifyMethod = Literal["default", "left", "center", "right", "full"]
OverflowMethod = Literal["fold", "crop", "ellipsis", "ignore"]
AlignMethod = Literal["left", "center", "right"]
StyleType = Union[str, "Style"]
PaddingDimensions = Union[int, Tuple[int, int], Tuple[int, int, int, int]]

class RichCast(Protocol):
    def __rich__(self) -> Any: ...

class ConsoleRenderable(Protocol):
    def __rich_console__(self, console: "Console", options: "ConsoleOptions") -> Iterable[Any]: ...

RenderableType = Union[str, "Text", "Table", "Panel", ConsoleRenderable, RichCast]

# --- area: foundation (errors) ---

class ConsoleError(Exception):
    """The base of rs_rich's own errors, as ``rich.errors.ConsoleError``."""

class StyleError(Exception):
    """An error in styles."""

class StyleSyntaxError(ConsoleError):
    """A style definition that does not parse."""

class MissingStyle(StyleError):
    """No style of that name, and the name does not parse as a style."""

class StyleStackError(ConsoleError):
    """The style stack is invalid."""

class NotRenderableError(ConsoleError):
    """An object that is not a str, a Segment or a renderable."""

class MarkupError(ConsoleError):
    """Console markup that does not parse, such as a closing tag with no opener."""

class LiveError(ConsoleError):
    """An error in a Live display."""

class NoAltScreen(ConsoleError):
    """The alternate screen was required."""

class CaptureError(Exception):
    """``Capture.get`` called before the ``with`` block ended."""

class ThemeStackError(Exception):
    """Popping the console's own theme."""

def escape(markup: str) -> str:
    """Backslash-escape ``[`` so text is printed literally rather than read as markup."""

# --- area: foundation (protocol) ---

class ConsoleDimensions(NamedTuple):
    width: int
    height: int

class ConsoleOptions:
    legacy_windows: bool
    min_width: int
    max_width: int
    is_terminal: bool
    encoding: str
    max_height: int
    justify: Optional[JustifyMethod]
    overflow: Optional[OverflowMethod]
    no_wrap: Optional[bool]
    highlight: Optional[bool]
    markup: Optional[bool]
    height: Optional[int]
    @property
    def size(self) -> ConsoleDimensions: ...
    @property
    def ascii_only(self) -> bool: ...
    def copy(self) -> "ConsoleOptions": ...
    def update(
        self,
        *,
        width: int = ...,
        min_width: int = ...,
        max_width: int = ...,
        justify: Optional[JustifyMethod] = ...,
        overflow: Optional[OverflowMethod] = ...,
        no_wrap: Optional[bool] = ...,
        highlight: Optional[bool] = ...,
        markup: Optional[bool] = ...,
        height: Optional[int] = ...,
    ) -> "ConsoleOptions": ...
    def update_width(self, width: int) -> "ConsoleOptions": ...
    def update_height(self, height: int) -> "ConsoleOptions": ...
    def reset_height(self) -> "ConsoleOptions": ...
    def update_dimensions(self, width: int, height: int) -> "ConsoleOptions": ...

class Measurement:
    def __init__(self, minimum: int, maximum: int) -> None: ...
    @property
    def minimum(self) -> int: ...
    @property
    def maximum(self) -> int: ...
    @property
    def span(self) -> int: ...
    def normalize(self) -> "Measurement": ...
    def with_maximum(self, width: int) -> "Measurement": ...
    def with_minimum(self, width: int) -> "Measurement": ...
    def clamp(self, min_width: Optional[int] = None, max_width: Optional[int] = None) -> "Measurement": ...
    @classmethod
    def get(cls, console: "Console", options: ConsoleOptions, renderable: RenderableType) -> "Measurement": ...
    def __iter__(self) -> Iterator[int]: ...
    def __getitem__(self, index: int) -> int: ...
    def __len__(self) -> int: ...

class Segment:
    def __init__(self, text: str = "", style: Optional[StyleType] = None, control: Optional[Any] = None) -> None: ...
    @classmethod
    def line(cls) -> "Segment": ...
    @property
    def text(self) -> str: ...
    @property
    def style(self) -> Optional["Style"]: ...
    @property
    def control(self) -> Optional[Any]: ...
    @property
    def cell_length(self) -> int: ...
    @property
    def is_control(self) -> bool: ...
    def __iter__(self) -> Iterator[Any]: ...
    def __getitem__(self, index: int) -> Any: ...
    def __len__(self) -> int: ...
    def __bool__(self) -> bool: ...

# --- area: foundation (boxes, table, panel) ---

class Box:
    """A box style; use the constants in ``rs_rich.box``."""

ASCII: Box
ASCII2: Box
ASCII_DOUBLE_HEAD: Box
SQUARE: Box
SQUARE_DOUBLE_HEAD: Box
MINIMAL: Box
MINIMAL_HEAVY_HEAD: Box
MINIMAL_DOUBLE_HEAD: Box
SIMPLE: Box
SIMPLE_HEAD: Box
SIMPLE_HEAVY: Box
HORIZONTALS: Box
ROUNDED: Box
HEAVY: Box
HEAVY_EDGE: Box
HEAVY_HEAD: Box
DOUBLE: Box
DOUBLE_EDGE: Box
MARKDOWN: Box

class Table:
    def __init__(
        self,
        *headers: str,
        title: Optional[str] = None,
        caption: Optional[str] = None,
        box: Optional[Box] = ...,
        show_header: bool = True,
        show_lines: bool = False,
        show_edge: bool = True,
        expand: bool = False,
        border_style: Optional[StyleType] = None,
    ) -> None: ...
    def add_column(
        self,
        header: str = "",
        *,
        style: Optional[StyleType] = None,
        header_style: Optional[StyleType] = None,
        justify: JustifyMethod = "left",
        overflow: OverflowMethod = "ellipsis",
        width: Optional[int] = None,
        min_width: Optional[int] = None,
        max_width: Optional[int] = None,
        ratio: Optional[int] = None,
        no_wrap: bool = False,
    ) -> None: ...
    def add_row(self, *renderables: Optional[RenderableType]) -> None: ...
    @property
    def row_count(self) -> int: ...

class Panel:
    def __init__(
        self,
        renderable: RenderableType,
        box: Box = ...,
        *,
        title: Optional[str] = None,
        title_align: AlignMethod = "center",
        subtitle: Optional[str] = None,
        subtitle_align: AlignMethod = "center",
        expand: bool = True,
        border_style: Optional[StyleType] = None,
        width: Optional[int] = None,
        padding: Optional[PaddingDimensions] = None,
    ) -> None: ...
    @classmethod
    def fit(
        cls,
        renderable: RenderableType,
        box: Box = ...,
        *,
        title: Optional[str] = None,
        title_align: AlignMethod = "center",
        subtitle: Optional[str] = None,
        subtitle_align: AlignMethod = "center",
        border_style: Optional[StyleType] = None,
        width: Optional[int] = None,
        padding: Optional[PaddingDimensions] = None,
    ) -> "Panel": ...

# --- area: foundation (terminal themes) ---

class TerminalTheme:
    def __init__(
        self,
        background: Tuple[int, int, int],
        foreground: Tuple[int, int, int],
        normal: List[Tuple[int, int, int]],
        bright: Optional[List[Tuple[int, int, int]]] = None,
    ) -> None: ...
    @property
    def background_color(self) -> Tuple[int, int, int]: ...
    @property
    def foreground_color(self) -> Tuple[int, int, int]: ...
    @property
    def ansi_colors(self) -> List[Tuple[int, int, int]]: ...

DEFAULT_TERMINAL_THEME: TerminalTheme
SVG_EXPORT_THEME: TerminalTheme
MONOKAI: TerminalTheme
DIMMED_MONOKAI: TerminalTheme
NIGHT_OWLISH: TerminalTheme

# --- area: foundation (console) ---

class Capture:
    def __enter__(self) -> "Capture": ...
    def __exit__(self, *args: Any) -> None: ...
    def get(self) -> str: ...

class ThemeContext:
    def __enter__(self) -> "ThemeContext": ...
    def __exit__(self, *args: Any) -> None: ...

class Console:
    def __init__(
        self,
        *,
        color_system: Optional[Literal["auto", "standard", "256", "truecolor", "windows"]] = "auto",
        force_terminal: Optional[bool] = None,
        force_jupyter: Optional[bool] = None,
        force_interactive: Optional[bool] = None,
        soft_wrap: bool = False,
        theme: Optional["Theme"] = None,
        stderr: bool = False,
        file: Optional[IO[str]] = None,
        quiet: bool = False,
        width: Optional[int] = None,
        height: Optional[int] = None,
        style: Optional[StyleType] = None,
        no_color: Optional[bool] = None,
        tab_size: int = 8,
        record: bool = False,
        markup: bool = True,
        emoji: bool = True,
        emoji_variant: None = None,
        highlight: bool = True,
        log_time: bool = True,
        log_path: bool = True,
        log_time_format: Union[str, Callable[[Any], "Text"], None] = None,
        legacy_windows: Optional[bool] = None,
        safe_box: bool = True,
        get_datetime: Optional[Callable[[], Any]] = None,
        get_time: Optional[Callable[[], float]] = None,
    ) -> None: ...
    def __enter__(self) -> "Console": ...
    def __exit__(self, *args: Any) -> None: ...
    file: IO[str]
    width: int
    height: int
    size: ConsoleDimensions
    quiet: bool
    soft_wrap: bool
    record: bool
    is_interactive: bool
    @property
    def is_terminal(self) -> bool: ...
    @property
    def is_dumb_terminal(self) -> bool: ...
    @property
    def color_system(self) -> Optional[str]: ...
    @property
    def encoding(self) -> str: ...
    @property
    def no_color(self) -> bool: ...
    @property
    def legacy_windows(self) -> bool: ...
    @property
    def safe_box(self) -> bool: ...
    @property
    def tab_size(self) -> int: ...
    @property
    def stderr(self) -> bool: ...
    @property
    def is_alt_screen(self) -> bool: ...
    @property
    def get_time(self) -> Callable[[], float]: ...
    @property
    def get_datetime(self) -> Callable[[], Any]: ...
    @property
    def options(self) -> ConsoleOptions: ...
    def print(
        self,
        *objects: Any,
        sep: str = " ",
        end: str = "\n",
        style: Optional[StyleType] = None,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = None,
        emoji: Optional[bool] = None,
        markup: Optional[bool] = None,
        highlight: Optional[bool] = None,
        width: Optional[int] = None,
        height: Optional[int] = None,
        crop: bool = True,
        soft_wrap: Optional[bool] = None,
        new_line_start: bool = False,
    ) -> None: ...
    def out(
        self,
        *objects: Any,
        sep: str = " ",
        end: str = "\n",
        style: Optional[StyleType] = None,
        highlight: Optional[bool] = None,
    ) -> None: ...
    def log(
        self,
        *objects: Any,
        sep: str = " ",
        end: str = "\n",
        style: Optional[StyleType] = None,
        justify: Optional[JustifyMethod] = None,
        emoji: Optional[bool] = None,
        markup: Optional[bool] = None,
        highlight: Optional[bool] = None,
        log_locals: bool = False,
        _stack_offset: int = 1,
    ) -> None: ...
    def rule(
        self,
        title: str = "",
        *,
        characters: str = "─",
        style: Optional[StyleType] = None,
        align: AlignMethod = "center",
    ) -> None: ...
    def line(self, count: int = 1) -> None: ...
    def clear(self, home: bool = True) -> None: ...
    def bell(self) -> None: ...
    def show_cursor(self, show: bool = True) -> bool: ...
    def set_alt_screen(self, enable: bool = True) -> bool: ...
    def input(
        self,
        prompt: Union[str, "Text", None] = None,
        *,
        markup: bool = True,
        emoji: bool = True,
        password: bool = False,
        stream: Optional[IO[str]] = None,
    ) -> str: ...
    def print_json(
        self,
        json: Optional[str] = None,
        *,
        data: Any = None,
        indent: Optional[int] = None,
        highlight: bool = True,
        skip_keys: bool = False,
        ensure_ascii: bool = False,
        check_circular: bool = True,
        allow_nan: bool = True,
        default: Optional[Callable[[Any], Any]] = None,
        sort_keys: bool = False,
    ) -> None: ...
    def measure(self, renderable: RenderableType, *, options: Optional[ConsoleOptions] = None) -> Measurement: ...
    def render(self, renderable: RenderableType, options: Optional[ConsoleOptions] = None) -> List[Segment]: ...
    def render_lines(
        self,
        renderable: RenderableType,
        options: Optional[ConsoleOptions] = None,
        *,
        style: Optional[StyleType] = None,
        pad: bool = True,
        new_lines: bool = False,
    ) -> List[List[Segment]]: ...
    def render_str(
        self,
        text: str,
        *,
        style: Optional[StyleType] = None,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        emoji: Optional[bool] = None,
        markup: Optional[bool] = None,
        highlight: Optional[bool] = None,
    ) -> "Text": ...
    def get_style(self, name: StyleType, *, default: Optional[StyleType] = None) -> "Style": ...
    def push_theme(self, theme: "Theme", *, inherit: bool = True) -> None: ...
    def pop_theme(self) -> None: ...
    def use_theme(self, theme: "Theme", *, inherit: bool = True) -> ThemeContext: ...
    def capture(self) -> Capture: ...
    def begin_capture(self) -> None: ...
    def end_capture(self) -> str: ...
    def export_text(self, *, clear: bool = True, styles: bool = False) -> str: ...
    def save_text(self, path: str, *, clear: bool = True, styles: bool = False) -> None: ...
    def export_html(
        self,
        *,
        theme: Optional[TerminalTheme] = None,
        clear: bool = True,
        code_format: None = None,
        inline_styles: bool = False,
    ) -> str: ...
    def save_html(
        self,
        path: str,
        *,
        theme: Optional[TerminalTheme] = None,
        clear: bool = True,
        code_format: None = None,
        inline_styles: bool = False,
    ) -> None: ...
    def export_svg(
        self,
        *,
        title: str = "Rich",
        theme: Optional[TerminalTheme] = None,
        clear: bool = True,
        code_format: None = None,
        font_aspect_ratio: float = 0.61,
        unique_id: Optional[str] = None,
    ) -> str: ...
    def save_svg(
        self,
        path: str,
        *,
        title: str = "Rich",
        theme: Optional[TerminalTheme] = None,
        clear: bool = True,
        code_format: None = None,
        font_aspect_ratio: float = 0.61,
        unique_id: Optional[str] = None,
    ) -> None: ...
    def status(self, *args: Any, **kwargs: Any) -> Any: ...
    def pager(self, *args: Any, **kwargs: Any) -> Any: ...
    def screen(self, *args: Any, **kwargs: Any) -> Any: ...
    def print_exception(self, *args: Any, **kwargs: Any) -> Any: ...

# --- area: text-style (Text, Style, Theme, Color, emoji) ---

class Style:
    def __init__(
        self,
        *,
        color: Optional[str] = None,
        bgcolor: Optional[str] = None,
        bold: Optional[bool] = None,
        dim: Optional[bool] = None,
        italic: Optional[bool] = None,
        underline: Optional[bool] = None,
        blink: Optional[bool] = None,
        reverse: Optional[bool] = None,
        conceal: Optional[bool] = None,
        strike: Optional[bool] = None,
        link: Optional[str] = None,
    ) -> None: ...
    @staticmethod
    def parse(definition: str) -> "Style": ...
    def __add__(self, other: "Style") -> "Style": ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class Text:
    def __init__(
        self,
        text: str = "",
        style: Optional[StyleType] = None,
        *,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = None,
    ) -> None: ...
    @classmethod
    def from_markup(
        cls,
        text: str,
        *,
        style: Optional[StyleType] = None,
        justify: Optional[JustifyMethod] = None,
    ) -> "Text": ...
    @property
    def plain(self) -> str: ...
    def append(self, text: Union[str, "Text"], style: Optional[StyleType] = None) -> "Text": ...
    def stylize(self, style: StyleType, start: int = 0, end: Optional[int] = None) -> None: ...
    def __len__(self) -> int: ...

class Theme:
    def __init__(self, styles: Optional[Dict[str, StyleType]] = None, inherit: bool = True) -> None: ...
    @property
    def styles(self) -> Dict[str, Style]: ...
    @property
    def config(self) -> str: ...

# --- area: renderables (Rule, Padding, Align, Columns, Group, Constrain, Tree, Layout, Bar, Spinner, Styled) ---

# --- area: code (Markdown, Syntax, JSON, Pretty, inspect, Traceback, highlighters) ---

# --- area: live (Live, Progress, Status, Screen, Pager, prompts, logging) ---

# --- area: ext (rich-ext) ---

# --- area: art (rich-art, Mermaid) ---

# --- area: plugins (plugin API) ---

# --- area: cli (the rich command line) ---
