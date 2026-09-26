"""Python glue for rs_rich's live area (``rs_rich._live_glue``).

Only what has to be Python lives here: a ``logging.Handler`` subclass, the
prompt exceptions, and the binary reader ``Progress.wrap_file`` returns (an
``io.RawIOBase``, so ``io.TextIOWrapper`` accepts it). Everything these show
is rendered in Rust (``_native``).
"""

import io
import logging
from io import RawIOBase, UnsupportedOperation
from typing import Any, BinaryIO, ClassVar, Iterable, List, Optional


class PromptError(Exception):
    """Exception base class for prompt related errors."""


class InvalidResponse(PromptError):
    """Raised by ``process_response`` for an invalid response; the message is shown."""

    def __init__(self, message: Any) -> None:
        self.message = message

    def __rich__(self) -> Any:
        return self.message


class RichHandler(logging.Handler):
    """A logging handler that renders records with rs_rich (``rich.logging.RichHandler``)."""

    KEYWORDS: ClassVar[Optional[List[str]]] = [
        "GET",
        "POST",
        "HEAD",
        "PUT",
        "DELETE",
        "OPTIONS",
        "TRACE",
        "PATCH",
    ]
    #: ``None``: the Rust ``ReprHighlighter``.
    HIGHLIGHTER_CLASS: ClassVar[Optional[type]] = None

    def __init__(
        self,
        level: Any = logging.NOTSET,
        console: Any = None,
        *,
        show_time: bool = True,
        omit_repeated_times: bool = True,
        show_level: bool = True,
        show_path: bool = True,
        enable_link_path: bool = True,
        highlighter: Any = None,
        markup: bool = False,
        rich_tracebacks: bool = False,
        tracebacks_width: Optional[int] = None,
        tracebacks_code_width: Optional[int] = 88,
        tracebacks_extra_lines: int = 3,
        tracebacks_theme: Optional[str] = None,
        tracebacks_word_wrap: bool = True,
        tracebacks_show_locals: bool = False,
        tracebacks_suppress: Iterable[Any] = (),
        tracebacks_max_frames: int = 100,
        locals_max_length: int = 10,
        locals_max_string: int = 80,
        log_time_format: Any = "[%x %X]",
        keywords: Optional[List[str]] = None,
    ) -> None:
        from rs_rich import _native, get_console

        super().__init__(level=level)
        self.console = console or get_console()
        if highlighter is None:
            highlighter_class = self.HIGHLIGHTER_CLASS or _native._ReprHighlighter
            highlighter = highlighter_class()
        self.highlighter = highlighter
        self._log_render = _native._LogRender(
            show_time=show_time,
            show_level=show_level,
            show_path=show_path,
            time_format=log_time_format,
            omit_repeated_times=omit_repeated_times,
            level_width=None,
        )
        self.enable_link_path = enable_link_path
        self.markup = markup
        self.rich_tracebacks = rich_tracebacks
        self.tracebacks_width = tracebacks_width
        self.tracebacks_extra_lines = tracebacks_extra_lines
        self.tracebacks_theme = tracebacks_theme
        self.tracebacks_word_wrap = tracebacks_word_wrap
        self.tracebacks_show_locals = tracebacks_show_locals
        self.tracebacks_suppress = tracebacks_suppress
        self.tracebacks_max_frames = tracebacks_max_frames
        self.tracebacks_code_width = tracebacks_code_width
        self.locals_max_length = locals_max_length
        self.locals_max_string = locals_max_string
        self.keywords = keywords

    def get_level_text(self, record: logging.LogRecord) -> Any:
        """The level name, padded and styled ``logging.level.<name>``."""
        from rs_rich import _native

        return _native._rich_handler_level_text(record)

    def emit(self, record: logging.LogRecord) -> None:
        """Invoked by logging."""
        message = self.format(record)
        traceback = None
        if self.rich_tracebacks and record.exc_info and record.exc_info != (None, None, None):
            from rs_rich import _native

            traceback_class = getattr(_native, "Traceback", None)
            if traceback_class is None:
                raise NotImplementedError(
                    "RichHandler(rich_tracebacks=True) needs rs_rich.traceback.Traceback"
                )
            exc_type, exc_value, exc_traceback = record.exc_info
            traceback = traceback_class.from_exception(
                exc_type,
                exc_value,
                exc_traceback,
                width=self.tracebacks_width,
                code_width=self.tracebacks_code_width,
                extra_lines=self.tracebacks_extra_lines,
                theme=self.tracebacks_theme,
                word_wrap=self.tracebacks_word_wrap,
                show_locals=self.tracebacks_show_locals,
                locals_max_length=self.locals_max_length,
                locals_max_string=self.locals_max_string,
                suppress=self.tracebacks_suppress,
                max_frames=self.tracebacks_max_frames,
            )
            message = record.getMessage()
            if self.formatter:
                record.message = record.getMessage()
                formatter = self.formatter
                if hasattr(formatter, "usesTime") and formatter.usesTime():
                    record.asctime = formatter.formatTime(record, formatter.datefmt)
                message = formatter.formatMessage(record)

        message_renderable = self.render_message(record, message)
        log_renderable = self.render(
            record=record, traceback=traceback, message_renderable=message_renderable
        )
        try:
            self.console.print(log_renderable)
        except Exception:
            self.handleError(record)

    def render_message(self, record: logging.LogRecord, message: str) -> Any:
        """The message as ``Text``: markup (if enabled), highlighted, keywords styled."""
        from rs_rich import _native

        return _native._rich_handler_render_message(self, record, message)

    def render(self, *, record: logging.LogRecord, traceback: Any, message_renderable: Any) -> Any:
        """The log row for a record."""
        from rs_rich import _native

        return _native._rich_handler_render(self, record, traceback, message_renderable)


