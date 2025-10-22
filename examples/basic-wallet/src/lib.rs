// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use miden::{account, component, export_type, tx, Asset, Felt, NoteIdx, Word};

#[export_type]
enum EnumA {
    VariantA,
    VariantB,
}

#[export_type]
struct StructA {
    foo: Word,
    an_enum: EnumA,
}

#[export_type]
struct StructB {
    bar: Felt,
    baz: Felt,
}

#[export_type]
struct StructC {
    c_inner: Felt,
}

#[component]
struct MyAccount;

#[component]
impl MyAccount {
    /// Adds an asset to the account.
    ///
    /// This function adds the specified asset to the account's asset list.
    ///
    /// # Arguments
    /// * `asset` - The asset to be added to the account
    pub fn receive_asset(&self, asset: Asset) {
        account::add_asset(asset);
    }

    /// Moves an asset from the account to a note.
    ///
    /// This function removes the specified asset from the account and adds it to
    /// the note identified by the given index.
    ///
    /// # Arguments
    /// * `asset` - The asset to move from the account to the note
    /// * `note_idx` - The index of the note to receive the asset
    pub fn move_asset_to_note(&self, asset: Asset, note_idx: NoteIdx) {
        let asset = account::remove_asset(asset);
        tx::add_asset_to_note(asset, note_idx);
    }

    pub fn test_custom_types(&self, a: StructA, asset: Asset) -> StructB {
        StructB {
            bar: a.foo.inner.0,
            baz: asset.inner.inner.0,
        }
    }

    fn test_custom_types_private(&self, a: StructA, _b: EnumA, _asset: Asset) -> StructB {
        StructB {
            bar: a.foo.inner.0,
            baz: a.foo.inner.1,
        }
    }

    fn test_exported_type_in_private_method(&self, c: StructC) -> StructB {
        StructB {
            bar: c.c_inner,
            baz: c.c_inner,
        }
    }
}
