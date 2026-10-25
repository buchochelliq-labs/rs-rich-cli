# Widget index

![Memo holding up a sign with a prompt on it](../../assets/intuituive/mascot/elephant-sign.svg){ .tui-mascot width="112" }
Every widget and component an intuiTUIve app is built from, with the
constructor that makes it and the guide section that documents it. Widgets
from an off-by-default feature name it: `art`, `micro`, `viz`, `files` and
`audio`.

## Layout and building blocks

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/column.svg){ .tui-icon width="20" } | [column](layout.md#nodes) | `column` | Children stacked top to bottom |
| ![](../../assets/intuituive/widgets/row.svg){ .tui-icon width="20" } | [row](layout.md#nodes) | `row` | Children side by side |
| ![](../../assets/intuituive/widgets/grid.svg){ .tui-icon width="20" } | [grid](layout.md#grids) | `grid` | Children placed on row and column tracks |
| ![](../../assets/intuituive/widgets/layers.svg){ .tui-icon width="20" } | [layers](layout.md#layers) | `layers` | Children drawn over each other |
| ![](../../assets/intuituive/widgets/scroll.svg){ .tui-icon width="20" } | [scroll](tables.md#tables-tabs-and-scrolling) | `scroll`, `scroll_x`, `scroll_both` | A viewport onto a child laid out at its full size |
| ![](../../assets/intuituive/widgets/split.svg){ .tui-icon width="20" } | [split](lists.md#trees-split-panes-and-virtual-lists) | `hsplit`, `vsplit`, `split` | Two panes with a divider you can drag |
| ![](../../assets/intuituive/widgets/treemap.svg){ .tui-icon width="20" } | [treemap](layout.md#treemaps) | `treemap` | Rectangles sized by weight |
| ![](../../assets/intuituive/widgets/switch.svg){ .tui-icon width="20" } | [switch](layout.md#nodes) | `switch`, `keyed` | One child at a time, chosen by a key; the others are kept |
| ![](../../assets/intuituive/widgets/each.svg){ .tui-icon width="20" } | [each](layout.md#nodes) | `each` | One child per key, kept by key across reorders |
| ![](../../assets/intuituive/widgets/panel.svg){ .tui-icon width="20" } | [panel](panels.md#panels) | `.panel(title)` | A border with a title around any node |
| ![](../../assets/intuituive/widgets/rule.svg){ .tui-icon width="20" } | [rule](panels.md#rules-and-a-border-on-any-node) | `rule` | A horizontal line, with an optional title |
| ![](../../assets/intuituive/widgets/fill.svg){ .tui-icon width="20" } | [fill](layout.md#nodes) | `fill` | An area filled with one glyph |
| ![](../../assets/intuituive/widgets/renderable.svg){ .tui-icon width="20" } | [renderable](layout.md#nodes) | `renderable`, `described` | Any rich renderable, rebuilt when the signals it read change |
| ![](../../assets/intuituive/widgets/component.svg){ .tui-icon width="20" } | [component](components.md#components) | `component`, `repeating` | A hosted rs-rich-interact component |
| ![](../../assets/intuituive/widgets/widget.svg){ .tui-icon width="20" } | [widget](widgets.md#your-own-widgets) | `widget` | Your own widget, through the Widget trait |

## Text and input

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/text.svg){ .tui-icon width="20" } | [text](layout.md#nodes) | `text`, `text!` | Console markup that redraws when a signal it reads changes |
| ![](../../assets/intuituive/widgets/label.svg){ .tui-icon width="20" } | [label](layout.md#nodes) | `label` | Fixed console markup |
| ![](../../assets/intuituive/widgets/fit_text.svg){ .tui-icon width="20" } | [fit_text](layout.md#text-alignment-wrapping-and-overflow) | `fit_text` | The longest of several forms of a text that fits |
| ![](../../assets/intuituive/widgets/digits.svg){ .tui-icon width="20" } | [digits](plugins.md#levels-sparklines-and-big-text) | `digits` | Large seven-segment digits |
| ![](../../assets/intuituive/widgets/big_text.svg){ .tui-icon width="20" } | [big_text](plugins.md#levels-sparklines-and-big-text) | `big_text` | FIGlet lettering (`art` feature) |
| ![](../../assets/intuituive/widgets/facts.svg){ .tui-icon width="20" } | [facts](widgets.md#bars-for-an-apps-chrome) | `facts`, `facts_of` | A description list of keys and values |
| ![](../../assets/intuituive/widgets/input.svg){ .tui-icon width="20" } | [input](components.md#inputs-bound-to-signals) | `input` | A one-line input bound to a String signal |
| ![](../../assets/intuituive/widgets/number_input.svg){ .tui-icon width="20" } | [number_input](components.md#inputs-bound-to-signals) | `number_input` | A number in a range, stepped with keys |
| ![](../../assets/intuituive/widgets/text_area.svg){ .tui-icon width="20" } | [text_area](components.md#text-areas) | `text_area` | Multi-line text editing |
| ![](../../assets/intuituive/widgets/search_bar.svg){ .tui-icon width="20" } | [search_bar](text-editing.md#search-and-the-find-bar) | `search_bar` | A find bar over a text area's search: the pattern, where it is, regex and case flags |
| ![](../../assets/intuituive/widgets/command_line.svg){ .tui-icon width="20" } | [command_line](components.md#completion-history-and-a-command-line) | `command_line` | A vim-style command line, with completion and history |
| ![](../../assets/intuituive/widgets/calendar.svg){ .tui-icon width="20" } | [calendar](components.md#calendars-and-dates) | `calendar` | A month to pick a date from |
| ![](../../assets/intuituive/widgets/month.svg){ .tui-icon width="20" } | [month](components.md#calendars-and-dates) | `month` | One month, drawn from a date |
| ![](../../assets/intuituive/widgets/year_view.svg){ .tui-icon width="20" } | [year_view](components.md#calendars-and-dates) | `year_view` | A year of months |
| ![](../../assets/intuituive/widgets/agenda.svg){ .tui-icon width="20" } | [agenda](components.md#calendars-and-dates) | `agenda` | Events listed by day |
| ![](../../assets/intuituive/widgets/time_picker.svg){ .tui-icon width="20" } | [time_picker](components.md#times-and-a-date-with-a-time) | `time_picker` | A time of day, picked field by field |
| ![](../../assets/intuituive/widgets/datetime_picker.svg){ .tui-icon width="20" } | [datetime_picker](components.md#times-and-a-date-with-a-time) | `datetime_picker` | A date and a time together |
| ![](../../assets/intuituive/widgets/hex_view.svg){ .tui-icon width="20" } | [hex_view](widgets.md#bars-for-an-apps-chrome) | `hex_view` | Bytes as hex and text, scrolled |

## Form controls

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/button.svg){ .tui-icon width="20" } | [button](controls.md#buttons) | `button`, `button_of` | A button pressed with Enter, Space or a click, in three looks; its label may change |
| ![](../../assets/intuituive/widgets/checkbox.svg){ .tui-icon width="20" } | [checkbox](controls.md#check-boxes-and-switches) | `checkbox`, `checkbox_tri`, `checkbox_of` | A box ticked on and off, bound to a `Signal<bool>`, or with a mixed third state |
| ![](../../assets/intuituive/widgets/switch_toggle.svg){ .tui-icon width="20" } | [switch toggle](controls.md#check-boxes-and-switches) | `switch_toggle`, `switch_of` | An on/off switch, bound to a `Signal<bool>` |
| ![](../../assets/intuituive/widgets/radio.svg){ .tui-icon width="20" } | [radio group](controls.md#radio-groups) | `radio`, `radio_keyed` | One choice of several, shown at once; by index or by key |
| ![](../../assets/intuituive/widgets/slider.svg){ .tui-icon width="20" } | [slider](controls.md#sliders) | `slider` | A number in a range, by keys, click, drag or wheel |
| ![](../../assets/intuituive/widgets/select_field.svg){ .tui-icon width="20" } | [select field](controls.md#select-fields) | `select_field`, `select_field_keyed` | One choice of several, from a menu that opens under it; by index or by key |
| ![](../../assets/intuituive/widgets/collapsible.svg){ .tui-icon width="20" } | [collapsible](controls.md#collapsible-sections) | `collapsible` | A titled section that opens and closes |
| ![](../../assets/intuituive/widgets/spinner.svg){ .tui-icon width="20" } | [spinner](controls.md#spinners-and-loading) | `spinner`, `.loading_when` | An animated busy sign; a node dimmed while it loads |

## Lists and data

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/list.svg){ .tui-icon width="20" } | [list](lists.md#lists) | `list`, `list_optional` | Rows with a cursor |
| ![](../../assets/intuituive/widgets/virtual_list.svg){ .tui-icon width="20" } | [virtual_list](lists.md#trees-split-panes-and-virtual-lists) | `virtual_list`, `list_lazy` | A list that draws only the rows on screen |
| ![](../../assets/intuituive/widgets/list_search.svg){ .tui-icon width="20" } | [list_search](lists.md#picking-several-items-and-finding-in-a-list) | `list_search` | Finding in a list as you type |
| ![](../../assets/intuituive/widgets/multi_select.svg){ .tui-icon width="20" } | [multi_select](lists.md#picking-several-items-and-finding-in-a-list) | `multi_select` | Several items picked at once |
| ![](../../assets/intuituive/widgets/tree.svg){ .tui-icon width="20" } | [tree](lists.md#trees-split-panes-and-virtual-lists) | `tree`, `tree_keyed` | A tree with expanding nodes |
| ![](../../assets/intuituive/widgets/tree_lazy.svg){ .tui-icon width="20" } | [tree_lazy](lists.md#trees-split-panes-and-virtual-lists) | `tree_lazy` | A tree whose children load when opened |
| ![](../../assets/intuituive/widgets/table.svg){ .tui-icon width="20" } | [table](tables.md#tables-tabs-and-scrolling) | `table`, `table_with` | Rows and columns with a header, sorting and a cell cursor |
| ![](../../assets/intuituive/widgets/virtual_table.svg){ .tui-icon width="20" } | [virtual_table](tables.md#tables-tabs-and-scrolling) | `virtual_table` | A table that draws only the rows on screen |
| ![](../../assets/intuituive/widgets/source_table.svg){ .tui-icon width="20" } | [source_table](tables.md#rows-from-a-source) | `source_table`, `view_of` | A table whose rows come from a source |
| ![](../../assets/intuituive/widgets/cells.svg){ .tui-icon width="20" } | [cells](widgets.md#bars-for-an-apps-chrome) | `cells` | One row of fixed-width cells |
| ![](../../assets/intuituive/widgets/scrollbar.svg){ .tui-icon width="20" } | [scrollbar](tables.md#scrollbars) | `scrollbar`, `.scrollbar()` | A scrollbar beside any scrolled node |
| ![](../../assets/intuituive/widgets/log.svg){ .tui-icon width="20" } | [log](lists.md#logs) | `Log::new(..).view()` | Lines appended at the bottom, kept to a capacity |
| ![](../../assets/intuituive/widgets/log_targets.svg){ .tui-icon width="20" } | [log sources](lists.md#levels-targets-and-filters) | `log.targets_view()` | A log's sources, each with its own level |

## Navigation and chrome

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/tabs.svg){ .tui-icon width="20" } | [tabs](lists.md#how-a-tab-strip-looks) | `tabs` | A tab strip over one selected index |
| ![](../../assets/intuituive/widgets/tab_set.svg){ .tui-icon width="20" } | [tab_set](lists.md#a-set-of-tabs) | `tab_set`, `tabs_for` | Tabs you open and close, each with its own state |
| ![](../../assets/intuituive/widgets/status_bar.svg){ .tui-icon width="20" } | [status_bar](widgets.md#bars-for-an-apps-chrome) | `status_bar`, `seg` | Segments along the bottom line |
| ![](../../assets/intuituive/widgets/gauge.svg){ .tui-icon width="20" } | [gauge](widgets.md#bars-for-an-apps-chrome) | `seg_gauge` | A status-bar segment drawn as a gauge |
| ![](../../assets/intuituive/widgets/breadcrumb.svg){ .tui-icon width="20" } | [breadcrumb](widgets.md#bars-for-an-apps-chrome) | `breadcrumb` | A path of parts with separators |
| ![](../../assets/intuituive/widgets/key_hints.svg){ .tui-icon width="20" } | [key_hints](widgets.md#bars-for-an-apps-chrome) | `key_hints`, `key_hints_with` | The keys that work here, from the bindings |
| ![](../../assets/intuituive/widgets/menu_bar.svg){ .tui-icon width="20" } | [menu_bar](screens.md#menus) | `menu_bar` | A bar of menus that drop down |
| ![](../../assets/intuituive/widgets/miller.svg){ .tui-icon width="20" } | [miller](browsing.md#miller-columns) | `miller_of`, `miller_in` | Miller columns over any tree |
| ![](../../assets/intuituive/widgets/fuzzy_list.svg){ .tui-icon width="20" } | [fuzzy_list](browsing.md#a-streaming-fuzzy-list) | `fuzzy_list`, `finder_of` | A list filtered by a fuzzy query as it streams in |
| ![](../../assets/intuituive/widgets/preview_of.svg){ .tui-icon width="20" } | [preview_of](browsing.md#a-preview-pane) | `preview_of` | A pane showing whatever is selected |
| ![](../../assets/intuituive/widgets/jobs_panel.svg){ .tui-icon width="20" } | [jobs_panel](browsing.md#jobs-with-progress) | `jobs`, `jobs_panel` | Jobs with progress, their state and outcome |
| ![](../../assets/intuituive/widgets/transport.svg){ .tui-icon width="20" } | [transport](background.md#a-play-head-and-a-feed-as-one-value) | `transport` | A play head: play, pause and seek |

## Overlays

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/modal.svg){ .tui-icon width="20" } | [modal](screens.md#screens-and-modals) | `cx.modal` | A screen over the app that takes the keys |
| ![](../../assets/intuituive/widgets/popup.svg){ .tui-icon width="20" } | [popup](screens.md#pop-ups) | `cx.popup`, `cx.popup_at` | A box placed beside an anchor |
| ![](../../assets/intuituive/widgets/prompt.svg){ .tui-icon width="20" } | [prompt](screens.md#pop-ups) | `cx.prompt` | A one-line input in a pop-up |
| ![](../../assets/intuituive/widgets/pick.svg){ .tui-icon width="20" } | [pick](screens.md#pop-ups) | `cx.pick` | A list in a pop-up, picked with a key |
| ![](../../assets/intuituive/widgets/confirm.svg){ .tui-icon width="20" } | [confirm](screens.md#confirming) | `cx.confirm` | A yes-or-no question |
| ![](../../assets/intuituive/widgets/context_menu.svg){ .tui-icon width="20" } | [context_menu](screens.md#menus) | `context_menu`, `open_menu` | A menu at the pointer or an anchor |
| ![](../../assets/intuituive/widgets/palette.svg){ .tui-icon width="20" } | [palette](input.md#the-command-palette-and-help) | `cx.palette`, `cx.action_palette`, `action_overlay` | Every command, or actions of your own, found by typing, live while open |
| ![](../../assets/intuituive/widgets/help.svg){ .tui-icon width="20" } | [help](input.md#the-command-palette-and-help) | `cx.help` | The bindings, listed by section |
| ![](../../assets/intuituive/widgets/toast.svg){ .tui-icon width="20" } | [toast](screens.md#toasts-and-animations) | `cx.toast` | A short message that goes away on its own |
| ![](../../assets/intuituive/widgets/notify.svg){ .tui-icon width="20" } | [notify](screens.md#toasts-and-animations) | `cx.notify` | A notification with a level and a title |
| ![](../../assets/intuituive/widgets/tooltip.svg){ .tui-icon width="20" } | [tooltip](input.md#tooltips) | `.tooltip(markup)` | Text shown while the pointer rests on a node |

## Charts and media

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/pixels.svg){ .tui-icon width="20" } | [pixels](timers.md#sprites-stages-and-pixel-buffers) | `pixels`, `pixels_fit` | A pixel buffer drawn with half blocks |
| ![](../../assets/intuituive/widgets/stage.svg){ .tui-icon width="20" } | [stage](timers.md#sprites-stages-and-pixel-buffers) | `stage`, `stage_with` | Sprites placed over a backdrop, in z order |
| ![](../../assets/intuituive/widgets/effect.svg){ .tui-icon width="20" } | [effect](widgets.md#effects) | `effect` | A pass over a node's cells after it draws |
| ![](../../assets/intuituive/widgets/fx.svg){ .tui-icon width="20" } | [effect presets](widgets.md#effect-presets) | `fx::fade_in`, `fx::sweep`, …, `fx::play` | Ready effects (fades, dissolve, sweep, slide, glitch, rain, shimmer) that compose and run on cue |
| ![](../../assets/intuituive/widgets/placeholder.svg){ .tui-icon width="20" } | [placeholder](widgets.md#effect-presets) | `fx::placeholder` | Pulsing bars shown while content loads |
| ![](../../assets/intuituive/widgets/splash.svg){ .tui-icon width="20" } | [splash](widgets.md#a-splash-screen) | `fx::splash` | A cover node that fades into the app |
| ![](../../assets/intuituive/widgets/image.svg){ .tui-icon width="20" } | [image](plugins.md#images-art) | `image`, `image_with`, `image_of` | An image, as half blocks or terminal graphics (`art` feature) |
| ![](../../assets/intuituive/widgets/sprite_sheet.svg){ .tui-icon width="20" } | [sprite_sheet](timers.md#sprites-stages-and-pixel-buffers) | `sprite_sheet` | Frames cut from one image, for animation (`art` feature) |
| ![](../../assets/intuituive/widgets/micro_sprite.svg){ .tui-icon width="20" } | [micro_sprite](timers.md#sprites-stages-and-pixel-buffers) | `micro_sprite` | A micro asset as a sprite (`micro` feature) |
| ![](../../assets/intuituive/widgets/sparkline.svg){ .tui-icon width="20" } | [sparkline](plugins.md#levels-sparklines-and-big-text) | `sparkline` | Values as a line of bars (`viz` feature) |
| ![](../../assets/intuituive/widgets/meter.svg){ .tui-icon width="20" } | [meter](plugins.md#levels-sparklines-and-big-text) | `meter` | A level with a peak hold (`viz` feature) |
| ![](../../assets/intuituive/widgets/bar_spectrum.svg){ .tui-icon width="20" } | [bar_spectrum](plugins.md#levels-sparklines-and-big-text) | `bar_spectrum` | Bands as vertical bars (`viz` feature) |
| ![](../../assets/intuituive/widgets/spectrogram.svg){ .tui-icon width="20" } | [spectrogram](plugins.md#levels-sparklines-and-big-text) | `spectrogram` | Spectra over time, as colour (`viz` feature) |
| ![](../../assets/intuituive/widgets/persistence.svg){ .tui-icon width="20" } | [persistence](plugins.md#levels-sparklines-and-big-text) | `persistence` | Waveforms that fade like phosphor (`viz` feature) |
| ![](../../assets/intuituive/widgets/tuner.svg){ .tui-icon width="20" } | [tuner](plugins.md#levels-sparklines-and-big-text) | `tuner` | A pitch reading on a dial (`viz` feature) |

## Plugins

| | Widget | Constructor | What it is |
|---|---|---|---|
| ![](../../assets/intuituive/widgets/files-miller.svg){ .tui-icon width="20" } | [files-miller](plugins.md#a-file-managers-components) | `files::miller` | Miller columns over the file system (`files` feature) |
| ![](../../assets/intuituive/widgets/files-list.svg){ .tui-icon width="20" } | [files-list](plugins.md#a-file-managers-components) | `files::file_list` | A directory's entries as a file manager draws them (`files` feature) |
| ![](../../assets/intuituive/widgets/files-preview.svg){ .tui-icon width="20" } | [files-preview](plugins.md#a-file-managers-components) | `files::preview` | A file's contents: text, code, images, directories (`files` feature) |
| ![](../../assets/intuituive/widgets/files-finder.svg){ .tui-icon width="20" } | [files-finder](plugins.md#a-file-managers-components) | `files::finder` | Files found by a fuzzy query (`files` feature) |
| ![](../../assets/intuituive/widgets/files-crumb.svg){ .tui-icon width="20" } | [files-crumb](plugins.md#a-file-managers-components) | `files::path_crumb` | A path as a breadcrumb, home as ~ (`files` feature) |
| ![](../../assets/intuituive/widgets/files-pick.svg){ .tui-icon width="20" } | [files-pick](plugins.md#a-file-managers-components) | `files::pick_file` | A screen to pick a file (`files` feature) |
| ![](../../assets/intuituive/widgets/files-spot.svg){ .tui-icon width="20" } | [files-spot](plugins.md#a-file-managers-components) | `files::spot` | A file's details: size, times, permissions (`files` feature) |
| ![](../../assets/intuituive/widgets/files-tasks.svg){ .tui-icon width="20" } | [files-tasks](plugins.md#a-file-managers-components) | `files::tasks_panel` | Copies, moves and deletes in progress (`files` feature) |
| ![](../../assets/intuituive/widgets/audio-picker.svg){ .tui-icon width="20" } | [audio-picker](plugins.md#sound-audio-pulse-pcm-wav) | `audio::picker` | A list of audio devices to pick from (`audio` feature) |
