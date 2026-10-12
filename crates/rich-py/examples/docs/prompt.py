"""The pictures on docs/python/prompt.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import io

from rs_rich.prompt import Confirm, IntPrompt, InvalidResponse, Prompt


def ask(console):
    # --8<-- [start:ask]
    fruit = Prompt.ask("Fruit", choices=["apple", "pear"], console=console,
                       stream=io.StringIO("kiwi\npear\n"))
    count = IntPrompt.ask("How many", default=2, console=console, stream=io.StringIO("lots\n3\n"))
    sure = Confirm.ask("Sure", default=True, console=console, stream=io.StringIO("y\n"))
    print(repr(fruit), count, sure)
    # --8<-- [end:ask]


# --8<-- [start:username]
class Username(Prompt):
    def process_response(self, value):
        value = super().process_response(value)
        if len(value) < 3:
            raise InvalidResponse("[prompt.invalid]At least 3 characters")
        return value
# --8<-- [end:username]


def custom(console):
    # --8<-- [start:custom]
    name = Username.ask("User", console=console, stream=io.StringIO("al\nalice\n"))
    print(repr(name))
    # --8<-- [end:custom]


shots = Shots("prompt")
shots.shot("ask", ask, width=70)
shots.shot("custom", custom, width=70)
shots.finish()
