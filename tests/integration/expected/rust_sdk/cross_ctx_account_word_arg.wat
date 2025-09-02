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
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;2;) (func (param f32 f32) (result f32)))
    (type (;3;) (func (param i32) (result f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account-word-arg/foo@1.0.0#process-word" (func $miden:cross-ctx-account-word-arg/foo@1.0.0#process-word))
    (elem (;0;) (i32.const 1) func $cross_ctx_account_word_arg::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $cross_ctx_account_word_arg::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:cross-ctx-account-word-arg/foo@1.0.0#process-word (;2;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      local.get 1
      i32.const 2
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 2
      i32.const 4
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 3
      i32.const 8
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 3
      local.get 4
      i32.const 16
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      local.get 5
      i32.const 32
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 6
      i32.const 64
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 7
      i32.const 128
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 7
      local.get 8
      i32.const 256
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      local.get 9
      i32.const 512
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 10
      i32.const 1024
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 11
      i32.const 2048
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 11
      local.get 12
      i32.const 4096
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      local.get 13
      i32.const 8192
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 14
      i32.const 16384
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 14
      local.get 3
      local.get 7
      call $intrinsics::felt::add
      local.get 11
      call $intrinsics::felt::add
      local.get 14
      call $intrinsics::felt::add
      local.get 15
      call $intrinsics::felt::add
    )
    (func $wit_bindgen_rt::run_ctors_once (;3;) (type 0)
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
    (func $intrinsics::felt::add (;4;) (type 2) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;5;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::mul (;6;) (type 2) (param f32 f32) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "word" (type (;1;)))
  (alias export 0 "felt" (type (;2;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;3;) (func (param "input1" 1) (param "input2" 1) (param "input3" 1) (param "felt1" 2) (param "felt2" 2) (param "felt3" 2) (param "felt4" 2) (result 2)))
  (alias core export 0 "miden:cross-ctx-account-word-arg/foo@1.0.0#process-word" (core func (;0;)))
  (func (;0;) (type 3) (canon lift (core func 0)))
  (alias export 0 "felt" (type (;4;)))
  (alias export 0 "word" (type (;5;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (import "import-type-felt0" (type (;6;) (eq 1)))
    (type (;7;) (func (param "input1" 5) (param "input2" 5) (param "input3" 5) (param "felt1" 6) (param "felt2" 6) (param "felt3" 6) (param "felt4" 6) (result 6)))
    (import "import-func-process-word" (func (;0;) (type 7)))
    (export (;8;) "word" (type 4))
    (export (;9;) "felt" (type 1))
    (type (;10;) (func (param "input1" 8) (param "input2" 8) (param "input3" 8) (param "felt1" 9) (param "felt2" 9) (param "felt3" 9) (param "felt4" 9) (result 9)))
    (export (;1;) "process-word" (func 0) (func (type 10)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-process-word" (func 0))
      (with "import-type-felt" (type 4))
      (with "import-type-word" (type 5))
      (with "import-type-word0" (type 1))
      (with "import-type-felt0" (type 2))
    )
  )
  (export (;2;) "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance 1))
)
