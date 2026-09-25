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

class ColorParseError(Exception):
    """A colour that does not parse."""

class ColorSystem(int):
    STANDARD: "ColorSystem"
    EIGHT_BIT: "ColorSystem"
    TRUECOLOR: "ColorSystem"
    WINDOWS: "ColorSystem"
    @property
    def name(self) -> str: ...
    @property
    def value(self) -> int: ...

class ColorType(int):
    DEFAULT: "ColorType"
    STANDARD: "ColorType"
    EIGHT_BIT: "ColorType"
    TRUECOLOR: "ColorType"
    WINDOWS: "ColorType"
    @property
    def name(self) -> str: ...
    @property
    def value(self) -> int: ...

class ColorTriplet(NamedTuple):
    red: int
    green: int
    blue: int
    @property
    def hex(self) -> str: ...
    @property
    def rgb(self) -> str: ...
    @property
    def normalized(self) -> Tuple[float, float, float]: ...

class Color(NamedTuple):
    name: str
    type: ColorType
    number: Optional[int] = None
    triplet: Optional[ColorTriplet] = None
    @property
    def system(self) -> ColorSystem: ...
    @property
    def is_system_defined(self) -> bool: ...
    @property
    def is_default(self) -> bool: ...
    def get_truecolor(self, theme: Optional[TerminalTheme] = None, foreground: bool = True) -> ColorTriplet: ...
    @classmethod
    def from_ansi(cls, number: int) -> "Color": ...
    @classmethod
    def from_triplet(cls, triplet: ColorTriplet) -> "Color": ...
    @classmethod
    def from_rgb(cls, red: float, green: float, blue: float) -> "Color": ...
    @classmethod
    def default(cls) -> "Color": ...
    @classmethod
    def parse(cls, color: str) -> "Color": ...
    def get_ansi_codes(self, foreground: bool = True) -> Tuple[str, ...]: ...
    def downgrade(self, system: ColorSystem) -> "Color": ...

def parse_rgb_hex(hex_color: str) -> ColorTriplet: ...
def blend_rgb(
    color1: Tuple[int, int, int], color2: Tuple[int, int, int], cross_fade: float = 0.5
) -> ColorTriplet: ...

class Style:
    def __init__(
        self,
        *,
        color: Optional[Union[Color, str]] = None,
        bgcolor: Optional[Union[Color, str]] = None,
        bold: Optional[bool] = None,
        dim: Optional[bool] = None,
        italic: Optional[bool] = None,
        underline: Optional[bool] = None,
        blink: Optional[bool] = None,
        blink2: Optional[bool] = None,
        reverse: Optional[bool] = None,
        conceal: Optional[bool] = None,
        strike: Optional[bool] = None,
        underline2: Optional[bool] = None,
        frame: Optional[bool] = None,
        encircle: Optional[bool] = None,
        overline: Optional[bool] = None,
        link: Optional[str] = None,
        meta: Optional[Dict[str, Any]] = None,
    ) -> None: ...
    @classmethod
    def null(cls) -> "Style": ...
    @classmethod
    def from_color(cls, color: Optional[Color] = None, bgcolor: Optional[Color] = None) -> "Style": ...
    @classmethod
    def from_meta(cls, meta: Optional[Dict[str, Any]]) -> "Style": ...
    @classmethod
    def on(cls, meta: Optional[Dict[str, Any]] = None, **handlers: Any) -> "Style": ...
    @classmethod
    def parse(cls, style_definition: str) -> "Style": ...
    @classmethod
    def normalize(cls, style: str) -> str: ...
    @classmethod
    def pick_first(cls, *values: Optional[StyleType]) -> StyleType: ...
    @classmethod
    def combine(cls, styles: Iterable["Style"]) -> "Style": ...
    @classmethod
    def chain(cls, *styles: "Style") -> "Style": ...
    @property
    def bold(self) -> Optional[bool]: ...
    @property
    def dim(self) -> Optional[bool]: ...
    @property
    def italic(self) -> Optional[bool]: ...
    @property
    def underline(self) -> Optional[bool]: ...
    @property
    def blink(self) -> Optional[bool]: ...
    @property
    def blink2(self) -> Optional[bool]: ...
    @property
    def reverse(self) -> Optional[bool]: ...
    @property
    def conceal(self) -> Optional[bool]: ...
    @property
    def strike(self) -> Optional[bool]: ...
    @property
    def underline2(self) -> Optional[bool]: ...
    @property
    def frame(self) -> Optional[bool]: ...
    @property
    def encircle(self) -> Optional[bool]: ...
    @property
    def overline(self) -> Optional[bool]: ...
    @property
    def color(self) -> Optional[Color]: ...
    @property
    def bgcolor(self) -> Optional[Color]: ...
    @property
    def link(self) -> Optional[str]: ...
    @property
    def link_id(self) -> str: ...
    @property
    def transparent_background(self) -> bool: ...
    @property
    def background_style(self) -> "Style": ...
    @property
    def meta(self) -> Dict[str, Any]: ...
    @property
    def without_color(self) -> "Style": ...
    def copy(self) -> "Style": ...
    def clear_meta_and_links(self) -> "Style": ...
    def update_link(self, link: Optional[str] = None) -> "Style": ...
    def get_html_style(self, theme: Optional[TerminalTheme] = None) -> str: ...
    def render(
        self,
        text: str = "",
        *,
        color_system: Optional[ColorSystem] = ...,
        legacy_windows: bool = False,
    ) -> str: ...
    def test(self, text: Optional[str] = None) -> None: ...
    def __add__(self, style: Optional["Style"]) -> "Style": ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def __bool__(self) -> bool: ...

class StyleStack:
    def __init__(self, default_style: Style) -> None: ...
    @property
    def current(self) -> Style: ...
    def push(self, style: Style) -> None: ...
    def pop(self) -> Style: ...

class Span(NamedTuple):
    start: int
    end: int
    style: Union[str, Style]
    def split(self, offset: int) -> Tuple["Span", Optional["Span"]]: ...
    def move(self, offset: int) -> "Span": ...
    def right_crop(self, offset: int) -> "Span": ...
    def extend(self, cells: int) -> "Span": ...

class Lines:
    def __init__(self, lines: Iterable["Text"] = ()) -> None: ...
    def __iter__(self) -> Iterator["Text"]: ...
    def __getitem__(self, index: Any) -> Any: ...
    def __setitem__(self, index: int, value: "Text") -> None: ...
    def __len__(self) -> int: ...
    def __rich_console__(self, console: Console, options: ConsoleOptions) -> List["Text"]: ...
    def append(self, line: "Text") -> None: ...
    def extend(self, lines: Iterable["Text"]) -> None: ...
    def pop(self, index: int = -1) -> "Text": ...
    def justify(
        self,
        console: Console,
        width: int,
        justify: JustifyMethod = "left",
        overflow: OverflowMethod = "fold",
    ) -> None: ...

