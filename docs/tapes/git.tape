# A gitui-style git client rebuilt on intuiTUIve (examples/git.rs), in a
# throwaway repository the tape makes (fixed names and dates, so the hashes
# and the log are the same on every run): the Status tab with the
# unstaged and staged changes beside the diff, a file staged with Enter,
# the Log tab with its branch and tag markers, and a commit's details.
# The work folder's path, in the tab bar, is masked.
Set Title "git · gitui on intuiTUIve"
Set Size 100x26
Set TypingDelay 40ms
Set Env TZ UTC
Set Env GIT_AUTHOR_NAME "Ada Lovelace"
Set Env GIT_AUTHOR_EMAIL ada@example.com
Set Env GIT_COMMITTER_NAME "Ada Lovelace"
Set Env GIT_COMMITTER_EMAIL ada@example.com
Set Env GIT_CONFIG_NOSYSTEM 1
Output gif png svg
Mask /\S*\/orbit/ "<tmp>/orbit"
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/git\" bin/git-ui"
Exec "mkdir orbit && cd orbit && git init -q -b main && git config commit.gpgsign false && cp $REPO/docs/tapes/fixtures/project/README.md README.md && mkdir src && cp $REPO/docs/tapes/fixtures/project/src/*.rs src/ && cp $REPO/docs/tapes/fixtures/project/Cargo.toml Cargo.toml && git add -A && GIT_AUTHOR_DATE='2026-09-01T09:00:00Z' GIT_COMMITTER_DATE='2026-09-01T09:00:00Z' git commit -q -m 'Start orbit' && git tag v0.1.0 && printf '\\n## Usage\\n\\nRun `orbit` in a project.\\n' >> README.md && git add README.md && GIT_AUTHOR_DATE='2026-09-03T14:30:00Z' GIT_COMMITTER_DATE='2026-09-03T14:30:00Z' git commit -q -m 'Document usage' && printf 'pub fn tick() -> u32 {\\n    1\\n}\\n' >> src/lib.rs && printf 'todo: tests\\n' > NOTES.md && sed -i 's/orbit/Orbit/' Cargo.toml"
Type "export PATH=\"$PWD/bin:$PATH\"; cd orbit; clear"
Enter
Wait /❯\s*$/
Show
Type "git-ui"
Enter
Wait /Unstaged Changes/
Sleep 1s
Screenshot status
Down
Sleep 500ms
Enter
Wait /To stage \[w\] Commit/
Type "w"
Wait /Diff: NOTES\.md/
Sleep 800ms
Screenshot staged
Type "2"
Wait /Start orbit/
Sleep 800ms
Screenshot log
Enter
Wait /Ada Lovelace/
Sleep 800ms
Screenshot details
Type "q"
Wait /❯\s*$/
