use std::{
    env,
    path::{Path, PathBuf},
};

use miden_assembly::{Assembler, Report, diagnostics::IntoDiagnostic};
use miden_core_lib::CoreLibrary;
use midenc_session::miden_package_registry::PackageCache;

fn main() -> Result<(), Report> {
    use miden_assembly::diagnostics::reporting::ReportHandlerOpts;

    // Rebuild the package if the content of the `intrinsics` directory changes
    println!("cargo:rerun-if-changed=intrinsics");

    miden_assembly::diagnostics::reporting::set_hook(Box::new(|_| {
        Box::new(ReportHandlerOpts::new().build())
    }))
    .unwrap();
    miden_assembly::diagnostics::reporting::set_panic_hook();

    // Enable debug tracing to stderr via the MIDEN_LOG environment variable, if present
    midenc_log::Builder::from_env("MIDENC_TRACE").format_timestamp(None).init();

    // Build compiler-intrinsics library
    let intrinsics_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("intrinsics");
    let toolchain_dir = std::env::var_os("MIDEN_SYSROOT").map(PathBuf::from);
    let cwd = env::current_dir().into_diagnostic()?;
    let target_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let options =
        midenc_session::Options::new(None, None, cwd, target_dir.clone(), None, toolchain_dir);

    let mut registry = midenc_session::registry::HybridPackageRegistry::new(&options)?;
    // Extend the registry with the built-in core library, in case the midenup toolchain is not
    // available
    registry.cache_package(CoreLibrary::default().package())?;

    let assembler = Assembler::default();
    let mut project_assembler =
        assembler.for_project_at_path(intrinsics_dir.join("miden-project.toml"), &mut registry)?;

    let package =
        project_assembler.assemble(miden_assembly::ProjectTargetSelector::Library, "release")?;

    let build_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // write the masp output
    package.write_masp_file(&build_dir).into_diagnostic()?;

    Ok(())
}
