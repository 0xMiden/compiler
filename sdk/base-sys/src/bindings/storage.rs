use miden_stdlib_sys::{Felt, Word};

use super::StorageCommitmentRoot;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::active_account::get_item"]
    pub fn extern_get_storage_item(index: Felt, ptr: *mut Word);

    #[link_name = "miden::protocol::active_account::get_initial_item"]
    pub fn extern_get_initial_storage_item(index: Felt, ptr: *mut Word);

    #[link_name = "miden::protocol::native_account::set_item"]
    pub fn extern_set_storage_item(
        index: Felt,
        v0: Felt,
        v1: Felt,
        v2: Felt,
        v3: Felt,
        ptr: *mut (StorageCommitmentRoot, Word),
    );

    #[link_name = "miden::protocol::active_account::get_map_item"]
    pub fn extern_get_storage_map_item(
        index: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        ptr: *mut Word,
    );

    #[link_name = "miden::protocol::active_account::get_initial_map_item"]
    pub fn extern_get_initial_storage_map_item(
        index: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        ptr: *mut Word,
    );

    #[link_name = "miden::protocol::native_account::set_map_item"]
    pub fn extern_set_storage_map_item(
        index: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        v0: Felt,
        v1: Felt,
        v2: Felt,
        v3: Felt,
        ptr: *mut (StorageCommitmentRoot, Word),
    );
}

/// Gets an item from the account storage.
///
/// Inputs: index
/// Outputs: value
///
/// Where:
/// - index is the index of the item to get.
/// - value is the value of the item.
///
/// Panics if:
/// - the index of the requested item is out of bounds.
#[inline]
pub fn get_item(index: u8) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_get_storage_item(index.into(), ret_area.as_mut_ptr());
        let word = ret_area.assume_init();
        word.reverse()
    }
}

/// Gets the initial value of an item from the account storage.
#[inline]
pub fn get_initial_item(index: u8) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_get_initial_storage_item(index.into(), ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Sets an item in the account storage.
///
/// Inputs: index, value
/// Outputs: (new_root, old_value)
///
/// Where:
/// - index is the index of the item to set.
/// - value is the value to set.
/// - new_root is the new storage commitment.
/// - old_value is the previous value of the item.
///
/// Panics if:
/// - the index of the item is out of bounds.
#[inline]
pub fn set_item(index: u8, value: Word) -> (StorageCommitmentRoot, Word) {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(StorageCommitmentRoot, Word)>::uninit();
        extern_set_storage_item(
            index.into(),
            value[3],
            value[2],
            value[1],
            value[0],
            ret_area.as_mut_ptr(),
        );
        let (comm, value) = ret_area.assume_init();
        (comm.reverse(), value.reverse())
    }
}

/// Gets a map item from the account storage.
///
/// Inputs: index, key
/// Outputs: value
///
/// Where:
/// - index is the index of the map where the key value should be read.
/// - key is the key of the item to get.
/// - value is the value of the item.
///
/// Panics if:
/// - the index for the map is out of bounds, meaning > 255.
/// - the slot item at index is not a map.
#[inline]
pub fn get_map_item(index: u8, key: &Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_get_storage_map_item(
            index.into(),
            key[3],
            key[2],
            key[1],
            key[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Gets the initial value from a storage map.
#[inline]
pub fn get_initial_map_item(index: u8, key: &Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_get_initial_storage_map_item(
            index.into(),
            key[3],
            key[2],
            key[1],
            key[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reverse()
    }
}

/// Sets a map item in the account storage.
///
/// Inputs: index, key, value
/// Outputs: (map_old_root, map_old_value)
///
/// Where:
/// - index is the index of the map where the key value should be set.
/// - key is the key to set.
/// - value is the value to set.
/// - map_old_root is the old map root.
/// - map_old_value is the old value at key.
///
/// Panics if:
/// - the index for the map is out of bounds, meaning > 255.
/// - the slot item at index is not a map.
#[inline]
pub fn set_map_item(index: u8, key: Word, value: Word) -> (StorageCommitmentRoot, Word) {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(StorageCommitmentRoot, Word)>::uninit();
        extern_set_storage_map_item(
            index.into(),
            key[3],
            key[2],
            key[1],
            key[0],
            value[3],
            value[2],
            value[1],
            value[0],
            ret_area.as_mut_ptr(),
        );
        let (comm, value) = ret_area.assume_init();
        (comm.reverse(), value.reverse())
    }
}
