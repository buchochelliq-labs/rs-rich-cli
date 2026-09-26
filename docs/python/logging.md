# Logging

```python
from rs_rich.logging import RichHandler
```

`RichHandler` corresponds to `rich.logging.RichHandler`: a
`logging.Handler` that prints each record as a row of time, level, message
and `file:line`, with the message highlighted. The class is a Python
`logging.Handler` subclass (it has to be one); the row is built in Rust.

```text
RichHandler(level=logging.NOTSET, console=None, *, show_time=True,
            omit_repeated_times=True, show_level=True, show_path=True,
            enable_link_path=True, highlighter=None, markup=False,
            rich_tracebacks=False, tracebacks_width=None, ...,
            log_time_format="[%x %X]", keywords=None)
```

| Argument | Meaning |
|---|---|
| `console` | Where records go; the global console by default. |
| `show_time`, `show_level`, `show_path` | Which columns to show. |
| `omit_repeated_times` | Blank a time equal to the previous record's. |
| `enable_link_path` | Link the path to the source file (on terminals that support links). |
| `highlighter` | A callable taking and returning `Text`; `ReprHighlighter` by default. |
| `markup` | Read messages as console markup (a record's `markup` attribute overrides it). |
| `log_time_format` | A `strftime` format, or a callable taking a `datetime` and returning `Text`. |
| `keywords` | Words to style `logging.keyword` (default: HTTP methods, `RichHandler.KEYWORDS`). |
| `rich_tracebacks` and `tracebacks_*` | Render exceptions with `rs_rich.traceback.Traceback`. |

`get_level_text(record)`, `render_message(record, message)` and
`render(*, record, traceback, message_renderable)` can be overridden, as in
Rich.

```python
import logging
from rs_rich.console import Console
from rs_rich.logging import RichHandler

handler = RichHandler(console=Console(width=70), show_time=False, show_path=False)
log = logging.getLogger("docs.example")
log.propagate = False
log.addHandler(handler)
log.warning("GET /index.html took %.1f seconds", 2.5)
```

```text
WARNING  GET /index.html took 2.5 seconds                             
```
