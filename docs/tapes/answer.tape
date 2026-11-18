# A Markdown answer streaming in (examples/answer.rs, #797): each Space
# appends a fragment, as an agent's reply arrives. A loose list draws as
# one list, fenced code draws as code while it is still open, and a
# reference link resolves when its definition arrives last. The fragments
# come from keys, not a clock, so the screenshots are the same every run.
Set Title "answer · streaming Markdown on intuiTUIve"
Set Size 72x24
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/answer\" bin/answer"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "answer"
Enter
Wait /Space for the next/
Space
Space
Space
Wait /Keep its comments/
Sleep 300ms
Screenshot list
Space
Space
Wait /3 of|5 of/
Sleep 300ms
Screenshot code
Space
Wait /Space for the next/
Space
Wait /done/
Sleep 300ms
Screenshot done
Tab
Enter
Wait /link: /
Sleep 300ms
Screenshot link
Ctrl+Q