class Text:
    plain: str
    spans: List[Span]
    style: Union[str, Style]
    justify: Optional[JustifyMethod]
    overflow: Optional[OverflowMethod]
    no_wrap: Optional[bool]
    end: str
    tab_size: Optional[int]
    def __init__(
        self,
        text: str = "",
        style: Union[str, Style] = "",
        *,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = None,
        end: str = "\n",
        tab_size: Optional[int] = None,
        spans: Optional[List[Span]] = None,
    ) -> None: ...
    @classmethod
    def from_markup(
        cls,
        text: str,
        *,
        style: Union[str, Style] = "",
        emoji: bool = True,
        emoji_variant: Optional[Literal["emoji", "text"]] = None,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        end: str = "\n",
    ) -> "Text": ...
    @classmethod
    def from_ansi(
        cls,
        text: str,
        *,
        style: Union[str, Style] = "",
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = None,
        end: str = "\n",
        tab_size: Optional[int] = 8,
    ) -> "Text": ...
    @classmethod
    def styled(
        cls,
        text: str,
        style: StyleType = "",
        *,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
    ) -> "Text": ...
    @classmethod
    def assemble(
        cls,
        *parts: Union[str, "Text", Tuple[str, StyleType]],
        style: Union[str, Style] = "",
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = None,
        end: str = "\n",
        tab_size: int = 8,
        meta: Optional[Dict[str, Any]] = None,
    ) -> "Text": ...
    @property
    def cell_len(self) -> int: ...
    @property
    def markup(self) -> str: ...
    def blank_copy(self, plain: str = "") -> "Text": ...
    def copy(self) -> "Text": ...
    def stylize(self, style: Union[str, Style], start: int = 0, end: Optional[int] = None) -> None: ...
    def stylize_before(self, style: Union[str, Style], start: int = 0, end: Optional[int] = None) -> None: ...
    def apply_meta(self, meta: Dict[str, Any], start: int = 0, end: Optional[int] = None) -> None: ...
    def on(self, meta: Optional[Dict[str, Any]] = None, **handlers: Any) -> "Text": ...
    def remove_suffix(self, suffix: str) -> None: ...
    def right_crop(self, amount: int = 1) -> None: ...
    def get_style_at_offset(self, console: Console, offset: int) -> Style: ...
    def extend_style(self, spaces: int) -> None: ...
    def highlight_regex(
        self,
        re_highlight: Any,
        style: Optional[Union[Callable[[str], Optional[StyleType]], StyleType]] = None,
        *,
        style_prefix: str = "",
    ) -> int: ...
    def highlight_words(
        self, words: Iterable[str], style: Union[str, Style], *, case_sensitive: bool = True
    ) -> int: ...
    def rstrip(self) -> None: ...
    def rstrip_end(self, size: int) -> None: ...
    def set_length(self, new_length: int) -> None: ...
    def __rich_console__(self, console: Console, options: ConsoleOptions) -> List[Segment]: ...
    def __rich_measure__(self, console: Console, options: ConsoleOptions) -> Measurement: ...
    def render(self, console: Console, end: str = "") -> List[Segment]: ...
    def join(self, lines: Iterable["Text"]) -> "Text": ...
    def expand_tabs(self, tab_size: Optional[int] = None) -> None: ...
    def truncate(self, max_width: int, *, overflow: Optional[OverflowMethod] = None, pad: bool = False) -> None: ...
    def pad(self, count: int, character: str = " ") -> None: ...
    def pad_left(self, count: int, character: str = " ") -> None: ...
    def pad_right(self, count: int, character: str = " ") -> None: ...
    def align(self, align: AlignMethod, width: int, character: str = " ") -> None: ...
    def append(self, text: Union["Text", str], style: Optional[Union[str, Style]] = None) -> "Text": ...
    def append_text(self, text: "Text") -> "Text": ...
    def append_tokens(self, tokens: Iterable[Tuple[str, Optional[StyleType]]]) -> "Text": ...
    def copy_styles(self, text: "Text") -> None: ...
    def split(
        self, separator: str = "\n", *, include_separator: bool = False, allow_blank: bool = False
    ) -> Lines: ...
    def divide(self, offsets: Iterable[int]) -> Lines: ...
    def wrap(
        self,
        console: Console,
        width: int,
        *,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        tab_size: int = 8,
        no_wrap: Optional[bool] = None,
    ) -> Lines: ...
    def fit(self, width: int) -> Lines: ...
    def detect_indentation(self) -> int: ...
    def with_indent_guides(
        self, indent_size: Optional[int] = None, *, character: str = "│", style: StyleType = "dim green"
    ) -> "Text": ...
    def __len__(self) -> int: ...
    def __bool__(self) -> bool: ...
    def __add__(self, other: Any) -> "Text": ...
    def __eq__(self, other: object) -> bool: ...
    def __contains__(self, other: object) -> bool: ...
    def __getitem__(self, slice: Union[int, slice]) -> "Text": ...

class Theme:
    def __init__(self, styles: Optional[Dict[str, StyleType]] = None, inherit: bool = True) -> None: ...
    @property
    def styles(self) -> Dict[str, Style]: ...
    @property
    def config(self) -> str: ...
    @classmethod
    def from_file(cls, config_file: IO[str], source: Optional[str] = None, inherit: bool = True) -> "Theme": ...
    @classmethod
    def read(cls, path: str, inherit: bool = True, encoding: Optional[str] = None) -> "Theme": ...

class ThemeStack:
    def __init__(self, theme: Theme) -> None: ...
    def get(self, name: str, default: Optional[Style] = None) -> Optional[Style]: ...
    def push_theme(self, theme: Theme, inherit: bool = True) -> None: ...
    def pop_theme(self) -> None: ...

class NoEmoji(Exception):
    """No emoji by that name."""

class Emoji:
    VARIANTS: Dict[str, str]
    name: str
    style: StyleType
    variant: Optional[Literal["emoji", "text"]]
    def __init__(
        self, name: str, style: StyleType = "none", variant: Optional[Literal["emoji", "text"]] = None
    ) -> None: ...
    @classmethod
    def replace(cls, text: str) -> str: ...

class Tag(NamedTuple):
    name: str
    parameters: Optional[str]
    @property
    def markup(self) -> str: ...

def render_markup(
    markup: str,
    style: Union[str, Style] = "",
    emoji: bool = True,
    emoji_variant: Optional[Literal["emoji", "text"]] = None,
) -> Text:
    """``rich.markup.render`` (``rs_rich.markup.render``): console markup to a ``Text``."""

# --- area: renderables (Rule, Padding, Align, Columns, Group, Constrain, Tree, Layout, Bar, Spinner, Styled) ---

class Rule:
    """``rich.rule.Rule``: a horizontal line, optionally with a title."""

    title: Union[str, "Text"]
    characters: str
    style: StyleType
    end: str
    align: AlignMethod
    def __init__(
        self,
        title: Union[str, "Text"] = "",
        *,
        characters: str = "─",
        style: StyleType = "rule.line",
        end: str = "\n",
        align: AlignMethod = "center",
    ) -> None: ...

class Padding:
    """``rich.padding.Padding``: space around a renderable."""

    renderable: RenderableType
    top: int
    right: int
    bottom: int
    left: int
    style: StyleType
    expand: bool
    def __init__(
        self,
        renderable: RenderableType,
        pad: PaddingDimensions = (0, 0, 0, 0),
        *,
        style: StyleType = "none",
        expand: bool = True,
    ) -> None: ...
    @classmethod
    def indent(cls, renderable: RenderableType, level: int) -> "Padding": ...
    @staticmethod
    def unpack(pad: PaddingDimensions) -> Tuple[int, int, int, int]: ...

