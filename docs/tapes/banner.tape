# rich banner (0.0.19, #708): a FIGlet banner in a colour gradient,
# centred, then a vertical preset with a bold style, then plain text when
# the banner does not fit; and `rich shell status` (#715), the line the
# shell integration prints after a failed command.
Set Title "rich banner · FIGlet banners"
Set Size 100x30
Set TypingDelay 40ms
Type "rich banner --gradient rainbow --align center 'rs-rich'"
Enter
Wait /\|_\|/
Sleep 500ms
Screenshot gradient
Type "clear"
Enter
Wait /❯\s*$/
Type "rich banner --gradient sunset --direction vertical --style bold 0.0.19"
Enter
Wait /\|_\|/
Sleep 500ms
Screenshot vertical
Type "clear"
Enter
Wait /❯\s*$/
Type "rich banner -w 30 --fit plain 'Too wide to draw'"
Enter
Wait /❯\s*$/
Type "rich shell status --exit 101 --duration 4.2 --command 'cargo test'"
Enter
Wait /error \$ cargo test/
Sleep 500ms
Screenshot fit-and-status
