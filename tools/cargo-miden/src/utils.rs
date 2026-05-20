use std::path::{Path, PathBuf};

pub(crate) fn set_default_test_compiler(define: &mut Vec<String>) {
    let compiler_path = compiler_path();
    define.push(format!("compiler_path={}", compiler_path.display()));
}

pub(crate) fn compiler_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let compiler_path = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    compiler_path.to_path_buf()
}
