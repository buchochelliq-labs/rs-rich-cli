"""The first intuiTUIve app, in Python: a counter.

    python crates/rich-py/examples/tui/counter.py

+ adds one, - takes one away, q quits. The same app as the guide's first
Rust one (docs/guide/intuituive/index.md), name for name.
"""

from rs_rich.tui import App, column, label, signal, text


def build():
    count = signal(0)
    return (
        column([
            text(lambda: f"[b]Count:[/] {count.get()}").panel("Counter"),
            label("[dim]+ adds one · - takes one away · q quits"),
        ])
        .on_key("+", lambda cx: count.update(lambda c: c + 1))
        .on_key("-", lambda cx: count.update(lambda c: c - 1))
        .on_key("q", lambda cx: cx.quit())
    )


def counter_app():
    return App(build)


if __name__ == "__main__":
    counter_app().run()
