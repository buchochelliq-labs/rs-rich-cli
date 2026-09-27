# The README hero: one binary renders Markdown, tables and code.
Set Title "rich · the terminal toolbox"
Set Size 100x26
Write notes.md "# Release 0.0.13\n\nInteractive **tapes**, *frames* and a `rich choose` preview.\n\n| Crate | Version |\n|---|---|\n| rs-rich-ext | 0.0.11 |\n| rs-rich-cli | 0.0.13 |\n\n> Every frame in this recording is real output.\n\n- [x] frames with a cell diff\n- [ ] interactive components\n"
Write orders.csv "order,customer,total,status\n1001,Ada Lovelace,129.00,shipped\n1002,Grace Hopper,54.50,pending\n1003,Alan Turing,310.25,shipped\n1004,Edsger Dijkstra,12.99,refunded\n"
Write fib.py "from functools import cache\n\n\n@cache\ndef fib(n: int) -> int:\n    \"\"\"The n-th Fibonacci number.\"\"\"\n    return n if n < 2 else fib(n - 1) + fib(n - 2)\n\n\nprint([fib(n) for n in range(10)])\n"
Type "rich notes.md"
Enter
Wait "Every frame"
Sleep 1500ms
Screenshot markdown
Hide
Type "clear"
Enter
Show
Type "rich orders.csv"
Enter
Wait "Dijkstra"
Sleep 1500ms
Screenshot table
Hide
Type "clear"
Enter
Show
Type "rich fib.py"
Enter
Wait "range(10)"
Sleep 2s
Screenshot code
