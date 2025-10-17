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
      (export (;8;) "note-idx" (type (eq 7)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (param f32 f32 f32 f32 f32)))
    (type (;3;) (func (param f32 f32 f32 f32 i32 i32) (result i32)))
    (type (;4;) (func (param i32 i32)))
    (type (;5;) (func (param i32 i32 f32)))
    (type (;6;) (func (param f32 f32 f32 f32 i32)))
    (type (;7;) (func (param f32 f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:basic-wallet/basic-wallet@0.1.0#receive-asset" (func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset))
    (export "miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note" (func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note))
    (export "miden:basic-wallet/basic-wallet@0.1.0#test-custom-types" (func $miden:basic-wallet/basic-wallet@0.1.0#test-custom-types))
    (elem (;0;) (i32.const 1) func $basic_wallet::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $basic_wallet::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:basic-wallet/basic-wallet@0.1.0#receive-asset (;2;) (type 1) (param f32 f32 f32 f32)
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
    (func $miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note (;3;) (type 2) (param f32 f32 f32 f32 f32)
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
    (func $miden:basic-wallet/basic-wallet@0.1.0#test-custom-types (;4;) (type 3) (param f32 f32 f32 f32 i32 i32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 6
      call $wit_bindgen::rt::run_ctors_once
      local.get 6
      i32.const 1048584
      i32.add
      local.tee 6
      local.get 1
      f32.store offset=4
      local.get 6
      local.get 0
      f32.store
      local.get 6
    )
    (func $wit_bindgen::rt::run_ctors_once (;5;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048592
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048592
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $miden_base_sys::bindings::account::add_asset (;6;) (type 4) (param i32 i32)
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
    (func $miden_base_sys::bindings::account::remove_asset (;7;) (type 4) (param i32 i32)
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
    (func $miden_base_sys::bindings::tx::add_asset_to_note (;8;) (type 5) (param i32 i32 f32)
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
    (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;9;) (type 4) (param i32 i32)
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
    (func $miden::account::add_asset (;10;) (type 6) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::account::remove_asset (;11;) (type 6) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::tx::add_asset_to_note (;12;) (type 7) (param f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "\19basic_wallet\01\0b0.1.0\03\01\01\00\00\00\00\00\00\00\00\00")
  )
  (alias export 0 "asset" (type (;1;)))
  (alias export 0 "felt" (type (;2;)))
  (alias export 0 "note-idx" (type (;3;)))
  (alias export 0 "word" (type (;4;)))
  (type (;5;) (variant (case "variant-a") (case "variant-b")))
  (type (;6;) (record (field "foo" 4) (field "an-enum" 5)))
  (type (;7;) (record (field "bar" 2) (field "baz" 2)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;8;) (func (param "asset" 1)))
  (alias core export 0 "miden:basic-wallet/basic-wallet@0.1.0#receive-asset" (core func (;0;)))
  (func (;0;) (type 8) (canon lift (core func 0)))
  (type (;9;) (func (param "asset" 1) (param "note-idx" 3)))
  (alias core export 0 "miden:basic-wallet/basic-wallet@0.1.0#move-asset-to-note" (core func (;1;)))
  (func (;1;) (type 9) (canon lift (core func 1)))
  (type (;10;) (func (param "a" 6) (param "b" 5) (result 7)))
  (alias core export 0 "miden:basic-wallet/basic-wallet@0.1.0#test-custom-types" (core func (;2;)))
  (func (;2;) (type 10) (canon lift (core func 2) (memory 0)))
  (alias export 0 "felt" (type (;11;)))
  (alias export 0 "word" (type (;12;)))
  (alias export 0 "asset" (type (;13;)))
  (alias export 0 "note-idx" (type (;14;)))
  (component (;0;)
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
    (import "import-type-word0" (type (;13;) (eq 4)))
    (type (;14;) (variant (case "variant-a") (case "variant-b")))
    (import "import-type-enum-a" (type (;15;) (eq 14)))
    (type (;16;) (record (field "foo" 13) (field "an-enum" 15)))
    (import "import-type-struct-a" (type (;17;) (eq 16)))
    (import "import-type-felt0" (type (;18;) (eq 1)))
    (type (;19;) (record (field "bar" 18) (field "baz" 18)))
    (import "import-type-struct-b" (type (;20;) (eq 19)))
    (type (;21;) (func (param "a" 17) (param "b" 15) (result 20)))
    (import "import-func-test-custom-types" (func (;2;) (type 21)))
    (export (;22;) "asset" (type 6))
    (export (;23;) "felt" (type 1))
    (export (;24;) "note-idx" (type 8))
    (export (;25;) "word" (type 4))
    (type (;26;) (variant (case "variant-a") (case "variant-b")))
    (export (;27;) "enum-a" (type 26))
    (type (;28;) (record (field "foo" 25) (field "an-enum" 27)))
    (export (;29;) "struct-a" (type 28))
    (type (;30;) (record (field "bar" 23) (field "baz" 23)))
    (export (;31;) "struct-b" (type 30))
    (type (;32;) (func (param "asset" 22)))
    (export (;3;) "receive-asset" (func 0) (func (type 32)))
    (type (;33;) (func (param "asset" 22) (param "note-idx" 24)))
    (export (;4;) "move-asset-to-note" (func 1) (func (type 33)))
    (type (;34;) (func (param "a" 29) (param "b" 27) (result 31)))
    (export (;5;) "test-custom-types" (func 2) (func (type 34)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-receive-asset" (func 0))
      (with "import-func-move-asset-to-note" (func 1))
      (with "import-func-test-custom-types" (func 2))
      (with "import-type-felt" (type 11))
      (with "import-type-word" (type 12))
      (with "import-type-asset" (type 13))
      (with "import-type-note-idx" (type 14))
      (with "import-type-asset0" (type 1))
      (with "import-type-note-idx0" (type 3))
      (with "import-type-word0" (type 4))
      (with "import-type-enum-a" (type 5))
      (with "import-type-struct-a" (type 6))
      (with "import-type-felt0" (type 2))
      (with "import-type-struct-b" (type 7))
    )
  )
  (export (;2;) "miden:basic-wallet/basic-wallet@0.1.0" (instance 1))
)
