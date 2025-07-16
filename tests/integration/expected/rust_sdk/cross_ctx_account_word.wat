(component
  (type (;0;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32) (result f32)))
      (export (;1;) "add" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;1;) (type 1)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32) (result f32)))
    (type (;2;) (func))
    (type (;3;) (func (param f32 f32 f32 f32) (result i32)))
    (type (;4;) (func (param f32) (result f32)))
    (type (;5;) (func (param f32 f32) (result i32)))
    (type (;6;) (func (param f32 f32 f32) (result i32)))
    (type (;7;) (func (param i64 f32 i32 f32 i32 i32 i32) (result i32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-word))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-pair" (func $miden:cross-ctx-account-word/foo@1.0.0#process-pair))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-triple" (func $miden:cross-ctx-account-word/foo@1.0.0#process-triple))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-mixed" (func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-nested" (func $miden:cross-ctx-account-word/foo@1.0.0#process-nested))
    (elem (;0;) (i32.const 1) func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;2;) (type 2))
    (func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports (;3;) (type 2))
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-word (;4;) (type 3) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 2
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 2
      local.get 4
      i32.const 1048600
      i32.add
      local.tee 4
      local.get 3
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
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
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word (;5;) (type 3) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 2
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 2
      local.get 4
      i32.const 1048600
      i32.add
      local.tee 4
      local.get 3
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
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
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt (;6;) (type 4) (param f32) (result f32)
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-pair (;7;) (type 5) (param f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 2
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 2
      i32.const 1048600
      i32.add
      local.tee 2
      local.get 1
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      f32.store offset=4
      local.get 2
      local.get 0
      f32.store
      local.get 2
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-triple (;8;) (type 6) (param f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 3
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 3
      i32.const 1048600
      i32.add
      local.tee 3
      local.get 2
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      f32.store offset=8
      local.get 3
      local.get 1
      f32.store offset=4
      local.get 3
      local.get 0
      f32.store
      local.get 3
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed (;9;) (type 7) (param i64 f32 i32 f32 i32 i32 i32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 7
      call $wit_bindgen_rt::run_ctors_once
      local.get 1
      i32.const 6
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 3
      i32.const 7
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 3
      local.get 7
      i32.const 1048600
      i32.add
      local.tee 7
      local.get 6
      i32.const 9
      i32.add
      i32.store16 offset=22
      local.get 7
      local.get 5
      i32.const 255
      i32.and
      i32.eqz
      i32.store8 offset=21
      local.get 7
      local.get 4
      i32.const 11
      i32.add
      i32.store8 offset=20
      local.get 7
      local.get 3
      f32.store offset=16
      local.get 7
      local.get 2
      i32.const 10
      i32.add
      i32.store offset=12
      local.get 7
      local.get 1
      f32.store offset=8
      local.get 7
      local.get 0
      i64.const 1000
      i64.add
      i64.store
      local.get 7
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-nested (;10;) (type 6) (param f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 3
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 8
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 0
      local.get 1
      i32.const 8
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 3
      i32.const 1048600
      i32.add
      local.tee 3
      local.get 2
      i32.const 8
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      f32.store offset=8
      local.get 3
      local.get 1
      f32.store offset=4
      local.get 3
      local.get 0
      f32.store
      local.get 3
    )
    (func $wit_bindgen_rt::run_ctors_once (;11;) (type 2)
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
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 0 "add" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "add" (func 1))
  )
  (core instance (;1;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
    )
  )
  (alias core export 1 "memory" (core memory (;0;)))
  (alias export 1 "word" (type (;2;)))
  (type (;3;) (func (param "input" 2) (result 2)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-word" (core func (;2;)))
  (func (;2;) (type 3) (canon lift (core func 2) (memory 0)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (core func (;3;)))
  (func (;3;) (type 3) (canon lift (core func 3) (memory 0)))
  (alias export 1 "felt" (type (;4;)))
  (type (;5;) (func (param "input" 4) (result 4)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (core func (;4;)))
  (func (;4;) (type 5) (canon lift (core func 4)))
  (type (;6;) (record (field "first" 4) (field "second" 4)))
  (type (;7;) (func (param "input" 6) (result 6)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-pair" (core func (;5;)))
  (func (;5;) (type 7) (canon lift (core func 5) (memory 0)))
  (type (;8;) (record (field "x" 4) (field "y" 4) (field "z" 4)))
  (type (;9;) (func (param "input" 8) (result 8)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-triple" (core func (;6;)))
  (func (;6;) (type 9) (canon lift (core func 6) (memory 0)))
  (type (;10;) (record (field "f" u64) (field "a" 4) (field "b" u32) (field "c" 4) (field "d" u8) (field "e" bool) (field "g" u16)))
  (type (;11;) (func (param "input" 10) (result 10)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-mixed" (core func (;7;)))
  (func (;7;) (type 11) (canon lift (core func 7) (memory 0)))
  (type (;12;) (record (field "inner" 6) (field "value" 4)))
  (type (;13;) (func (param "input" 12) (result 12)))
  (alias core export 1 "miden:cross-ctx-account-word/foo@1.0.0#process-nested" (core func (;8;)))
  (func (;8;) (type 13) (canon lift (core func 8) (memory 0)))
  (alias export 1 "felt" (type (;14;)))
  (alias export 1 "word" (type (;15;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "input" 5) (result 5)))
    (import "import-func-process-word" (func (;0;) (type 6)))
    (import "import-func-process-another-word" (func (;1;) (type 6)))
    (import "import-type-felt0" (type (;7;) (eq 1)))
    (type (;8;) (func (param "input" 7) (result 7)))
    (import "import-func-process-felt" (func (;2;) (type 8)))
    (type (;9;) (record (field "first" 7) (field "second" 7)))
    (import "import-type-pair" (type (;10;) (eq 9)))
    (type (;11;) (func (param "input" 10) (result 10)))
    (import "import-func-process-pair" (func (;3;) (type 11)))
    (type (;12;) (record (field "x" 7) (field "y" 7) (field "z" 7)))
    (import "import-type-triple" (type (;13;) (eq 12)))
    (type (;14;) (func (param "input" 13) (result 13)))
    (import "import-func-process-triple" (func (;4;) (type 14)))
    (type (;15;) (record (field "f" u64) (field "a" 7) (field "b" u32) (field "c" 7) (field "d" u8) (field "e" bool) (field "g" u16)))
    (import "import-type-mixed-struct" (type (;16;) (eq 15)))
    (type (;17;) (func (param "input" 16) (result 16)))
    (import "import-func-process-mixed" (func (;5;) (type 17)))
    (type (;18;) (record (field "inner" 10) (field "value" 7)))
    (import "import-type-nested-struct" (type (;19;) (eq 18)))
    (type (;20;) (func (param "input" 19) (result 19)))
    (import "import-func-process-nested" (func (;6;) (type 20)))
    (export (;21;) "word" (type 4))
    (export (;22;) "felt" (type 1))
    (type (;23;) (record (field "first" 22) (field "second" 22)))
    (export (;24;) "pair" (type 23))
    (type (;25;) (record (field "x" 22) (field "y" 22) (field "z" 22)))
    (export (;26;) "triple" (type 25))
    (type (;27;) (record (field "f" u64) (field "a" 22) (field "b" u32) (field "c" 22) (field "d" u8) (field "e" bool) (field "g" u16)))
    (export (;28;) "mixed-struct" (type 27))
    (type (;29;) (record (field "inner" 24) (field "value" 22)))
    (export (;30;) "nested-struct" (type 29))
    (type (;31;) (func (param "input" 21) (result 21)))
    (export (;7;) "process-word" (func 0) (func (type 31)))
    (export (;8;) "process-another-word" (func 1) (func (type 31)))
    (type (;32;) (func (param "input" 22) (result 22)))
    (export (;9;) "process-felt" (func 2) (func (type 32)))
    (type (;33;) (func (param "input" 24) (result 24)))
    (export (;10;) "process-pair" (func 3) (func (type 33)))
    (type (;34;) (func (param "input" 26) (result 26)))
    (export (;11;) "process-triple" (func 4) (func (type 34)))
    (type (;35;) (func (param "input" 28) (result 28)))
    (export (;12;) "process-mixed" (func 5) (func (type 35)))
    (type (;36;) (func (param "input" 30) (result 30)))
    (export (;13;) "process-nested" (func 6) (func (type 36)))
  )
  (instance (;2;) (instantiate 0
      (with "import-func-process-word" (func 2))
      (with "import-func-process-another-word" (func 3))
      (with "import-func-process-felt" (func 4))
      (with "import-func-process-pair" (func 5))
      (with "import-func-process-triple" (func 6))
      (with "import-func-process-mixed" (func 7))
      (with "import-func-process-nested" (func 8))
      (with "import-type-felt" (type 14))
      (with "import-type-word" (type 15))
      (with "import-type-word0" (type 2))
      (with "import-type-felt0" (type 4))
      (with "import-type-pair" (type 6))
      (with "import-type-triple" (type 8))
      (with "import-type-mixed-struct" (type 10))
      (with "import-type-nested-struct" (type 12))
    )
  )
  (export (;3;) "miden:cross-ctx-account-word/foo@1.0.0" (instance 2))
)
