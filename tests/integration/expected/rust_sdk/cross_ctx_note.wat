(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;1;)))
  (type $ty-miden:cross-ctx-account/foo@1.0.0 (;2;)
    (instance
      (alias outer 1 $felt (type (;0;)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (func (param "input" 1) (result 1)))
      (export (;0;) "process-felt" (func (type 2)))
    )
  )
  (import "miden:cross-ctx-account/foo@1.0.0" (instance $miden:cross-ctx-account/foo@1.0.0 (;1;) (type $ty-miden:cross-ctx-account/foo@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func (param f32) (result f32)))
    (type (;1;) (func))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param f32) (result i64)))
    (type (;4;) (func (param f32 f32)))
    (type (;5;) (func (param i64) (result f32)))
    (import "miden:cross-ctx-account/foo@1.0.0" "process-felt" (func $cross_ctx_note::bindings::miden::cross_ctx_account::foo::process_felt::wit_import1 (;0;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $cross_ctx_note::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;1;) (type 1))
    (func $cross_ctx_note::bindings::__link_custom_section_describing_imports (;2;) (type 1))
    (func $miden:base/note-script@1.0.0#run (;3;) (type 2) (param f32 f32 f32 f32)
      (local i32 f32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      i32.const 1048588
      i32.add
      local.tee 4
      i64.load32_u
      call $intrinsics::felt::from_u64_unchecked
      local.tee 5
      i64.const 11
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::assert_eq
      local.get 5
      call $cross_ctx_note::bindings::miden::cross_ctx_account::foo::process_felt::wit_import1
      local.tee 5
      i64.const 53
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::assert_eq
      local.get 4
      local.get 5
      call $intrinsics::felt::as_u64
      i64.store32
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048592
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048592
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::as_u64 (;5;) (type 3) (param f32) (result i64)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;6;) (type 4) (param f32 f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;7;) (type 5) (param i64) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\0b\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;3;)))
  (alias export $miden:cross-ctx-account/foo@1.0.0 "process-felt" (func $process-felt (;0;)))
  (core func $process-felt (;0;) (canon lower (func $process-felt)))
  (core instance $miden:cross-ctx-account/foo@1.0.0 (;0;)
    (export "process-felt" (func $process-felt))
  )
  (core instance $main (;1;) (instantiate $main
      (with "miden:cross-ctx-account/foo@1.0.0" (instance $miden:cross-ctx-account/foo@1.0.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;4;) (func (param "arg" $word)))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;1;)))
  (func $run (;1;) (type 4) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type5 felt" (@name "felt") (;5;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type6 word" (@name "word") (;6;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
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
  (instance $miden:base/note-script@1.0.0-shim-instance (;2;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $"#type5 felt"))
      (with "import-type-word" (type $"#type6 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/note-script@1.0.0 (;3;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
