# Tasks, feeds and other threads

Work off the app's thread: tasks whose results come back as signals,
resources that load and reload, values from threads of your own (the
newest one, with a state that says when the feed stalls or ends), signals
over time (history, recent values, smoothing), handing the terminal to
another program, and a play head on the app's clock. Timers are on
[Timers, frames and games](timers.md).

## Tasks and resources

- **Tasks:** `spawn(work, done)` runs `work` on its own thread and `done`
  with the result back on the app's thread, where it can write signals,
  open a screen or quit. `spawn_future` does the same for a future that does
  not need a particular async runtime. Both return a `Task`, which you can
  `cancel()` (its result is dropped). `done` must be `Send`;
  **`spawn_then(work, done)`** keeps `done` on the app's thread while the
  work runs, so it can hold an `Rc` or a node's handle. **`spawn_stream(
  |stream| ..)`** runs a thread that sends results as it goes through
  `stream.proxy()`, counted as a task until it returns (an app that waits
  for its tasks waits for the whole stream) and told of a `cancel()` by
  `stream.cancelled()`.
- **Cleaning up:** `node.on_unmount(|| ..)` runs once when the node leaves
  the tree: its screen or pop-up closes, its `each` row goes, or the app
  ends (a `switch` keeps the pages it built). Stop what the node started
  there: set the flag a background walk reads, drop a watcher. The app
  may be ending, so leave its signals alone. An app dropped without
  `Driver::finish` (a test that stops early) closes its runtime all the
  same, so `latest` threads asleep on its clock end.

```rust
.on_key("r", move |_| {
    spawn(fetch_report, move |report, cx| {
        cx.push(move || report_screen(report));
    });
})
```

- **Resources:** `resource(fetch)` loads a value in the background and
  holds `Load::Loading`, `Load::Ready(value)` or `Load::Failed(error)`. A
  node that reads it redraws when it arrives; `reload()` fetches again and
  drops any older result still on its way. `resource_for(move || key(),
  |key| fetch(key))` fetches again whenever the key changes (the hovered
  file, the pane's width), dropping results for keys left behind, and
  `.stale_after(Duration::from_millis(150))` keeps the last value on show
  while the next loads, for that long at most: a preview pane that neither
  blinks nor shows the last file for long.

```rust
let user = resource(|| api::user(42));
text(move || match user.get() {
    Load::Loading => "[muted]loading…".into(),
    Load::Ready(user) => format!("Hello, {}", user.name),
    Load::Failed(error) => format!("[bad]{error}"),
})
.on_key("r", move |_| user.reload())
```

## Other threads and feeds

```rust
use intuituive::{latest_with_state, FeedState};

