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
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (param i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/transaction-script@1.0.0#run" (func $miden:base/transaction-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $rust_sdk_account_tx_get_input_notes_commitment_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_account_tx_get_input_notes_commitment_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:base/transaction-script@1.0.0#run (;2;) (type 1) (param f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      call $miden_base_sys::bindings::tx::get_input_notes_commitment
      local.get 4
      i32.const 16
      i32.add
      global.set $__stack_pointer
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
    (func $miden_base_sys::bindings::tx::get_input_notes_commitment (;4;) (type 2) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::protocol::tx::get_input_notes_commitment
      local.get 0
      local.get 1
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden::protocol::tx::get_input_notes_commitment (;5;) (type 2) (param i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (param "arg" $word)))
  (alias core export $main "miden:base/transaction-script@1.0.0#run" (core func $miden:base/transaction-script@1.0.0#run (;0;)))
  (func $run (;0;) (type 2) (canon lift (core func $miden:base/transaction-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:base/transaction-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (type (;5;) (func (param "arg" 4)))
    (import "import-func-run" (func (;0;) (type 5)))
    (export (;6;) "word" (type 3))
    (type (;7;) (func (param "arg" 6)))
    (export (;1;) "run" (func 0) (func (type 7)))
  )
  (instance $miden:base/transaction-script@1.0.0-shim-instance (;1;) (instantiate $miden:base/transaction-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/transaction-script@1.0.0 (;2;) "miden:base/transaction-script@1.0.0" (instance $miden:base/transaction-script@1.0.0-shim-instance))
)
