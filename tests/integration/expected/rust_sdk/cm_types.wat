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
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32) (result i32)))
    (type (;2;) (func (param i32 i32 i32) (result i32)))
    (type (;3;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;4;) (func (param i32 i64) (result i32)))
    (type (;5;) (func (param i32 i64 i32 i32) (result i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cm-types/cm-types@0.1.0#func-enum" (func $miden:cm-types/cm-types@0.1.0#func-enum))
    (export "miden:cm-types/cm-types@0.1.0#func-flags" (func $miden:cm-types/cm-types@0.1.0#func-flags))
    (export "miden:cm-types/cm-types@0.1.0#func-record" (func $miden:cm-types/cm-types@0.1.0#func-record))
    (export "miden:cm-types/cm-types@0.1.0#func-option" (func $miden:cm-types/cm-types@0.1.0#func-option))
    (export "miden:cm-types/cm-types@0.1.0#func-tuple" (func $miden:cm-types/cm-types@0.1.0#func-tuple))
    (export "miden:cm-types/cm-types@0.1.0#func-result-small" (func $miden:cm-types/cm-types@0.1.0#func-result-small))
    (export "miden:cm-types/cm-types@0.1.0#func-result-enum" (func $miden:cm-types/cm-types@0.1.0#func-result-enum))
    (export "miden:cm-types/cm-types@0.1.0#func-result-large" (func $miden:cm-types/cm-types@0.1.0#func-result-large))
    (elem (;0;) (i32.const 1) func $cm_types::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $cm_types::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:cm-types/cm-types@0.1.0#func-enum (;2;) (type 1) (param i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048576
      i32.add
      local.get 0
      i32.const 3
      i32.and
      i32.const 2
      i32.shl
      i32.add
      i32.load
      local.set 0
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
    )
    (func $miden:cm-types/cm-types@0.1.0#func-flags (;3;) (type 1) (param i32) (result i32)
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 253
      i32.and
      i32.const 2
      i32.or
      i32.const 4
      local.get 0
      i32.const 1
      i32.and
      select
    )
    (func $miden:cm-types/cm-types@0.1.0#func-record (;4;) (type 2) (param i32 i32 i32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 3
      call $wit_bindgen::rt::run_ctors_once
      local.get 3
      i32.const 1048600
      i32.add
      local.get 0
      i32.store8
      i32.const 0
      local.set 3
      block ;; label = @1
        local.get 0
        i32.const 2
        i32.and
        br_if 0 (;@1;)
        local.get 2
        i32.const 3
        local.get 1
        select
        local.tee 0
        i32.const 255
        i32.and
        i32.const 3
        i32.eq
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        local.get 0
        i32.store8 offset=2
        i32.const 1
        local.set 3
      end
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
      local.tee 0
      local.get 3
      i32.store8 offset=1
      local.get 0
    )
    (func $miden:cm-types/cm-types@0.1.0#func-option (;5;) (type 3) (param i32 i32 i32 i32) (result i32)
      (local i32)
      call $wit_bindgen::rt::run_ctors_once
      i32.const 0
      local.set 4
      block ;; label = @1
        local.get 0
        i32.eqz
        br_if 0 (;@1;)
        local.get 2
        i32.eqz
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        local.get 3
        i32.store8 offset=1
        i32.const 1
        local.set 4
      end
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
      local.tee 0
      local.get 4
      i32.store8
      local.get 0
    )
    (func $miden:cm-types/cm-types@0.1.0#func-tuple (;6;) (type 4) (param i32 i64) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 2
      call $wit_bindgen::rt::run_ctors_once
      local.get 2
      i32.const 1048600
      i32.add
      local.tee 2
      local.get 0
      i32.store8 offset=10
      local.get 2
      i32.const 257
      i32.store16 offset=8
      local.get 2
      local.get 1
      i64.store
      local.get 2
    )
    (func $miden:cm-types/cm-types@0.1.0#func-result-small (;7;) (type 4) (param i32 i64) (result i32)
      (local i32 i32)
      call $wit_bindgen::rt::run_ctors_once
      local.get 1
      i32.wrap_i64
      local.set 2
      block ;; label = @1
        block ;; label = @2
          local.get 0
          i32.eqz
          br_if 0 (;@2;)
          local.get 2
          i32.const 8
          i32.shl
          i32.const 5632
          i32.add
          i32.const 65280
          i32.and
          local.set 0
          i32.const 1
          local.set 3
          br 1 (;@1;)
        end
        local.get 2
        i32.const 16
        i32.shl
        i32.const -720896
        i32.add
        local.set 0
        i32.const 0
        local.set 3
      end
      i32.const 1
      local.set 2
      block ;; label = @1
        block ;; label = @2
          local.get 0
          local.get 3
          i32.or
          i32.const 1
          i32.and
          i32.eqz
          br_if 0 (;@2;)
          global.get $GOT.data.internal.__memory_base
          i32.const 1048600
          i32.add
          local.get 0
          i32.const 8
          i32.shr_u
          i32.store8 offset=2
          br 1 (;@1;)
        end
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        local.get 0
        i32.const 16
        i32.shr_u
        i32.store16 offset=2
        i32.const 0
        local.set 2
      end
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
      local.tee 0
      local.get 2
      i32.store8
      local.get 0
    )
    (func $miden:cm-types/cm-types@0.1.0#func-result-enum (;8;) (type 1) (param i32) (result i32)
      (local i32)
      call $wit_bindgen::rt::run_ctors_once
      i32.const 1
      local.set 1
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            block ;; label = @4
              local.get 0
              i32.const 255
              i32.and
              br_table 1 (;@3;) 2 (;@2;) 0 (;@4;) 1 (;@3;)
            end
            i32.const 0
            local.set 1
          end
          global.get $GOT.data.internal.__memory_base
          i32.const 1048600
          i32.add
          i32.const 1
          i32.store8
          br 1 (;@1;)
        end
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        i32.const 0
        i32.store8
        i32.const 1
        local.set 1
      end
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
      local.tee 0
      local.get 1
      i32.store8 offset=1
      local.get 0
    )
    (func $miden:cm-types/cm-types@0.1.0#func-result-large (;9;) (type 5) (param i32 i64 i32 i32) (result i32)
      call $wit_bindgen::rt::run_ctors_once
      block ;; label = @1
        block ;; label = @2
          local.get 0
          i32.eqz
          br_if 0 (;@2;)
          global.get $GOT.data.internal.__memory_base
          i32.const 1048600
          i32.add
          i32.const 1
          i32.store8
          br 1 (;@1;)
        end
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        local.tee 0
        local.get 2
        i64.extend_i32_u
        i64.store offset=24
        local.get 0
        local.get 1
        i64.const 22
        i64.add
        i64.store offset=16
        local.get 0
        local.get 1
        i64.const 11
        i64.add
        i64.store offset=8
        local.get 0
        i32.const 0
        i32.store8
      end
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
    )
    (func $wit_bindgen::rt::run_ctors_once (;10;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048632
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048632
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\02\00\00\00\00\00\00\00")
    (data $.data (;1;) (i32.const 1048588) "\01\00\00\00\01\00\00\00")
  )
  (alias export 0 "felt" (type (;1;)))
  (alias export 0 "asset" (type (;2;)))
  (type (;3;) (enum "variant-a" "variant-b" "variant-c"))
  (type (;4;) (flags "flag-a" "flag-b" "flag-c"))
  (type (;5;) (option 3))
  (type (;6;) (record (field "rec-flags" 4) (field "optional-enum" 5)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;7;) (func (param "e" 3) (result 3)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-enum" (core func (;0;)))
  (func (;0;) (type 7) (canon lift (core func 0)))
  (type (;8;) (func (param "f" 4) (result 4)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-flags" (core func (;1;)))
  (func (;1;) (type 8) (canon lift (core func 1)))
  (type (;9;) (func (param "r" 6) (result 6)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-record" (core func (;2;)))
  (func (;2;) (type 9) (canon lift (core func 2) (memory 0)))
  (type (;10;) (option 6))
  (type (;11;) (func (param "o" 10) (result 5)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-option" (core func (;3;)))
  (func (;3;) (type 11) (canon lift (core func 3) (memory 0)))
  (type (;12;) (tuple 3 u64))
  (type (;13;) (tuple u64 6))
  (type (;14;) (func (param "t" 12) (result 13)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-tuple" (core func (;4;)))
  (func (;4;) (type 14) (canon lift (core func 4) (memory 0)))
  (type (;15;) (result u64 (error u32)))
  (type (;16;) (result u16 (error u8)))
  (type (;17;) (func (param "r" 15) (result 16)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-result-small" (core func (;5;)))
  (func (;5;) (type 17) (canon lift (core func 5) (memory 0)))
  (type (;18;) (result 3 (error bool)))
  (type (;19;) (func (param "e" 3) (result 18)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-result-enum" (core func (;6;)))
  (func (;6;) (type 19) (canon lift (core func 6) (memory 0)))
  (type (;20;) (tuple u64 u32))
  (type (;21;) (result 20 (error 6)))
  (type (;22;) (tuple u64 u64 u64))
  (type (;23;) (result 22))
  (type (;24;) (func (param "r" 21) (result 23)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-result-large" (core func (;7;)))
  (func (;7;) (type 24) (canon lift (core func 7) (memory 0)))
  (alias export 0 "felt" (type (;25;)))
  (alias export 0 "word" (type (;26;)))
  (alias export 0 "asset" (type (;27;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (type (;7;) (enum "variant-a" "variant-b" "variant-c"))
    (import "import-type-the-enum" (type (;8;) (eq 7)))
    (type (;9;) (func (param "e" 8) (result 8)))
    (import "import-func-func-enum" (func (;0;) (type 9)))
    (type (;10;) (flags "flag-a" "flag-b" "flag-c"))
    (import "import-type-the-flags" (type (;11;) (eq 10)))
    (type (;12;) (func (param "f" 11) (result 11)))
    (import "import-func-func-flags" (func (;1;) (type 12)))
    (type (;13;) (option 8))
    (type (;14;) (record (field "rec-flags" 11) (field "optional-enum" 13)))
    (import "import-type-the-record" (type (;15;) (eq 14)))
    (type (;16;) (func (param "r" 15) (result 15)))
    (import "import-func-func-record" (func (;2;) (type 16)))
    (type (;17;) (option 15))
    (type (;18;) (func (param "o" 17) (result 13)))
    (import "import-func-func-option" (func (;3;) (type 18)))
    (type (;19;) (tuple 8 u64))
    (type (;20;) (tuple u64 15))
    (type (;21;) (func (param "t" 19) (result 20)))
    (import "import-func-func-tuple" (func (;4;) (type 21)))
    (type (;22;) (result u64 (error u32)))
    (type (;23;) (result u16 (error u8)))
    (type (;24;) (func (param "r" 22) (result 23)))
    (import "import-func-func-result-small" (func (;5;) (type 24)))
    (type (;25;) (result 8 (error bool)))
    (type (;26;) (func (param "e" 8) (result 25)))
    (import "import-func-func-result-enum" (func (;6;) (type 26)))
    (type (;27;) (tuple u64 u32))
    (type (;28;) (result 27 (error 15)))
    (type (;29;) (tuple u64 u64 u64))
    (type (;30;) (result 29))
    (type (;31;) (func (param "r" 28) (result 30)))
    (import "import-func-func-result-large" (func (;7;) (type 31)))
    (export (;32;) "felt" (type 1))
    (export (;33;) "asset" (type 6))
    (type (;34;) (enum "variant-a" "variant-b" "variant-c"))
    (export (;35;) "the-enum" (type 34))
    (type (;36;) (flags "flag-a" "flag-b" "flag-c"))
    (export (;37;) "the-flags" (type 36))
    (type (;38;) (option 35))
    (type (;39;) (record (field "rec-flags" 37) (field "optional-enum" 38)))
    (export (;40;) "the-record" (type 39))
    (type (;41;) (func (param "e" 35) (result 35)))
    (export (;8;) "func-enum" (func 0) (func (type 41)))
    (type (;42;) (func (param "f" 37) (result 37)))
    (export (;9;) "func-flags" (func 1) (func (type 42)))
    (type (;43;) (func (param "r" 40) (result 40)))
    (export (;10;) "func-record" (func 2) (func (type 43)))
    (type (;44;) (option 40))
    (type (;45;) (func (param "o" 44) (result 38)))
    (export (;11;) "func-option" (func 3) (func (type 45)))
    (type (;46;) (tuple 35 u64))
    (type (;47;) (tuple u64 40))
    (type (;48;) (func (param "t" 46) (result 47)))
    (export (;12;) "func-tuple" (func 4) (func (type 48)))
    (type (;49;) (result u64 (error u32)))
    (type (;50;) (result u16 (error u8)))
    (type (;51;) (func (param "r" 49) (result 50)))
    (export (;13;) "func-result-small" (func 5) (func (type 51)))
    (type (;52;) (result 35 (error bool)))
    (type (;53;) (func (param "e" 35) (result 52)))
    (export (;14;) "func-result-enum" (func 6) (func (type 53)))
    (type (;54;) (tuple u64 u32))
    (type (;55;) (result 54 (error 40)))
    (type (;56;) (tuple u64 u64 u64))
    (type (;57;) (result 56))
    (type (;58;) (func (param "r" 55) (result 57)))
    (export (;15;) "func-result-large" (func 7) (func (type 58)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-func-enum" (func 0))
      (with "import-func-func-flags" (func 1))
      (with "import-func-func-record" (func 2))
      (with "import-func-func-option" (func 3))
      (with "import-func-func-tuple" (func 4))
      (with "import-func-func-result-small" (func 5))
      (with "import-func-func-result-enum" (func 6))
      (with "import-func-func-result-large" (func 7))
      (with "import-type-felt" (type 25))
      (with "import-type-word" (type 26))
      (with "import-type-asset" (type 27))
      (with "import-type-the-enum" (type 3))
      (with "import-type-the-flags" (type 4))
      (with "import-type-the-record" (type 6))
    )
  )
  (export (;2;) "miden:cm-types/cm-types@0.1.0" (instance 1))
)
