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
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (type (;5;) (func (param "input" 4) (result 4)))
    (import "import-func-process-word" (func (;0;) (type 5)))
    (import "import-func-process-another-word" (func (;1;) (type 5)))
    (import "import-type-felt0" (type (;6;) (eq 1)))
    (type (;7;) (func (param "input" 6) (result 6)))
    (import "import-func-process-felt" (func (;2;) (type 7)))
    (type (;8;) (record (field "first" 6) (field "second" 6)))
    (import "import-type-pair" (type (;9;) (eq 8)))
    (type (;10;) (func (param "input" 9) (result 9)))
    (import "import-func-process-pair" (func (;3;) (type 10)))
    (type (;11;) (record (field "x" 6) (field "y" 6) (field "z" 6)))
    (import "import-type-triple" (type (;12;) (eq 11)))
    (type (;13;) (func (param "input" 12) (result 12)))
    (import "import-func-process-triple" (func (;4;) (type 13)))
    (type (;14;) (record (field "f" u64) (field "a" 6) (field "b" u32) (field "c" 6) (field "d" u8) (field "e" bool) (field "g" u16)))
    (import "import-type-mixed-struct" (type (;15;) (eq 14)))
    (type (;16;) (func (param "input" 15) (result 15)))
    (import "import-func-process-mixed" (func (;5;) (type 16)))
    (type (;17;) (record (field "inner" 9) (field "value" 6)))
    (import "import-type-nested-struct" (type (;18;) (eq 17)))
    (type (;19;) (func (param "input" 18) (result 18)))
    (import "import-func-process-nested" (func (;6;) (type 19)))
    (export (;20;) "word" (type 3))
    (export (;21;) "felt" (type 1))
    (type (;22;) (record (field "first" 21) (field "second" 21)))
    (export (;23;) "pair" (type 22))
    (type (;24;) (record (field "x" 21) (field "y" 21) (field "z" 21)))
    (export (;25;) "triple" (type 24))
    (type (;26;) (record (field "f" u64) (field "a" 21) (field "b" u32) (field "c" 21) (field "d" u8) (field "e" bool) (field "g" u16)))
    (export (;27;) "mixed-struct" (type 26))
    (type (;28;) (record (field "inner" 23) (field "value" 21)))
    (export (;29;) "nested-struct" (type 28))
    (type (;30;) (func (param "input" 20) (result 20)))
    (export (;7;) "process-word" (func 0) (func (type 30)))
    (export (;8;) "process-another-word" (func 1) (func (type 30)))
    (type (;31;) (func (param "input" 21) (result 21)))
    (export (;9;) "process-felt" (func 2) (func (type 31)))
    (type (;32;) (func (param "input" 23) (result 23)))
    (export (;10;) "process-pair" (func 3) (func (type 32)))
    (type (;33;) (func (param "input" 25) (result 25)))
    (export (;11;) "process-triple" (func 4) (func (type 33)))
    (type (;34;) (func (param "input" 27) (result 27)))
    (export (;12;) "process-mixed" (func 5) (func (type 34)))
    (type (;35;) (func (param "input" 29) (result 29)))
    (export (;13;) "process-nested" (func 6) (func (type 35)))
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
