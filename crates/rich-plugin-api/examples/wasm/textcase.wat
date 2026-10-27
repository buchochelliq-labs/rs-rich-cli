;; textcase: the official example WASM plugin, listed in the plugin
;; marketplace (docs/design/marketplace.md) as `textcase`.
;;
;; Written by hand in the text format, like shout.wat, so it builds with
;; nothing but a WAT assembler and imports nothing. It exports the four items
;; the ABI asks for (rich_plugin_api::abi::wasm):
;;
;;   memory                                        its linear memory
;;   rich_plugin_alloc(len) -> ptr                 room for the input
;;   rich_plugin_manifest() -> ptr << 32 | len     its self-description
;;   rich_plugin_call(cap, ptr, len, width) -> ptr << 32 | len (bit 63: error)
;;
;; Capability 0, the `upcase` transform, upper-cases ASCII letters.
;; Capability 1, the `downcase` transform, lower-cases ASCII letters.
;; Capability 2, the `loud` fence (fence-ansi), draws the fence's code in
;; bold capitals with SGR codes, so its listing declares `ansi-output`.
;; Every other byte, UTF-8 included, passes through unchanged.
;;
;; The released textcase.wasm is this file assembled by the `wat` crate at
;; the version in Cargo.lock. Its SHA-256 is pinned in rs-rich-cli's
;; crates/rich-cli/tests/fixtures/official-artifacts.txt and checked by
;; tests/official_artifacts.rs, which also writes the module out:
;;
;;   RICH_OFFICIAL_ARTIFACTS=dir cargo test -p rs-rich-cli --locked \
;;       --test official_artifacts -- --ignored write_official_artifacts
;;
;; This file is textcase 0.1.0 and stays so: a listed version never changes.
;; Later versions (0.2.0 adds titlecase, snakecase, kebabcase and camelcase)
;; are built from crates/rich-cli/official/wasm/textcase-VERSION.wat, beside
;; the second official plugin, wordplay.
(module
  (memory (export "memory") 1)
  ;; A bump allocator: every call runs in a fresh instance, so nothing is freed.
  (global $heap (mut i32) (i32.const 4096))

  (data (i32.const 0) "rich-plugin-abi 1.0\nname textcase\nversion 0.1.0\ndescription Upper- and lower-cases ASCII text, and draws loud fences in bold capitals\ncapability transform upcase\ncapability transform downcase\ncapability fence-ansi loud\n")
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

  (func (export "rich_plugin_manifest") (result i64)
    (call $pack (i32.const 0) (i32.const 219)))

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
    ;; Anything else is an error: bit 63 set, the message at 1056.
    (i64.or
      (call $pack (i32.const 1056) (i32.const 18))
      (i64.const 0x8000000000000000)))
)
