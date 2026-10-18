# Sharing output (#722, #723; docs/cli.md#preview-a-redaction-before-you-share-0019
# and #a-png-of-the-output-0019): a deploy script that prints a token and a
# password is previewed with --redact-preview (each match highlighted with
# its rule and replacement, a dry-run line naming the files it did not
# write), then captured redacted with --export-png, and the PNG's size is
# shown with `file`.
Set Title "rich capture · preview, then share"
Set Size 96x30
Set TypingDelay 40ms
Mask /· \d+ms/ "· Nms"
Write deploy.sh "echo 'deploying build 42'\necho 'GITHUB_TOKEN=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB'\necho 'db: postgres://app:s3cret@db/app'\necho 'done'\n"
Type "rich capture --redact --redact-preview --export-png deploy.png -- sh deploy.sh"
Enter
Wait /dry run, nothing was written/
Wait /❯\s*$/
Sleep 800ms
Screenshot preview
Type "clear; rich capture --redact --export-png deploy.png -- sh deploy.sh && file deploy.png"
Enter
Wait /PNG image data/
Wait /❯\s*$/
Sleep 800ms
Screenshot shared
