# intuiTUIve's flex alignment (0.0.19): rs-rich-intuituive's `align`
# example. A row of cards of different sizes, placed by `align_items` (a),
# `justify` (j), `wrap` (w) and `align_content` (c); the test card keeps
# to the end of the line with `align_self`. Three cards fit, so `justify`
# has room to share; two more (+) wrap onto a second line.
Set Title "intuiTUIve · flex alignment"
Set Size 72x18
Set TypingDelay 40ms
Hide
Exec "mkdir -p bin && ln -sf \"$(dirname \"$(command -v rich)\")/examples/align\" bin/align"
Type "export PATH=\"$PWD/bin:$PATH\"; clear"
Enter
Wait /❯\s*$/
Show
Type "align"
Enter
Wait /items Stretch/
Sleep 800ms
Screenshot stretch
Type "aa"
Wait /items Center/
Sleep 800ms
Screenshot center
Type "jjj"
Wait /justify SpaceBetween/
Sleep 800ms
Screenshot between
Type "++w"
Wait /wrap true/
Sleep 800ms
Screenshot wrap
Type "cc"
Wait /lines Center/
Sleep 800ms
Screenshot lines
Type "q"
Wait /❯\s*$/