class _Reader(RawIOBase, BinaryIO):  # type: ignore[misc]
    """A reader that advances a progress task as it is read."""

    def __init__(self, handle: BinaryIO, progress: Any, task: Any, close_handle: bool = True) -> None:
        self.handle = handle
        self.progress = progress
        self.task = task
        self.close_handle = close_handle
        self._closed = False

    def __enter__(self) -> "_Reader":
        self.handle.__enter__()
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()

    def __iter__(self) -> BinaryIO:
        return self

    def __next__(self) -> bytes:
        line = next(self.handle)
        self.progress.advance(self.task, advance=len(line))
        return line

    @property
    def closed(self) -> bool:
        return self._closed

    def fileno(self) -> int:
        return self.handle.fileno()

    def isatty(self) -> bool:
        return self.handle.isatty()

    @property
    def mode(self) -> str:
        return self.handle.mode

    @property
    def name(self) -> str:
        return self.handle.name

    def readable(self) -> bool:
        return self.handle.readable()

    def seekable(self) -> bool:
        return self.handle.seekable()

    def writable(self) -> bool:
        return False

    def read(self, size: int = -1) -> bytes:
        block = self.handle.read(size)
        self.progress.advance(self.task, advance=len(block))
        return block

    def readinto(self, b: Any) -> Any:  # type: ignore[override]
        n = self.handle.readinto(b)  # type: ignore[attr-defined]
        self.progress.advance(self.task, advance=n)
        return n

    def readline(self, size: int = -1) -> bytes:  # type: ignore[override]
        line = self.handle.readline(size)
        self.progress.advance(self.task, advance=len(line))
        return line

    def readlines(self, hint: int = -1) -> List[bytes]:
        lines = self.handle.readlines(hint)
        self.progress.advance(self.task, advance=sum(map(len, lines)))
        return lines

    def close(self) -> None:
        if self.close_handle:
            self.handle.close()
        self._closed = True

    def seek(self, offset: int, whence: int = 0) -> int:
        pos = self.handle.seek(offset, whence)
        self.progress.update(self.task, completed=pos)
        return pos

    def tell(self) -> int:
        return self.handle.tell()

    def write(self, s: Any) -> int:
        raise UnsupportedOperation("write")

    def writelines(self, lines: Iterable[Any]) -> None:
        raise UnsupportedOperation("writelines")


class _ReadContext:
    """Starts the progress and opens the reader together (``wrap_file`` / ``open``)."""

    def __init__(self, progress: Any, reader: Any) -> None:
        self.progress = progress
        self.reader = reader

    def __enter__(self) -> Any:
        self.progress.start()
        return self.reader.__enter__()

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.progress.stop()
        self.reader.__exit__(exc_type, exc_val, exc_tb)


PromptError.__module__ = "rs_rich.prompt"
InvalidResponse.__module__ = "rs_rich.prompt"
RichHandler.__module__ = "rs_rich.logging"
_Reader.__module__ = "rs_rich.progress"
_ReadContext.__module__ = "rs_rich.progress"
del io
