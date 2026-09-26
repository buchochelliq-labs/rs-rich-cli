"""``rich.emoji``: ``Emoji`` and ``NoEmoji``."""

from typing import Literal

from ._native import Emoji, NoEmoji

EmojiVariant = Literal["emoji", "text"]

__all__ = ["Emoji", "NoEmoji"]
