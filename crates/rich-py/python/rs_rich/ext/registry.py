"""``rs_rich.ext.registry``: the extension registry (``rich_ext::registry``).

The registry is the plugin host, so ``rs_rich.plugins`` owns it; this module
re-exports it under the Rust module's name. No Rich counterpart.
"""

from .._native import ExtensionRegistry, install_defaults
from .._native import NumberHighlighter

__all__ = ["ExtensionRegistry", "install_defaults", "NumberHighlighter"]
