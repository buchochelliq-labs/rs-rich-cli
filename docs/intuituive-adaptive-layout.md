# Container-aware grids

`grid(...).adaptive_columns(min_track, max_tracks)` chooses equal flexible
columns from the grid's actual content width, after padding. The same retained
children survive a resize: input values, focus, widget IDs and scroll state do
not need to be rebuilt. A 40-column split uses 40 columns even in a 160-column
terminal. This additive post-1.0 API supports #866/#867; existing fixed grids
keep their behavior.

```rust
use intuituive::prelude::*;
let cards = grid([], [label("CPU"), label("Memory"), label("Disk")])
    .adaptive_columns(24, 3)
    .gap(1)
    .rows([Size::Auto])
    .auto();
```

Use measured `Auto` rows for wrapping labels. Set a minimum that accommodates
your controls; it is a fit preference, so a parent narrower than that minimum
still gets one track. Zero minimum and maximum normalize to one for the widget.
Adaptive columns override constructor and stylesheet columns, but retain row,
span, visibility and gap rules. Equal flexible tracks can differ by one cell
when space does not divide evenly. Collapsed borders conservatively calculate
fit with zero gap, without counting the shared border cells as extra capacity.

Adapters arranging their own rectangles can call
`layout::adaptive_columns(available, min_track, gap, max_tracks)`. This pure
policy returns zero when `max_tracks` is zero, and one logical track for a
nonempty grid even at zero width. It does not measure or wrap application text.

Compose with existing `split` for independently sized panes and
`scroll_virtual_with(...).item_key(...).follow_end_with(...)` for keyed history.
Keep `Transport` as a local clock; remote state must remain authoritative.
Themes continue to use `App::theme`, `Theme::light/dark`, and ordinary node
styles; adaptive layout introduces no new theme mechanism.

Run the framework-only example (no art or browser feature required):

```sh
cargo run -p rs-rich-intuituive --no-default-features --example adaptive_layout
cargo run -p rs-rich-intuituive --no-default-features --example adaptive_layout -- --record work/adaptive
```

The recorded mode exercises 80×24, 120×40, 160×50 and 40×24 and asserts that the
same focused input and its draft survive the round trip. SVGs are Driver
renderings, not evidence of native terminal graphics support.

Noctiluca uses this grid for text-field labels, inputs and draft status and uses
the same fit policy for source grids. Its narrow two-pane adapter requires an
actual flex container with exactly two visible named sections, separately
containing a video and a live log. This reflows source structure without
inventing shared playback/chat state. An unnamed common ancestor or an unrelated
third child does not qualify. The existing SnapGrids adaptation toggle controls
source reflow. Original alternative text remains accessible in reader view (`r`)
even when complete equivalent figure text suppresses the extra image caption.

At terminal widths below 80, Noctiluca's adapted projection with SnapGrids enabled
requests an 80-column CSS source viewport (640 CSS pixels at its existing 8px
calibration), then projects into the actual terminal/container width. This avoids
source flex layouts collapsing video to zero pixels. Reader, faithful/no-adapt,
SnapGrids-off and pixel views use the actual viewport unless an existing explicit
site width applies; pixel view always uses its actual viewport. Physical terminal
cell metrics are independent. The floor adds no configuration option.
