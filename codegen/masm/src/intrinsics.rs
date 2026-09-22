use alloc::sync::Arc;

use miden_mast_package::Package;
use miden_utils_sync::LazyLock;

const INTRINSICS_PACKAGE_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compiler-intrinsics.masp"));

static INTRINSICS: LazyLock<Arc<Package>> = LazyLock::new(|| {
    Package::read_from_bytes_trusted(INTRINSICS_PACKAGE_BYTES)
        .map(Arc::new)
        .expect("failed to read compiler-intrinsics!")
});

/// Load the compiler-intrinsics package for use in assembly
pub fn load() -> Arc<Package> {
    INTRINSICS.clone()
}
