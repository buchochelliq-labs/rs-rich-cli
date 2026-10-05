# A ```dot fence in Markdown: rich draws the graph in place of the code
# block, and --dot-backend off leaves it as code.
Set Title "rich · DOT in Markdown"
Set Size 100x28
Set TypingDelay 40ms
Write release.md "# Release pipeline\n\nEvery change goes through the same stages:\n\n```dot\ndigraph release {\n  rankdir=LR;\n  node [shape=box];\n  commit -> build -> test -> ship;\n  test -> fix [style=dashed, label=\"fails\"];\n  fix -> build;\n}\n```\n\nA failed test sends the change back to *build*.\n"
Type "rich release.md"
Enter
Wait "sends the change back"
Sleep 1500ms
Screenshot drawn
Hide
Type "clear"
Enter
Show
Type "rich release.md --dot-backend off"
Enter
Wait "sends the change back"
Sleep 1500ms
Screenshot code
