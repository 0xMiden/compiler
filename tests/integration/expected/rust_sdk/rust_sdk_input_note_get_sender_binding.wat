(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "prefix" 1) (field "suffix" 1)))
      (export (;3;) "account-id" (type (eq 2)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (result i32)))
    (type (;2;) (func (param i32 f32)))
    (type (;3;) (func (param i64) (result f32)))
    (type (;4;) (func (param f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding" (func $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding))
    (elem (;0;) (i32.const 1) func $rust_sdk_input_note_get_sender_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_input_note_get_sender_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding (;2;) (type 1) (result i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 8
      i32.add
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      call $miden_base_sys::bindings::input_note::get_sender
      global.get $GOT.data.internal.__memory_base
      i32.const 1048584
      i32.add
      local.tee 1
      local.get 0
      i64.load offset=8
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
    (func $miden_base_sys::bindings::input_note::get_sender (;4;) (type 2) (param i32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      local.get 2
      i32.const 8
      i32.add
      call $miden::protocol::input_note::get_sender
      local.get 0
      local.get 2
      i64.load offset=8 align=4
      i64.store
      local.get 2
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $intrinsics::felt::from_u64_unchecked (;5;) (type 3) (param i64) (result f32)
      unreachable
    )
    (func $miden::protocol::input_note::get_sender (;6;) (type 4) (param f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "Mrust_sdk_input_note_get_sender_binding\01\0b0.0.1\03\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "account-id" (type $account-id (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (result $account-id)))
  (alias core export $main "miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding" (core func $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding (;0;)))
  (func $binding (;0;) (type 2) (canon lift (core func $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1#binding) (memory $memory)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "account-id" (type $"#type4 account-id" (@name "account-id") (;4;)))
  (component $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "prefix" 1) (field "suffix" 1)))
    (import "import-type-account-id" (type (;3;) (eq 2)))
    (import "import-type-account-id0" (type (;4;) (eq 3)))
    (type (;5;) (func (result 4)))
    (import "import-func-binding" (func (;0;) (type 5)))
    (export (;6;) "account-id" (type 3))
    (type (;7;) (func (result 6)))
    (export (;1;) "binding" (func 0) (func (type 7)))
  )
  (instance $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1-shim-instance (;1;) (instantiate $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1-shim-component
      (with "import-func-binding" (func $binding))
      (with "import-type-felt" (type $felt))
      (with "import-type-account-id" (type $"#type4 account-id"))
      (with "import-type-account-id0" (type $account-id))
    )
  )
  (export $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1 (;2;) "miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1" (instance $miden:rust-sdk-input-note-get-sender-binding/rust-sdk-input-note-get-sender-binding@0.0.1-shim-instance))
)
