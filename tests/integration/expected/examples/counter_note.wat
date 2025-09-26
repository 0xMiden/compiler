(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
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
  (core module (;0;)
    (type (;0;) (func (result f32)))
    (type (;1;) (func))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param f32 f32) (result f32)))
    (type (;4;) (func (param i32) (result f32)))
    (type (;5;) (func (param f32 f32)))
    (import "miden:counter-contract/counter@0.1.0" "get-count" (func $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0 (;0;) (type 0)))
    (import "miden:counter-contract/counter@0.1.0" "increment-count" (func $counter_note::bindings::miden::counter_contract::counter::increment_count::wit_import0 (;1;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $counter_note::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;2;) (type 1))
    (func $counter_note::bindings::__link_custom_section_describing_imports (;3;) (type 1))
    (func $miden:base/note-script@1.0.0#run (;4;) (type 2) (param f32 f32 f32 f32)
      (local f32)
      call $wit_bindgen::rt::run_ctors_once
      call $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0
      local.set 4
      call $counter_note::bindings::miden::counter_contract::counter::increment_count::wit_import0
      drop
      local.get 4
      i32.const 1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::add
      local.set 4
      call $counter_note::bindings::miden::counter_contract::counter::get_count::wit_import0
      local.get 4
      call $intrinsics::felt::assert_eq
    )
    (func $wit_bindgen::rt::run_ctors_once (;5;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048588
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048588
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::add (;6;) (type 3) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;7;) (type 4) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;8;) (type 5) (param f32 f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "word" (type (;3;)))
  (alias export 1 "get-count" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 1 "increment-count" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "get-count" (func 0))
    (export "increment-count" (func 1))
  )
  (core instance (;1;) (instantiate 0
      (with "miden:counter-contract/counter@0.1.0" (instance 0))
    )
  )
  (alias core export 1 "memory" (core memory (;0;)))
  (type (;4;) (func (param "arg" 3)))
  (alias core export 1 "miden:base/note-script@1.0.0#run" (core func (;2;)))
  (func (;2;) (type 4) (canon lift (core func 2)))
  (alias export 0 "felt" (type (;5;)))
  (alias export 0 "word" (type (;6;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-run" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "run" (func 0) (func (type 8)))
  )
  (instance (;2;) (instantiate 0
      (with "import-func-run" (func 2))
      (with "import-type-felt" (type 5))
      (with "import-type-word" (type 6))
      (with "import-type-word0" (type 3))
    )
  )
  (export (;3;) "miden:base/note-script@1.0.0" (instance 2))
)