class Align:
    """``rich.align.Align``: align a renderable horizontally (and vertically)."""

    renderable: RenderableType
    align: AlignMethod
    style: Optional[StyleType]
    vertical: Optional[Literal["top", "middle", "bottom"]]
    pad: bool
    width: Optional[int]
    height: Optional[int]
    def __init__(
        self,
        renderable: RenderableType,
        align: AlignMethod = "left",
        style: Optional[StyleType] = None,
        *,
        vertical: Optional[Literal["top", "middle", "bottom"]] = None,
        pad: bool = True,
        width: Optional[int] = None,
        height: Optional[int] = None,
    ) -> None: ...
    @classmethod
    def left(
        cls,
        renderable: RenderableType,
        style: Optional[StyleType] = None,
        *,
        vertical: Optional[Literal["top", "middle", "bottom"]] = None,
        pad: bool = True,
        width: Optional[int] = None,
        height: Optional[int] = None,
    ) -> "Align": ...
    @classmethod
    def center(
        cls,
        renderable: RenderableType,
        style: Optional[StyleType] = None,
        *,
        vertical: Optional[Literal["top", "middle", "bottom"]] = None,
        pad: bool = True,
        width: Optional[int] = None,
        height: Optional[int] = None,
    ) -> "Align": ...
    @classmethod
    def right(
        cls,
        renderable: RenderableType,
        style: Optional[StyleType] = None,
        *,
        vertical: Optional[Literal["top", "middle", "bottom"]] = None,
        pad: bool = True,
        width: Optional[int] = None,
        height: Optional[int] = None,
    ) -> "Align": ...

class VerticalCenter:
    """``rich.align.VerticalCenter``: center a renderable vertically."""

    renderable: RenderableType
    style: Optional[StyleType]
    def __init__(self, renderable: RenderableType, style: Optional[StyleType] = None) -> None: ...

class Columns:
    """``rich.columns.Columns``: renderables in neat columns."""

    renderables: List[RenderableType]
    width: Optional[int]
    padding: PaddingDimensions
    expand: bool
    equal: bool
    column_first: bool
    right_to_left: bool
    align: Optional[AlignMethod]
    title: Optional[Union[str, "Text"]]
    def __init__(
        self,
        renderables: Optional[Iterable[RenderableType]] = None,
        padding: PaddingDimensions = (0, 1),
        *,
        width: Optional[int] = None,
        expand: bool = False,
        equal: bool = False,
        column_first: bool = False,
        right_to_left: bool = False,
        align: Optional[AlignMethod] = None,
        title: Optional[Union[str, "Text"]] = None,
    ) -> None: ...
    def add_renderable(self, renderable: RenderableType) -> None: ...

class Group:
    """``rich.console.Group``: several renderables, one after another."""

    fit: bool
    def __init__(self, *renderables: RenderableType, fit: bool = True) -> None: ...
    @property
    def renderables(self) -> List[RenderableType]: ...

def group(fit: bool = True) -> Callable[[Callable[..., Iterable[RenderableType]]], Callable[..., Group]]:
    """``rich.console.group``: make a function returning renderables return a ``Group``."""

class Renderables:
    """``rich.containers.Renderables``: a list of renderables that renders them in turn."""

    def __init__(self, renderables: Optional[Iterable[RenderableType]] = None) -> None: ...
    def append(self, renderable: RenderableType) -> None: ...
    def __iter__(self) -> Iterator[RenderableType]: ...

def measure_renderables(
    console: "Console", options: "ConsoleOptions", renderables: Iterable[RenderableType]
) -> "Measurement":
    """``rich.measure.measure_renderables``: the widest minimum and maximum."""

class Constrain:
    """``rich.constrain.Constrain``: render within at most ``width`` cells."""

    renderable: RenderableType
    width: Optional[int]
    def __init__(self, renderable: RenderableType, width: Optional[int] = 80) -> None: ...

class Styled:
    """``rich.styled.Styled``: apply a style across a whole renderable."""

    renderable: RenderableType
    style: StyleType
    def __init__(self, renderable: RenderableType, style: StyleType) -> None: ...

class Tree:
    """``rich.tree.Tree``: a renderable tree structure."""

    ASCII_GUIDES: Tuple[str, str, str, str]
    TREE_GUIDES: List[Tuple[str, str, str, str]]
    label: RenderableType
    style: StyleType
    guide_style: StyleType
    children: List["Tree"]
    expanded: bool
    highlight: bool
    hide_root: bool
    def __init__(
        self,
        label: RenderableType,
        *,
        style: StyleType = "tree",
        guide_style: StyleType = "tree.line",
        expanded: bool = True,
        highlight: bool = False,
        hide_root: bool = False,
    ) -> None: ...
    def add(
        self,
        label: RenderableType,
        *,
        style: Optional[StyleType] = None,
        guide_style: Optional[StyleType] = None,
        expanded: bool = True,
        highlight: Optional[bool] = False,
    ) -> "Tree": ...

class LayoutError(Exception):
    """``rich.layout.LayoutError``: a layout related error."""

class NoSplitter(LayoutError):
    """``rich.layout.NoSplitter``: the requested splitter does not exist."""

class Region(NamedTuple):
    """``rich.region.Region``: a rectangle of the screen."""

    x: int
    y: int
    width: int
    height: int

class LayoutRender(NamedTuple):
    """``rich.layout.LayoutRender``: one leaf's region and rendered lines."""

    region: Region
    render: List[List["Segment"]]

class Splitter:
    """``rich.layout.Splitter``: divides a region among child layouts."""

    name: str

class RowSplitter(Splitter):
    """``rich.layout.RowSplitter``: children side by side."""

    def get_tree_icon(self) -> str: ...
    def divide(self, children: Iterable["Layout"], region: Region) -> List[Tuple["Layout", Region]]: ...

class ColumnSplitter(Splitter):
    """``rich.layout.ColumnSplitter``: children stacked."""

    def get_tree_icon(self) -> str: ...
    def divide(self, children: Iterable["Layout"], region: Region) -> List[Tuple["Layout", Region]]: ...

class Layout:
    """``rich.layout.Layout``: divide a fixed height into rows or columns."""

    splitters: Dict[str, type]
    size: Optional[int]
    minimum_size: int
    ratio: int
    name: Optional[str]
    visible: bool
    splitter: Splitter
    def __init__(
        self,
        renderable: Optional[RenderableType] = None,
        *,
        name: Optional[str] = None,
        size: Optional[int] = None,
        minimum_size: int = 1,
        ratio: int = 1,
        visible: bool = True,
    ) -> None: ...
    @property
    def renderable(self) -> RenderableType: ...
    @property
    def children(self) -> List["Layout"]: ...
    @property
    def map(self) -> Dict["Layout", LayoutRender]: ...
    @property
    def tree(self) -> Tree: ...
    def get(self, name: str) -> Optional["Layout"]: ...
    def __getitem__(self, name: str) -> "Layout": ...
    def split(self, *layouts: Union["Layout", RenderableType], splitter: Union[Splitter, str] = "column") -> None: ...
    def add_split(self, *layouts: Union["Layout", RenderableType]) -> None: ...
    def split_row(self, *layouts: Union["Layout", RenderableType]) -> None: ...
    def split_column(self, *layouts: Union["Layout", RenderableType]) -> None: ...
    def unsplit(self) -> None: ...
    def update(self, renderable: RenderableType) -> None: ...
    def refresh_screen(self, console: "Console", layout_name: str) -> None:
        """Not supported: raises ``NotImplementedError``."""
    def render(self, console: "Console", options: "ConsoleOptions") -> Dict["Layout", LayoutRender]: ...

