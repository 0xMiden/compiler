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
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;2;) (func (param f32 f32) (result f32)))
    (type (;3;) (func (param i64) (result f32)))
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
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 1
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      local.get 1
      i64.const 2
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 2
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 3
      i64.const 8
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 3
      local.get 4
      i64.const 16
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      local.get 5
      i64.const 32
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 6
      i64.const 64
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 7
      i64.const 128
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 7
      local.get 8
      i64.const 256
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      local.get 9
      i64.const 512
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 10
      i64.const 1024
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 11
      i64.const 2048
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.set 11
      local.get 12
      i64.const 4096
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      local.get 13
      i64.const 8192
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::mul
      call $intrinsics::felt::add
      local.get 14
      i64.const 16384
      call $intrinsics::felt::from_u64_unchecked
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
    (func $wit_bindgen::rt::run_ctors_once (;3;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048584
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048584
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::add (;4;) (type 2) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;5;) (type 3) (param i64) (result f32)
      unreachable
    )
    (func $intrinsics::felt::mul (;6;) (type 2) (param f32 f32) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;2;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;3;) (func (param "input1" $word) (param "input2" $word) (param "input3" $word) (param "felt1" $felt) (param "felt2" $felt) (param "felt3" $felt) (param "felt4" $felt) (result $felt)))
  (alias core export $main "miden:cross-ctx-account-word-arg/foo@1.0.0#process-word" (core func $miden:cross-ctx-account-word-arg/foo@1.0.0#process-word (;0;)))
  (func $process-word (;0;) (type 3) (canon lift (core func $miden:cross-ctx-account-word-arg/foo@1.0.0#process-word)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type4 felt" (@name "felt") (;4;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type5 word" (@name "word") (;5;)))
  (component $miden:cross-ctx-account-word-arg/foo@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (import "import-type-felt0" (type (;5;) (eq 1)))
    (type (;6;) (func (param "input1" 4) (param "input2" 4) (param "input3" 4) (param "felt1" 5) (param "felt2" 5) (param "felt3" 5) (param "felt4" 5) (result 5)))
    (import "import-func-process-word" (func (;0;) (type 6)))
    (export (;7;) "word" (type 3))
    (export (;8;) "felt" (type 1))
    (type (;9;) (func (param "input1" 7) (param "input2" 7) (param "input3" 7) (param "felt1" 8) (param "felt2" 8) (param "felt3" 8) (param "felt4" 8) (result 8)))
    (export (;1;) "process-word" (func 0) (func (type 9)))
  )
  (instance $miden:cross-ctx-account-word-arg/foo@1.0.0-shim-instance (;1;) (instantiate $miden:cross-ctx-account-word-arg/foo@1.0.0-shim-component
      (with "import-func-process-word" (func $process-word))
      (with "import-type-felt" (type $"#type4 felt"))
      (with "import-type-word" (type $"#type5 word"))
      (with "import-type-word0" (type $word))
      (with "import-type-felt0" (type $felt))
    )
  )
  (export $miden:cross-ctx-account-word-arg/foo@1.0.0 (;2;) "miden:cross-ctx-account-word-arg/foo@1.0.0" (instance $miden:cross-ctx-account-word-arg/foo@1.0.0-shim-instance))
)
