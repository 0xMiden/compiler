use miden_stdlib_sys::Word;

use super::StorageSlotId;
use crate::raw::protocol::{active_account, native_account};

/// Gets an item from the account storage.
///
/// Inputs: slot_id
/// Outputs: value
///
/// Where:
/// - slot_id identifies the storage slot to access using the public `(prefix, suffix)` shape.
/// - value is the value of the item.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
#[inline]
pub fn get_item(slot_id: StorageSlotId) -> Word {
    active_account::get_item(slot_id.into())
}

/// Gets the initial value of an item from the account storage.
#[inline]
pub fn get_initial_item(slot_id: StorageSlotId) -> Word {
    native_account::get_initial_item(slot_id.into())
}

/// Sets an item in the account storage.
///
/// Inputs: slot_id, value
/// Outputs: old_value
///
/// Where:
/// - slot_id identifies the storage slot to update using the public `(prefix, suffix)` shape.
/// - value is the value to set.
/// - old_value is the previous value of the item.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
#[inline]
pub fn set_item(slot_id: StorageSlotId, value: Word) -> Word {
    native_account::set_item(slot_id.into(), value)
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
    active_account::get_map_item(slot_id.into(), *key)
}

/// Gets the initial value from a storage map.
#[inline]
pub fn get_initial_map_item(slot_id: StorageSlotId, key: &Word) -> Word {
    native_account::get_initial_map_item(slot_id.into(), *key)
}

/// Sets a map item in the account storage.
///
/// Inputs: slot_id, key, value
/// Outputs: old_value
///
/// Where:
/// - slot_id identifies the map slot where the key should be set using the public `(prefix,
///   suffix)` shape.
/// - key is the key to set.
/// - value is the value to set.
/// - old_value is the old value at key.
///
/// Panics if:
/// - the requested slot does not exist in the account storage.
/// - the slot content is not a map.
#[inline]
pub fn set_map_item(slot_id: StorageSlotId, key: Word, value: Word) -> Word {
    native_account::set_map_item(slot_id.into(), key, value)
}

/// Returns `true` if the active account has a storage slot with the given slot id.
///
/// Inputs: slot_id
/// Outputs: has_slot
///
/// Where:
/// - slot_id identifies the storage slot to probe using the public `(prefix, suffix)` shape.
/// - has_slot is `true` when a slot with that id exists on the active account.
#[inline]
pub fn has_storage_slot(slot_id: StorageSlotId) -> bool {
    active_account::has_storage_slot(slot_id.into())
}