class Bar:
    """``rich.bar.Bar``: a solid block bar."""

    size: float
    begin: float
    end: float
    width: Optional[int]
    @property
    def style(self) -> "Style": ...
    def __init__(
        self,
        size: float,
        begin: float,
        end: float,
        *,
        width: Optional[int] = None,
        color: Union["Color", str] = "default",
        bgcolor: Union["Color", str] = "default",
    ) -> None: ...

SPINNERS: Dict[str, Dict[str, Any]]

class Spinner:
    """``rich.spinner.Spinner``: an animation frame for a point in time."""

    name: str
    text: RenderableType
    frames: List[str]
    interval: float
    start_time: Optional[float]
    style: Optional[StyleType]
    speed: float
    frame_no_offset: float
    def __init__(
        self,
        name: str,
        text: RenderableType = "",
        *,
        style: Optional[StyleType] = None,
        speed: float = 1.0,
    ) -> None: ...
    def render(self, time: float) -> RenderableType: ...
    def update(
        self,
        *,
        text: RenderableType = "",
        style: Optional[StyleType] = None,
        speed: Optional[float] = None,
    ) -> None: ...

# --- area: code (Markdown, Syntax, JSON, Pretty, inspect, Traceback, highlighters) ---

class Highlighter:
    """Base class: calling one highlights a copy of a ``str`` or ``Text``."""
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def __call__(self, text: Union[str, "Text"]) -> "Text": ...
    def highlight(self, text: "Text") -> None: ...

class NullHighlighter(Highlighter): ...

class RegexHighlighter(Highlighter):
    highlights: List[str]
    base_style: str

class ReprHighlighter(RegexHighlighter): ...

class JSONHighlighter(RegexHighlighter):
    JSON_STR: str
    JSON_WHITESPACE: Any

class ISO8601Highlighter(RegexHighlighter): ...

class Node:
    key_repr: str
    value_repr: str
    open_brace: str
    close_brace: str
    empty: str
    last: bool
    is_tuple: bool
    is_namedtuple: bool
    children: Optional[List["Node"]]
    key_separator: str
    separator: str
    def __init__(
        self,
        key_repr: str = "",
        value_repr: str = "",
        open_brace: str = "",
        close_brace: str = "",
        empty: str = "",
        last: bool = False,
        is_tuple: bool = False,
        is_namedtuple: bool = False,
        children: Optional[List["Node"]] = None,
        key_separator: str = ": ",
        separator: str = ", ",
    ) -> None: ...
    def iter_tokens(self) -> Iterator[str]: ...
    def check_length(self, start_length: int, max_length: int) -> bool: ...
    def render(self, max_width: int = 80, indent_size: int = 4, expand_all: bool = False) -> str: ...

class Pretty:
    indent_size: int
    justify: Optional[JustifyMethod]
    overflow: Optional[OverflowMethod]
    no_wrap: Optional[bool]
    indent_guides: bool
    max_length: Optional[int]
    max_string: Optional[int]
    max_depth: Optional[int]
    expand_all: bool
    margin: int
    insert_line: bool
    def __init__(
        self,
        _object: Any,
        highlighter: Optional[Callable[[Union[str, "Text"]], "Text"]] = None,
        *,
        indent_size: int = 4,
        justify: Optional[JustifyMethod] = None,
        overflow: Optional[OverflowMethod] = None,
        no_wrap: Optional[bool] = False,
        indent_guides: bool = False,
        max_length: Optional[int] = None,
        max_string: Optional[int] = None,
        max_depth: Optional[int] = None,
        expand_all: bool = False,
        margin: int = 0,
        insert_line: bool = False,
    ) -> None: ...
    @property
    def _object(self) -> Any: ...
    @property
    def highlighter(self) -> Callable[[Union[str, "Text"]], "Text"]: ...

def traverse(
    _object: Any,
    max_length: Optional[int] = None,
    max_string: Optional[int] = None,
    max_depth: Optional[int] = None,
) -> Node: ...
def pretty_repr(
    _object: Any,
    *,
    max_width: int = 80,
    indent_size: int = 4,
    max_length: Optional[int] = None,
    max_string: Optional[int] = None,
    max_depth: Optional[int] = None,
    expand_all: bool = False,
) -> str: ...
def pprint(
    _object: Any,
    *,
    console: Optional["Console"] = None,
    indent_guides: bool = True,
    max_length: Optional[int] = None,
    max_string: Optional[int] = None,
    max_depth: Optional[int] = None,
    expand_all: bool = False,
) -> None: ...
def pretty_install(
    console: Optional["Console"] = None,
    overflow: OverflowMethod = "ignore",
    crop: bool = False,
    indent_guides: bool = False,
    max_length: Optional[int] = None,
    max_string: Optional[int] = None,
    max_depth: Optional[int] = None,
    expand_all: bool = False,
) -> None:
    """``rich.pretty.install`` (re-exported as ``rs_rich.pretty.install``)."""

class JSON:
    def __init__(
        self,
        json: str,
        indent: Union[None, int, str] = 2,
        highlight: bool = True,
        skip_keys: bool = False,
        ensure_ascii: bool = False,
        check_circular: bool = True,
        allow_nan: bool = True,
        default: Optional[Callable[[Any], Any]] = None,
        sort_keys: bool = False,
    ) -> None: ...
    @classmethod
    def from_data(
        cls,
        data: Any,
        indent: Union[None, int, str] = 2,
        highlight: bool = True,
        skip_keys: bool = False,
        ensure_ascii: bool = False,
        check_circular: bool = True,
        allow_nan: bool = True,
        default: Optional[Callable[[Any], Any]] = None,
        sort_keys: bool = False,
    ) -> "JSON": ...
    @property
    def text(self) -> "Text": ...
    def __rich__(self) -> "Text": ...

class Markdown:
    def __init__(
        self,
        markup: str,
        code_theme: str = "monokai",
        justify: Optional[JustifyMethod] = None,
        style: StyleType = "none",
        hyperlinks: bool = True,
        inline_code_lexer: Optional[str] = None,
        inline_code_theme: Optional[str] = None,
        *,
        highlighter: Optional[str] = None,
    ) -> None: ...
    @property
    def markup(self) -> str: ...
    @property
    def code_theme(self) -> str: ...
    @property
    def justify(self) -> Optional[JustifyMethod]: ...
    @property
    def style(self) -> StyleType: ...
    @property
    def hyperlinks(self) -> bool: ...
    @property
    def inline_code_lexer(self) -> Optional[str]: ...
    @property
    def inline_code_theme(self) -> Optional[str]: ...
    @property
    def highlighter(self) -> Optional[str]: ...

