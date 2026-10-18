# rich date: a day on a month grid within a range, then a meeting slot whose
# time is refused before the range's first moment; the answers are printed
# in ISO 8601 form.
Set Title "rich date · pick a date and a time"
Set Size 100x26
Set TypingDelay 45ms
Type "due=$(rich date --header Due --value 2026-10-17 --min 2026-10-05 --max 2026-11-20)"
Enter
Wait /enter pick/
Right 3
Down
PageDown
Wait /November 2026/
Sleep 500ms
Screenshot grid
Enter
Wait /❯\s*$/
Type "echo \"due $due\""
Enter
Wait /due 2026-/
Type "rich date --kind datetime --value '2026-10-17 08:00' --min '2026-10-17 09:00'"
Enter
Wait /tab time/
Tab
Enter
Wait /the earliest is/
Sleep 500ms
Screenshot refused
Up
Enter
Wait /2026-10-17 09:00/
Sleep 500ms
Screenshot answer
