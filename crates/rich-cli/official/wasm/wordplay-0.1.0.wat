;; wordplay 0.1.0: an official WASM plugin, listed in the plugin marketplace
;; (docs/design/marketplace.md) as `wordplay`. Three transforms:
;;
;;   0 `rot13`    rotates each ASCII letter 13 places (rot13 twice is the
;;                text again).
;;   1 `reverse`  reverses each line by character cluster, keeping the lines
;;                in order (and a line's "\r\n" at its end).
;;   2 `slug`     each line as a slug: ASCII letters lower-cased, and every
;;                run of other ASCII bytes but the newline one `-`, none at
;;                either end.
;;
;; `rot13` and `slug` are ASCII-only: every byte of a non-ASCII character
;; passes through unchanged (in `slug`, as part of a word: "Crème brûlée!"
;; is "crème-brûlée").
;;
;; `reverse` decodes UTF-8 and moves whole clusters, so its output is UTF-8.
;; A cluster is a character and what follows it of: combining marks
;; (U+0300-036F, U+1AB0-1AFF, U+1DC0-1DFF, U+20D0-20FF, U+FE20-FE2F),
;; variation selectors (U+FE00-FE0F, U+E0100-E01EF), emoji skin tones
;; (U+1F3FB-1F3FF), tags (U+E0020-E007F), U+200C, and U+200D with the
;; character after it; and two regional indicators (one flag) stay together.
;; This is an approximation of Unicode's extended grapheme clusters (UAX #29)
;; that needs no tables: Hangul syllables built from conjoining jamo and the
;; spacing marks of Indic scripts are split.
;;
;; Written by hand in the text format; it imports nothing. It exports the
;; four items the ABI asks for (rich_plugin_api::abi::wasm). The released
;; wordplay-0.1.0.wasm is this file assembled by the `wat` crate at the
;; version in Cargo.lock; its SHA-256 is pinned in
;; crates/rich-cli/tests/fixtures/official-artifacts.txt and checked by
;; tests/official_artifacts.rs. A listed version never changes: a change here
;; is a new file with a new version.
(module
  (memory (export "memory") 1)
  ;; A bump allocator: every call runs in a fresh instance, so nothing is freed.
  (global $heap (mut i32) (i32.const 4096))
  ;; The length in bytes of the character $decode read last.
  (global $width (mut i32) (i32.const 0))

  (data (i32.const 0) "rich-plugin-abi 1.0\nname wordplay\nversion 0.1.0\ndescription Word games: rot13, reversal by character cluster, and slugs\ncapability transform rot13\ncapability transform reverse\ncapability transform slug\n")
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

  ;; Whether lo <= c <= hi, as 0 or 1.
  (func $within (param $c i32) (param $lo i32) (param $hi i32) (result i32)
    (i32.le_u (i32.sub (local.get $c) (local.get $lo)) (i32.sub (local.get $hi) (local.get $lo))))
  (func $is_upper (param $c i32) (result i32)
    (call $within (local.get $c) (i32.const 65) (i32.const 90)))
  (func $is_lower (param $c i32) (result i32)
    (call $within (local.get $c) (i32.const 97) (i32.const 122)))
  ;; A byte of a word, for `slug`: an ASCII letter or digit, or a byte of a
  ;; non-ASCII character.
  (func $is_word (param $c i32) (result i32)
    (i32.or
      (i32.or (call $is_upper (local.get $c)) (call $is_lower (local.get $c)))
      (i32.or
        (call $within (local.get $c) (i32.const 48) (i32.const 57))
        (i32.ge_u (local.get $c) (i32.const 128)))))

  ;; rot13, in place.
  (func $rot13 (param $ptr i32) (param $len i32)
    (local $i i32)
    (local $c i32)
    (local $base i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (local.set $base (i32.const 0))
        (if (call $is_upper (local.get $c)) (then (local.set $base (i32.const 65))))
        (if (call $is_lower (local.get $c)) (then (local.set $base (i32.const 97))))
        (if (local.get $base)
          (then
            (i32.store8
              (i32.add (local.get $ptr) (local.get $i))
              (i32.add
                (local.get $base)
                (i32.rem_u
                  (i32.add (i32.sub (local.get $c) (local.get $base)) (i32.const 13))
                  (i32.const 26))))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next))))

  ;; The character at `p` (the input is UTF-8: the host passes a string),
  ;; setting $width to its length in bytes.
  (func $decode (param $p i32) (result i32)
    (local $b i32)
    (local.set $b (i32.load8_u (local.get $p)))
    (if (i32.lt_u (local.get $b) (i32.const 0x80))
      (then
        (global.set $width (i32.const 1))
        (return (local.get $b))))
    (if (i32.lt_u (local.get $b) (i32.const 0xE0))
      (then
        (global.set $width (i32.const 2))
        (return
          (i32.or
            (i32.shl (i32.and (local.get $b) (i32.const 0x1F)) (i32.const 6))
            (i32.and (i32.load8_u offset=1 (local.get $p)) (i32.const 0x3F))))))
    (if (i32.lt_u (local.get $b) (i32.const 0xF0))
      (then
        (global.set $width (i32.const 3))
        (return
          (i32.or
            (i32.or
              (i32.shl (i32.and (local.get $b) (i32.const 0x0F)) (i32.const 12))
              (i32.shl (i32.and (i32.load8_u offset=1 (local.get $p)) (i32.const 0x3F)) (i32.const 6)))
            (i32.and (i32.load8_u offset=2 (local.get $p)) (i32.const 0x3F))))))
    (global.set $width (i32.const 4))
    (i32.or
      (i32.or
        (i32.shl (i32.and (local.get $b) (i32.const 0x07)) (i32.const 18))
        (i32.shl (i32.and (i32.load8_u offset=1 (local.get $p)) (i32.const 0x3F)) (i32.const 12)))
      (i32.or
        (i32.shl (i32.and (i32.load8_u offset=2 (local.get $p)) (i32.const 0x3F)) (i32.const 6))
        (i32.and (i32.load8_u offset=3 (local.get $p)) (i32.const 0x3F)))))

  ;; Whether a character stays with the one before it (see the top).
  (func $extends (param $c i32) (result i32)
    (i32.or
      (i32.or
        (i32.or
          (call $within (local.get $c) (i32.const 0x0300) (i32.const 0x036F))
          (call $within (local.get $c) (i32.const 0x1AB0) (i32.const 0x1AFF)))
        (i32.or
          (call $within (local.get $c) (i32.const 0x1DC0) (i32.const 0x1DFF))
          (call $within (local.get $c) (i32.const 0x20D0) (i32.const 0x20FF))))
      (i32.or
        (i32.or
          (call $within (local.get $c) (i32.const 0xFE00) (i32.const 0xFE2F))
          (call $within (local.get $c) (i32.const 0x200C) (i32.const 0x200D)))
        (i32.or
          (call $within (local.get $c) (i32.const 0x1F3FB) (i32.const 0x1F3FF))
          (i32.or
            (call $within (local.get $c) (i32.const 0xE0020) (i32.const 0xE007F))
            (call $within (local.get $c) (i32.const 0xE0100) (i32.const 0xE01EF)))))))

  (func $is_regional (param $c i32) (result i32)
    (call $within (local.get $c) (i32.const 0x1F1E6) (i32.const 0x1F1FF)))

  ;; Reverse the clusters of input bytes [s, e) into the same span of `out`.
  (func $reverse_span (param $ptr i32) (param $out i32) (param $s i32) (param $e i32)
    (local $i i32)
    (local $start i32)
    (local $c i32)
    (local $regional i32)
    (local.set $i (local.get $s))
    (block $done
      (loop $cluster
        (br_if $done (i32.ge_u (local.get $i) (local.get $e)))
        (local.set $start (local.get $i))
        (local.set $regional
          (call $is_regional (call $decode (i32.add (local.get $ptr) (local.get $i)))))
        (local.set $i (i32.add (local.get $i) (global.get $width)))
        (block $end
          (loop $more
            (br_if $end (i32.ge_u (local.get $i) (local.get $e)))
            (local.set $c (call $decode (i32.add (local.get $ptr) (local.get $i))))
            (if (call $extends (local.get $c))
              (then
                (local.set $i (i32.add (local.get $i) (global.get $width)))
                ;; A zero-width joiner takes the character after it too.
                (if (i32.and
                      (i32.eq (local.get $c) (i32.const 0x200D))
                      (i32.lt_u (local.get $i) (local.get $e)))
                  (then
                    (drop (call $decode (i32.add (local.get $ptr) (local.get $i))))
                    (local.set $i (i32.add (local.get $i) (global.get $width)))))
                (br $more)))
            ;; The second regional indicator of a flag.
            (if (i32.and (local.get $regional) (call $is_regional (local.get $c)))
              (then
                (local.set $i (i32.add (local.get $i) (global.get $width)))
                (local.set $regional (i32.const 0))
                (br $more)))))
        ;; [start, i) goes to [s + e - i, s + e - start).
        (memory.copy
          (i32.add (local.get $out) (i32.sub (i32.add (local.get $s) (local.get $e)) (local.get $i)))
          (i32.add (local.get $ptr) (local.get $start))
          (i32.sub (local.get $i) (local.get $start)))
        (br $cluster))))

  ;; Each line reversed, into a new buffer of the same length.
  (func $reverse (param $ptr i32) (param $len i32) (result i64)
    (local $out i32)
    (local $i i32)
    (local $s i32)
    (local $e i32)
    (local.set $out (call $alloc (local.get $len)))
    ;; Newlines (and a "\r" before one) are copied where they are.
    (memory.copy (local.get $out) (local.get $ptr) (local.get $len))
    (block $done
      (loop $line
        (br_if $done (i32.gt_u (local.get $s) (local.get $len)))
        (local.set $i (local.get $s))
        (block $found
          (loop $scan
            (br_if $found (i32.ge_u (local.get $i) (local.get $len)))
            (br_if $found
              (i32.eq (i32.load8_u (i32.add (local.get $ptr) (local.get $i))) (i32.const 10)))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br $scan)))
        (local.set $e (local.get $i))
        (if (i32.and
              (i32.gt_u (local.get $e) (local.get $s))
              (i32.lt_u (local.get $i) (local.get $len)))
          (then
            (if (i32.eq (i32.load8_u (i32.add (local.get $ptr) (i32.sub (local.get $e) (i32.const 1)))) (i32.const 13))
              (then (local.set $e (i32.sub (local.get $e) (i32.const 1)))))))
        (call $reverse_span (local.get $ptr) (local.get $out) (local.get $s) (local.get $e))
        (local.set $s (i32.add (local.get $i) (i32.const 1)))
        (br $line)))
    (call $pack (local.get $out) (local.get $len)))

  ;; Each line as a slug, into a new buffer (never longer than the input).
  (func $slug (param $ptr i32) (param $len i32) (result i64)
    (local $out i32)
    (local $o i32)
    (local $i i32)
    (local $c i32)
    (local $gap i32)
    (local $any i32)
    (local.set $out (call $alloc (local.get $len)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (if (i32.eq (local.get $c) (i32.const 10))
          (then
            (i32.store8 (i32.add (local.get $out) (local.get $o)) (i32.const 10))
            (local.set $o (i32.add (local.get $o) (i32.const 1)))
            (local.set $gap (i32.const 0))
            (local.set $any (i32.const 0)))
          (else
            (if (call $is_word (local.get $c))
              (then
                (if (i32.and (local.get $gap) (local.get $any))
                  (then
                    (i32.store8 (i32.add (local.get $out) (local.get $o)) (i32.const 45))
                    (local.set $o (i32.add (local.get $o) (i32.const 1)))))
                (i32.store8
                  (i32.add (local.get $out) (local.get $o))
                  (if (result i32) (call $is_upper (local.get $c))
                    (then (i32.add (local.get $c) (i32.const 32)))
                    (else (local.get $c))))
                (local.set $o (i32.add (local.get $o) (i32.const 1)))
                (local.set $gap (i32.const 0))
                (local.set $any (i32.const 1)))
              (else (local.set $gap (i32.const 1))))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $pack (local.get $out) (local.get $o)))

  (func (export "rich_plugin_manifest") (result i64)
    (call $pack (i32.const 0) (i32.const 202)))

  (func (export "rich_plugin_call")
    (param $cap i32) (param $ptr i32) (param $len i32) (param $cols i32) (result i64)
    ;; 0: rot13, in place.
    (if (i32.eqz (local.get $cap))
      (then
        (call $rot13 (local.get $ptr) (local.get $len))
        (return (call $pack (local.get $ptr) (local.get $len)))))
    ;; 1: reverse.
    (if (i32.eq (local.get $cap) (i32.const 1))
      (then (return (call $reverse (local.get $ptr) (local.get $len)))))
    ;; 2: slug.
    (if (i32.eq (local.get $cap) (i32.const 2))
      (then (return (call $slug (local.get $ptr) (local.get $len)))))
    ;; Anything else is an error: bit 63 set, the message at 1056.
    (i64.or
      (call $pack (i32.const 1056) (i32.const 18))
      (i64.const 0x8000000000000000)))
)
