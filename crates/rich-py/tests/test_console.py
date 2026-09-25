"""rs_rich.console.Console."""

from __future__ import annotations

import gc
import io
import threading
import time
import weakref

import pytest

from conftest import in_thread, render
from rs_rich.console import Console
from rs_rich.errors import ConsoleError, MarkupError
from rs_rich.panel import Panel
from rs_rich.style import Style
from rs_rich.text import Text


class TestConstruction:
    def test_a_file_that_is_not_a_terminal_is_80_columns_and_colourless(self):
        console = Console(file=io.StringIO())
        assert console.is_terminal is False
        assert console.color_system is None
        assert console.width == 80

    def test_columns_sets_the_width_of_a_non_terminal_file(self, monkeypatch):
        monkeypatch.setenv("COLUMNS", "33")
        assert Console(file=io.StringIO()).width == 33

    def test_explicit_width_and_height(self):
        console = Console(file=io.StringIO(), width=12, height=7)
        assert (console.width, console.height) == (12, 7)

    @pytest.mark.parametrize(
        "name, expected",
        [("standard", "standard"), ("256", "256"), ("truecolor", "truecolor"), (None, None)],
    )
    def test_colour_systems(self, name, expected):
        console = Console(file=io.StringIO(), force_terminal=True, color_system=name)
        assert console.is_terminal is True
        assert console.color_system == expected

    def test_an_unknown_colour_system_is_a_value_error(self):
        with pytest.raises(ValueError, match="not a valid color system"):
            Console(color_system="16bit")

    @pytest.mark.parametrize("width", [2**16 + 1, 2**40])
    def test_a_width_past_65536_is_a_value_error(self, width):
        # rich 15.0.0 accepts it and fails on allocation at print time; core
        # would abort the process instead, so the binding refuses it up front.
        with pytest.raises(ValueError, match="width must be at most 65536"):
            Console(file=io.StringIO(), width=width)

    def test_the_widest_console(self):
        out = io.StringIO()
        Console(file=out, width=2**16).rule()
        assert out.getvalue() == "─" * 2**16 + "\n"

    def test_file_is_the_given_file_or_stdout(self, capsys):
        out = io.StringIO()
        assert Console(file=out).file is out
        import sys

        assert Console().file is sys.stdout


class TestPrint:
    def test_markup_is_rendered(self):
        assert render("[bold]hi[/] there") == "hi there\n"
        assert render("[bold]hi[/]", color=True) == "\x1b[1mhi\x1b[0m\n"

    def test_strings_are_joined_with_sep(self):
        assert render("a", "b", "c") == "a b c\n"
        assert render("a", "b", sep="-") == "a-b\n"

    def test_numbers_bools_and_none_print_as_their_str(self):
        assert render(1, 2.5, True, None) == "1 2.5 True None\n"

    def test_numbers_are_highlighted_unless_disabled(self):
        assert render("n=42", color=True) == "\x1b[33mn\x1b[0m=\x1b[1;36m42\x1b[0m\n"
        out = io.StringIO()
        Console(file=out, force_terminal=True, color_system="truecolor", highlight=False).print("n=42")
        assert out.getvalue() == "n=42\n"

    def test_emoji_codes(self):
        assert render(":thumbs_up:") == "👍\n"
        out = io.StringIO()
        Console(file=out, emoji=False).print(":thumbs_up:")
        assert out.getvalue() == ":thumbs_up:\n"

    def test_no_color_keeps_other_attributes(self):
        out = io.StringIO()
        console = Console(file=out, force_terminal=True, color_system="truecolor", no_color=True)
        console.print("[bold red]x[/]")
        assert out.getvalue() == "\x1b[1mx\x1b[0m\n"

    def test_an_empty_print_is_a_blank_line(self):
        assert render() == "\n"

    def test_text_joins_strings_and_renderables_print_on_their_own_lines(self):
        # As Rich's `_collect_renderables`: a Text joins the strings around it.
        assert render("before", Text("text"), "after") == "before text after\n"
        assert render("before", Panel.fit("p"), "after") == "before\n╭───╮\n│ p │\n╰───╯\nafter\n"

    def test_long_strings_wrap_at_the_width(self):
        assert render("one two three four", width=9) == "one two \nthree \nfour\n"

    def test_justify_strings(self):
        assert render("mid", width=9, justify="center") == "   mid   \n"
        assert render("end", width=9, justify="right") == "      end\n"

    def test_writes_to_stdout_when_no_file_is_given(self, capsys):
        Console(width=20).print("to stdout")
        assert capsys.readouterr().out == "to stdout\n"

    def test_bad_markup_raises_markup_error(self):
        with pytest.raises(MarkupError):
            render("[/bold]")
        assert issubclass(MarkupError, ConsoleError) and not issubclass(MarkupError, ValueError)

    def test_containers_need_pretty_printing_which_is_not_implemented_yet(self):
        with pytest.raises(NotImplementedError, match="cannot render dict"):
            render({"a": 1})

    def test_an_invalid_justify_is_a_value_error(self):
        with pytest.raises(ValueError, match="invalid justify"):
            render("x", justify="middle")


