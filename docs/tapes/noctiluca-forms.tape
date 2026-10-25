# Noctiluca's page controls as intuiTUIve widgets (0.0.22): a form page
# in reader mode, its controls labelled by link hints, a field typed into
# (a draft until Enter), a select opened with its disabled option shown
# as one, and the same form in the projection, where a hint opens a
# control's widget in a box. It replays Chrome's saved snapshots of the
# crate's fixture pages (docs/tapes/fixtures/noctiluca/make.py writes
# them with https://fixture.example/ addresses), which needs the binary
# built with the test-only `fake-engine` feature. The replayed page never
# answers, so nothing is committed here.
Set Title "Noctiluca · page controls as widgets"
Set Size 100x30
Set TypingDelay 40ms
Hide
Exec "python3 $REPO/docs/tapes/fixtures/noctiluca/make.py $REPO ."
Type "export NOCTILUCA_FAKE_ENGINE=$PWD/pages XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "noctiluca --reader https://fixture.example/form.html"
Enter
Wait /Schedule remote update/
Sleep 1s
Screenshot form
Type "f"
Wait /type a label/
Sleep 600ms
Screenshot form-hints
Type "a"
Sleep 600ms
Type " Lovelace"
Wait /edited/
Sleep 800ms
Screenshot draft
Tab 4
Sleep 400ms
Enter
Wait /Unavailable mirror/
Sleep 800ms
Screenshot select
Escape
Sleep 300ms
Escape
Sleep 300ms
Type "r"
Wait /projection/
Sleep 800ms
Screenshot form-projection
Type "f"
Wait /type a label/
Sleep 400ms
Type "h"
Wait /The "Mirror" select/
Sleep 800ms
Screenshot control-box
Escape
Sleep 300ms
Type "q"
Wait /❯\s*$/
