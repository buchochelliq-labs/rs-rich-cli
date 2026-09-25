"""Type stubs for the compiled ``rs_rich._native`` module.

Every public class of ``rs_rich`` is defined here and re-exported under Rich's
module paths (``rs_rich.console.Console`` and so on). See docs/python/.
"""

from typing import IO, Literal, Optional, Tuple, Union

__version__: str

JustifyMethod = Literal["default", "left", "center", "right", "full"]
OverflowMethod = Literal["fold", "crop", "ellipsis", "ignore"]
AlignMethod = Literal["left", "center", "right"]
StyleType = Union[str, "Style"]
PaddingDimensions = Union[int, Tuple[int, int], Tuple[int, int, int, int]]
RenderableType = Union[str, "Text", "Table", "Panel"]

class ConsoleError(Exception):
    """The base of rs_rich's own errors, as ``rich.errors.ConsoleError``."""

class MarkupError(ConsoleError):
    """Console markup that does not parse, such as a closing tag with no opener."""

class StyleSyntaxError(ConsoleError):
    """A style definition that does not parse."""

def escape(markup: str) -> str:
    """Backslash-escape ``[`` so text is printed literally rather than read as markup."""

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
    def add_row(self, *renderables: Union[str, Text, None]) -> None: ...
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

class Console:
    def __init__(
        self,
        *,
        file: Optional[IO[str]] = None,
        width: Optional[int] = None,
        height: Optional[int] = None,
        color_system: Optional[Literal["auto", "standard", "256", "truecolor", "windows"]] = "auto",
        force_terminal: Optional[bool] = None,
        no_color: Optional[bool] = None,
        record: bool = False,
        highlight: bool = True,
        emoji: bool = True,
        safe_box: bool = True,
    ) -> None: ...
    @property
    def file(self) -> IO[str]: ...
    @property
    def width(self) -> int: ...
    @property
    def height(self) -> int: ...
    @property
    def is_terminal(self) -> bool: ...
    @property
    def color_system(self) -> Optional[str]: ...
    def print(
        self,
        *objects: Union[RenderableType, int, float, bool, None],
        sep: str = " ",
        end: str = "\n",
        justify: Optional[JustifyMethod] = None,
    ) -> None: ...
    def rule(
        self, title: str = "", *, characters: str = "─", style: Optional[StyleType] = None
    ) -> None: ...
    def export_text(self, *, clear: bool = True, styles: bool = False) -> str: ...
