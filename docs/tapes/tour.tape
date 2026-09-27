# The guided tour: `rich --demo`, every section in one pass. Each screenshot
# is taken while its section is paused on screen. Replaces
# scripts/capture_demo_tour.py.
Set Title "rich --demo · the guided tour"
Set Size 88x28
Set TypingDelay 30ms
Set Timeout 60s
# Output that differs on every run, masked in the text grids --check compares
# (the images keep what was shown): temporary paths and command timings.
Mask /\/tmp\/\.tmp\w+/ "<tmp>"
Mask /─+ exit (\d+) · \d+ms ─+/ "── exit · Nms ──"
Type "rich --demo --demo-delay 1.5"
Enter
Wait "Inspect structured data" 120s
Sleep 400ms
Screenshot inspect
Wait "Text diffs" 60s
Sleep 400ms
Screenshot diff
Wait "Mermaid flowcharts" 120s
Sleep 400ms
Screenshot mermaid
Wait "Filter and highlight" 60s
Sleep 400ms
Screenshot filter
Wait "FIGlet banners" 60s
Sleep 400ms
Screenshot banner
Wait "Image diff" 120s
Sleep 400ms
Screenshot image-diff
Wait "Tour complete" 120s
Sleep 400ms
Screenshot complete
Wait /❯\s*$/ 60s