class Syntax:
    code: str
    dedent: bool
    line_numbers: bool
    start_line: int
    line_range: Optional[Tuple[Optional[int], Optional[int]]]
    highlight_lines: set
    code_width: Optional[int]
    tab_size: int
    word_wrap: bool
    indent_guides: bool
    padding: Tuple[int, int, int, int]
    def __init__(
        self,
        code: str,
        lexer: str,
        *,
        theme: Optional[str] = "monokai",
        dedent: bool = False,
        line_numbers: bool = False,
        start_line: int = 1,
        line_range: Optional[Tuple[Optional[int], Optional[int]]] = None,
        highlight_lines: Optional[set] = None,
        code_width: Optional[int] = None,
        tab_size: int = 4,
        word_wrap: bool = False,
        background_color: Optional[str] = None,
        indent_guides: bool = False,
        padding: PaddingDimensions = 0,
        highlighter: Optional[str] = None,
    ) -> None: ...
    @classmethod
    def from_path(
        cls,
        path: str,
        encoding: str = "utf-8",
        lexer: Optional[str] = None,
        theme: Optional[str] = "monokai",
        dedent: bool = False,
        line_numbers: bool = False,
        line_range: Optional[Tuple[int, int]] = None,
        start_line: int = 1,
        highlight_lines: Optional[set] = None,
        code_width: Optional[int] = None,
        tab_size: int = 4,
        word_wrap: bool = False,
        background_color: Optional[str] = None,
        indent_guides: bool = False,
        padding: PaddingDimensions = 0,
        highlighter: Optional[str] = None,
    ) -> "Syntax": ...
    @classmethod
    def guess_lexer(cls, path: str, code: Optional[str] = None) -> str: ...
    @classmethod
    def get_theme(cls, name: str) -> str: ...
    def highlight(
        self, code: str, line_range: Optional[Tuple[Optional[int], Optional[int]]] = None
    ) -> "Text": ...
    def stylize_range(
        self,
        style: StyleType,
        start: Tuple[int, int],
        end: Tuple[int, int],
        style_before: bool = False,
    ) -> None: ...
    @property
    def lexer(self) -> str: ...
    @property
    def theme(self) -> Optional[str]: ...
    @property
    def background_color(self) -> Optional[str]: ...
    @property
    def highlighter(self) -> Optional[str]: ...
    def __rich_measure__(self, console: "Console", options: "ConsoleOptions") -> "Measurement": ...

def code_highlighters() -> List[str]:
    """The code highlighters this build has (``syntect``; ``lumis`` in a lumis build)."""

def code_themes(highlighter: Optional[str] = None) -> List[str]:
    """The theme names a code highlighter accepts (default: ``syntect``)."""

class Inspect:
    def __init__(
        self,
        obj: Any,
        *,
        title: Optional[Union[str, "Text"]] = None,
        help: bool = False,
        methods: bool = False,
        docs: bool = True,
        private: bool = False,
        dunder: bool = False,
        sort: bool = True,
        all: bool = True,
        value: bool = True,
    ) -> None: ...
    @property
    def obj(self) -> Any: ...

def inspect(
    obj: Any,
    *,
    console: Optional["Console"] = None,
    title: Optional[Union[str, "Text"]] = None,
    help: bool = False,
    methods: bool = False,
    docs: bool = True,
    private: bool = False,
    dunder: bool = False,
    sort: bool = True,
    all: bool = False,
    value: bool = True,
) -> None: ...

class Frame:
    filename: str
    lineno: int
    name: str
    line: str
    locals: Optional[Dict[str, Node]]
    last_instruction: Optional[Tuple[Tuple[int, int], Tuple[int, int]]]
    def __init__(
        self,
        filename: str,
        lineno: int,
        name: str,
        line: str = "",
        locals: Optional[Dict[str, Node]] = None,
        last_instruction: Optional[Tuple[Tuple[int, int], Tuple[int, int]]] = None,
    ) -> None: ...

class Stack:
    exc_type: str
    exc_value: str
    syntax_error: Any
    is_cause: bool
    frames: List[Frame]
    notes: List[str]
    is_group: bool
    exceptions: List["Trace"]
    def __init__(
        self,
        exc_type: str,
        exc_value: str,
        syntax_error: Any = None,
        is_cause: bool = False,
        frames: Optional[List[Frame]] = None,
        notes: List[str] = ...,
        is_group: bool = False,
        exceptions: Optional[List["Trace"]] = None,
    ) -> None: ...

class Trace:
    stacks: List[Stack]
    def __init__(self, stacks: List[Stack]) -> None: ...

class Traceback:
    trace: Trace
    def __init__(
        self,
        trace: Optional[Trace] = None,
        *,
        width: Optional[int] = 100,
        code_width: Optional[int] = 88,
        extra_lines: int = 3,
        theme: Optional[str] = None,
        word_wrap: bool = False,
        show_locals: bool = False,
        locals_max_length: Optional[int] = 10,
        locals_max_string: Optional[int] = 80,
        locals_max_depth: Optional[int] = None,
        locals_hide_dunder: bool = True,
        locals_hide_sunder: bool = False,
        locals_overlow: Optional[OverflowMethod] = None,
        indent_guides: bool = True,
        suppress: Iterable[Any] = (),
        max_frames: int = 100,
    ) -> None: ...
    @classmethod
    def from_exception(
        cls,
        exc_type: Any,
        exc_value: BaseException,
        traceback: Any,
        *,
        width: Optional[int] = 100,
        code_width: Optional[int] = 88,
        extra_lines: int = 3,
        theme: Optional[str] = None,
        word_wrap: bool = False,
        show_locals: bool = False,
        locals_max_length: Optional[int] = 10,
        locals_max_string: Optional[int] = 80,
        locals_max_depth: Optional[int] = None,
        locals_hide_dunder: bool = True,
        locals_hide_sunder: bool = False,
        locals_overflow: Optional[OverflowMethod] = None,
        indent_guides: bool = True,
        suppress: Iterable[Any] = (),
        max_frames: int = 100,
    ) -> "Traceback": ...
    @classmethod
    def extract(
        cls,
        exc_type: Any,
        exc_value: BaseException,
        traceback: Any,
        *,
        show_locals: bool = False,
        locals_max_length: Optional[int] = 10,
        locals_max_string: Optional[int] = 80,
        locals_max_depth: Optional[int] = None,
        locals_hide_dunder: bool = True,
        locals_hide_sunder: bool = False,
        _visited_exceptions: Any = None,
    ) -> Trace: ...
    @property
    def width(self) -> Optional[int]: ...
    @property
    def code_width(self) -> Optional[int]: ...
    @property
    def extra_lines(self) -> int: ...
    @property
    def theme(self) -> str: ...
    @property
    def word_wrap(self) -> bool: ...
    @property
    def show_locals(self) -> bool: ...
    @property
    def indent_guides(self) -> bool: ...
    @property
    def locals_hide_dunder(self) -> bool: ...
    @property
    def locals_hide_sunder(self) -> bool: ...
    @property
    def suppress(self) -> List[str]: ...
    @property
    def max_frames(self) -> int: ...

