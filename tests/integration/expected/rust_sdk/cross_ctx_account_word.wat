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
    (type (;1;) (func (param f32 f32 f32 f32) (result i32)))
    (type (;2;) (func (param f32) (result f32)))
    (type (;3;) (func (param i64 f32 i32 f32 i32 i32 i32) (result i32)))
    (type (;4;) (func (param f32 f32 f32) (result i32)))
    (type (;5;) (func (param f32 f32) (result i32)))
    (type (;6;) (func (param f32 f32) (result f32)))
    (type (;7;) (func (param i64) (result f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-mixed" (func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-nested" (func $miden:cross-ctx-account-word/foo@1.0.0#process-nested))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-pair" (func $miden:cross-ctx-account-word/foo@1.0.0#process-pair))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-triple" (func $miden:cross-ctx-account-word/foo@1.0.0#process-triple))
    (export "miden:cross-ctx-account-word/foo@1.0.0#process-word" (func $miden:cross-ctx-account-word/foo@1.0.0#process-word))
    (elem (;0;) (i32.const 1) func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $cross_ctx_account_word::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word (;2;) (type 1) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 2
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 0
      local.get 1
      i64.const 3
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 1
      local.get 2
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 2
      local.get 4
      i32.const 1048584
      i32.add
      local.tee 4
      local.get 3
      i64.const 5
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
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
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-felt (;3;) (type 2) (param f32) (result f32)
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 3
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed (;4;) (type 3) (param i64 f32 i32 f32 i32 i32 i32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 7
      call $wit_bindgen::rt::run_ctors_once
      local.get 1
      i64.const 6
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 1
      local.get 3
      i64.const 7
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 3
      local.get 7
      i32.const 1048584
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
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-nested (;5;) (type 4) (param f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 3
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 8
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 0
      local.get 1
      i64.const 8
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 1
      local.get 3
      i32.const 1048584
      i32.add
      local.tee 3
      local.get 2
      i64.const 8
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      f32.store offset=8
      local.get 3
      local.get 1
      f32.store offset=4
      local.get 3
      local.get 0
      f32.store
      local.get 3
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-pair (;6;) (type 5) (param f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 2
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 0
      local.get 2
      i32.const 1048584
      i32.add
      local.tee 2
      local.get 1
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      f32.store offset=4
      local.get 2
      local.get 0
      f32.store
      local.get 2
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-triple (;7;) (type 4) (param f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 3
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 5
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 0
      local.get 1
      i64.const 5
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 1
      local.get 3
      i32.const 1048584
      i32.add
      local.tee 3
      local.get 2
      i64.const 5
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      f32.store offset=8
      local.get 3
      local.get 1
      f32.store offset=4
      local.get 3
      local.get 0
      f32.store
      local.get 3
    )
    (func $miden:cross-ctx-account-word/foo@1.0.0#process-word (;8;) (type 1) (param f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 4
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i64.const 1
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 0
      local.get 1
      i64.const 2
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 1
      local.get 2
      i64.const 3
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
      local.set 2
      local.get 4
      i32.const 1048584
      i32.add
      local.tee 4
      local.get 3
      i64.const 4
      call $intrinsics::felt::from_u64_unchecked
      call $intrinsics::felt::add
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
    (func $wit_bindgen::rt::run_ctors_once (;9;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048608
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048608
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::add (;10;) (type 6) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;11;) (type 7) (param i64) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;2;)))
  (type (;3;) (record (field "first" $felt) (field "second" $felt)))
  (type (;4;) (record (field "x" $felt) (field "y" $felt) (field "z" $felt)))
  (type (;5;) (record (field "f" u64) (field "a" $felt) (field "b" u32) (field "c" $felt) (field "d" u8) (field "e" bool) (field "g" u16)))
  (type (;6;) (record (field "inner" 3) (field "value" $felt)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;7;) (func (param "input" $word) (result $word)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-word" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-word (;0;)))
  (func $process-word (;0;) (type 7) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-word) (memory $memory)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-another-word" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word (;1;)))
  (func $process-another-word (;1;) (type 7) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-another-word) (memory $memory)))
  (type (;8;) (func (param "input" $felt) (result $felt)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-felt" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-felt (;2;)))
  (func $process-felt (;2;) (type 8) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-felt)))
  (type (;9;) (func (param "input" 3) (result 3)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-pair" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-pair (;3;)))
  (func $process-pair (;3;) (type 9) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-pair) (memory $memory)))
  (type (;10;) (func (param "input" 4) (result 4)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-triple" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-triple (;4;)))
  (func $process-triple (;4;) (type 10) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-triple) (memory $memory)))
  (type (;11;) (func (param "input" 5) (result 5)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-mixed" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed (;5;)))
  (func $process-mixed (;5;) (type 11) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-mixed) (memory $memory)))
  (type (;12;) (func (param "input" 6) (result 6)))
  (alias core export $main "miden:cross-ctx-account-word/foo@1.0.0#process-nested" (core func $miden:cross-ctx-account-word/foo@1.0.0#process-nested (;6;)))
  (func $process-nested (;6;) (type 12) (canon lift (core func $miden:cross-ctx-account-word/foo@1.0.0#process-nested) (memory $memory)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type13 felt" (@name "felt") (;13;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type14 word" (@name "word") (;14;)))
  (component $miden:cross-ctx-account-word/foo@1.0.0-shim-component (;0;)
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
  (instance $miden:cross-ctx-account-word/foo@1.0.0-shim-instance (;1;) (instantiate $miden:cross-ctx-account-word/foo@1.0.0-shim-component
      (with "import-func-process-word" (func $process-word))
      (with "import-func-process-another-word" (func $process-another-word))
      (with "import-func-process-felt" (func $process-felt))
      (with "import-func-process-pair" (func $process-pair))
      (with "import-func-process-triple" (func $process-triple))
      (with "import-func-process-mixed" (func $process-mixed))
      (with "import-func-process-nested" (func $process-nested))
      (with "import-type-felt" (type $"#type13 felt"))
      (with "import-type-word" (type $"#type14 word"))
      (with "import-type-word0" (type $word))
      (with "import-type-felt0" (type $felt))
      (with "import-type-pair" (type 3))
      (with "import-type-triple" (type 4))
      (with "import-type-mixed-struct" (type 5))
      (with "import-type-nested-struct" (type 6))
    )
  )
  (export $miden:cross-ctx-account-word/foo@1.0.0 (;2;) "miden:cross-ctx-account-word/foo@1.0.0" (instance $miden:cross-ctx-account-word/foo@1.0.0-shim-instance))
)
