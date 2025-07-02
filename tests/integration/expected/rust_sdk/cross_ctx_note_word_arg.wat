(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "word" (type (;1;)))
  (alias export 0 "felt" (type (;2;)))
  (type (;3;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "word" (type (eq 0)))
      (alias outer 1 2 (type (;2;)))
      (export (;3;) "felt" (type (eq 2)))
      (type (;4;) (func (param "input1" 1) (param "input2" 1) (param "input3" 1) (param "felt1" 3) (param "felt2" 3) (param "felt3" 3) (param "felt4" 3) (result 3)))
      (export (;0;) "process-word" (func (type 4)))
    )
  )
  (import "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance (;1;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32)))
      (export (;1;) "assert-eq" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;2;) (type 4)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;2;) (func (param f32 f32)))
    (type (;3;) (func))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:cross-ctx-account-word-arg/foo@1.0.0" "process-word" (func $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import22 (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;2;) (type 2)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#note-script" (func $miden:base/note-script@1.0.0#note-script))
    (elem (;0;) (i32.const 1) func $cross_ctx_note_word_arg::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;3;) (type 3))
    (func $cross_ctx_note_word_arg::bindings::__link_custom_section_describing_imports (;4;) (type 3))
    (func $miden:base/note-script@1.0.0#note-script (;5;) (type 3)
      call $wit_bindgen_rt::run_ctors_once
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 6
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 7
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 8
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 9
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 10
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 11
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 12
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 13
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 14
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 15
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 7
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import22
      i32.const 458760
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    )
    (func $wit_bindgen_rt::run_ctors_once (;6;) (type 3)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048604
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048604
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 2 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 2 "assert-eq" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "assert-eq" (func 1))
  )
  (alias export 1 "process-word" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;1;)
    (export "process-word" (func 2))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (type (;5;) (func))
  (alias core export 2 "miden:base/note-script@1.0.0#note-script" (core func (;3;)))
  (func (;3;) (type 5) (canon lift (core func 3)))
  (component (;0;)
    (type (;0;) (func))
    (import "import-func-note-script" (func (;0;) (type 0)))
    (type (;1;) (func))
    (export (;1;) "note-script" (func 0) (func (type 1)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-note-script" (func 3))
    )
  )
  (export (;4;) "miden:base/note-script@1.0.0" (instance 3))
)