class TestRule:
    def rule(self, *args, **kwargs):
        out = io.StringIO()
        Console(file=out, width=20).rule(*args, **kwargs)
        return out.getvalue()

    def test_a_plain_rule_spans_the_width(self):
        assert self.rule() == "─" * 20 + "\n"

    def test_a_titled_rule(self):
        assert self.rule("Title") == "────── Title ───────\n"

    def test_characters(self):
        assert self.rule(characters="=") == "=" * 20 + "\n"

    def test_style(self):
        out = io.StringIO()
        Console(file=out, width=4, force_terminal=True, color_system="truecolor").rule(style="red")
        assert out.getvalue() == "\x1b[31m────\x1b[0m\n"
        out = io.StringIO()
        Console(file=out, width=4, force_terminal=True, color_system="truecolor").rule(
            style=Style(color="blue")
        )
        assert out.getvalue() == "\x1b[34m────\x1b[0m\n"


class TestExportText:
    def test_exports_what_was_printed_and_clears(self):
        out = io.StringIO()
        console = Console(file=out, width=20, record=True, force_terminal=True, color_system="truecolor")
        console.print("[bold]one[/]")
        console.rule("two")
        assert console.export_text() == "one\n" + "─────── two ────────\n"
        assert console.export_text() == ""

    def test_styles_and_keeping_the_record(self):
        console = Console(file=io.StringIO(), record=True, force_terminal=True, color_system="truecolor")
        console.print("[bold]x[/]")
        assert console.export_text(clear=False, styles=True) == "\x1b[1mx\x1b[0m\n"
        assert console.export_text() == "x\n"

    def test_requires_record(self):
        with pytest.raises(RuntimeError, match="record=True"):
            Console(file=io.StringIO()).export_text()


class TestFlush:
    def test_every_print_and_rule_flushes_the_file(self):
        flushed = []

        class Sink(io.StringIO):
            def flush(self):
                flushed.append(self.getvalue())

        console = Console(file=Sink(), width=10)
        console.print("hi")
        console.rule()
        console.print()
        # rich 15.0.0 flushes once after each call, after writing.
        assert flushed == ["hi\n", "hi\n" + "─" * 10 + "\n", "hi\n" + "─" * 10 + "\n\n"]

    def test_a_file_without_flush_is_an_attribute_error(self):
        written = []

        class WriteOnly:
            def write(self, text):
                written.append(text)

        with pytest.raises(AttributeError, match="flush"):
            Console(file=WriteOnly()).print("x")
        assert written == ["x\n"]  # as in rich, the write happens first

    def test_errors_from_flush_propagate(self):
        class Broken(io.StringIO):
            def flush(self):
                raise OSError("disk full")

        with pytest.raises(OSError, match="disk full"):
            Console(file=Broken()).print("x")


