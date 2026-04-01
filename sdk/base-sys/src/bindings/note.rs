extern crate alloc;

use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word};

use super::Recipient;

const MAX_NOTE_STORAGE_ITEMS: usize = 1024;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::note::build_recipient"]
    fn extern_note_build_recipient(
        storage_ptr: *mut Felt,
        num_storage_items: usize,
        serial_num_f0: Felt,
        serial_num_f1: Felt,
        serial_num_f2: Felt,
        serial_num_f3: Felt,
        script_root_f0: Felt,
        script_root_f1: Felt,
        script_root_f2: Felt,
        script_root_f3: Felt,
        ptr: *mut Recipient,
    );
}

/// Builds a note recipient from the provided serial number, script root, and storage elements.
///
/// This maps to `miden::protocol::note::build_recipient`, which also inserts the provided storage
/// into the advice map under the storage commitment used by the returned recipient digest.
///
/// Panics if `storage` contains more than 1024 elements.
pub fn build_recipient(serial_num: Word, script_root: Word, storage: Vec<Felt>) -> Recipient {
    assert!(
        storage.len() <= MAX_NOTE_STORAGE_ITEMS,
        "note storage cannot contain more than {MAX_NOTE_STORAGE_ITEMS} items"
    );

    let rust_ptr = if storage.is_empty() {
        0
    } else {
        storage.as_ptr().addr() as u32
    };
    let miden_ptr = rust_ptr / 4;

    // Vec storage comes from the SDK allocator, which only produces word-aligned pointers.
    assert_eq!(miden_ptr % 4, 0, "storage pointer must be word-aligned");

    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_note_build_recipient(
            miden_ptr as *mut Felt,
            storage.len(),
            serial_num[0],
            serial_num[1],
            serial_num[2],
            serial_num[3],
            script_root[0],
            script_root[1],
            script_root[2],
            script_root[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}
