(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "felt" (type (;1;)))
  (type (;2;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "f" u64) (field "a" 1) (field "b" u32) (field "c" 1) (field "d" u16) (field "e" u16) (field "g" u16)))
      (export (;3;) "mixed-struct" (type (eq 2)))
      (type (;4;) (func (param "input" 3) (result 3)))
      (export (;0;) "process-mixed" (func (type 4)))
    )
  )
  (import "miden:cross-ctx-account-word/foo@1.0.0" (instance (;1;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (func (param "a" u64) (result f32)))
      (export (;0;) "from-u64-unchecked" (func (type 0)))
      (type (;1;) (func (param "a" u32) (result f32)))
      (export (;1;) "from-u32" (func (type 1)))
      (type (;2;) (func (param "a" f32) (param "b" f32)))
      (export (;2;) "assert-eq" (func (type 2)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;2;) (type 3)))
  (core module (;0;)
    (type (;0;) (func (param i64) (result f32)))
    (type (;1;) (func (param i32) (result f32)))
    (type (;2;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (type (;3;) (func (param f32 f32)))
    (type (;4;) (func))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u64-unchecked" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;1;) (type 1)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-mixed" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_mixed::wit_import4 (;2;) (type 2)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;3;) (type 3)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#note-script" (func $miden:base/note-script@1.0.0#note-script))
    (elem (;0;) (i32.const 1) func $cross_ctx_note_word::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;4;) (type 4))
    (func $cross_ctx_note_word::bindings::__link_custom_section_describing_imports (;5;) (type 4))
    (func $miden:base/note-script@1.0.0#note-script (;6;) (type 4)
      (local i32 i32 i32 i32 f32 f32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      i64.const -1001
      i64.const -4294967302
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      i32.const -11
      i32.const 50
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 111
      i32.const 2
      i32.const 3
      local.get 0
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_mixed::wit_import4
      block ;; label = @1
        local.get 0
        i64.load
        i64.const -1
        i64.eq
        br_if 0 (;@1;)
        unreachable
      end
      local.get 0
      i32.load16_u offset=24
      local.set 1
      local.get 0
      i32.load16_u offset=22
      local.set 2
      local.get 0
      i32.load16_u offset=20
      local.set 3
      local.get 0
      f32.load offset=16
      local.set 4
      local.get 0
      f32.load offset=12
      local.set 5
      local.get 0
      f32.load offset=8
      i64.const -4294967296
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const -1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 4
      i32.const 57
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 3
      f32.reinterpret_i32
      i32.const 122
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 2
      f32.reinterpret_i32
      i32.const 9
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 1
      f32.reinterpret_i32
      i32.const 12
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 0
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen_rt::run_ctors_once (;7;) (type 4)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048600
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (core module (;1;)
    (type (;0;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (table (;0;) 1 1 funcref)
    (export "0" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-mixed))
    (export "$imports" (table 0))
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-mixed (;0;) (type 0) (param i64 f32 i32 f32 i32 i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      local.get 5
      local.get 6
      local.get 7
      i32.const 0
      call_indirect (type 0)
    )
  )
  (core module (;2;)
    (type (;0;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "$imports" (table (;0;) 1 1 funcref))
    (elem (;0;) (i32.const 0) func 0)
  )
  (core instance (;0;) (instantiate 1))
  (alias export 2 "from-u64-unchecked" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 2 "from-u32" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (alias export 2 "assert-eq" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;1;)
    (export "from-u64-unchecked" (func 0))
    (export "from-u32" (func 1))
    (export "assert-eq" (func 2))
  )
  (alias core export 0 "0" (core func (;3;)))
  (core instance (;2;)
    (export "process-mixed" (func 3))
  )
  (core instance (;3;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 1))
      (with "miden:cross-ctx-account-word/foo@1.0.0" (instance 2))
    )
  )
  (alias core export 3 "memory" (core memory (;0;)))
  (alias core export 0 "$imports" (core table (;0;)))
  (alias export 1 "process-mixed" (func (;3;)))
  (core func (;4;) (canon lower (func 3) (memory 0)))
  (core instance (;4;)
    (export "$imports" (table 0))
    (export "0" (func 4))
  )
  (core instance (;5;) (instantiate 2
      (with "" (instance 4))
    )
  )
  (type (;4;) (func))
  (alias core export 3 "miden:base/note-script@1.0.0#note-script" (core func (;5;)))
  (func (;4;) (type 4) (canon lift (core func 5)))
  (component (;0;)
    (type (;0;) (func))
    (import "import-func-note-script" (func (;0;) (type 0)))
    (type (;1;) (func))
    (export (;1;) "note-script" (func 0) (func (type 1)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-note-script" (func 4))
    )
  )
  (export (;4;) "miden:base/note-script@1.0.0" (instance 3))
)
