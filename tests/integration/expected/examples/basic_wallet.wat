(component
  (type (;0;)
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
      (type (;13;) (record (field "inner" 1)))
      (export (;14;) "note-idx" (type (eq 13)))
      (type (;15;) (record (field "inner" 1)))
      (export (;16;) "note-execution-hint" (type (eq 15)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (param f32 f32 f32 f32 f32)))
    (type (;3;) (func (param i32 i32)))
    (type (;4;) (func (param i32 i32 f32)))
    (type (;5;) (func (param f32 f32 f32 f32 i32)))
    (type (;6;) (func (param f32 f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset))
    (export "miden:basic-wallet/basic-wallet@1.0.0#move-asset-to-note" (func $miden:basic-wallet/basic-wallet@1.0.0#move-asset-to-note))
    (elem (;0;) (i32.const 1) func $basic_wallet::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $basic_wallet::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset (;2;) (type 1) (param f32 f32 f32 f32)
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
      call $miden_base_sys::bindings::account::add_asset
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden:basic-wallet/basic-wallet@1.0.0#move-asset-to-note (;3;) (type 2) (param f32 f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 64
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
      call $miden_base_sys::bindings::account::remove_asset
      local.get 5
      i32.const 32
      i32.add
      local.get 5
      i32.const 16
      i32.add
      local.get 4
      call $miden_base_sys::bindings::tx::add_asset_to_note
      local.get 5
      i32.const 64
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
    (func $miden_base_sys::bindings::account::add_asset (;5;) (type 3) (param i32 i32)
      local.get 1
      f32.load offset=12
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load
      local.get 0
      call $miden::account::add_asset
    )
    (func $miden_base_sys::bindings::account::remove_asset (;6;) (type 3) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
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
      call $miden::account::remove_asset
      local.get 2
      local.get 2
      i64.load offset=8
      i64.store offset=24
      local.get 2
      local.get 2
      i64.load
      i64.store offset=16
      local.get 0
      local.get 2
      i32.const 16
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 2
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::tx::add_asset_to_note (;7;) (type 4) (param i32 i32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 3
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
      local.get 3
      call $miden::tx::add_asset_to_note
      local.get 3
      local.get 3
      i64.load offset=8
      i64.store offset=40
      local.get 3
      local.get 3
      i64.load
      i64.store offset=32
      local.get 3
      f32.load offset=16
      local.set 2
      local.get 0
      local.get 3
      i32.const 32
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 0
      local.get 2
      f32.store offset=16
      local.get 3
      i32.const 48
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;8;) (type 3) (param i32 i32)
      (local i32 i32 i32 f32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      local.get 1
      i64.load offset=8
      i64.store offset=8 align=4
      local.get 2
      local.get 1
      i64.load
      i64.store align=4
      local.get 2
      i32.const 12
      i32.add
      local.set 3
      i32.const 0
      local.set 1
      block ;; label = @1
        loop ;; label = @2
          local.get 1
          i32.const 8
          i32.eq
          br_if 1 (;@1;)
          local.get 2
          local.get 1
          i32.add
          local.tee 4
          f32.load
          local.set 5
          local.get 4
          local.get 3
          i32.load
          i32.store
          local.get 3
          local.get 5
          f32.store
          local.get 1
          i32.const 4
          i32.add
          local.set 1
          local.get 3
          i32.const -4
          i32.add
          local.set 3
          br 0 (;@2;)
        end
      end
      local.get 0
      local.get 2
      i64.load offset=8 align=4
      i64.store offset=8
      local.get 0
      local.get 2
      i64.load align=4
      i64.store
    )
    (func $miden::account::add_asset (;9;) (type 5) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::account::remove_asset (;10;) (type 5) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::tx::add_asset_to_note (;11;) (type 6) (param f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "\19basic_wallet\01\0b0.1.0\03\01\01\00\00\00\00\00\00\00\00\00")
  )
  (alias export 0 "asset" (type (;1;)))
  (alias export 0 "tag" (type (;2;)))
  (alias export 0 "recipient" (type (;3;)))
  (alias export 0 "note-type" (type (;4;)))
  (alias export 0 "note-idx" (type (;5;)))
  (alias export 0 "felt" (type (;6;)))
  (alias export 0 "note-execution-hint" (type (;7;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;8;) (func (param "asset" 1)))
  (alias core export 0 "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (core func (;0;)))
  (func (;0;) (type 8) (canon lift (core func 0)))
  (type (;9;) (func (param "asset" 1) (param "note-idx" 5)))
  (alias core export 0 "miden:basic-wallet/basic-wallet@1.0.0#move-asset-to-note" (core func (;1;)))
  (func (;1;) (type 9) (canon lift (core func 1)))
  (alias export 0 "felt" (type (;10;)))
  (alias export 0 "word" (type (;11;)))
  (alias export 0 "asset" (type (;12;)))
  (alias export 0 "tag" (type (;13;)))
  (alias export 0 "recipient" (type (;14;)))
  (alias export 0 "note-type" (type (;15;)))
  (alias export 0 "note-idx" (type (;16;)))
  (alias export 0 "note-execution-hint" (type (;17;)))
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
    (type (;13;) (record (field "inner" 1)))
    (import "import-type-note-idx" (type (;14;) (eq 13)))
    (type (;15;) (record (field "inner" 1)))
    (import "import-type-note-execution-hint" (type (;16;) (eq 15)))
    (import "import-type-asset0" (type (;17;) (eq 6)))
    (type (;18;) (func (param "asset" 17)))
    (import "import-func-receive-asset" (func (;0;) (type 18)))
    (import "import-type-note-idx0" (type (;19;) (eq 14)))
    (type (;20;) (func (param "asset" 17) (param "note-idx" 19)))
    (import "import-func-move-asset-to-note" (func (;1;) (type 20)))
    (export (;21;) "asset" (type 6))
    (export (;22;) "tag" (type 8))
    (export (;23;) "recipient" (type 10))
    (export (;24;) "note-type" (type 12))
    (export (;25;) "note-idx" (type 14))
    (export (;26;) "felt" (type 1))
    (export (;27;) "note-execution-hint" (type 16))
    (type (;28;) (func (param "asset" 21)))
    (export (;2;) "receive-asset" (func 0) (func (type 28)))
    (type (;29;) (func (param "asset" 21) (param "note-idx" 25)))
    (export (;3;) "move-asset-to-note" (func 1) (func (type 29)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-receive-asset" (func 0))
      (with "import-func-move-asset-to-note" (func 1))
      (with "import-type-felt" (type 10))
      (with "import-type-word" (type 11))
      (with "import-type-asset" (type 12))
      (with "import-type-tag" (type 13))
      (with "import-type-recipient" (type 14))
      (with "import-type-note-type" (type 15))
      (with "import-type-note-idx" (type 16))
      (with "import-type-note-execution-hint" (type 17))
      (with "import-type-asset0" (type 1))
      (with "import-type-note-idx0" (type 5))
    )
  )
  (export (;2;) "miden:basic-wallet/basic-wallet@1.0.0" (instance 1))
)
