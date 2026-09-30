# The 0.0.13 release, in a real terminal: the interactive commands answering
# a script, a piped list filtered as you type, rich-cli's options and
# reStructuredText. Recorded against the installed binary for the release
# notes (docs/releases/0.0.13.md), and checked in CI like every tape.
Set Title "rich 0.0.13 · ask, choose and browse"
Set Size 96x24
Set TypingDelay 35ms
Hide
Write fruits.txt "apple\nbanana\nblueberry\ncherry\ndragonfruit\nelderberry\nfig\ngrape\nraspberry\nstrawberry\n"
Write service.log "INFO  api listening on :8080\nINFO  worker 1 ready\nERROR upstream timeout after 30s\nINFO  retrying upstream\nWARN  slow query: 1.8s\nERROR connection refused by db:5432\nINFO  recovered\nINFO  worker 2 ready\n"
Write release.rst "Release 0.0.13\n==============\n\nAsk, choose and browse from a script:\n\n* ``rich choose`` picks one item\n* ``rich filter`` narrows a list as you type\n* ``rich confirm`` answers with its exit code\n\n.. note:: Keys come from the terminal even when stdin is the list.\n"
Write retry.py "import time\n\n\ndef retry(job, attempts=3, delay=1.0):\n    for n in range(attempts):\n        try:\n            return job()\n        except ConnectionError:\n            time.sleep(delay * 2**n)\n    raise RuntimeError(f\"gave up after {attempts}\")\n"
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "fruit=$(rich choose apple banana cherry --header 'Pick a fruit')"
Enter
Wait /enter pick/
Down
Wait /❯ banana/
Screenshot choose
Enter
Type "echo \"picked: $fruit (exit $?)\""
Enter
Wait /picked: banana \(exit 0\)/
Screenshot choose-answer
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "berry=$(rich filter < fruits.txt)"
Enter
Wait /enter pick/
Type "rry"
Wait /\d+\/10 ·/
Screenshot filter
Enter
Type "echo \"$berry\""
Enter
Wait /❯\s*$/
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich confirm 'Deploy 0.0.13?' && echo deploying"
Enter
Wait /Deploy 0\.0\.13\?/
Screenshot confirm
Type "y"
Wait /deploying/
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich pager --search ERROR service.log"
Enter
Wait /match 1\//
Screenshot pager
Type "q"
Wait /❯\s*$/
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich --syntax retry.py -n --guides --head 8"
Enter
Wait /raise|except/
Screenshot options
Hide
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich --rst release.rst"
Enter
Wait /Keys come from the terminal/
Screenshot rst
