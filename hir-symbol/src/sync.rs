#[cfg(all(not(feature = "std"), target_family = "wasm"))]
mod lazy_lock;
#[cfg(all(not(feature = "std"), target_family = "wasm"))]
mod rw_lock;

#[cfg(feature = "std")]
pub use std::sync::LazyLock;

#[cfg(feature = "std")]
#[allow(unused)]
pub use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(all(not(feature = "std"), target_family = "wasm"))]
pub use self::lazy_lock::RacyLock as LazyLock;
#[cfg(all(not(feature = "std"), target_family = "wasm"))]
#[allow(unused)]
pub use self::rw_lock::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(all(not(feature = "std"), not(target_family = "wasm")))]
compile_error!("no_std builds of this crate are only supported on wasm targets");