def traceback_install(
    *,
    console: Optional["Console"] = None,
    width: Optional[int] = 100,
    code_width: Optional[int] = 88,
    extra_lines: int = 3,
    theme: Optional[str] = None,
    word_wrap: bool = False,
    show_locals: bool = False,
    locals_max_length: int = 10,
    locals_max_string: int = 80,
    locals_max_depth: Optional[int] = None,
    locals_hide_dunder: bool = True,
    locals_hide_sunder: Optional[bool] = None,
    locals_overflow: Optional[OverflowMethod] = None,
    indent_guides: bool = True,
    suppress: Iterable[Any] = (),
    max_frames: int = 100,
) -> Callable[..., Any]:
    """``rich.traceback.install`` (re-exported as ``rs_rich.traceback.install``)."""

# --- area: live (Live, Progress, Status, Screen, Pager, prompts, logging) ---

# --- area: ext (rich-ext) ---

# --- area: art (rich-art, Mermaid) ---

# ``rs_rich.art``: an image argument is a path (``str`` / ``os.PathLike``),
# encoded bytes (``bytes``, ``bytearray``, ``memoryview``), an ``ArtImage``,
# or a Pillow image (any object with ``mode``, ``size``, ``convert`` and
# ``tobytes``).

class ArtError(Exception):
    """The base of the art errors."""

class ImageArtError(ArtError):
    """An image cannot be rendered as asked; ``kind`` says why."""
    kind: str

class ImageDecodeError(ArtError):
    """Image bytes could not be decoded."""

class FigletFontError(ArtError):
    """A FIGfont could not be parsed."""

class ImageDiffError(ArtError):
    """Two images cannot be compared; ``kind`` says why."""
    kind: str

class ArtImage:
    def __init__(self, source: Any) -> None: ...
    @staticmethod
    def open(path: Any) -> "ArtImage": ...
    @staticmethod
    def from_bytes(data: Union[bytes, bytearray, memoryview]) -> "ArtImage": ...
    @staticmethod
    def from_pil(image: Any) -> "ArtImage": ...
    @staticmethod
    def frombytes(
        mode: Literal["RGBA", "RGB", "LA", "L"],
        size: Tuple[int, int],
        data: Union[bytes, bytearray, memoryview],
    ) -> "ArtImage": ...
    @property
    def width(self) -> int: ...
    @property
    def height(self) -> int: ...
    @property
    def size(self) -> Tuple[int, int]: ...
    @property
    def mode(self) -> str: ...
    def tobytes(self) -> bytes: ...
    def to_rgba(self) -> bytes: ...
    def to_png(self) -> bytes: ...
    def save(self, path: Any) -> None: ...
    def to_pil(self) -> Any: ...

class ImageOptions:
    def __init__(
        self,
        mode: str = "auto",
        width: Optional[int] = None,
        height: Optional[int] = None,
        color: bool = False,
    ) -> None: ...
    @property
    def mode(self) -> str: ...
    @property
    def width(self) -> Optional[int]: ...
    @property
    def height(self) -> Optional[int]: ...
    @property
    def color(self) -> bool: ...

class RenderCapabilities:
    color: bool
    sixel_supported: bool
    def __init__(self, color: bool = False, sixel_supported: bool = False) -> None: ...
    @staticmethod
    def from_console(console: "Console") -> "RenderCapabilities": ...

class ImageArt:
    def __init__(
        self,
        image: Any,
        *,
        mode: Literal["auto", "ascii", "blocks", "braille", "quadrants", "sixel"] = "auto",
        width: Optional[int] = None,
        height: Optional[int] = None,
        fit: Optional[Literal["contain", "cover", "stretch", "native"]] = None,
        anchor: str = "center",
        background: Union[None, str, Tuple[int, int, int]] = None,
        color: bool = False,
        color_mode: Literal["truecolor", "ansi256", "ansi16", "grayscale"] = "truecolor",
        dither: Optional[Literal["none", "floyd-steinberg", "bayer4x4", "atkinson"]] = None,
        color_distance: Literal["rgb", "oklab"] = "rgb",
        rotate: int = 0,
        flip_horizontal: bool = False,
        flip_vertical: bool = False,
        grayscale: bool = False,
        brightness: float = 1.0,
        contrast: float = 1.0,
        gamma: float = 1.0,
        max_width: Optional[int] = None,
        max_height: Optional[int] = None,
        options: Optional[ImageOptions] = None,
    ) -> None: ...
    def resolve_mode(self, capabilities: RenderCapabilities) -> str: ...
    def native_grid(self, mode: str, available: int) -> Tuple[int, int]: ...
    @property
    def image(self) -> ArtImage: ...
    @property
    def options(self) -> ImageOptions: ...
    @property
    def mode(self) -> str: ...
    @property
    def width(self) -> Optional[int]: ...
    @property
    def height(self) -> Optional[int]: ...
    @property
    def color(self) -> bool: ...
    @property
    def fit(self) -> Optional[str]: ...
    @property
    def anchor(self) -> str: ...
    @property
    def background(self) -> Union[None, str, Tuple[int, int, int]]: ...
    @property
    def color_mode(self) -> str: ...
    @property
    def dither(self) -> Optional[str]: ...
    @property
    def color_distance(self) -> str: ...
    @property
    def rotate(self) -> int: ...
    @property
    def flip_horizontal(self) -> bool: ...
    @property
    def flip_vertical(self) -> bool: ...
    @property
    def grayscale(self) -> bool: ...
    @property
    def brightness(self) -> float: ...
    @property
    def contrast(self) -> float: ...
    @property
    def gamma(self) -> float: ...
    @property
    def max_width(self) -> Optional[int]: ...
    @property
    def max_height(self) -> Optional[int]: ...

class AsciiArt:
    def __init__(
        self,
        image: Any,
        *,
        width: Optional[int] = None,
        height: Optional[int] = None,
        ramp: Optional[str] = None,
        invert: bool = False,
        color: bool = False,
        normalize: bool = True,
    ) -> None: ...
    def columns(self, available: int) -> int: ...
    def to_text(self, width: int) -> str: ...

class BlockArt:
    def __init__(self, image: Any, *, width: Optional[int] = None, height: Optional[int] = None) -> None: ...

class BrailleArt:
    def __init__(self, image: Any, *, width: Optional[int] = None, height: Optional[int] = None) -> None: ...
    def to_text(self, width: int) -> str: ...

class QuadrantArt:
    def __init__(self, image: Any, *, width: Optional[int] = None, height: Optional[int] = None) -> None: ...

class SixelArt:
    def __init__(
        self,
        image: Any,
        *,
        width: Optional[int] = None,
        height: Optional[int] = None,
        cell_px: Optional[Tuple[int, int]] = None,
        max_colors: int = 256,
    ) -> None: ...
    def encode(self, available: int) -> Optional[str]: ...

DEFAULT_RAMP: str
SIXEL_DEFAULT_CELL_PX: Tuple[int, int]
SIXEL_MAX_PIXELS: int

def sixel_is_probably_supported() -> bool: ...

class FigletFont:
    def __init__(self, source: Optional[str] = None) -> None: ...
    @staticmethod
    def parse(source: str) -> "FigletFont": ...
    @staticmethod
    def standard() -> "FigletFont": ...
    @staticmethod
    def from_path(path: Any) -> "FigletFont": ...
    @property
    def height(self) -> int: ...
    @property
    def hard_blank(self) -> str: ...

