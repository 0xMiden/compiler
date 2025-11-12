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
    (type (;1;) (func (result f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:rust-sdk-account-faucet-get-total-issuance-binding/rust-sdk-account-faucet-get-total-issuance-binding@0.0.1#binding" (func $miden:rust-sdk-account-faucet-get-total-issuance-binding/rust-sdk-account-faucet-get-total-issuance-binding@0.0.1#binding))
    (elem (;0;) (i32.const 1) func $rust_sdk_account_faucet_get_total_issuance_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_account_faucet_get_total_issuance_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:rust-sdk-account-faucet-get-total-issuance-binding/rust-sdk-account-faucet-get-total-issuance-binding@0.0.1#binding (;2;) (type 1) (result f32)
      call $wit_bindgen::rt::run_ctors_once
      call $miden::faucet::get_total_issuance
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
    (func $miden::faucet::get_total_issuance (;4;) (type 1) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "erust_sdk_account_faucet_get_total_issuance_binding\01\0b0.0.1\05\02\03\01\00\00")
  )
  (alias export 0 "felt" (type (;1;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;2;) (func (result 1)))
  (alias core export 0 "miden:rust-sdk-account-faucet-get-total-issuance-binding/rust-sdk-account-faucet-get-total-issuance-binding@0.0.1#binding" (core func (;0;)))
  (func (;0;) (type 2) (canon lift (core func 0)))
  (alias export 0 "felt" (type (;3;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (import "import-type-felt0" (type (;2;) (eq 1)))
    (type (;3;) (func (result 2)))
    (import "import-func-binding" (func (;0;) (type 3)))
    (export (;4;) "felt" (type 1))
    (type (;5;) (func (result 4)))
    (export (;1;) "binding" (func 0) (func (type 5)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-binding" (func 0))
      (with "import-type-felt" (type 3))
      (with "import-type-felt0" (type 1))
    )
  )
  (export (;2;) "miden:rust-sdk-account-faucet-get-total-issuance-binding/rust-sdk-account-faucet-get-total-issuance-binding@0.0.1" (instance 1))
)
