use core::ffi::c_void;

/// Stubs for std::mem procedures used via SDK.
define_stub! {
    #[unsafe(export_name = "miden::core::mem::pipe_words_to_memory")]
    pub extern "C" fn std_mem_pipe_words_to_memory_stub(
        num_words: f32,
        write_ptr: *mut c_void,
        out_ptr: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::core::mem::pipe_double_words_to_memory")]
    pub extern "C" fn std_mem_pipe_double_words_to_memory_stub(
        r00: f32,
        r01: f32,
        r02: f32,
        r03: f32,
        r10: f32,
        r11: f32,
        r12: f32,
        r13: f32,
        c0: f32,
        c1: f32,
        c2: f32,
        c3: f32,
        write_ptr: *mut c_void,
        end_ptr: *mut c_void,
        out_ptr: *mut c_void,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::core::mem::pipe_preimage_to_memory")]
    pub extern "C" fn std_mem_pipe_preimage_to_memory_stub(
        num_words: f32,
        write_ptr: *mut c_void,
        c0: f32,
        c1: f32,
        c2: f32,
        c3: f32,
    ) -> i32;
}