class Figlet:
    def __init__(
        self,
        text: str,
        *,
        font: Optional[FigletFont] = None,
        justify: Literal["left", "center", "right"] = "left",
        style: Optional[StyleType] = None,
        width: Optional[int] = None,
    ) -> None: ...
    def to_text(self, width: Optional[int] = None) -> str: ...
    @property
    def text(self) -> str: ...
    @property
    def font(self) -> FigletFont: ...
    @property
    def justify(self) -> str: ...
    @property
    def width(self) -> Optional[int]: ...

def figlet_render(
    text: str,
    font: Optional[FigletFont] = None,
    width: int = 80,
    justify: Literal["left", "center", "right"] = "left",
) -> str: ...

STANDARD_FONT: str

class GifFrame:
    @property
    def index(self) -> int: ...

class AnimatedArt:
    MAX_DECODED_BYTES: int
    def __init__(
        self,
        source: Any,
        *,
        width: Optional[int] = None,
        height: Optional[int] = None,
        ramp: Optional[str] = None,
        invert: bool = False,
        color: bool = False,
        blocks: bool = False,
        color_mode: Literal["truecolor", "ansi256", "ansi16", "grayscale"] = "truecolor",
        dither: Optional[Literal["none", "floyd-steinberg", "bayer4x4", "atkinson"]] = None,
        color_distance: Literal["rgb", "oklab"] = "rgb",
        repeat: Union[None, int, Literal["once", "forever"]] = None,
        max_fps: Optional[float] = None,
    ) -> None: ...
    @property
    def frame_count(self) -> int: ...
    def __len__(self) -> int: ...
    @property
    def duration(self) -> float: ...
    @property
    def repeat(self) -> Union[int, Literal["forever"]]: ...
    @property
    def color(self) -> bool: ...
    @property
    def blocks(self) -> bool: ...
    @property
    def color_mode(self) -> str: ...
    @property
    def dither(self) -> Optional[str]: ...
    @property
    def color_distance(self) -> str: ...
    def frame_delay(self, index: int) -> Optional[float]: ...
    def frame(self, index: int) -> Optional[AsciiArt]: ...
    def render_frame(self, index: int) -> Optional[GifFrame]: ...
    def frames(self) -> List[Tuple[GifFrame, float]]: ...
    def play(self, console: Optional["Console"] = None) -> None: ...

class Stage:
    gap: int
    until: Optional[float]
    def __init__(self, *arts: AnimatedArt, gap: int = 2, until: Optional[float] = None) -> None: ...
    def add(self, art: AnimatedArt) -> "Stage": ...
    def __len__(self) -> int: ...
    def play(self, console: Optional["Console"] = None) -> None: ...

def show_cursor_sequence() -> str: ...

class DiffSettings:
    blur: float
    threshold: float
    open_kernel: int
    min_region: int
    top: int
    def __init__(
        self,
        *,
        blur: Optional[float] = None,
        threshold: Optional[float] = None,
        open_kernel: Optional[int] = None,
        min_region: Optional[int] = None,
        top: Optional[int] = None,
    ) -> None: ...

class DiffRegion:
    x: int
    y: int
    width: int
    height: int
    area_px: int
    share_of_change: float
    mean_delta_e: float

class DiffReport:
    @property
    def width(self) -> int: ...
    @property
    def height(self) -> int: ...
    @property
    def changed_fraction(self) -> float: ...
    @property
    def naive_changed_fraction(self) -> float: ...
    @property
    def mean_delta_e(self) -> float: ...
    @property
    def max_delta_e(self) -> float: ...
    @property
    def regions(self) -> List[DiffRegion]: ...
    @property
    def delta_e(self) -> List[float]: ...
    def heatmap(self) -> ArtImage: ...
    def highlight(self, after: Any) -> ArtImage: ...

def image_diff(
    before: Any,
    after: Any,
    settings: Optional[DiffSettings] = None,
    *,
    blur: Optional[float] = None,
    threshold: Optional[float] = None,
    open_kernel: Optional[int] = None,
    min_region: Optional[int] = None,
    top: Optional[int] = None,
) -> DiffReport: ...

# ``rs_rich.mermaid``

class MermaidError(Exception):
    """The base of the Mermaid errors."""

class MermaidParseError(MermaidError):
    """Not a flowchart the text renderer draws; ``kind`` is ``"empty"``,
    ``"unsupported"``, ``"too_large"`` or ``"syntax"`` (with ``line``)."""
    kind: str
    line: Optional[int]

class MermaidLayoutError(MermaidError):
    """A flowchart too large to lay out as text."""
    kind: str

class MmdcError(MermaidError):
    """Mermaid's CLI produced no image; ``kind`` says why."""
    kind: str

class MmdcOptions:
    def __init__(
        self,
        *,
        program: str = "mmdc",
        timeout: float = 20.0,
        max_input: int = 65536,
        max_output: int = 16777216,
        puppeteer_config: Optional[str] = None,
        background: str = "white",
    ) -> None: ...
    @property
    def program(self) -> str: ...
    @property
    def timeout(self) -> float: ...
    @property
    def max_input(self) -> int: ...
    @property
    def max_output(self) -> int: ...
    @property
    def puppeteer_config(self) -> Optional[str]: ...
    @property
    def background(self) -> str: ...

class Mermaid:
    def __init__(
        self,
        source: str,
        *,
        backend: Literal["text", "mmdc"] = "text",
        ascii: Optional[bool] = None,
        mmdc: Optional[MmdcOptions] = None,
    ) -> None: ...
    @property
    def source(self) -> str: ...
    @property
    def backend(self) -> str: ...
    @property
    def ascii(self) -> Optional[bool]: ...
    @property
    def mmdc(self) -> Optional[MmdcOptions]: ...

class MermaidFences:
    def __init__(
        self,
        *,
        backend: Literal["text", "mmdc"] = "text",
        ascii: Optional[bool] = None,
        mmdc: Optional[MmdcOptions] = None,
    ) -> None: ...
    def accepts(self, language: str) -> bool: ...
    def render_fence(
        self, language: str, code: str, console: Any = None, options: Any = None
    ) -> Optional[Mermaid]: ...
    @property
    def backend(self) -> str: ...
    @property
    def ascii(self) -> Optional[bool]: ...
    @property
    def mmdc(self) -> Optional[MmdcOptions]: ...

class FlowchartNode:
    id: str
    label: str
    shape: str

class FlowchartEdge:
    source: int
    target: int
    label: Optional[str]
    stroke: str
    start: Optional[str]
    end: Optional[str]
    length: int

class Flowchart:
    @property
    def direction(self) -> Literal["TD", "BT", "LR", "RL"]: ...
    @property
    def nodes(self) -> List[FlowchartNode]: ...
    @property
    def edges(self) -> List[FlowchartEdge]: ...
    @property
    def notes(self) -> List[str]: ...

class MermaidDiagram:
    lines: List[str]
    width: int

def parse_flowchart(source: str) -> Flowchart: ...
def draw_flowchart(chart: Union[str, Flowchart], ascii: bool = False) -> MermaidDiagram: ...
def mermaid_clean_label(text: str) -> str: ...
def mmdc_render_png(source: str, options: Optional[MmdcOptions] = None) -> bytes: ...

MERMAID_HAS_MMDC: bool
MERMAID_MAX_SOURCE: int
MERMAID_MAX_NODES: int
MERMAID_MAX_EDGES: int
MERMAID_MAX_LINK_LENGTH: int

