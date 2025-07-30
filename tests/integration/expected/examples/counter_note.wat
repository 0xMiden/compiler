(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "felt" (type (;1;)))
  (type (;2;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (func (result 1)))
      (export (;0;) "get-count" (func (type 2)))
      (export (;1;) "increment-count" (func (type 2)))
    )
  )
  (import "miden:counter-contract/counter@0.1.0" (instance (;1;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32) (result f32)))
      (export (;1;) "add" (func (type 1)))
      (type (;2;) (func (param "a" f32) (param "b" f32)))
      (export (;2;) "assert-eq" (func (type 2)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;2;) (type 3)))
  (core module (;0;)
    (type (;0;) (func (result f32)))
    (type (;1;) (func (param i32) (result f32)))
    (type (;2;) (func (param f32 f32) (result f32)))
    (type (;3;) (func (param f32 f32)))
    (type (;4;) (func))
    (import "miden:counter-contract/counter@0.1.0" "get-count" (func $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0 (;0;) (type 0)))
    (import "miden:counter-contract/counter@0.1.0" "increment-count" (func $counter_note::bindings::miden::counter_contract::counter::increment_count::wit_import0 (;1;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;2;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;3;) (type 2)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;4;) (type 3)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/script@1.0.0#script" (func $miden:base/script@1.0.0#script))
    (elem (;0;) (i32.const 1) func $counter_note::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;5;) (type 4))
    (func $counter_note::bindings::__link_custom_section_describing_imports (;6;) (type 4))
    (func $miden:base/script@1.0.0#script (;7;) (type 4)
      (local f32)
      call $wit_bindgen_rt::run_ctors_once
      call $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0
      local.set 0
      call $counter_note::bindings::miden::counter_contract::counter::increment_count::wit_import0
      drop
      local.get 0
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      call $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0
      local.get 0
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    )
    (func $wit_bindgen_rt::run_ctors_once (;8;) (type 4)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048624
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048624
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 1 "get-count" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 1 "increment-count" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "get-count" (func 0))
    (export "increment-count" (func 1))
  )
  (alias export 2 "from-u32" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 2 "add" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 2 "assert-eq" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;1;)
    (export "from-u32" (func 2))
    (export "add" (func 3))
    (export "assert-eq" (func 4))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:counter-contract/counter@0.1.0" (instance 0))
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (type (;4;) (func))
  (alias core export 2 "miden:base/script@1.0.0#script" (core func (;5;)))
  (func (;5;) (type 4) (canon lift (core func 5)))
  (component (;0;)
    (type (;0;) (func))
    (import "import-func-script" (func (;0;) (type 0)))
    (type (;1;) (func))
    (export (;1;) "script" (func 0) (func (type 1)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-script" (func 5))
    )
  )
  (export (;4;) "miden:base/script@1.0.0" (instance 3))
)
