//! The compiler-intrinsics library: MASM procedures code generation calls into.
//!
//! The sources are embedded in the compiler and assembled on first use *per session*, against the
//! core library the session links. They cannot be assembled at build time: the intrinsics `exec`
//! core procedures, so the assembled package records the core's digest as a runtime dependency,
//! and the assembler rejects a build whose other packages depend on a different core. Assembling
//! against the session's own core keeps every toolchain the compiler is used with consistent.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use std::sync::Mutex;

use miden_assembly::{Assembler, Linkage, ModuleParser, Path, ast::ModuleKind};
use miden_mast_package::{Package, Word};
use midenc_session::{
    LinkLibrary, Session,
    diagnostics::{Report, SourceLanguage},
};

/// The embedded sources, keyed by module path.
const SOURCES: &[(&str, &str)] = &[
    ("::intrinsics", include_str!("../intrinsics/mod.masm")),
    ("::intrinsics::advice", include_str!("../intrinsics/advice.masm")),
    ("::intrinsics::i32", include_str!("../intrinsics/i32.masm")),
    ("::intrinsics::i64", include_str!("../intrinsics/i64.masm")),
    ("::intrinsics::mem", include_str!("../intrinsics/mem.masm")),
];

const NAME: &str = "compiler-intrinsics";
const VERSION: &str = "0.9.0";

/// The assembled package, cached by the digest of the core library it was assembled against.
static CACHE: Mutex<BTreeMap<Word, Arc<Package>>> = Mutex::new(BTreeMap::new());

/// The intrinsics package for `session`, assembled against the core library it links.
///
/// Assembled once per core digest; later calls against a session linking the same core return
/// the same cached [`Arc`].
pub fn load(session: &Session) -> Result<Arc<Package>, Report> {
    let core = LinkLibrary::core().load(&session.options)?;

    if let Some(cached) = CACHE.lock().unwrap().get(&core.dependency_commitment()) {
        return Ok(cached.clone());
    }

    let package = assemble(session, &core)?;

    // Another session racing us to assemble against the same core loses: keep whichever of the
    // two ended up in the cache first, so callers can rely on pointer equality for a given digest.
    let mut cache = CACHE.lock().unwrap();
    Ok(cache.entry(core.dependency_commitment()).or_insert(package).clone())
}

/// Assemble the intrinsics sources against `core`, recording `core` — and everything `core`
/// itself already depends on — as this package's own runtime dependencies.
///
/// This mirrors what the project-level assembler does when it links a package dynamically
/// (`miden_assembly::project::runtime_dependencies`): [`Assembler::assemble_library`] does not
/// populate a package's manifest with the packages it was linked against, since a low-level
/// assembly has no notion of "runtime dependency" bookkeeping — only the project assembler's
/// dependency graph does. Since the intrinsics are assembled directly rather than through a
/// project, that bookkeeping is redone here by hand: `RuntimeDependencies::merge_package`, for a
/// dynamic non-kernel dependency, records `package.to_dependency()` followed by the package's own
/// manifest dependencies, and that is exactly what the two `add_dependency` calls below do for
/// `core`. Two differences from the original are inert today: the order here is core first, then
/// core's manifest order, where the project assembler's `BTreeMap<PackageId, _>` emits name
/// order — identical for today's core, and consumers look dependencies up by name, not position.
/// And a core carrying a kernel as one of its own runtime dependencies would have that dependency
/// recorded in the manifest here, same as any other, but not *linked* the way the project
/// assembler links one — there is no bookkeeping here to check it against other kernel
/// dependencies or to expose it as the package's linked kernel. Unreachable today: kernel
/// packages are out of scope.
fn assemble(session: &Session, core: &Arc<Package>) -> Result<Arc<Package>, Report> {
    let source_manager = session.source_manager.clone();
    let mut modules = SOURCES.iter().map(|(path, source)| {
        let file = source_manager.load(SourceLanguage::Masm, (*path).into(), (*source).to_string());
        ModuleParser::new(Some(ModuleKind::Library)).parse(
            Some(Path::new(path)),
            file,
            source_manager.clone(),
        )
    });
    let root = modules.next().expect("SOURCES is non-empty; the root module is first")?;
    let support = modules.collect::<Result<Vec<_>, _>>()?;

    let mut assembler = Assembler::new(source_manager);
    assembler.link_package(core.clone(), Linkage::Dynamic)?;
    let mut package = assembler.assemble_library(NAME, root, support)?;
    package.version = VERSION.parse().expect("a valid version");

    package.manifest.add_dependency(core.to_dependency()).map_err(Report::msg)?;
    for dependency in core.manifest.dependencies() {
        package.manifest.add_dependency(dependency.clone()).map_err(Report::msg)?;
    }

    Ok(Arc::from(package))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> alloc::rc::Rc<midenc_session::Session> {
        // A session whose sysroot is the toolchain under test: `MIDEN_SYSROOT` when it is set,
        // and otherwise the one `Options::default()` derives from `MIDENUP_HOME` and
        // `MIDENUP_TOOLCHAIN`.
        let sysroot = match std::env::var_os("MIDEN_SYSROOT") {
            Some(dir) => std::path::PathBuf::from(dir),
            None => midenc_session::Options::default().sysroot.expect(
                "this test needs a Miden toolchain: set MIDEN_SYSROOT to one, or set MIDENUP_HOME \
                 and MIDENUP_TOOLCHAIN so that one can be derived",
            ),
        };
        let dir = std::env::temp_dir();
        let options = alloc::boxed::Box::new(midenc_session::Options::new(
            None,
            None,
            dir.clone(),
            dir,
            None,
            Some(sysroot),
        ));
        alloc::rc::Rc::new(midenc_session::Session::new_project(
            "intrinsics-test".into(),
            None,
            options,
            None,
            alloc::sync::Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
        ))
    }

    #[test]
    fn the_intrinsics_assemble_against_the_toolchain_core_and_keep_their_identity() {
        let package = load(&session()).unwrap();
        assert_eq!(package.name, "compiler-intrinsics");
        assert_eq!(package.version.to_string(), "0.9.0");
        let exports: Vec<alloc::string::String> =
            package.manifest.exports().map(|e| e.path().to_string()).collect();
        for expected in ["::intrinsics::mem::heap_init", "::intrinsics::i64::checked_div"] {
            assert!(exports.iter().any(|e| e == expected), "missing {expected}");
        }
        let core_dep = package
            .manifest
            .dependencies()
            .find(|d| d.name == "miden-core")
            .expect("the intrinsics depend on the linked core");
        let core = midenc_session::LinkLibrary::core().load(&session().options).unwrap();
        assert_eq!(
            core_dep.digest,
            core.dependency_commitment(),
            "linked against the session's core, by digest"
        );
    }

    #[test]
    fn the_package_is_assembled_once_per_core_digest() {
        let s = session();
        let a = load(&s).unwrap();
        let b = load(&s).unwrap();
        assert!(alloc::sync::Arc::ptr_eq(&a, &b));
    }
}
