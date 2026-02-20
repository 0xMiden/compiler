use miden_stdlib_sys::{Felt, Word};

use super::StorageSlotId;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::active_account::get_item"]
    pub fn extern_get_storage_item(index_prefix: Felt, index_suffix: Felt, ptr: *mut Word);

    #[link_name = "miden::protocol::active_account::get_initial_item"]
    pub fn extern_get_initial_storage_item(index_prefix: Felt, index_suffix: Felt, ptr: *mut Word);

    #[link_name = "miden::protocol::native_account::set_item"]
    pub fn extern_set_storage_item(
        index_prefix: Felt,
        index_suffix: Felt,
        v0: Felt,
        v1: Felt,
        v2: Felt,
        v3: Felt,
        ptr: *mut Word,
    );

    #[link_name = "miden::protocol::active_account::get_map_item"]
    pub fn extern_get_storage_map_item(
        index_prefix: Felt,
        index_suffix: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        ptr: *mut Word,
    );

    #[link_name = "miden::protocol::active_account::get_initial_map_item"]
    pub fn extern_get_initial_storage_map_item(
        index_prefix: Felt,
        index_suffix: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        ptr: *mut Word,
    );

    #[link_name = "miden::protocol::native_account::set_map_item"]
    pub fn extern_set_storage_map_item(
        index_prefix: Felt,
        index_suffix: Felt,
        k0: Felt,
        k1: Felt,
        k2: Felt,
        k3: Felt,
        v0: Felt,
        v1: Felt,
        v2: Felt,
        v3: Felt,
        ptr: *mut Word,
    );
}

/// Gets an item from the account storage.
///
/// Inputs: slot_id
/// Outputs: value
///
/// Where:
/// - slot_id identifies the storage slot to access.
/// - value is the value of the item.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
#[inline]
pub fn get_item(slot_id: StorageSlotId) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_get_storage_item(prefix, suffix, ret_area.as_mut_ptr());
        let word = ret_area.assume_init();
        word.reversed()
    }
}

/// Gets the initial value of an item from the account storage.
#[inline]
pub fn get_initial_item(slot_id: StorageSlotId) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_get_initial_storage_item(prefix, suffix, ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Sets an item in the account storage.
///
/// Inputs: slot_id, value
/// Outputs: old_value
///
/// Where:
/// - slot_id identifies the storage slot to update.
/// - value is the value to set.
/// - old_value is the previous value of the item.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
#[inline]
pub fn set_item(slot_id: StorageSlotId, value: Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_set_storage_item(
            prefix,
            suffix,
            value[3],
            value[2],
            value[1],
            value[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reversed()
    }
}

/// Gets a map item from the account storage.
///
/// Inputs: slot_id, key
/// Outputs: value
///
/// Where:
/// - slot_id identifies the map slot where the key should be read.
/// - key is the key of the item to get.
/// - value is the value of the item.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
/// - the slot content is not a map.
#[inline]
pub fn get_map_item(slot_id: StorageSlotId, key: &Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_get_storage_map_item(
            prefix,
            suffix,
            key[3],
            key[2],
            key[1],
            key[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reversed()
    }
}

/// Gets the initial value from a storage map.
#[inline]
pub fn get_initial_map_item(slot_id: StorageSlotId, key: &Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_get_initial_storage_map_item(
            prefix,
            suffix,
            key[3],
            key[2],
            key[1],
            key[0],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().reversed()
    }
}

/// Sets a map item in the account storage.
///
/// Inputs: slot_id, key, value
/// Outputs: old_value
///
/// Where:
/// - slot_id identifies the map slot where the key should be set.
/// - key is the key to set.
/// - value is the value to set.
/// - old_value is the old value at key.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
/// - the slot content is not a map.
#[inline]
pub fn set_map_item(slot_id: StorageSlotId, key: Word, value: Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let (prefix, suffix) = slot_id.to_prefix_suffix();
        extern_set_storage_map_item(
            prefix,
            suffix,
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
        ret_area.assume_init().reversed()
    }
}
