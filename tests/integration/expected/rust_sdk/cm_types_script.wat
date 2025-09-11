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
  (type (;1;)
    (instance
      (type (;0;) (flags "flag-a" "flag-b" "flag-c"))
      (export (;1;) "the-flags" (type (eq 0)))
      (type (;2;) (enum "variant-a" "variant-b" "variant-c"))
      (export (;3;) "the-enum" (type (eq 2)))
      (type (;4;) (option 3))
      (type (;5;) (record (field "rec-flags" 1) (field "optional-enum" 4)))
      (export (;6;) "the-record" (type (eq 5)))
      (type (;7;) (func (param "e" 3) (result 3)))
      (export (;0;) "func-enum" (func (type 7)))
      (type (;8;) (func (param "f" 1) (result 1)))
      (export (;1;) "func-flags" (func (type 8)))
      (type (;9;) (func (param "r" 6) (result 6)))
      (export (;2;) "func-record" (func (type 9)))
      (type (;10;) (option 6))
      (type (;11;) (func (param "o" 10) (result 4)))
      (export (;3;) "func-option" (func (type 11)))
      (type (;12;) (tuple 3 u64))
      (type (;13;) (tuple u64 6))
      (type (;14;) (func (param "t" 12) (result 13)))
      (export (;4;) "func-tuple" (func (type 14)))
      (type (;15;) (result u64 (error u32)))
      (type (;16;) (result u16 (error u8)))
      (type (;17;) (func (param "r" 15) (result 16)))
      (export (;5;) "func-result-small" (func (type 17)))
    )
  )
  (import "miden:cm-types/cm-types@0.1.0" (instance (;1;) (type 1)))
  (core module (;0;)
    (type (;0;) (func (param i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32 i32)))
    (type (;2;) (func (param i32 i64 i32)))
    (type (;3;) (func (param i32) (result i32)))
    (type (;4;) (func))
    (type (;5;) (func (param f32 f32 f32 f32)))
    (import "miden:cm-types/cm-types@0.1.0" "func-record" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_record::wit_import4 (;0;) (type 0)))
    (import "miden:cm-types/cm-types@0.1.0" "func-option" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_option::wit_import5 (;1;) (type 1)))
    (import "miden:cm-types/cm-types@0.1.0" "func-result-small" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_result_small::wit_import2 (;2;) (type 2)))
    (import "miden:cm-types/cm-types@0.1.0" "func-enum" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_enum::wit_import0 (;3;) (type 3)))
    (import "miden:cm-types/cm-types@0.1.0" "func-flags" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_flags::wit_import1 (;4;) (type 3)))
    (import "miden:cm-types/cm-types@0.1.0" "func-tuple" (func $cm_types_script::bindings::miden::cm_types::cm_types::func_tuple::wit_import2 (;5;) (type 2)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $cm_types_script::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;6;) (type 4))
    (func $cm_types_script::bindings::miden::cm_types::cm_types::func_record (;7;) (type 3) (param i32) (result i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 0
      i32.const 255
      i32.and
      i32.const 1
      i32.const 0
      local.get 1
      i32.const 13
      i32.add
      call $cm_types_script::bindings::miden::cm_types::cm_types::func_record::wit_import4
      block ;; label = @1
        block ;; label = @2
          local.get 1
          i32.load8_u offset=14
          br_if 0 (;@2;)
          i32.const 3
          local.set 0
          br 1 (;@1;)
        end
        local.get 1
        i32.load8_u offset=15
        local.set 0
      end
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 0
    )
    (func $cm_types_script::bindings::miden::cm_types::cm_types::func_option (;8;) (type 3) (param i32) (result i32)
      (local i32 i32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      i32.const 0
      local.set 2
      i32.const 0
      local.set 3
      i32.const 0
      local.set 4
      i32.const 0
      local.set 5
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 0
            i32.const 255
            i32.and
            local.tee 0
            i32.const -3
            i32.add
            br_table 1 (;@2;) 2 (;@1;) 0 (;@3;)
          end
          i32.const 2
          local.set 4
          i32.const 1
          local.set 3
          local.get 0
          local.set 2
          i32.const 1
          local.set 5
          br 1 (;@1;)
        end
        i32.const 1
        local.set 5
        i32.const 2
        local.set 4
        i32.const 0
        local.set 3
      end
      local.get 5
      local.get 4
      local.get 3
      local.get 2
      local.get 1
      i32.const 14
      i32.add
      call $cm_types_script::bindings::miden::cm_types::cm_types::func_option::wit_import5
      block ;; label = @1
        block ;; label = @2
          local.get 1
          i32.load8_u offset=14
          br_if 0 (;@2;)
          i32.const 3
          local.set 3
          br 1 (;@1;)
        end
        local.get 1
        i32.load8_u offset=15
        local.set 3
      end
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 3
    )
    (func $cm_types_script::bindings::miden::cm_types::cm_types::func_result_small (;9;) (type 3) (param i32) (result i32)
      (local i32 i32 i64 i64 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 0
      i32.load
      local.set 2
      local.get 0
      i64.load offset=8
      local.set 3
      local.get 0
      i64.load32_s offset=4
      local.set 4
      local.get 1
      i32.const 0
      i32.store offset=12
      local.get 2
      local.get 4
      local.get 3
      local.get 2
      select
      local.get 1
      i32.const 12
      i32.add
      call $cm_types_script::bindings::miden::cm_types::cm_types::func_result_small::wit_import2
      local.get 1
      i32.load16_u offset=14
      local.set 2
      local.get 1
      i32.load8_u offset=14
      local.set 5
      local.get 1
      i32.load8_u offset=12
      local.set 0
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 5
      i32.const 8
      i32.shl
      local.get 2
      i32.const 16
      i32.shl
      local.get 0
      select
      local.get 0
      i32.const 0
      i32.ne
      i32.or
    )
    (func $cm_types_script::bindings::__link_custom_section_describing_imports (;10;) (type 4))
    (func $miden:base/note-script@1.0.0#run (;11;) (type 5) (param f32 f32 f32 f32)
      (local i32 i32 i64)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      block ;; label = @1
        i32.const 1
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_enum::wit_import0
        i32.const 255
        i32.and
        i32.const 2
        i32.ne
        br_if 0 (;@1;)
        i32.const 5
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_flags::wit_import1
        i32.const 2
        i32.and
        i32.eqz
        br_if 0 (;@1;)
        i32.const 2
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_flags::wit_import1
        i32.const 255
        i32.and
        i32.const 4
        i32.ne
        br_if 0 (;@1;)
        block ;; label = @2
          i32.const 1
          call $cm_types_script::bindings::miden::cm_types::cm_types::func_record
          i32.const 255
          i32.and
          br_table 0 (;@2;) 1 (;@1;) 1 (;@1;) 1 (;@1;) 1 (;@1;)
        end
        i32.const 2
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_record
        i32.const 255
        i32.and
        i32.const 3
        i32.ne
        br_if 0 (;@1;)
        block ;; label = @2
          i32.const 1
          call $cm_types_script::bindings::miden::cm_types::cm_types::func_option
          i32.const 255
          i32.and
          i32.const -1
          i32.add
          br_table 0 (;@2;) 1 (;@1;) 1 (;@1;) 1 (;@1;)
        end
        i32.const 3
        local.set 5
        i32.const 4
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_option
        i32.const 255
        i32.and
        i32.const 3
        i32.ne
        br_if 0 (;@1;)
        i32.const 2
        i64.const 11
        local.get 4
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_tuple::wit_import2
        local.get 4
        i64.load
        local.set 6
        block ;; label = @2
          local.get 4
          i32.load8_u offset=9
          i32.eqz
          br_if 0 (;@2;)
          local.get 4
          i32.load8_u offset=10
          local.set 5
        end
        local.get 6
        i64.const 11
        i64.ne
        br_if 0 (;@1;)
        local.get 4
        i32.load8_u offset=8
        i32.const 255
        i32.and
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        block ;; label = @2
          local.get 5
          i32.const 255
          i32.and
          i32.const -2
          i32.add
          br_table 0 (;@2;) 1 (;@1;) 1 (;@1;)
        end
        local.get 4
        i32.const 0
        i32.store
        local.get 4
        i64.const 33
        i64.store offset=8
        local.get 4
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_result_small
        local.tee 5
        i32.const 255
        i32.and
        br_if 0 (;@1;)
        local.get 5
        i32.const -65536
        i32.and
        i32.const 1441792
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i64.const 188978561025
        i64.store
        local.get 4
        call $cm_types_script::bindings::miden::cm_types::cm_types::func_result_small
        local.tee 5
        i32.const 255
        i32.and
        i32.eqz
        br_if 0 (;@1;)
        local.get 5
        i32.const 1
        i32.and
        i32.eqz
        br_if 0 (;@1;)
        local.get 5
        i32.const 65280
        i32.and
        i32.const 16896
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i32.const 16
        i32.add
        global.set $__stack_pointer
        return
      end
      unreachable
    )
    (func $wit_bindgen::rt::run_ctors_once (;12;) (type 4)
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
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (core module (;1;)
    (type (;0;) (func (param i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32 i32)))
    (type (;2;) (func (param i32 i64 i32)))
    (table (;0;) 4 4 funcref)
    (export "0" (func $indirect-miden:cm-types/cm-types@0.1.0-func-record))
    (export "1" (func $indirect-miden:cm-types/cm-types@0.1.0-func-option))
    (export "2" (func $indirect-miden:cm-types/cm-types@0.1.0-func-result-small))
    (export "3" (func $indirect-miden:cm-types/cm-types@0.1.0-func-tuple))
    (export "$imports" (table 0))
    (func $indirect-miden:cm-types/cm-types@0.1.0-func-record (;0;) (type 0) (param i32 i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      i32.const 0
      call_indirect (type 0)
    )
    (func $indirect-miden:cm-types/cm-types@0.1.0-func-option (;1;) (type 1) (param i32 i32 i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      i32.const 1
      call_indirect (type 1)
    )
    (func $indirect-miden:cm-types/cm-types@0.1.0-func-result-small (;2;) (type 2) (param i32 i64 i32)
      local.get 0
      local.get 1
      local.get 2
      i32.const 2
      call_indirect (type 2)
    )
    (func $indirect-miden:cm-types/cm-types@0.1.0-func-tuple (;3;) (type 2) (param i32 i64 i32)
      local.get 0
      local.get 1
      local.get 2
      i32.const 3
      call_indirect (type 2)
    )
  )
  (core module (;2;)
    (type (;0;) (func (param i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32 i32)))
    (type (;2;) (func (param i32 i64 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "1" (func (;1;) (type 1)))
    (import "" "2" (func (;2;) (type 2)))
    (import "" "3" (func (;3;) (type 2)))
    (import "" "$imports" (table (;0;) 4 4 funcref))
    (elem (;0;) (i32.const 0) func 0 1 2 3)
  )
  (core instance (;0;) (instantiate 1))
  (alias export 0 "word" (type (;2;)))
  (alias core export 0 "0" (core func (;0;)))
  (alias core export 0 "1" (core func (;1;)))
  (alias core export 0 "2" (core func (;2;)))
  (alias export 1 "func-enum" (func (;0;)))
  (core func (;3;) (canon lower (func 0)))
  (alias export 1 "func-flags" (func (;1;)))
  (core func (;4;) (canon lower (func 1)))
  (alias core export 0 "3" (core func (;5;)))
  (core instance (;1;)
    (export "func-record" (func 0))
    (export "func-option" (func 1))
    (export "func-result-small" (func 2))
    (export "func-enum" (func 3))
    (export "func-flags" (func 4))
    (export "func-tuple" (func 5))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:cm-types/cm-types@0.1.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (alias core export 0 "$imports" (core table (;0;)))
  (alias export 1 "func-record" (func (;2;)))
  (core func (;6;) (canon lower (func 2) (memory 0)))
  (alias export 1 "func-option" (func (;3;)))
  (core func (;7;) (canon lower (func 3) (memory 0)))
  (alias export 1 "func-result-small" (func (;4;)))
  (core func (;8;) (canon lower (func 4) (memory 0)))
  (alias export 1 "func-tuple" (func (;5;)))
  (core func (;9;) (canon lower (func 5) (memory 0)))
  (core instance (;3;)
    (export "$imports" (table 0))
    (export "0" (func 6))
    (export "1" (func 7))
    (export "2" (func 8))
    (export "3" (func 9))
  )
  (core instance (;4;) (instantiate 2
      (with "" (instance 3))
    )
  )
  (type (;3;) (func (param "arg" 2)))
  (alias core export 2 "miden:base/note-script@1.0.0#run" (core func (;10;)))
  (func (;6;) (type 3) (canon lift (core func 10)))
  (alias export 0 "felt" (type (;4;)))
  (alias export 0 "word" (type (;5;)))
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
      (with "import-func-run" (func 6))
      (with "import-type-felt" (type 4))
      (with "import-type-word" (type 5))
      (with "import-type-word0" (type 2))
    )
  )
  (export (;3;) "miden:base/note-script@1.0.0" (instance 2))
)
