(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
      (export (;3;) "word" (type (eq 2)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;2;)))
  (type $ty-miden:cross-ctx-account-word-arg/foo@1.0.0 (;3;)
    (instance
      (alias outer 1 $word (type (;0;)))
      (export (;1;) "word" (type (eq 0)))
      (alias outer 1 $felt (type (;2;)))
      (export (;3;) "felt" (type (eq 2)))
      (type (;4;) (func (param "input1" 1) (param "input2" 1) (param "input3" 1) (param "felt1" 3) (param "felt2" 3) (param "felt3" 3) (param "felt4" 3) (result 3)))
      (export (;0;) "process-word" (func (type 4)))
    )
  )
  (import "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance $miden:cross-ctx-account-word-arg/foo@1.0.0 (;1;) (type $ty-miden:cross-ctx-account-word-arg/foo@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;1;) (func))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param f32 f32)))
    (type (;4;) (func (param i64) (result f32)))
    (import "miden:cross-ctx-account-word-arg/foo@1.0.0" "process-word" (func $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import19 (;0;) (type 0)))
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
      call $wit_bindgen::rt::run_ctors_once
      i64.const 1
      call $intrinsics::felt::from_u64_unchecked
      i64.const 2
      call $intrinsics::felt::from_u64_unchecked
      i64.const 3
      call $intrinsics::felt::from_u64_unchecked
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      i64.const 5
      call $intrinsics::felt::from_u64_unchecked
      i64.const 6
      call $intrinsics::felt::from_u64_unchecked
      i64.const 7
      call $intrinsics::felt::from_u64_unchecked
      i64.const 8
      call $intrinsics::felt::from_u64_unchecked
      i64.const 9
      call $intrinsics::felt::from_u64_unchecked
      i64.const 10
      call $intrinsics::felt::from_u64_unchecked
      i64.const 11
      call $intrinsics::felt::from_u64_unchecked
      i64.const 12
      call $intrinsics::felt::from_u64_unchecked
      i64.const 13
      call $intrinsics::felt::from_u64_unchecked
      i64.const 14
      call $intrinsics::felt::from_u64_unchecked
      i64.const 15
      call $intrinsics::felt::from_u64_unchecked
      i64.const 7
      call $intrinsics::felt::from_u64_unchecked
      call $cross_ctx_note_word_arg::bindings::miden::cross_ctx_account_word_arg::foo::process_word::wit_import19
      i64.const 458760
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::assert_eq
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048588
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048588
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::assert_eq (;5;) (type 3) (param f32 f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;6;) (type 4) (param i64) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (alias export $miden:cross-ctx-account-word-arg/foo@1.0.0 "process-word" (func $process-word (;0;)))
  (core func $process-word (;0;) (canon lower (func $process-word)))
  (core instance $miden:cross-ctx-account-word-arg/foo@1.0.0 (;0;)
    (export "process-word" (func $process-word))
  )
  (core instance $main (;1;) (instantiate $main
      (with "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance $miden:cross-ctx-account-word-arg/foo@1.0.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;5;) (func (param "arg" $"#type4 word")))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;1;)))
  (func $run (;1;) (type 5) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type6 felt" (@name "felt") (;6;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type7 word" (@name "word") (;7;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (type (;5;) (func (param "arg" 4)))
    (import "import-func-run" (func (;0;) (type 5)))
    (export (;6;) "word" (type 3))
    (type (;7;) (func (param "arg" 6)))
    (export (;1;) "run" (func 0) (func (type 7)))
  )
  (instance $miden:base/note-script@1.0.0-shim-instance (;2;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $"#type6 felt"))
      (with "import-type-word" (type $"#type7 word"))
      (with "import-type-word0" (type $"#type4 word"))
    )
  )
  (export $miden:base/note-script@1.0.0 (;3;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
