# Documents beside Markdown: an Org file, an AsciiDoc file and a man page,
# each found by its extension (the man page by its section number and the
# roff inside it), then the man page's source with --syntax. The files are
# the renderer's own samples, crates/rich-ext/tests/fixtures/documents/.
Set Title "rich · Org, AsciiDoc and man pages"
Set Size 100x40
Set TypingDelay 40ms
Hide
Exec "cp $REPO/crates/rich-ext/tests/fixtures/documents/sample.org notes.org"
Exec "cp $REPO/crates/rich-ext/tests/fixtures/documents/sample.adoc guide.adoc"
Exec "cp $REPO/crates/rich-ext/tests/fixtures/documents/sample.1 greet.1"
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich --force-terminal notes.org | head -36"
Enter
Wait "Unterminated blocks"
Sleep 1500ms
Screenshot org
Hide
Type "clear"
Enter
Show
Type "rich --force-terminal guide.adoc | head -36"
Enter
Wait "Getting started"
Sleep 1500ms
Screenshot asciidoc
Hide
Type "clear"
Enter
Show
Type "rich --force-terminal greet.1 | head -36"
Enter
Wait "Repeat each greeting"
Sleep 1500ms
Screenshot man
Hide
Type "clear"
Enter
Show
Type "rich --force-terminal --syntax greet.1 | head -14"
Enter
Wait ".SH NAME"
Sleep 1500ms
Screenshot source
