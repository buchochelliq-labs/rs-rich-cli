;; A complete WASM plugin for rich, written by hand in the text format.
;;
;; It imports nothing (the host offers nothing to import) and exports the four
;; items the ABI asks for (rich_plugin_api::abi::wasm):
;;
;;   memory                                        its linear memory
;;   rich_plugin_alloc(len) -> ptr                 room for the input
;;   rich_plugin_manifest() -> ptr << 32 | len     its self-description
;;   rich_plugin_call(cap, ptr, len, width) -> ptr << 32 | len (bit 63: error)
;;
;; Capability 0, the `upper` transform, upper-cases ASCII letters in place.
;; Capability 1, the `shout` fence renderer, wraps the fence in bold markup.
;;
;; Build it with `wat2wasm shout.wat` (or the `wat` crate) and load it with
;; `rich --plugin shout.wasm` in a build with the wasm-plugins feature.
(module
  (memory (export "memory") 1)
  ;; A bump allocator: every call runs in a fresh instance, so nothing is freed.
  (global $heap (mut i32) (i32.const 4096))

  (data (i32.const 0) "rich-plugin-abi 1.0\nname shout\nversion 0.1.0\ndescription Upper-cases text and shouts fences\ncapability transform upper\ncapability fence-markup shout\n")
  (data (i32.const 1024) "[bold]")
  (data (i32.const 1040) "[/bold]")
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

  (func (export "rich_plugin_manifest") (result i64)
    (call $pack (i32.const 0) (i32.const 149)))

  (func (export "rich_plugin_call")
    (param $cap i32) (param $ptr i32) (param $len i32) (param $width i32) (result i64)
    (local $i i32)
    (local $c i32)
    (local $out i32)
    ;; 0: upper-case in place.
    (if (i32.eqz (local.get $cap))
      (then
        (block $done
          (loop $next
            (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
            (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
            (if (i32.and
                  (i32.ge_u (local.get $c) (i32.const 97))
                  (i32.le_u (local.get $c) (i32.const 122)))
              (then
                (i32.store8
                  (i32.add (local.get $ptr) (local.get $i))
                  (i32.sub (local.get $c) (i32.const 32)))))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br $next)))
        (return (call $pack (local.get $ptr) (local.get $len)))))
    ;; 1: [bold]input[/bold]
    (if (i32.eq (local.get $cap) (i32.const 1))
      (then
        (local.set $out (call $alloc (i32.add (local.get $len) (i32.const 13))))
        (memory.copy (local.get $out) (i32.const 1024) (i32.const 6))
        (memory.copy
          (i32.add (local.get $out) (i32.const 6)) (local.get $ptr) (local.get $len))
        (memory.copy
          (i32.add (i32.add (local.get $out) (i32.const 6)) (local.get $len))
          (i32.const 1040) (i32.const 7))
        (return (call $pack (local.get $out) (i32.add (local.get $len) (i32.const 13))))))
    ;; Anything else is an error: bit 63 set, the message at 1056.
    (i64.or
      (call $pack (i32.const 1056) (i32.const 18))
      (i64.const 0x8000000000000000)))
)
