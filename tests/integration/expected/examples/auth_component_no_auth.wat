(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (param f32 f32) (result i32)))
    (type (;3;) (func (param i32)))
    (type (;4;) (func (result f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure" (func $miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure))
    (elem (;0;) (i32.const 1) func $auth_component_no_auth::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $auth_component_no_auth::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure (;2;) (type 1) (param f32 f32 f32 f32)
      (local i32 f32 f32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      call $miden::active_account::get_initial_commitment
      local.get 4
      f32.load offset=8
      local.set 5
      local.get 4
      f32.load offset=4
      local.set 6
      local.get 4
      f32.load
      local.set 7
      local.get 4
      f32.load offset=12
      local.set 8
      local.get 4
      call $miden::active_account::compute_commitment
      local.get 4
      f32.load offset=8
      local.set 9
      local.get 4
      f32.load offset=4
      local.set 10
      local.get 4
      f32.load
      local.set 11
      block ;; label = @1
        block ;; label = @2
          local.get 4
          f32.load offset=12
          local.get 8
          call $intrinsics::felt::eq
          i32.const 1
          i32.ne
          br_if 0 (;@2;)
          local.get 9
          local.get 5
          call $intrinsics::felt::eq
          i32.const 1
          i32.ne
          br_if 0 (;@2;)
          local.get 10
          local.get 6
          call $intrinsics::felt::eq
          i32.const 1
          i32.ne
          br_if 0 (;@2;)
          local.get 11
          local.get 7
          call $intrinsics::felt::eq
          i32.const 1
          i32.eq
          br_if 1 (;@1;)
        end
        call $miden::native_account::incr_nonce
        drop
      end
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
    (func $intrinsics::felt::eq (;4;) (type 2) (param f32 f32) (result i32)
      unreachable
    )
    (func $miden::active_account::compute_commitment (;5;) (type 3) (param i32)
      unreachable
    )
    (func $miden::active_account::get_initial_commitment (;6;) (type 3) (param i32)
      unreachable
    )
    (func $miden::native_account::incr_nonce (;7;) (type 4) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "-auth-component-no-auth\01\0b0.1.0\01\01")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (param "arg" $word)))
  (alias core export $main "miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure" (core func $miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure (;0;)))
  (func $auth-procedure (;0;) (type 2) (canon lift (core func $miden:auth-component-no-auth/auth-component-no-auth@0.1.0#auth-procedure)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:auth-component-no-auth/auth-component-no-auth@0.1.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-auth-procedure" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "auth-procedure" (func 0) (func (type 8)))
  )
  (instance $miden:auth-component-no-auth/auth-component-no-auth@0.1.0-shim-instance (;1;) (instantiate $miden:auth-component-no-auth/auth-component-no-auth@0.1.0-shim-component
      (with "import-func-auth-procedure" (func $auth-procedure))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:auth-component-no-auth/auth-component-no-auth@0.1.0 (;2;) "miden:auth-component-no-auth/auth-component-no-auth@0.1.0" (instance $miden:auth-component-no-auth/auth-component-no-auth@0.1.0-shim-instance))
)
