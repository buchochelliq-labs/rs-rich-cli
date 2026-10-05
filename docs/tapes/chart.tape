# rich chart: a CSV file drawn as bars and as lines, numbers piped in as a
# sparkline, and a missing column refused with the columns there are.
Set Title "rich chart · data to charts"
Set Size 100x28
Set TypingDelay 40ms
Write sales.csv "month,api,web\nJan,30,12\nFeb,42,18\nMar,35,25\nApr,51,22\nMay,48,31\nJun,57,29\n"
Type "rich chart sales.csv --kind bar --y api --width 60"
Enter
Wait "Jun"
Sleep 1s
Screenshot bars
Hide
Type "clear"
Enter
Show
Type "rich chart sales.csv --width 70"
Enter
Wait /api\s+\S web/
Sleep 1s
Screenshot lines
Hide
Type "clear"
Enter
Show
Type "printf '3 1 4 1 5 9 2 6 5 3 5 8 9 7 9' | rich chart --kind spark"
Enter
Wait /█/
Type "rich chart sales.csv --kind heatmap"
Enter
Wait /\] 57/
Type "rich chart sales.csv --y mobile"
Enter
Wait /no column "mobile"/
Sleep 1500ms
Screenshot pipe
