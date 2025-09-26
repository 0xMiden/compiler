#![no_std]
#![deny(warnings)]

pub use miden_base::*;
pub use miden_base_sys::bindings::*;
pub use miden_sdk_alloc::BumpAlloc;
pub use miden_stdlib_sys::*;
// pub use wit_bindgen_rt;
pub use wit_bindgen;

#[macro_export]
macro_rules! miden_generate {
    () => {
        $crate::wit_bindgen::generate!({
            // path: $path,
            // default_bindings_module: $crate::__miden_select_module!($($module)?),
            // runtime_path: "::miden::wit_bindgen_rt",
            with: {
                "miden:base/core-types@1.0.0": generate,
                "miden:base/core-types@1.0.0/felt": ::miden::Felt,
                "miden:base/core-types@1.0.0/word": ::miden::Word,
                "miden:base/core-types@1.0.0/asset": ::miden::Asset,
                "miden:base/core-types@1.0.0/account-id": ::miden::AccountId,
                "miden:base/core-types@1.0.0/tag": ::miden::Tag,
                "miden:base/core-types@1.0.0/note-type": ::miden::NoteType,
                "miden:base/core-types@1.0.0/recipient": ::miden::Recipient,
                "miden:base/core-types@1.0.0/note-idx": ::miden::NoteIdx,
            },
        });
    };
}
