(component
  (type (;0;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32) (result f32)))
      (export (;1;) "add" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;1;) (type 1)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32) (result f32)))
    (type (;2;) (func))
    (type (;3;) (func (param f32 f32 f32 f32) (result i32)))
    (type (;4;) (func (param f32) (result f32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-word))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt))
    (elem (;0;) (i32.const 1) func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;2;) (type 2))
    (func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports (;3;) (type 2))
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-word (;4;) (type 3) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 2
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 2
      local.get 4
      i32.const 1048596
      i32.add
      local.tee 4
      local.get 3
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      f32.store offset=12
      local.get 4
      local.get 2
      f32.store offset=8
      local.get 4
      local.get 1
      f32.store offset=4
      local.get 4
      local.get 0
      f32.store
      local.get 4
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word (;5;) (type 3) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 2
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 2
      local.get 4
      i32.const 1048596
      i32.add
      local.tee 4
      local.get 3
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      f32.store offset=12
      local.get 4
      local.get 2
      f32.store offset=8
      local.get 4
      local.get 1
      f32.store offset=4
      local.get 4
      local.get 0
      f32.store
      local.get 4
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt (;6;) (type 4) (param f32) (result f32)
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
    )
    (func $wit_bindgen_rt::run_ctors_once (;7;) (type 2)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048612
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048612
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 0 "add" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "add" (func 1))
  )
  (core instance (;1;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
    )
  )
  (alias core export 1 "memory" (core memory (;0;)))
  (alias export 1 "word" (type (;2;)))
  (type (;3;) (func (param "input" 2) (result 2)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-word" (core func (;2;)))
  (func (;2;) (type 3) (canon lift (core func 2) (memory 0)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (core func (;3;)))
  (func (;3;) (type 3) (canon lift (core func 3) (memory 0)))
  (alias export 1 "felt" (type (;4;)))
  (type (;5;) (func (param "input" 4) (result 4)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (core func (;4;)))
  (func (;4;) (type 5) (canon lift (core func 4)))
  (alias export 1 "felt" (type (;6;)))
  (alias export 1 "word" (type (;7;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "input" 5) (result 5)))
    (import "import-func-process-word" (func (;0;) (type 6)))
    (import "import-func-process-another-word" (func (;1;) (type 6)))
    (import "import-type-felt0" (type (;7;) (eq 1)))
    (type (;8;) (func (param "input" 7) (result 7)))
    (import "import-func-process-felt" (func (;2;) (type 8)))
    (export (;9;) "word" (type 4))
    (export (;10;) "felt" (type 1))
    (type (;11;) (func (param "input" 9) (result 9)))
    (export (;3;) "process-word" (func 0) (func (type 11)))
    (export (;4;) "process-another-word" (func 1) (func (type 11)))
    (type (;12;) (func (param "input" 10) (result 10)))
    (export (;5;) "process-felt" (func 2) (func (type 12)))
  )
  (instance (;2;) (instantiate 0
      (with "import-func-process-word" (func 2))
      (with "import-func-process-another-word" (func 3))
      (with "import-func-process-felt" (func 4))
      (with "import-type-felt" (type 6))
      (with "import-type-word" (type 7))
      (with "import-type-word0" (type 2))
      (with "import-type-felt0" (type 4))
    )
  )
  (export (;3;) "miden:cross-ctx-account-word/foo@1.0.0" (instance 2))
)
