# ratatui's showcase rebuilt on intuiTUIve (examples/demo2.rs), at the
# 81x18 of ratatui's own recording: each tab and a move inside it, then
# destroy. The calendar's day is set, so the screenshots are the same on
# every run; destroy runs on the clock, so it is in the recording only.
Set Title "demo2 · ratatui's showcase on intuiTUIve"
Set Size 81x18
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/demo2\" bin/demo2"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "demo2 --today 2026-10-18"
Enter
Wait /reactive terminal interfaces/
Sleep 1s
Type "j"
Sleep 300ms
Screenshot about
Type "l"
Wait /Tomato and lentil soup/
Sleep 1s
Type "j"
Type "j"
Sleep 300ms
Screenshot recipe
Type "l"
Wait /Inbox/
Sleep 1s
Type "j"
Wait /From: Sam/
Sleep 300ms
Screenshot email
Type "l"
Wait /Traceroute cdn/
Sleep 1s
Type "j"
Type "j"
Type "j"
Sleep 300ms
Screenshot traceroute
Type "l"
Wait /October 2026/
Sleep 500ms
Type "jjjjjjjjjj"
Wait /Downloading: 30%/
Sleep 300ms
Screenshot weather
Type "d"
Sleep 6s
Type "d"
Sleep 1s
Type "q"
Wait /❯\s*$/