// The newest reading, and whether the sensor still answers.
let (reading, state) = latest_with_state(Duration::from_secs(2), |tx| {
    while let Some(value) = sensor.read() {
        if !tx.send(value) {
            break;
        }
    }
});
text(move || match state.get() {
    FeedState::Stalled(_) => "[warn]no data for 2 s".into(),
    FeedState::Failed(why) => format!("[bad]{why}"),
    _ => format!("{:?}", reading.get()),
})
```

- **Other threads:** signals live on the app's thread. Work you start
  yourself (a tokio runtime, a file watcher) sends closures through a
  `Proxy` from `cx.proxy()` or `app.proxy()`: `proxy.run(move ||
  status.set(body))`, or `proxy.run_with(|cx| …)` for a `Ctx` too.
- **The newest value from a thread:** `latest(|tx| loop { if !tx.send(read()) { break } })`
  runs the closure on a thread of its own and returns a
  `Signal<Option<T>>`: `None` until the first value, then the newest.
  Nothing queues: values the app had no time for are skipped (an audio
  buffer, a sensor, scope-tui's reader). `send` returns `false` once the
  app has finished. A thread that keeps a beat sleeps with
  `tx.sleep(period)`, which keeps to its step as a timer does, and
  `latest_every(period, || value)` is a thread that makes a value every
  `period`. Both keep to the app's clock in a test (see
  [Testing](testing.md#testing)). The thread starts when the app starts running.
  Each value wakes the loop's wait on the terminal at once, on every
  backend, so a feed faster than the screen's frames is seen at its own
  rate (a `rate` of a 10 ms `latest_every` reads 100).
- **A feed with a state:** `let (value, state) =
  latest_with_state(stall_after, |tx| …)` is `latest` with a second
  signal, a `FeedState`: `Running`; `Stalled(since)` once no value came
  for `stall_after` (`None`: never), until the next comes; `Ended` when
  the closure returns `()` or `Ok(())`; `Failed(why)` when it returns an
  `Err` or panics. Any thread: a sensor, a log's tail, a socket.
- **An iterator at a pace:** `latest_iter(frames, Pace::Every(period))`
  plays an iterator onto a signal on the app's clock (`Pace::Free`: as
  fast as it comes), then `.then(fallback)` plays another at the same
  beat (`std::iter::repeat(silence)`), `.then_with(|tx| …)` runs a
  closure with the thread's `Latest`, and `.signal()` plays nothing after.
  `.then_at(pace, tail)` adds a tail at a pace of its own (a pipe read as
  fast as it comes, then silence a buffer's time apart) and starts
  nothing; `.with_state()` starts it with a `FeedState` too: `Ended` once
  the values run out, while a tail plays on (the end of a file, padded
  with silence), `Stalled` as `.stall_after(d)` says.
- **Waiting the test clock sees:** a feed thread that waits on something
  else (a device's buffer, a channel) waits with `tx.wait_for(|| ready,
  timeout)`, woken to look again by a `Notifier` (`tx.notifier()`,
  `Clone + Send`, for the device's callback or the channel's sender):
  under `wait_for_tasks` it counts as asleep, so the driver's turns do
  not wait for it, and the timeout is on the app's clock. `tx.blocking(||
  rx.recv())` counts any blocking call as asleep, without the exactness.

## Signals over time

- **Signals over time:** `history(signal)` goes `back()` and `forward()`
  through the values a signal has had (`can_back()`, `can_forward()`,
  `.limit(n)`), not recording its own moves: a browser's, a pager's, a
  file manager's tabs. `recent(signal, n)` is a `Memo<VecDeque<T>>` of its
  last `n` writes (`n` may be a signal): an average over the last frames,
  a peak, persistence; it clones each value once, so hold a big one as an
  `Arc` to share it, or keep a part with `recent_of(signal, n, |v| part)`.
  `recent_timed(signal, n)` has each value's time on the app's clock
  (`cx.now()` is the clock), for fading by age. `falling(signal,
  gravity)` follows a level up at once and falls back with gravity (in
  units a second, a second), as cava's bars and a meter's peak.
  `smoothed(signal, attack, release)` is a number
  that eases after another on the app's clock, rising with the `attack`
  time constant and falling with `release`: a meter's needle, a progress
  bar that glides, bars falling back slowly.
- **Each write, borrowed:** `on_write(signal, |value, cx| …)` gets a clone
  of each write; `on_write_ref(signal, |&value, cx| …)` a borrow of the
  value after a turn that wrote it, so a big frame is never copied.

## Another program

- **Another program:** `cx.handoff(command)` gives the terminal to a
  program once the handler returns (an editor, a pager, a shell): the app
  leaves the alternate screen and raw mode, runs it and waits, then takes
  the terminal back and draws everything again. `cx.handoff_then(command,
  |result, cx| …)` hears how it ended. Under the headless driver the
  program is recorded, not run; a loop of your own takes it with
  `Driver::take_handoff` and gives it back with `Driver::handed_back`.

## A play head, and a feed as one value

```rust
use intuituive::transport;
use intuituive::widgets::{seg_gauge, status_bar};

// Three minutes long, ending in the next track.
let head = transport(Duration::from_secs(180)).on_end(move |_| next_track());
row([
    text(move || clock(head.position())),
    status_bar([seg_gauge(move || head.progress(), String::new).width(40)], []),
])
.on_key("space", move |_| head.toggle())
.on_key("right", move |_| head.seek_by(5.0))
.on_key("plus", move |_| head.set_rate(head.rate() * 1.25))
```

- **`transport(length)`** is a play head on the app's clock: `play`,
  `pause`, `toggle`, `seek(to)`, `seek_by(seconds)`, `set_rate(r)`, and
  `position()`, `progress()` (0 to 1), `length()`, `is_playing()` to read,
  subscribing. Readers are drawn again every `tick` (a tenth of a second
  by default) while it plays, and the end is met on the app's clock
  exactly, so a test stepping the clock sees it at the same time on
  every run; `on_end(|cx| ..)` runs there (load the next track, loop,
  close). `None` for a length is a stream with no end. Any timeline: a
  track, a slideshow, a replay, an animation.
- **`Feed<T>`:** a feed's newest value and its `FeedState` as one `Copy`
  value (`feed.value()`, `feed.state()`, `feed.is_live()`, `parts()`),
  so an app holds one type whatever its source is.
  `latest_feed(stall_after, |tx, state| ..)` is `latest_with_state` with a
  `FeedTeller` to say how it goes before the thread ends
  (`state.tell(FeedState::Ended)` when a file runs out but silence plays
  on); `latest_iter(..).feed()` is `with_state()` as one;
  `Feed::from((value, state))` wraps a pair.
