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
  (core module (;0;)
    (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;1;) (func))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param i32) (result f32)))
    (type (;4;) (func (param f32 f32)))
    (import "miden:cross-ctx-account-word-arg/foo@1.0.0" "process-word" (func $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import22 (;0;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $cross_ctx_note_word_arg::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;1;) (type 1))
    (func $cross_ctx_note_word_arg::bindings::__link_custom_section_describing_imports (;2;) (type 1))
    (func $miden:base/note-script@1.0.0#run (;3;) (type 2) (param f32 f32 f32 f32)
      call $wit_bindgen_rt::run_ctors_once
      i32.const 1
      call $intrinsics::felt::from_u32
      i32.const 2
      call $intrinsics::felt::from_u32
      i32.const 3
      call $intrinsics::felt::from_u32
      i32.const 4
      call $intrinsics::felt::from_u32
      i32.const 5
      call $intrinsics::felt::from_u32
      i32.const 6
      call $intrinsics::felt::from_u32
      i32.const 7
      call $intrinsics::felt::from_u32
      i32.const 8
      call $intrinsics::felt::from_u32
      i32.const 9
      call $intrinsics::felt::from_u32
      i32.const 10
      call $intrinsics::felt::from_u32
      i32.const 11
      call $intrinsics::felt::from_u32
      i32.const 12
      call $intrinsics::felt::from_u32
      i32.const 13
      call $intrinsics::felt::from_u32
      i32.const 14
      call $intrinsics::felt::from_u32
      i32.const 15
      call $intrinsics::felt::from_u32
      i32.const 7
      call $intrinsics::felt::from_u32
      call $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import22
      i32.const 458760
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
    )
    (func $wit_bindgen_rt::run_ctors_once (;4;) (type 1)
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
    (func $intrinsics::felt::from_u32 (;5;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;6;) (type 4) (param f32 f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "word" (type (;4;)))
  (alias export 1 "process-word" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "process-word" (func 0))
  )
  (core instance (;1;) (instantiate 0
      (with "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance 0))
    )
  )
  (alias core export 1 "memory" (core memory (;0;)))
  (type (;5;) (func (param "arg" 4)))
  (alias core export 1 "miden:base/note-script@1.0.0#run" (core func (;1;)))
  (func (;1;) (type 5) (canon lift (core func 1)))
  (alias export 0 "felt" (type (;6;)))
  (alias export 0 "word" (type (;7;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-run" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "run" (func 0) (func (type 8)))
  )
  (instance (;2;) (instantiate 0
      (with "import-func-run" (func 1))
      (with "import-type-felt" (type 6))
      (with "import-type-word" (type 7))
      (with "import-type-word0" (type 4))
    )
  )
  (export (;3;) "miden:base/note-script@1.0.0" (instance 2))
)
