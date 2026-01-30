use core::ffi::c_void;

/// Unreachable stubs for std::mem procedures used via SDK

#[unsafe(export_name = "miden::core::mem::pipe_words_to_memory")]
pub extern "C" fn std_mem_pipe_words_to_memory_stub(
    _num_words: f32,
    _write_ptr: *mut c_void,
    _out_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::core::mem::pipe_double_words_to_memory")]
pub extern "C" fn std_mem_pipe_double_words_to_memory_stub(
    _c0: f32,
    _c1: f32,
    _c2: f32,
    _c3: f32,
    _b0: f32,
    _b1: f32,
    _b2: f32,
    _b3: f32,
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _write_ptr: *mut c_void,
    _end_ptr: *mut c_void,
    _out_ptr: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::core::mem::pipe_preimage_to_memory")]
pub extern "C" fn std_mem_pipe_preimage_to_memory_stub(
    _num_words: f32,
    _write_ptr: *mut c_void,
    _c0: f32,
    _c1: f32,
    _c2: f32,
    _c3: f32,
) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}
