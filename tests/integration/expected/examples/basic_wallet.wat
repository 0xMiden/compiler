(component
  (type (;0;)
    (instance
      (type (;0;) (func (param "asset0" f32) (param "asset1" f32) (param "asset2" f32) (param "asset3" f32) (param "result-ptr" s32)))
      (export (;0;) "add-asset" (func (type 0)))
      (export (;1;) "remove-asset" (func (type 0)))
      (type (;1;) (func (param "value" u32)))
      (export (;2;) "incr-nonce" (func (type 1)))
    )
  )
  (import "miden:core-base/account@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (param "asset0" f32) (param "asset1" f32) (param "asset2" f32) (param "asset3" f32) (param "tag" f32) (param "note-type" f32) (param "recipient0" f32) (param "recipient1" f32) (param "recipient2" f32) (param "recipient3" f32) (result f32)))
      (export (;0;) "create-note" (func (type 0)))
    )
  )
  (import "miden:core-base/tx@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
      (type (;5;) (record (field "inner" 4)))
      (export (;6;) "asset" (type (eq 5)))
      (type (;7;) (record (field "inner" 1)))
      (export (;8;) "tag" (type (eq 7)))
      (type (;9;) (record (field "inner" 4)))
      (export (;10;) "recipient" (type (eq 9)))
      (type (;11;) (record (field "inner" 1)))
      (export (;12;) "note-type" (type (eq 11)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;2;) (type 2)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32 f32 f32 i32)))
    (type (;1;) (func (param i32)))
    (type (;2;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;3;) (func))
    (type (;4;) (func (param f32 f32 f32 f32)))
    (type (;5;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;6;) (func (param i32 i32)))
    (type (;7;) (func (param i32 f32 f32 i32) (result f32)))
    (import "miden:core-base/account@1.0.0" "add-asset" (func $miden_base_sys::bindings::account::extern_account_add_asset (;0;) (type 0)))
    (import "miden:core-base/account@1.0.0" "remove-asset" (func $miden_base_sys::bindings::account::extern_account_remove_asset (;1;) (type 0)))
    (import "miden:core-base/account@1.0.0" "incr-nonce" (func $miden_base_sys::bindings::account::extern_account_incr_nonce (;2;) (type 1)))
    (import "miden:core-base/tx@1.0.0" "create-note" (func $miden_base_sys::bindings::tx::extern_tx_create_note (;3;) (type 2)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset))
    (export "miden:basic-wallet/basic-wallet@1.0.0#send-asset" (func $miden:basic-wallet/basic-wallet@1.0.0#send-asset))
    (elem (;0;) (i32.const 1) func $basic_wallet::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;4;) (type 3))
    (func $basic_wallet::bindings::__link_custom_section_describing_imports (;5;) (type 3))
    (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset (;6;) (type 4) (param f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 4
      local.get 3
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
      i32.const 16
      i32.add
      local.get 4
      call $miden_base_sys::bindings::account::add_asset
      i32.const 1
      call $miden_base_sys::bindings::account::incr_nonce
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden:basic-wallet/basic-wallet@1.0.0#send-asset (;7;) (type 5) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 10
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 10
      local.get 3
      f32.store offset=12
      local.get 10
      local.get 2
      f32.store offset=8
      local.get 10
      local.get 1
      f32.store offset=4
      local.get 10
      local.get 0
      f32.store
      local.get 10
      local.get 9
      f32.store offset=28
      local.get 10
      local.get 8
      f32.store offset=24
      local.get 10
      local.get 7
      f32.store offset=20
      local.get 10
      local.get 6
      f32.store offset=16
      local.get 10
      i32.const 32
      i32.add
      local.get 10
      call $miden_base_sys::bindings::account::remove_asset
      local.get 10
      i32.const 32
      i32.add
      local.get 4
      local.get 5
      local.get 10
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::tx::create_note
      drop
      i32.const 1
      call $miden_base_sys::bindings::account::incr_nonce
      local.get 10
      i32.const 48
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen_rt::run_ctors_once (;8;) (type 3)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048616
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048616
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $miden_base_sys::bindings::account::add_asset (;9;) (type 6) (param i32 i32)
      local.get 1
      f32.load
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_add_asset
    )
    (func $miden_base_sys::bindings::account::remove_asset (;10;) (type 6) (param i32 i32)
      local.get 1
      f32.load
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_remove_asset
    )
    (func $miden_base_sys::bindings::account::incr_nonce (;11;) (type 1) (param i32)
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_incr_nonce
    )
    (func $miden_base_sys::bindings::tx::create_note (;12;) (type 7) (param i32 f32 f32 i32) (result f32)
      local.get 0
      f32.load
      local.get 0
      f32.load offset=4
      local.get 0
      f32.load offset=8
      local.get 0
      f32.load offset=12
      local.get 1
      local.get 2
      local.get 3
      f32.load
      local.get 3
      f32.load offset=4
      local.get 3
      f32.load offset=8
      local.get 3
      f32.load offset=12
      call $miden_base_sys::bindings::tx::extern_tx_create_note
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "\19basic_wallet\01\0b0.1.0\03\01\01")
  )
  (alias export 2 "asset" (type (;3;)))
  (alias export 2 "tag" (type (;4;)))
  (alias export 2 "recipient" (type (;5;)))
  (alias export 2 "note-type" (type (;6;)))
  (alias export 2 "felt" (type (;7;)))
  (alias export 0 "add-asset" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 0 "remove-asset" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (alias export 0 "incr-nonce" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;0;)
    (export "add-asset" (func 0))
    (export "remove-asset" (func 1))
    (export "incr-nonce" (func 2))
  )
  (alias export 1 "create-note" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (core instance (;1;)
    (export "create-note" (func 3))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-base/account@1.0.0" (instance 0))
      (with "miden:core-base/tx@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (type (;8;) (func (param "asset" 3)))
  (alias core export 2 "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (core func (;4;)))
  (func (;4;) (type 8) (canon lift (core func 4)))
  (type (;9;) (func (param "core-asset" 3) (param "tag" 4) (param "note-type" 6) (param "recipient" 5)))
  (alias core export 2 "miden:basic-wallet/basic-wallet@1.0.0#send-asset" (core func (;5;)))
  (func (;5;) (type 9) (canon lift (core func 5)))
  (alias export 2 "felt" (type (;10;)))
  (alias export 2 "word" (type (;11;)))
  (alias export 2 "asset" (type (;12;)))
  (alias export 2 "tag" (type (;13;)))
  (alias export 2 "recipient" (type (;14;)))
  (alias export 2 "note-type" (type (;15;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (type (;7;) (record (field "inner" 1)))
    (import "import-type-tag" (type (;8;) (eq 7)))
    (type (;9;) (record (field "inner" 4)))
    (import "import-type-recipient" (type (;10;) (eq 9)))
    (type (;11;) (record (field "inner" 1)))
    (import "import-type-note-type" (type (;12;) (eq 11)))
    (import "import-type-asset0" (type (;13;) (eq 6)))
    (type (;14;) (func (param "asset" 13)))
    (import "import-func-receive-asset" (func (;0;) (type 14)))
    (import "import-type-tag0" (type (;15;) (eq 8)))
    (import "import-type-note-type0" (type (;16;) (eq 12)))
    (import "import-type-recipient0" (type (;17;) (eq 10)))
    (type (;18;) (func (param "core-asset" 13) (param "tag" 15) (param "note-type" 16) (param "recipient" 17)))
    (import "import-func-send-asset" (func (;1;) (type 18)))
    (export (;19;) "asset" (type 6))
    (export (;20;) "tag" (type 8))
    (export (;21;) "recipient" (type 10))
    (export (;22;) "note-type" (type 12))
    (export (;23;) "felt" (type 1))
    (type (;24;) (func (param "asset" 19)))
    (export (;2;) "receive-asset" (func 0) (func (type 24)))
    (type (;25;) (func (param "core-asset" 19) (param "tag" 20) (param "note-type" 22) (param "recipient" 21)))
    (export (;3;) "send-asset" (func 1) (func (type 25)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-receive-asset" (func 4))
      (with "import-func-send-asset" (func 5))
      (with "import-type-felt" (type 10))
      (with "import-type-word" (type 11))
      (with "import-type-asset" (type 12))
      (with "import-type-tag" (type 13))
      (with "import-type-recipient" (type 14))
      (with "import-type-note-type" (type 15))
      (with "import-type-asset0" (type 3))
      (with "import-type-tag0" (type 4))
      (with "import-type-note-type0" (type 6))
      (with "import-type-recipient0" (type 5))
    )
  )
  (export (;4;) "miden:basic-wallet/basic-wallet@1.0.0" (instance 3))
)
