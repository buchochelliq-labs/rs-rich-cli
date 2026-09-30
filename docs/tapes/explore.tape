# rich explore: fold open a YAML document, search it, copy a path and pick
# a node; the JSONPath goes to stdout for the script.
Set Title "rich explore · browse a document"
Set Size 100x22
Set TypingDelay 45ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/explore/config.yaml ."
Type "export RICH_CLIPBOARD=1; clear"
Enter
Wait /❯\s*$/
Show
Type "path=$(rich explore config.yaml)"
Enter
Wait /6 keys/
Sleep 500ms
Screenshot tree
Down 3
Right
Wait /port: 8080/
Down 2
Wait /server › port/
Sleep 500ms
Screenshot expanded
Ctrl+Y
Wait /copied path/
Sleep 500ms
Type "grace"
Wait /1\/\d+ ·|name: "grace"/
Sleep 500ms
Screenshot search
Enter
Wait /❯\s*$/
Type "echo \"$path\""
Enter
Wait /\$\.users\[1\]\.name\s*\n[^\n]*❯/
Sleep 500ms
Screenshot answer