class TestThreads:
    def test_print_from_another_thread(self):
        console = Console(file=io.StringIO(), width=20)
        console.print("main")
        assert in_thread(lambda: console.print("worker")) is None
        assert console.file.getvalue() == "main\nworker\n"

    def test_the_global_print_from_another_thread(self, capsys):
        import rs_rich

        rs_rich.get_console()  # created on this thread, as on first use
        assert in_thread(lambda: rs_rich.print("worker")) is None
        assert capsys.readouterr().out == "worker\n"

    def test_concurrent_prints_do_not_interleave(self):
        class Slow(io.StringIO):
            # Gives up the GIL mid-write, so other threads get to print.
            def write(self, text):
                for char in text:
                    super().write(char)
                    time.sleep(0)
                return len(text)

        console = Console(file=Slow(), width=20, record=True)

        def work(n):
            for i in range(20):
                console.print(f"{n}-{i}", "end")

        workers = [threading.Thread(target=work, args=(n,)) for n in range(4)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
        lines = console.file.getvalue().splitlines()
        assert sorted(lines) == sorted(f"{n}-{i} end" for n in range(4) for i in range(20))
        assert sorted(console.export_text().splitlines()) == sorted(lines)

    def test_printing_from_inside_file_write_is_a_runtime_error(self):
        class Echo(io.StringIO):
            def write(self, text):
                console.print("again")
                return super().write(text)

        console = Console(file=Echo())
        with pytest.raises(RuntimeError, match="already printing"):
            console.print("x")
        # The console is free again afterwards.
        console.file.write = io.StringIO().write
        console.print("y")


class TestGarbageCollection:
    def test_a_cycle_through_the_file_is_collected(self):
        class Holder:
            def write(self, text):
                return len(text)

            def flush(self):
                pass

        holder = Holder()
        holder.console = Console(file=holder, width=10)
        alive = weakref.ref(holder)
        del holder
        gc.collect()
        assert alive() is None


class TestFullConsole:
    def test_capture(self):
        from rs_rich.console import CaptureError

        console = Console(file=io.StringIO(), force_terminal=True, color_system="truecolor")
        with console.capture() as capture:
            with pytest.raises(CaptureError, match="not available"):
                capture.get()
            console.print("[bold]x[/]")
            console.rule(characters="-")
        assert capture.get() == "\x1b[1mx\x1b[0m\n\x1b[92m" + "-" * 80 + "\x1b[0m\n"
        assert console.file.getvalue() == ""

    def test_captures_nest_and_share_one_buffer_as_in_rich(self):
        console = Console(file=io.StringIO())
        console.begin_capture()
        console.print("outer")
        console.begin_capture()
        console.print("inner")
        assert console.end_capture() == "outer\ninner\n"
        console.print("after")
        assert console.end_capture() == "after\n"
        assert console.file.getvalue() == ""

    def test_capture_only_holds_this_threads_output(self):
        console = Console(file=io.StringIO())
        with console.capture() as capture:
            assert in_thread(lambda: console.print("worker")) is None
            console.print("main")
        assert capture.get() == "main\n"
        assert console.file.getvalue() == "worker\n"

    def test_with_console_holds_output_until_the_block_ends(self):
        flushes = []

        class Sink(io.StringIO):
            def flush(self):
                flushes.append(self.getvalue())

        console = Console(file=Sink(), record=True)
        with console:
            console.print("one")
            console.print("two")
            assert console.file.getvalue() == ""
        assert console.file.getvalue() == "one\ntwo\n"
        assert flushes == ["one\ntwo\n"]
        assert console.export_text() == "one\ntwo\n"

    def test_quiet_prints_and_records_nothing(self):
        console = Console(file=io.StringIO(), quiet=True, record=True)
        console.print("x")
        assert console.file.getvalue() == "" and console.export_text() == ""
        console.quiet = False
        console.print("y")
        assert console.file.getvalue() == "y\n"

    def test_stderr(self, capsys):
        console = Console(stderr=True, width=20)
        console.print("to stderr")
        assert capsys.readouterr().err == "to stderr\n"
        assert console.stderr is True

    def test_file_width_height_and_size_can_be_set(self):
        console = Console(file=io.StringIO(), width=20)
        other = io.StringIO()
        console.file = other
        console.width = 10
        console.print("one two three")
        assert other.getvalue() == "one two \nthree\n"
        console.size = (30, 5)
        assert (console.width, console.height) == (30, 5)
        assert tuple(console.size) == (30, 5) and console.size.width == 30
        with pytest.raises(ValueError, match="at most 65536"):
            console.width = 2**17

    def test_encoding_is_the_files(self):
        class Latin(io.StringIO):
            encoding = "Latin-1"

        assert Console(file=Latin()).encoding == "latin-1"
        assert Console(file=io.StringIO()).encoding == "utf-8"
        assert Console(file=Latin()).options.ascii_only is True

    def test_control_codes_reach_terminals_only(self):
        terminal = Console(file=io.StringIO(), force_terminal=True)
        terminal.clear()
        terminal.bell()
        assert terminal.show_cursor(False) is True
        assert terminal.set_alt_screen(True) is True and terminal.is_alt_screen
        assert terminal.file.getvalue() == "\x1b[2J\x1b[H\x07\x1b[?25l\x1b[?1049h\x1b[H"
        plain = Console(file=io.StringIO())
        plain.clear()
        assert plain.show_cursor() is False and plain.set_alt_screen() is False
        assert plain.file.getvalue() == ""

    def test_themes(self):
        from rs_rich.errors import MissingStyle
        from rs_rich.theme import Theme, ThemeStackError

        console = Console(file=io.StringIO(), theme=Theme({"accent": "bold"}))
        assert str(console.get_style("accent")) == "bold"
        console.push_theme(Theme({"accent": "red"}))
        assert str(console.get_style("accent")) == "red"
        console.pop_theme()
        with pytest.raises(ThemeStackError, match="Unable to pop base theme"):
            console.pop_theme()
        with pytest.raises(MissingStyle, match="Failed to get style 'nope'"):
            console.get_style("nope")
        assert str(console.get_style("nope", default="italic")) == "italic"
        style = Style(bold=True)
        assert console.get_style(style) == style
        assert Theme({"x": "red"}, inherit=False).styles == {"x": Style.parse("red")}
        assert "[styles]" in Theme({"x": "red"}, inherit=False).config

    def test_input(self, monkeypatch):
        out = io.StringIO()
        console = Console(file=out)
        monkeypatch.setattr("sys.stdin", io.StringIO("typed\n"))
        assert console.input("[b]prompt:[/] ") == "typed"
        assert out.getvalue() == "prompt: "
        monkeypatch.setattr("getpass.getpass", lambda prompt, stream=None: "secret")
        assert console.input(password=True) == "secret"
        assert console.input(stream=io.StringIO("line\n")) == "line\n"

    def test_log_locals_and_non_text_messages_are_not_implemented_yet(self):
        console = Console(file=io.StringIO())
        with pytest.raises(NotImplementedError, match="locals"):
            console.log("x", log_locals=True)
        with pytest.raises(NotImplementedError, match="Text message"):
            console.log(Panel("x"))

    def test_print_json_refuses_what_core_cannot_render(self):
        console = Console(file=io.StringIO())
        with pytest.raises(NotImplementedError, match="indent=2"):
            console.print_json("[1]", indent=4)
        with pytest.raises(NotImplementedError, match="ensure_ascii"):
            console.print_json("[1]", ensure_ascii=True)
        with pytest.raises(TypeError, match="json must be str"):
            console.print_json(1)

    def test_exports_need_record_and_a_default_svg_id(self, tmp_path):
        console = Console(file=io.StringIO())
        for export in (console.export_html, console.export_svg, console.export_text):
            with pytest.raises(RuntimeError, match="record=True"):
                export()
        recording = Console(file=io.StringIO(), record=True)
        recording.print("x")
        svg = recording.export_svg(clear=False)
        assert svg == recording.export_svg()  # a stable id by default
        with pytest.raises(NotImplementedError, match="code_format"):
            recording.export_html(code_format="{code}")

    def test_unsupported_constructor_options_are_refused(self):
        with pytest.raises(NotImplementedError, match="tab_size"):
            Console(tab_size=4)
        with pytest.raises(NotImplementedError, match="emoji_variant"):
            Console(emoji_variant="text")
        with pytest.raises(NotImplementedError, match="Jupyter"):
            Console(force_jupyter=True)

    def test_methods_other_areas_provide_raise_until_they_do(self):
        console = Console(file=io.StringIO())
        for method in (console.status, console.pager, console.screen, console.print_exception):
            with pytest.raises(NotImplementedError):
                method()

    def test_console_style_applies_to_everything(self):
        out = io.StringIO()
        Console(file=out, style="bold", force_terminal=True, color_system="truecolor").print("x")
        assert out.getvalue() == "\x1b[1mx\x1b[0m\n"


class TestProtocolTypes:
    def test_console_options(self):
        from rs_rich.console import ConsoleOptions

        options = Console(file=io.StringIO(), width=50, height=10).options
        assert isinstance(options, ConsoleOptions)
        assert (options.min_width, options.max_width, options.max_height) == (1, 50, 10)
        assert (options.justify, options.overflow, options.no_wrap, options.height) == (None, None, None, None)
        updated = options.update(width=-3, justify="right", overflow="crop", highlight=False)
        assert (updated.min_width, updated.max_width, updated.justify, updated.highlight) == (0, 0, "right", False)
        assert options.max_width == 50  # a copy
        assert options.update_height(4).height == 4 and options.update_height(4).reset_height().height is None
        assert options.update_dimensions(7, 3).max_width == 7
        with pytest.raises(ValueError, match="invalid justify"):
            options.update(justify="middle")
        with pytest.raises(TypeError, match="unexpected keyword"):
            options.update(colour="red")

    def test_measurement(self):
        from rs_rich.measure import Measurement

        measurement = Measurement(3, 8)
        assert tuple(measurement) == (3, 8) and measurement == (3, 8)
        assert measurement.span == 5
        assert Measurement(9, 2).normalize() == (2, 2)
        assert measurement.with_maximum(5) == (3, 5)
        assert measurement.with_minimum(4) == (4, 8)
        assert measurement.clamp(4, 6) == (4, 6)
        assert repr(measurement) == "Measurement(minimum=3, maximum=8)"
        console = Console(file=io.StringIO())
        assert Measurement.get(console, console.options, "abc") == (3, 3)

    def test_segment(self):
        from rs_rich.segment import Segment

        segment = Segment("hi", Style(bold=True))
        text, style, control = segment
        assert (text, str(style), control) == ("hi", "bold", None)
        assert segment.cell_length == 2 and bool(segment) and not segment.is_control
        assert Segment.line() == Segment("\n") and Segment.line().text == "\n"
        assert Segment("x", control=[(1,)]).cell_length == 0
        assert segment == ("hi", Style(bold=True), None)
