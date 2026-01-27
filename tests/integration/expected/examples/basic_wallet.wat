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
      (type (;7;) (record (field "inner" 1)))
      (export (;8;) "note-idx" (type (eq 7)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32 f32)))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param i32 i32)))
    (type (;4;) (func (param i32 f32)))
    (type (;5;) (func (param f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note" (func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note))
    (export "miden:basic-wallet/basic-wallet@0.1.0#receive-asset" (func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset))
    (elem (;0;) (i32.const 1) func $basic_wallet::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $basic_wallet::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note (;2;) (type 1) (param f32 f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 5
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 5
      local.get 3
      f32.store offset=12
      local.get 5
      local.get 2
      f32.store offset=8
      local.get 5
      local.get 1
      f32.store offset=4
      local.get 5
      local.get 0
      f32.store
      local.get 5
      i32.const 16
      i32.add
      local.get 5
      call $miden_base_sys::bindings::native_account::remove_asset
      local.get 5
      i32.const 16
      i32.add
      local.get 4
      call $miden_base_sys::bindings::output_note::add_asset
      local.get 5
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset (;3;) (type 2) (param f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
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
      call $miden_base_sys::bindings::native_account::add_asset
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 0)
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
    (func $miden_base_sys::bindings::native_account::add_asset (;5;) (type 3) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      f32.load offset=12
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load
      local.get 2
      call $miden::protocol::native_account::add_asset
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
    (func $miden_base_sys::bindings::output_note::add_asset (;6;) (type 4) (param i32 f32)
      local.get 0
      f32.load offset=12
      local.get 0
      f32.load offset=8
      local.get 0
      f32.load offset=4
      local.get 0
      f32.load
      local.get 1
      call $miden::protocol::output_note::add_asset
    )
    (func $miden_base_sys::bindings::native_account::remove_asset (;7;) (type 3) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      f32.load offset=12
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load
      local.get 2
      call $miden::protocol::native_account::remove_asset
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
    (func $miden::protocol::native_account::add_asset (;8;) (type 5) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::native_account::remove_asset (;9;) (type 5) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::output_note::add_asset (;10;) (type 1) (param f32 f32 f32 f32 f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "\19basic_wallet\01\0b0.1.0\03\01\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;1;)))
  (alias export $miden:base/core-types@1.0.0 "note-idx" (type $note-idx (;2;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;3;) (func (param "asset" $asset)))
  (alias core export $main "miden:basic-wallet/basic-wallet@0.1.0#receive-asset" (core func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset (;0;)))
  (func $receive-asset (;0;) (type 3) (canon lift (core func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset)))
  (type (;4;) (func (param "asset" $asset) (param "note-idx" $note-idx)))
  (alias core export $main "miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note" (core func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note (;1;)))
  (func $move-asset-to-note (;1;) (type 4) (canon lift (core func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;5;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;6;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $"#type7 asset" (@name "asset") (;7;)))
  (alias export $miden:base/core-types@1.0.0 "note-idx" (type $"#type8 note-idx" (@name "note-idx") (;8;)))
  (component $miden:basic-wallet/basic-wallet@0.1.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (type (;7;) (record (field "inner" 1)))
    (import "import-type-note-idx" (type (;8;) (eq 7)))
    (import "import-type-asset0" (type (;9;) (eq 6)))
    (type (;10;) (func (param "asset" 9)))
    (import "import-func-receive-asset" (func (;0;) (type 10)))
    (import "import-type-note-idx0" (type (;11;) (eq 8)))
    (type (;12;) (func (param "asset" 9) (param "note-idx" 11)))
    (import "import-func-move-asset-to-note" (func (;1;) (type 12)))
    (export (;13;) "asset" (type 6))
    (export (;14;) "note-idx" (type 8))
    (type (;15;) (func (param "asset" 13)))
    (export (;2;) "receive-asset" (func 0) (func (type 15)))
    (type (;16;) (func (param "asset" 13) (param "note-idx" 14)))
    (export (;3;) "move-asset-to-note" (func 1) (func (type 16)))
  )
  (instance $miden:basic-wallet/basic-wallet@0.1.0-shim-instance (;1;) (instantiate $miden:basic-wallet/basic-wallet@0.1.0-shim-component
      (with "import-func-receive-asset" (func $receive-asset))
      (with "import-func-move-asset-to-note" (func $move-asset-to-note))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $word))
      (with "import-type-asset" (type $"#type7 asset"))
      (with "import-type-note-idx" (type $"#type8 note-idx"))
      (with "import-type-asset0" (type $asset))
      (with "import-type-note-idx0" (type $note-idx))
    )
  )
  (export $miden:basic-wallet/basic-wallet@0.1.0 (;2;) "miden:basic-wallet/basic-wallet@0.1.0" (instance $miden:basic-wallet/basic-wallet@0.1.0-shim-instance))
)
