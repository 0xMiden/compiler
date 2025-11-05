(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32) (result f32)))
    (type (;2;) (func (param f32 f32) (result f32)))
    (type (;3;) (func (param i32) (result f32)))
    (type (;4;) (func (param f32) (result i64)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account/foo@1.0.0#process-felt" (func $miden:cross-ctx-account/foo@1.0.0#process-felt))
    (elem (;0;) (i32.const 1) func $cross_ctx_account::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $cross_ctx_account::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:cross-ctx-account/foo@1.0.0#process-felt (;2;) (type 1) (param f32) (result f32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048584
      i32.add
      local.set 1
      call $wit_bindgen::rt::run_ctors_once
      local.get 1
      local.get 0
      local.get 1
      i32.load
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::add
      local.tee 0
      call $intrinsics::felt::as_u64
      i64.store32
      local.get 0
    )
    (func $wit_bindgen::rt::run_ctors_once (;3;) (type 0)
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
    (func $intrinsics::felt::add (;4;) (type 2) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;5;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::as_u64 (;6;) (type 4) (param f32) (result i64)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00*\00\00\00")
  )
  (alias export 0 "felt" (type (;1;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;2;) (func (param "input" 1) (result 1)))
  (alias core export 0 "miden:cross-ctx-account/foo@1.0.0#process-felt" (core func (;0;)))
  (func (;0;) (type 2) (canon lift (core func 0)))
  (alias export 0 "felt" (type (;3;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (import "import-type-felt0" (type (;2;) (eq 1)))
    (type (;3;) (func (param "input" 2) (result 2)))
    (import "import-func-process-felt" (func (;0;) (type 3)))
    (export (;4;) "felt" (type 1))
    (type (;5;) (func (param "input" 4) (result 4)))
    (export (;1;) "process-felt" (func 0) (func (type 5)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-process-felt" (func 0))
      (with "import-type-felt" (type 3))
      (with "import-type-felt0" (type 1))
    )
  )
  (export (;2;) "miden:cross-ctx-account/foo@1.0.0" (instance 1))
)
