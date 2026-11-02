# Cornacai (0.0.24), a terminal client for coding agents, against a fake
# agent: rs-rich-acp's replay of docs/tapes/fixtures/cornacai/demo.jsonl,
# which needs the binary built with the test-only `fake-agent` feature. A
# prompt with an `@` file mention completed from the workspace, a plan and
# an answer streaming in as Markdown, the agent's edit reviewed as a diff
# and accepted, a permission prompt for a command, and the finished turn.
# The same transcript is driven by the crate's test `the_docs_tape_replays`.
# A running tool call's spinner turns on the real clock, so it is masked.
Set Title "Cornacai · a session with a coding agent"
Set Size 110x34
Set TypingDelay 40ms
Mask /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/ "⠋"
Hide
Exec "sh $REPO/docs/tapes/fixtures/cornacai/setup.sh $REPO"
Type "export CORNACAI_FAKE_AGENT=$PWD/demo.jsonl XDG_CONFIG_HOME=$PWD/config; clear"
Enter
Wait /❯\s*$/
Show
Type "cornacai"
Enter
Wait /demo-agent 1.0/
Wait /mode ask/
Sleep 500ms
Type "Greet by name in @src/ma"
Wait /src\/main.rs/
Sleep 800ms
Screenshot mention
Enter
Type "please"
Sleep 300ms
Enter
Wait /Review the edit/
Sleep 800ms
Screenshot review
Type "A"
Wait /The agent asks to run/
Sleep 800ms
Screenshot permission
Type "1"
Wait /and the tests pass/
Sleep 1s
Screenshot done
Ctrl+Q
Wait /❯\s*$/