# --- area: plugins (plugin API) ---

PLUGIN_API_VERSION: int

def is_valid_name(name: str) -> bool: ...

class PluginError(Exception):
    """A plugin was refused, or a plugin callback failed. ``kind`` is one of
    ``incompatible_api``, ``duplicate_plugin``, ``conflict``, ``invalid_name``,
    ``failed``, ``other`` or ``pipeline``; the other attributes are ``None``
    where the kind has no such field."""
    kind: str
    plugin: Optional[str]
    name: Optional[str]
    existing: Optional[str]
    capability: Optional["Capability"]
    built_for: Optional[int]
    host: Optional[int]
    message: Optional[str]
    stage: Optional[str]

class HighlightError(Exception):
    """A code highlighter failed."""

class UnknownThemeError(HighlightError):
    """A code highlighter has no theme of this name (``args[0]``)."""

class HighlighterChoiceError(ValueError):
    """``set_default_code_highlighter`` named an unknown highlighter or theme."""
    name: str
    available: Optional[List[str]]
    theme: Optional[str]

class PluginMetadata:
    def __init__(
        self,
        id: str,
        name: str,
        version: str,
        description: Optional[str] = None,
        *,
        api_version: int = ...,
    ) -> None: ...
    @property
    def id(self) -> str: ...
    @property
    def name(self) -> str: ...
    @property
    def version(self) -> str: ...
    @property
    def api_version(self) -> int: ...
    @property
    def description(self) -> Optional[str]: ...
    def __hash__(self) -> int: ...

class Capability:
    KINDS: List[str]
    def __init__(
        self,
        kind: Literal[
            "highlighter", "code_highlighter", "theme", "box_style", "renderer", "fence_renderer", "transform"
        ],
        name: Optional[str] = None,
    ) -> None: ...
    @property
    def kind(self) -> str: ...
    @property
    def name(self) -> Optional[str]: ...
    def __lt__(self, other: "Capability") -> bool: ...
    def __hash__(self) -> int: ...

class RegisteredPlugin:
    @property
    def metadata(self) -> PluginMetadata: ...
    @property
    def capabilities(self) -> List[Capability]: ...

class HighlightSpan:
    def __init__(self, start: int, end: int, style: StyleType) -> None: ...
    @property
    def start(self) -> int: ...
    @property
    def end(self) -> int: ...
    @property
    def style(self) -> "Style": ...

SpanLike = Union[HighlightSpan, Tuple[int, int, StyleType]]

class HighlightedLine:
    def __init__(self, spans: Optional[Iterable[SpanLike]] = None, newline_style: Optional[StyleType] = None) -> None: ...
    @property
    def spans(self) -> List[HighlightSpan]: ...
    @property
    def newline_style(self) -> Optional["Style"]: ...

class HighlightedCode:
    def __init__(
        self,
        lines: Optional[Iterable[Union[HighlightedLine, Iterable[SpanLike]]]] = None,
        background: Optional[str] = None,
        default_style: Optional[StyleType] = None,
    ) -> None: ...
    @property
    def lines(self) -> List[HighlightedLine]: ...
    @property
    def background(self) -> Optional[str]: ...
    @property
    def default_style(self) -> "Style": ...
    def __len__(self) -> int: ...

class CodeHighlighter:
    """Subclass to write a syntax-highlighting engine in Python."""
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def highlight(
        self, code: str, language: Optional[str] = None, theme: Optional[str] = None
    ) -> Union[HighlightedCode, Iterable[Union[HighlightedLine, Iterable[SpanLike]]]]: ...
    def default_theme(self) -> str: ...
    def themes(self) -> List[str]: ...
    def languages(self) -> List[str]: ...
    def language_for_path(self, path: Any) -> Optional[str]: ...

class TextTransform:
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def transform(self, text: "Text") -> "Text": ...
    def __call__(self, text: "Text") -> "Text": ...

class TextPipeline:
    def names(self) -> List[str]: ...
    def __len__(self) -> int: ...
    def apply(self, text: "Text") -> "Text": ...
    def __call__(self, text: "Text") -> "Text": ...

class Rendered:
    """A renderable a ``SourceRenderer`` returned."""

class SourceRenderer:
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def render(self, source: str) -> Any: ...
    def __call__(self, source: str) -> Any: ...

class FenceRenderer:
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def render_fence(
        self, language: str, code: str, console: "Console", options: Optional["ConsoleOptions"] = None
    ) -> Optional[List["Segment"]]: ...

class PluginRegistrar:
    def highlighter(self, highlighter: Any) -> None: ...
    def code_highlighter(self, name: str, highlighter: Any) -> None: ...
    def theme(self, name: str, theme: "Theme") -> None: ...
    def box_style(self, name: str, box: "Box") -> None: ...
    def renderer(self, name: str, renderer: Union[Callable[[str], Any], Any]) -> None: ...
    def fence_renderer(self, language: str, renderer: Union[Callable[[str, str], Any], Any]) -> None: ...
    def transform(self, name: str, transform: Union[Callable[["Text"], Optional["Text"]], Any]) -> None: ...

class Plugin:
    """Subclass and implement ``metadata()`` and ``register(registrar)``."""
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def metadata(self) -> PluginMetadata: ...
    def register(self, registrar: PluginRegistrar) -> None: ...

class BuiltinPlugin(Plugin):
    def __init__(self) -> None: ...

class MermaidPlugin(Plugin):
    def __init__(self, *, ascii: Optional[bool] = None, backend: Literal["text", "mmdc"] = "text") -> None: ...

class ExtensionRegistry:
    def __init__(self) -> None: ...
    @staticmethod
    def with_defaults() -> "ExtensionRegistry": ...
    def add_plugin(self, plugin: Any) -> None: ...
    def register_highlighter(self, highlighter: Any) -> None: ...
    def register_code_highlighter(self, name: str, highlighter: Any) -> None: ...
    def register_transform(self, name: str, transform: Any) -> None: ...
    def plugins(self) -> List[RegisteredPlugin]: ...
    def provided_by(self, capability: Capability) -> Optional[str]: ...
    def code_highlighter_names(self) -> List[str]: ...
    def code_highlighter(self, name: str) -> Optional[CodeHighlighter]: ...
    def set_default_code_highlighter(self, name: str, theme: Optional[str] = None) -> None: ...
    def default_code_highlighter(self) -> Optional[str]: ...
    def theme(self, name: str) -> Optional["Theme"]: ...
    def box_style(self, name: str) -> Optional["Box"]: ...
    def renderer(self, name: str) -> Optional[SourceRenderer]: ...
    def fence_renderer(self, language: str) -> Optional[FenceRenderer]: ...
    def fences(self) -> Optional[FenceRenderer]: ...
    def transform(self, name: str) -> Optional[TextTransform]: ...
    def transform_names(self) -> List[str]: ...
    def text_pipeline(self, names: Iterable[str]) -> TextPipeline: ...
    def install(self, console: "Console") -> None: ...

def install_defaults(console: "Console") -> None: ...

# --- area: cli (the rich command line) ---

def cli_main(program: list[str], argv: list[str]) -> int:
    """Run the ``rich`` command line in-process (GIL released); return its exit status."""
    ...
