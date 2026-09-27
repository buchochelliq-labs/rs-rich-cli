# rs-rich-interact's components, one after another, through the example
# program (cargo build -p rs-rich-interact --examples), in the fixture
# project docs/tapes/fixtures/project: a fuzzy file picker
# with a preview, a multi-select, an input with suggestions and validation,
# a confirmation sheet, a form, and a pager with search.
Set Title "rs-rich-interact · components"
Set Size 100x22
Set TypingDelay 45ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/components\" bin/c"
Exec "cp -r $REPO/docs/tapes/fixtures/project/. ."
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "c select"
Enter
Wait /enter pick/
Type "evlp"
Wait /1\/\d+ ·/
Screenshot select
Enter
Wait /"src/event_loop.rs"/
Hide
Type "clear"
Enter
Show
Type "c multi"
Enter
Wait /tab mark/
Tab
Down
Tab
Tab
Wait /3 marked/
Screenshot multi
Enter
Wait /\["rs-rich"/
Hide
Type "clear"
Enter
Show
Type "c input"
Enter
Wait /a crate name/
Type "recrd"
Wait /rs-rich-record/
Screenshot input-suggestions
Enter
Wait /not one of/
Screenshot input-error
Tab
Enter
Wait /"rs-rich-record"/
Hide
Type "clear"
Enter
Show
Type "c confirm"
Enter
Wait /Dry run/
Screenshot confirm
Right
Enter
Wait /"edit"/
Hide
Type "clear"
Enter
Show
Type "c form"
Enter
Wait /enter next/
Type "API"
Enter
Enter
Right
Right
Enter
Enter
Type "hunter2"
Enter
Wait /lowercase letters/
Screenshot form-error
Backspace 3
Type "api"
Enter
Enter
Enter
Enter
Enter
Wait /token: \(set\)/
Screenshot form-done
Hide
Type "clear"
Enter
Show
Type "c pager README.md"
Enter
Wait /search · q quit/
Type "/job"
Enter
Wait /match 1\//
Type "n"
Wait /match 2\//
Screenshot pager
Type "q"
Wait /\(\)/
