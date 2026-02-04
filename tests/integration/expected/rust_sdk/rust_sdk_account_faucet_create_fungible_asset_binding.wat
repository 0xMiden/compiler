(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
      (type (;5;) (record (field "inner" 4)))
      (export (;6;) "asset" (type (eq 5)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (result i32)))
    (type (;2;) (func (param i32 f32)))
    (type (;3;) (func (param i32) (result f32)))
    (type (;4;) (func (param f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding" (func $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding))
    (elem (;0;) (i32.const 1) func $rust_sdk_account_faucet_create_fungible_asset_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_account_faucet_create_fungible_asset_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding (;2;) (type 1) (result i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 10
      call $intrinsics::felt::from_u32
      call $miden_base_sys::bindings::faucet::create_fungible_asset
      global.get $GOT.data.internal.__memory_base
      i32.const 1048584
      i32.add
      local.tee 1
      local.get 0
      i64.load offset=8
      i64.store offset=8 align=4
      local.get 1
      local.get 0
      i64.load
      i64.store align=4
      local.get 0
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 1
    )
    (func $wit_bindgen::rt::run_ctors_once (;3;) (type 0)
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
    (func $miden_base_sys::bindings::faucet::create_fungible_asset (;4;) (type 2) (param i32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      local.get 2
      call $miden::protocol::faucet::create_fungible_asset
      local.get 0
      local.get 2
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 2
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 2
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $intrinsics::felt::from_u32 (;5;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $miden::protocol::faucet::create_fungible_asset (;6;) (type 4) (param f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "krust_sdk_account_faucet_create_fungible_asset_binding\01\0b0.0.1\05\02\03\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (result $asset)))
  (alias core export $main "miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding" (core func $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding (;0;)))
  (func $binding (;0;) (type 2) (canon lift (core func $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1#binding) (memory $memory)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;4;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $"#type5 asset" (@name "asset") (;5;)))
  (component $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (import "import-type-asset0" (type (;7;) (eq 6)))
    (type (;8;) (func (result 7)))
    (import "import-func-binding" (func (;0;) (type 8)))
    (export (;9;) "asset" (type 6))
    (type (;10;) (func (result 9)))
    (export (;1;) "binding" (func 0) (func (type 10)))
  )
  (instance $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1-shim-instance (;1;) (instantiate $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1-shim-component
      (with "import-func-binding" (func $binding))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $word))
      (with "import-type-asset" (type $"#type5 asset"))
      (with "import-type-asset0" (type $asset))
    )
  )
  (export $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1 (;2;) "miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1" (instance $miden:rust-sdk-account-faucet-create-fungible-asset-binding/rust-sdk-account-faucet-create-fungible-asset-binding@0.0.1-shim-instance))
)
