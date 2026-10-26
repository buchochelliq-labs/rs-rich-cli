;; textcase 0.2.0: the official WASM plugin `textcase`, listed in the plugin
;; marketplace (docs/design/marketplace.md). Version 0.1.0 is
;; crates/rich-plugin-api/examples/wasm/textcase.wat; this file keeps its
;; three capabilities, at the same numbers, and adds four transforms.
;;
;; Written by hand in the text format, so it builds with nothing but a WAT
;; assembler and imports nothing. It exports the four items the ABI asks for
;; (rich_plugin_api::abi::wasm):
;;
;;   memory                                        its linear memory
;;   rich_plugin_alloc(len) -> ptr                 room for the input
;;   rich_plugin_manifest() -> ptr << 32 | len     its self-description
;;   rich_plugin_call(cap, ptr, len, width) -> ptr << 32 | len (bit 63: error)
;;
;; Capabilities, by number:
;;
;;   0 `upcase` (transform)      upper-cases ASCII letters.
;;   1 `downcase` (transform)    lower-cases ASCII letters.
;;   2 `loud` (fence-ansi)       the fence's code upper-cased, in bold (SGR),
;;                               so the listing declares `ansi-output`.
;;   3 `titlecase` (transform)   the first letter of each word upper-cased,
;;                               the rest lower-cased.
;;   4 `snakecase` (transform)   each line as an identifier: words lower-cased
;;                               and joined by `_`.
;;   5 `kebabcase` (transform)   the same, joined by `-`.
;;   6 `camelcase` (transform)   the same, unjoined, every word after the
;;                               first starting with a capital.
;;
;; Only ASCII letters change case. Every byte of a non-ASCII character passes
;; through unchanged, and counts as part of a word, so a word is never split
;; inside a character and the output is always UTF-8.
;;
;; A word, for `titlecase`: a run of ASCII letters and digits, non-ASCII
;; characters, and apostrophes after the first of them ("don't" is one word,
;; "'quoted'" starts at the q).
;;
;; A word, for the identifier cases: a run of ASCII letters and digits and
;; non-ASCII characters, split where a lower-case letter or a digit meets a
;; capital ("parseHTTP2Header": parse, HTTP2, Header) and before the last
;; capital of a run that a lower-case letter follows ("HTTPServer": HTTP,
;; Server). Every other ASCII byte separates words and is dropped, except a
;; newline: each line is converted on its own, and its newline kept.
;;
;; The released textcase-0.2.0.wasm is this file assembled by the `wat` crate
;; at the version in Cargo.lock. Its SHA-256 is pinned in
;; crates/rich-cli/tests/fixtures/official-artifacts.txt and checked by
;; tests/official_artifacts.rs. A listed version never changes: a change
;; here is a new file, textcase-0.3.0.wat, with a new version below.
(module
  (memory (export "memory") 1)
  ;; A bump allocator: every call runs in a fresh instance, so nothing is freed.
  (global $heap (mut i32) (i32.const 4096))

  (data (i32.const 0) "rich-plugin-abi 1.0\nname textcase\nversion 0.2.0\ndescription Changes the case of ASCII text (upper, lower, title, snake, kebab, camel), and draws loud fences in bold capitals\ncapability transform upcase\ncapability transform downcase\ncapability fence-ansi loud\ncapability transform titlecase\ncapability transform snakecase\ncapability transform kebabcase\ncapability transform camelcase\n")
  (data (i32.const 1024) "\1b[1m")
  (data (i32.const 1040) "\1b[0m")
  (data (i32.const 1056) "unknown capability")

  (func $pack (param $ptr i32) (param $len i32) (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (local.get $ptr)) (i64.const 32))
      (i64.extend_i32_u (local.get $len))))

  (func $alloc (export "rich_plugin_alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local $end i32)
    (local.set $ptr (global.get $heap))
    (local.set $end (i32.add (local.get $ptr) (local.get $len)))
    (block $done
      (loop $grow
        (br_if $done
          (i32.le_u (local.get $end) (i32.shl (memory.size) (i32.const 16))))
        (drop (memory.grow (i32.const 1)))
        (br $grow)))
    (global.set $heap (local.get $end))
    (local.get $ptr))

  ;; Byte classes. Each returns 0 or 1.
  (func $is_upper (param $c i32) (result i32)
    (i32.lt_u (i32.sub (local.get $c) (i32.const 65)) (i32.const 26)))
  (func $is_lower (param $c i32) (result i32)
    (i32.lt_u (i32.sub (local.get $c) (i32.const 97)) (i32.const 26)))
  (func $is_digit (param $c i32) (result i32)
    (i32.lt_u (i32.sub (local.get $c) (i32.const 48)) (i32.const 10)))
  ;; A byte of a word: an ASCII letter or digit, or a byte of a non-ASCII
  ;; character.
  (func $is_word (param $c i32) (result i32)
    (i32.or
      (i32.or (call $is_upper (local.get $c)) (call $is_lower (local.get $c)))
      (i32.or (call $is_digit (local.get $c)) (i32.ge_u (local.get $c) (i32.const 128)))))
  (func $to_upper (param $c i32) (result i32)
    (if (result i32) (call $is_lower (local.get $c))
      (then (i32.sub (local.get $c) (i32.const 32)))
      (else (local.get $c))))
  (func $to_lower (param $c i32) (result i32)
    (if (result i32) (call $is_upper (local.get $c))
      (then (i32.add (local.get $c) (i32.const 32)))
      (else (local.get $c))))

  ;; Add `delta` to every byte of [ptr, ptr + len) that is in [lo, hi].
  (func $shift (param $ptr i32) (param $len i32) (param $lo i32) (param $hi i32) (param $delta i32)
    (local $i i32)
    (local $c i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (if (i32.and
              (i32.ge_u (local.get $c) (local.get $lo))
              (i32.le_u (local.get $c) (local.get $hi)))
          (then
            (i32.store8
              (i32.add (local.get $ptr) (local.get $i))
              (i32.add (local.get $c) (local.get $delta)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next))))

  ;; Title case, in place: an ASCII letter is upper-cased at the start of a
  ;; word and lower-cased inside one.
  (func $title (param $ptr i32) (param $len i32)
    (local $i i32)
    (local $c i32)
    (local $in i32)
    (local $at i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $at (i32.add (local.get $ptr) (local.get $i)))
        (local.set $c (i32.load8_u (local.get $at)))
        (if (local.get $in)
          (then (i32.store8 (local.get $at) (call $to_lower (local.get $c))))
          (else (i32.store8 (local.get $at) (call $to_upper (local.get $c)))))
        ;; An apostrophe continues a word but never starts one.
        (local.set $in
          (i32.or
            (call $is_word (local.get $c))
            (i32.and (local.get $in) (i32.eq (local.get $c) (i32.const 39)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next))))

  ;; Each line as an identifier, into a new buffer (a joiner can be added
  ;; between two input bytes, so the output is at most twice the input).
  ;; `joiner` is the byte between words, or 0 for none; with `camel` set,
  ;; every word after a line's first starts with a capital.
  (func $ident (param $ptr i32) (param $len i32) (param $joiner i32) (param $camel i32) (result i64)
    (local $out i32)
    (local $o i32)
    (local $i i32)
    (local $c i32)
    (local $prev i32)
    (local $in i32)
    (local $words i32)
    (local $start i32)
    (local.set $out (call $alloc (i32.shl (local.get $len) (i32.const 1))))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (if (i32.eq (local.get $c) (i32.const 10))
          (then
            ;; A newline is kept, and the next line starts afresh.
            (i32.store8 (i32.add (local.get $out) (local.get $o)) (i32.const 10))
            (local.set $o (i32.add (local.get $o) (i32.const 1)))
            (local.set $words (i32.const 0))
            (local.set $in (i32.const 0)))
          (else
            (if (call $is_word (local.get $c))
              (then
                (local.set $start (i32.eqz (local.get $in)))
                (if (i32.and (local.get $in) (call $is_upper (local.get $c)))
                  (then
                    ;; aB and 2B: a new word at the capital.
                    (if (i32.or (call $is_lower (local.get $prev)) (call $is_digit (local.get $prev)))
                      (then (local.set $start (i32.const 1))))
                    ;; ABc: a new word at the B.
                    (if (i32.and
                          (call $is_upper (local.get $prev))
                          (i32.lt_u (i32.add (local.get $i) (i32.const 1)) (local.get $len)))
                      (then
                        (if (call $is_lower
                              (i32.load8_u (i32.add (i32.add (local.get $ptr) (local.get $i)) (i32.const 1))))
                          (then (local.set $start (i32.const 1))))))))
                (if (local.get $start)
                  (then
                    (if (i32.and (i32.ne (local.get $words) (i32.const 0)) (i32.ne (local.get $joiner) (i32.const 0)))
                      (then
                        (i32.store8 (i32.add (local.get $out) (local.get $o)) (local.get $joiner))
                        (local.set $o (i32.add (local.get $o) (i32.const 1)))))
                    (local.set $words (i32.add (local.get $words) (i32.const 1)))))
                (i32.store8
                  (i32.add (local.get $out) (local.get $o))
                  (if (result i32)
                    (i32.and
                      (i32.and (local.get $camel) (local.get $start))
                      (i32.gt_u (local.get $words) (i32.const 1)))
                    (then (call $to_upper (local.get $c)))
                    (else (call $to_lower (local.get $c)))))
                (local.set $o (i32.add (local.get $o) (i32.const 1)))
                (local.set $in (i32.const 1)))
              (else (local.set $in (i32.const 0))))))
        (local.set $prev (local.get $c))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $pack (local.get $out) (local.get $o)))

  (func (export "rich_plugin_manifest") (result i64)
    (call $pack (i32.const 0) (i32.const 383)))

  (func (export "rich_plugin_call")
    (param $cap i32) (param $ptr i32) (param $len i32) (param $width i32) (result i64)
    (local $out i32)
    ;; 0: upcase, in place.
    (if (i32.eqz (local.get $cap))
      (then
        (call $shift (local.get $ptr) (local.get $len) (i32.const 97) (i32.const 122) (i32.const -32))
        (return (call $pack (local.get $ptr) (local.get $len)))))
    ;; 1: downcase, in place.
    (if (i32.eq (local.get $cap) (i32.const 1))
      (then
        (call $shift (local.get $ptr) (local.get $len) (i32.const 65) (i32.const 90) (i32.const 32))
        (return (call $pack (local.get $ptr) (local.get $len)))))
    ;; 2: ESC[1m, the input upper-cased, ESC[0m.
    (if (i32.eq (local.get $cap) (i32.const 2))
      (then
        (call $shift (local.get $ptr) (local.get $len) (i32.const 97) (i32.const 122) (i32.const -32))
        (local.set $out (call $alloc (i32.add (local.get $len) (i32.const 8))))
        (memory.copy (local.get $out) (i32.const 1024) (i32.const 4))
        (memory.copy
          (i32.add (local.get $out) (i32.const 4)) (local.get $ptr) (local.get $len))
        (memory.copy
          (i32.add (i32.add (local.get $out) (i32.const 4)) (local.get $len))
          (i32.const 1040) (i32.const 4))
        (return (call $pack (local.get $out) (i32.add (local.get $len) (i32.const 8))))))
    ;; 3: titlecase, in place.
    (if (i32.eq (local.get $cap) (i32.const 3))
      (then
        (call $title (local.get $ptr) (local.get $len))
        (return (call $pack (local.get $ptr) (local.get $len)))))
    ;; 4: snakecase, 5: kebabcase, 6: camelcase.
    (if (i32.eq (local.get $cap) (i32.const 4))
      (then (return (call $ident (local.get $ptr) (local.get $len) (i32.const 95) (i32.const 0)))))
    (if (i32.eq (local.get $cap) (i32.const 5))
      (then (return (call $ident (local.get $ptr) (local.get $len) (i32.const 45) (i32.const 0)))))
    (if (i32.eq (local.get $cap) (i32.const 6))
      (then (return (call $ident (local.get $ptr) (local.get $len) (i32.const 0) (i32.const 1)))))
    ;; Anything else is an error: bit 63 set, the message at 1056.
    (i64.or
      (call $pack (i32.const 1056) (i32.const 18))
      (i64.const 0x8000000000000000)))
)
