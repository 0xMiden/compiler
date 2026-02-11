(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (result f32)))
    (type (;2;) (func (param f32 f32) (result i32)))
    (type (;3;) (func (param i32) (result f32)))
    (type (;4;) (func (param f32 f32 f32 f32) (result f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding" (func $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding))
    (elem (;0;) (i32.const 1) func $rust_sdk_account_has_procedure_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_account_has_procedure_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding (;2;) (type 1) (result f32)
      (local i32 i32 f32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      i32.const 0
      local.set 1
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 2
      block ;; label = @1
        loop ;; label = @2
          local.get 1
          i32.const 16
          i32.eq
          br_if 1 (;@1;)
          local.get 0
          local.get 1
          i32.add
          local.get 2
          f32.store
          local.get 1
          i32.const 4
          i32.add
          local.set 1
          br 0 (;@2;)
        end
      end
      local.get 0
      f32.load offset=12
      local.get 0
      f32.load offset=8
      local.get 0
      f32.load offset=4
      local.get 0
      f32.load
      call $miden::protocol::active_account::has_procedure
      i32.const 0
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::eq
      i32.const 1
      i32.ne
      call $intrinsics::felt::from_u32
      local.set 2
      local.get 0
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 2
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
    (func $intrinsics::felt::eq (;4;) (type 2) (param f32 f32) (result i32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;5;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $miden::protocol::active_account::has_procedure (;6;) (type 4) (param f32 f32 f32 f32) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "Mrust_sdk_account_has_procedure_binding\01\0b0.0.1\03\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (result $felt)))
  (alias core export $main "miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding" (core func $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding (;0;)))
  (func $binding (;0;) (type 2) (canon lift (core func $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1#binding)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type3 felt" (@name "felt") (;3;)))
  (component $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (import "import-type-felt0" (type (;2;) (eq 1)))
    (type (;3;) (func (result 2)))
    (import "import-func-binding" (func (;0;) (type 3)))
    (export (;4;) "felt" (type 1))
    (type (;5;) (func (result 4)))
    (export (;1;) "binding" (func 0) (func (type 5)))
  )
  (instance $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1-shim-instance (;1;) (instantiate $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1-shim-component
      (with "import-func-binding" (func $binding))
      (with "import-type-felt" (type $"#type3 felt"))
      (with "import-type-felt0" (type $felt))
    )
  )
  (export $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1 (;2;) "miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1" (instance $miden:rust-sdk-account-has-procedure-binding/rust-sdk-account-has-procedure-binding@0.0.1-shim-instance))
)
