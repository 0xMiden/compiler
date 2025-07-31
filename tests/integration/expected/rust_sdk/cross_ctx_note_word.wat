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
  (alias export 0 "word" (type (;1;)))
  (alias export 0 "felt" (type (;2;)))
  (type (;3;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "word" (type (eq 0)))
      (alias outer 1 2 (type (;2;)))
      (export (;3;) "felt" (type (eq 2)))
      (type (;4;) (record (field "first" 3) (field "second" 3)))
      (export (;5;) "pair" (type (eq 4)))
      (type (;6;) (record (field "x" 3) (field "y" 3) (field "z" 3)))
      (export (;7;) "triple" (type (eq 6)))
      (type (;8;) (record (field "f" u64) (field "a" 3) (field "b" u32) (field "c" 3) (field "d" u8) (field "e" bool) (field "g" u16)))
      (export (;9;) "mixed-struct" (type (eq 8)))
      (type (;10;) (record (field "inner" 5) (field "value" 3)))
      (export (;11;) "nested-struct" (type (eq 10)))
      (type (;12;) (func (param "input" 1) (result 1)))
      (export (;0;) "process-word" (func (type 12)))
      (export (;1;) "process-another-word" (func (type 12)))
      (type (;13;) (func (param "input" 3) (result 3)))
      (export (;2;) "process-felt" (func (type 13)))
      (type (;14;) (func (param "input" 5) (result 5)))
      (export (;3;) "process-pair" (func (type 14)))
      (type (;15;) (func (param "input" 7) (result 7)))
      (export (;4;) "process-triple" (func (type 15)))
      (type (;16;) (func (param "input" 9) (result 9)))
      (export (;5;) "process-mixed" (func (type 16)))
      (type (;17;) (func (param "input" 11) (result 11)))
      (export (;6;) "process-nested" (func (type 17)))
    )
  )
  (import "miden:cross-ctx-account-word/foo@1.0.0" (instance (;1;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (param "a" u64) (result f32)))
      (export (;0;) "from-u64-unchecked" (func (type 0)))
      (type (;1;) (func (param "a" u32) (result f32)))
      (export (;1;) "from-u32" (func (type 1)))
      (type (;2;) (func (param "a" f32) (param "b" f32)))
      (export (;2;) "assert-eq" (func (type 2)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;2;) (type 4)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32 f32 f32 i32)))
    (type (;2;) (func (param f32 f32)))
    (type (;3;) (func (param f32) (result f32)))
    (type (;4;) (func (param f32 f32 i32)))
    (type (;5;) (func (param f32 f32 f32 i32)))
    (type (;6;) (func (param i64) (result f32)))
    (type (;7;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (type (;8;) (func))
    (type (;9;) (func (param f32 f32 f32 f32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-word" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_word::wit_import7 (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;2;) (type 2)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-another-word" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_another_word::wit_import7 (;3;) (type 1)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-felt" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_felt::wit_import1 (;4;) (type 3)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-pair" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_pair::wit_import4 (;5;) (type 4)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-triple" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_triple::wit_import5 (;6;) (type 5)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u64-unchecked" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked (;7;) (type 6)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-mixed" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_mixed::wit_import4 (;8;) (type 7)))
    (import "miden:cross-ctx-account-word/foo@1.0.0" "process-nested" (func $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_nested::wit_import6 (;9;) (type 5)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/script@1.0.0#script" (func $miden:base/script@1.0.0#script))
    (elem (;0;) (i32.const 1) func $cross_ctx_note_word::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;10;) (type 8))
    (func $cross_ctx_note_word::bindings::__link_custom_section_describing_imports (;11;) (type 8))
    (func $miden:base/script@1.0.0#script (;12;) (type 9) (param f32 f32 f32 f32)
      (local i32 f32 f32 f32 f32 f32 f32 f32 i32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      i32.const 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.tee 5
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.tee 6
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.tee 7
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.tee 8
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_word::wit_import7
      local.get 4
      f32.load offset=20
      local.set 9
      local.get 4
      f32.load offset=16
      local.set 10
      local.get 4
      f32.load offset=12
      local.set 11
      local.get 4
      f32.load offset=8
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 11
      i32.const 5
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 10
      i32.const 7
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 9
      i32.const 9
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      local.get 6
      local.get 7
      local.get 8
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_another_word::wit_import7
      local.get 4
      f32.load offset=20
      local.set 5
      local.get 4
      f32.load offset=16
      local.set 6
      local.get 4
      f32.load offset=12
      local.set 7
      local.get 4
      f32.load offset=8
      i32.const 4
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 7
      i32.const 6
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 6
      i32.const 8
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const 10
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      i32.const 9
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_felt::wit_import1
      i32.const 12
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      i32.const 10
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 5
      i32.const 20
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 6
      local.get 4
      i64.const 0
      i64.store offset=8
      local.get 5
      local.get 6
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_pair::wit_import4
      local.get 4
      f32.load offset=12
      local.set 5
      local.get 4
      f32.load offset=8
      i32.const 14
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const 24
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      i32.const 100
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 200
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 300
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_triple::wit_import5
      local.get 4
      f32.load offset=16
      local.set 5
      local.get 4
      f32.load offset=12
      local.set 6
      local.get 4
      f32.load offset=8
      i32.const 105
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 6
      i32.const 205
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const 305
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      i64.const -1001
      i64.const -4294967302
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      i32.const -11
      i32.const 50
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 111
      i32.const 0
      i32.const 3
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_mixed::wit_import4
      block ;; label = @1
        local.get 4
        i64.load offset=8
        i64.const -1
        i64.eq
        br_if 0 (;@1;)
        unreachable
      end
      local.get 4
      i32.load16_u offset=30
      local.set 12
      local.get 4
      i32.load8_u offset=29
      local.set 13
      local.get 4
      i32.load8_u offset=28
      local.set 14
      local.get 4
      f32.load offset=24
      local.set 5
      local.get 4
      i32.load offset=20
      local.set 15
      local.get 4
      f32.load offset=16
      i64.const -4294967296
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 15
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u32>>::from
      i32.const -1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const 57
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 14
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      i32.const 122
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 13
      i32.const 255
      i32.and
      i32.const 0
      i32.ne
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 12
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u16>>::from
      i32.const 12
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      i32.const 30
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 40
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 50
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.get 4
      i32.const 8
      i32.add
      call $cross_ctx_note_word::bindings::miden::cross_ctx_account_word::foo::process_nested::wit_import6
      local.get 4
      f32.load offset=16
      local.set 5
      local.get 4
      f32.load offset=12
      local.set 6
      local.get 4
      f32.load offset=8
      i32.const 38
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 6
      i32.const 48
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 5
      i32.const 58
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen_rt::run_ctors_once (;13;) (type 8)
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
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u32>>::from (;14;) (type 0) (param i32) (result f32)
      local.get 0
      f32.reinterpret_i32
    )
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u16>>::from (;15;) (type 0) (param i32) (result f32)
      local.get 0
      i32.const 65535
      i32.and
      f32.reinterpret_i32
    )
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from (;16;) (type 0) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (core module (;1;)
    (type (;0;) (func (param f32 f32 f32 f32 i32)))
    (type (;1;) (func (param f32 f32 i32)))
    (type (;2;) (func (param f32 f32 f32 i32)))
    (type (;3;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (table (;0;) 6 6 funcref)
    (export "0" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-word))
    (export "1" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-another-word))
    (export "2" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-pair))
    (export "3" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-triple))
    (export "4" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-mixed))
    (export "5" (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-nested))
    (export "$imports" (table 0))
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-word (;0;) (type 0) (param f32 f32 f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      i32.const 0
      call_indirect (type 0)
    )
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-another-word (;1;) (type 0) (param f32 f32 f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      i32.const 1
      call_indirect (type 0)
    )
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-pair (;2;) (type 1) (param f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      i32.const 2
      call_indirect (type 1)
    )
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-triple (;3;) (type 2) (param f32 f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      i32.const 3
      call_indirect (type 2)
    )
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-mixed (;4;) (type 3) (param i64 f32 i32 f32 i32 i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      local.get 5
      local.get 6
      local.get 7
      i32.const 4
      call_indirect (type 3)
    )
    (func $indirect-miden:cross-ctx-account-word/foo@1.0.0-process-nested (;5;) (type 2) (param f32 f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      i32.const 5
      call_indirect (type 2)
    )
  )
  (core module (;2;)
    (type (;0;) (func (param f32 f32 f32 f32 i32)))
    (type (;1;) (func (param f32 f32 i32)))
    (type (;2;) (func (param f32 f32 f32 i32)))
    (type (;3;) (func (param i64 f32 i32 f32 i32 i32 i32 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "1" (func (;1;) (type 0)))
    (import "" "2" (func (;2;) (type 1)))
    (import "" "3" (func (;3;) (type 2)))
    (import "" "4" (func (;4;) (type 3)))
    (import "" "5" (func (;5;) (type 2)))
    (import "" "$imports" (table (;0;) 6 6 funcref))
    (elem (;0;) (i32.const 0) func 0 1 2 3 4 5)
  )
  (core instance (;0;) (instantiate 1))
  (alias export 0 "word" (type (;5;)))
  (alias export 2 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 2 "assert-eq" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (alias export 2 "from-u64-unchecked" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;1;)
    (export "from-u32" (func 0))
    (export "assert-eq" (func 1))
    (export "from-u64-unchecked" (func 2))
  )
  (alias core export 0 "0" (core func (;3;)))
  (alias core export 0 "1" (core func (;4;)))
  (alias export 1 "process-felt" (func (;3;)))
  (core func (;5;) (canon lower (func 3)))
  (alias core export 0 "2" (core func (;6;)))
  (alias core export 0 "3" (core func (;7;)))
  (alias core export 0 "4" (core func (;8;)))
  (alias core export 0 "5" (core func (;9;)))
  (core instance (;2;)
    (export "process-word" (func 3))
    (export "process-another-word" (func 4))
    (export "process-felt" (func 5))
    (export "process-pair" (func 6))
    (export "process-triple" (func 7))
    (export "process-mixed" (func 8))
    (export "process-nested" (func 9))
  )
  (core instance (;3;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 1))
      (with "miden:cross-ctx-account-word/foo@1.0.0" (instance 2))
    )
  )
  (alias core export 3 "memory" (core memory (;0;)))
  (alias core export 0 "$imports" (core table (;0;)))
  (alias export 1 "process-word" (func (;4;)))
  (core func (;10;) (canon lower (func 4) (memory 0)))
  (alias export 1 "process-another-word" (func (;5;)))
  (core func (;11;) (canon lower (func 5) (memory 0)))
  (alias export 1 "process-pair" (func (;6;)))
  (core func (;12;) (canon lower (func 6) (memory 0)))
  (alias export 1 "process-triple" (func (;7;)))
  (core func (;13;) (canon lower (func 7) (memory 0)))
  (alias export 1 "process-mixed" (func (;8;)))
  (core func (;14;) (canon lower (func 8) (memory 0)))
  (alias export 1 "process-nested" (func (;9;)))
  (core func (;15;) (canon lower (func 9) (memory 0)))
  (core instance (;4;)
    (export "$imports" (table 0))
    (export "0" (func 10))
    (export "1" (func 11))
    (export "2" (func 12))
    (export "3" (func 13))
    (export "4" (func 14))
    (export "5" (func 15))
  )
  (core instance (;5;) (instantiate 2
      (with "" (instance 4))
    )
  )
  (type (;6;) (func (param "arg" 5)))
  (alias core export 3 "miden:base/script@1.0.0#script" (core func (;16;)))
  (func (;10;) (type 6) (canon lift (core func 16)))
  (alias export 0 "felt" (type (;7;)))
  (alias export 0 "word" (type (;8;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-script" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "script" (func 0) (func (type 8)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-script" (func 10))
      (with "import-type-felt" (type 7))
      (with "import-type-word" (type 8))
      (with "import-type-word0" (type 5))
    )
  )
  (export (;4;) "miden:base/script@1.0.0" (instance 3))
)
