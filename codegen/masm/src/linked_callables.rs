//! Immutable callable authority derived from the assembler's actual inputs.
use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::{
    Assembler, Linkage,
    linker::{
        LinkLibrary, Linker, SymbolItem,
        namespaces::{NamespaceGraph, ResolvedUse},
    },
};
use miden_mast_package::{Package, Word};
use midenc_hir::Report;
use midenc_session::Session;

use crate::{masm, plan::ProcedureAbi};

/// One graph-selected package, retaining the assembler's linkage and order.
#[derive(Clone)]
pub struct LinkedPackageInput {
    pub package: Arc<Package>,
    pub linkage: Linkage,
}

/// Root-session inputs loaded once, with project dependency exclusions already applied.
/// Both assembly and metadata consume this bundle; neither reinterprets session discovery.
pub struct SessionLinkInputs {
    intrinsics: Arc<Package>,
    #[allow(clippy::vec_box)]
    modules: Vec<Box<masm::Module>>,
    libraries: Vec<LinkedPackageInput>,
    truncate: BTreeSet<Word>,
    source_manager: Arc<dyn midenc_session::SourceManager>,
}

impl SessionLinkInputs {
    pub fn load(
        session: &Session,
        excluded: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, Report> {
        let excluded = excluded
            .into_iter()
            .map(|name| name.as_ref().to_string())
            .collect::<BTreeSet<_>>();
        let intrinsics = crate::intrinsics::load(session)?;
        let core = midenc_session::LinkLibrary::core().load(&session.options)?;
        let mut truncate = BTreeSet::new();
        collect_truncate(&core, &mut truncate)?;
        let mut modules = Vec::new();
        for (path, content) in &session.options.link_modules {
            let source = session.source_manager.load(
                midenc_hir::diagnostics::SourceLanguage::Masm,
                path.as_str().into(),
                content.clone(),
            );
            modules.push(
                miden_assembly::ModuleParser::new(Some(masm::ModuleKind::Library)).parse(
                    Some(path.as_path()),
                    source,
                    session.source_manager.clone(),
                )?,
            );
        }
        let mut libraries = Vec::new();
        for input in &session.options.link_libraries {
            if !excluded.contains(input.canonical_name()) {
                libraries.push(LinkedPackageInput {
                    package: input.load(&session.options)?,
                    linkage: input.linkage,
                });
            }
        }
        Ok(Self {
            intrinsics,
            modules,
            libraries,
            truncate,
            source_manager: session.source_manager.clone(),
        })
    }

    /// Apply precisely the input ordering used to construct callable metadata.
    pub fn apply(&self, assembler: &mut Assembler) -> Result<(), Report> {
        assembler.link_package(self.intrinsics.clone(), Linkage::Static)?;
        assembler.compile_and_statically_link_all(self.modules.clone())?;
        for input in &self.libraries {
            assembler.link_package(input.package.clone(), input.linkage)?;
        }
        Ok(())
    }
}

/// A frozen named callable retains canonical identity independently of the name granting access.
struct Callable {
    canonical: masm::PathBuf,
    digest: Option<Word>,
    signature: Option<Arc<masm::types::FunctionType>>,
    abi: Result<ProcedureAbi, String>,
    kernel: bool,
}

/// Supported actual-link subset: explicit session inputs plus graph-selected direct dependencies.
/// Sibling libraries and solely inherited kernels require an upstream selected-package callback.
pub struct LinkedCallables {
    named: BTreeMap<masm::PathBuf, Callable>,
    truncate: BTreeSet<Word>,
}

impl LinkedCallables {
    pub fn for_session(session: &Session) -> Result<Self, Report> {
        Self::from_link_inputs(&SessionLinkInputs::load(session, core::iter::empty::<&str>())?, &[])
    }

    pub fn from_link_inputs(
        base: &SessionLinkInputs,
        dependencies: &[LinkedPackageInput],
    ) -> Result<Self, Report> {
        let mut linker = Linker::new(base.source_manager.clone());
        linker.link_library(
            LinkLibrary::from_package(base.intrinsics.clone()).with_linkage(Linkage::Static),
        )?;
        linker.link_modules(base.modules.clone())?;
        let mut truncate = base.truncate.clone();
        let mut kernel: Option<Arc<Package>> = None;
        for input in base.libraries.iter().chain(dependencies) {
            collect_truncate(&input.package, &mut truncate)?;
            if input.package.is_kernel() {
                if let Some(previous) = &kernel {
                    if previous.dependency_commitment() != input.package.dependency_commitment() {
                        return Err(Report::msg(
                            "linked callable inputs contain conflicting kernels",
                        ));
                    }
                } else {
                    kernel = Some(input.package.clone());
                }
            } else {
                linker.link_library(
                    LinkLibrary::from_package(input.package.clone()).with_linkage(input.linkage),
                )?;
            }
        }
        if let Some(kernel) = kernel {
            linker.link_with_kernel(kernel)?;
        }
        let namespaces = NamespaceGraph::build(&linker)?;
        let imports = namespaces.resolve_imports(&linker)?;
        let mut named = BTreeMap::new();
        for module in linker.modules() {
            for (index, symbol) in module.symbols().enumerate() {
                if symbol.visibility().is_exported() && symbol.is_procedure() {
                    let path = module.path().join(symbol.name());
                    let gid = module.id() + masm::ItemIndex::new(index);
                    named.insert(path, callable(&linker, gid)?);
                }
            }
            for import in module.imports().filter(|import| import.visibility().is_exported()) {
                if let Some(ResolvedUse::Item { id, .. }) =
                    imports.get(module.id(), import.local_name().as_str())
                    && linker[id].is_procedure()
                {
                    let path = module.path().join(import.local_name());
                    named.insert(path, callable(&linker, id)?);
                }
            }
        }
        Ok(Self { named, truncate })
    }

    pub(crate) fn resolve(
        &self,
        target: &masm::InvocationTarget,
    ) -> Result<(ProcedureAbi, masm::InvocationTarget), Report> {
        let missing = || {
            Report::msg(
                "inline assembly calls a procedure with no linked signature; assembler-selected \
                 sibling-library or inherited-kernel metadata may be unavailable",
            )
        };
        let (name, callable) = match target {
            masm::InvocationTarget::MastRoot(root) => {
                if self.truncate.contains(root.inner()) {
                    return Err(truncate_error());
                }
                let mut entries = self
                    .named
                    .iter()
                    .filter(|(_, callable)| callable.digest == Some(*root.inner()));
                let entry = entries.next().ok_or_else(missing)?;
                if entries.any(|(_, other)| {
                    other.signature != entry.1.signature || other.kernel != entry.1.kernel
                }) {
                    return Err(Report::msg(
                        "inline assembly digest has ambiguous linked signatures; use an \
                         unambiguous named export",
                    ));
                }
                entry
            }
            _ => {
                let path = target.unwrap_path();
                let path = path.to_absolute().map_err(Report::msg)?.into_owned();
                self.named.get_key_value(&path).ok_or_else(missing)?
            }
        };
        if callable.canonical.as_path() == masm::Path::new("::miden::core::sys::truncate_stack")
            || callable.digest.is_some_and(|digest| self.truncate.contains(&digest))
        {
            return Err(truncate_error());
        }
        if callable.signature.is_none() {
            return Err(missing());
        }
        let abi = callable.abi.as_ref().map_err(|reason| {
            Report::msg(format!(
                "inline assembly callee '{name}' has an unsupported typed contract: {reason}"
            ))
        })?;
        Ok((*abi, target.clone()))
    }
}

fn callable(linker: &Linker, id: masm::GlobalItemIndex) -> Result<Callable, Report> {
    let module = &linker[id.module];
    let symbol = &linker[id];
    let canonical = module.path().join(symbol.name());
    let kernel = linker.has_nonempty_kernel()
        && module.path().is_kernel_path()
        && symbol.visibility().is_exported();
    let digest = match symbol.item() {
        SymbolItem::Compiled(miden_assembly_syntax::module::ItemInfo::Procedure(procedure)) => {
            Some(procedure.digest)
        }
        _ => None,
    };
    let signature = linker.resolve_signature(id)?;
    let abi =
        signature
            .as_ref()
            .ok_or_else(|| "no typed signature".to_string())
            .and_then(|signature| {
                crate::lower::units::calls::signature_abi(
                    &signature.abi,
                    signature.params.iter().cloned(),
                    signature.results.iter().cloned(),
                    kernel,
                )
                .map_err(|error| error.to_string())
            });
    Ok(Callable {
        canonical,
        digest,
        signature,
        abi,
        kernel,
    })
}

fn collect_truncate(package: &Package, output: &mut BTreeSet<Word>) -> Result<(), Report> {
    for module in package
        .try_module_descriptors()
        .map_err(|error| Report::msg(error.to_string()))?
    {
        if module.path() != masm::Path::new("::miden::core::sys") {
            continue;
        }
        for (_, item) in module.items() {
            if let miden_assembly_syntax::module::ItemInfo::Procedure(procedure) = item
                && procedure.name.as_str() == "truncate_stack"
            {
                output.insert(procedure.digest);
            }
        }
    }
    Ok(())
}

fn truncate_error() -> Report {
    Report::msg("inline assembly truncate_stack is forbidden")
}

/// Avoid imposing sysroot loading on compilation units which never use inline assembly.
pub fn for_operation(op: &midenc_hir::Operation) -> Result<Option<LinkedCallables>, Report> {
    if needs_metadata(op) {
        LinkedCallables::for_session(op.context().session()).map(Some)
    } else {
        Ok(None)
    }
}

/// Whether this source unit requires a snapshot of actual linked callable facts.
pub fn needs_metadata(op: &midenc_hir::Operation) -> bool {
    let mut needed = false;
    op.prewalk_all(|op| needed |= op.is::<crate::hir::InlineAsm>());
    needed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn session(modules: &[(&str, &str)]) -> Session {
        let options = midenc_session::Options {
            link_modules: modules
                .iter()
                .map(|(path, text)| (masm::PathBuf::new(path).unwrap(), (*text).to_string()))
                .collect(),
            ..Default::default()
        };
        Session::new_project(
            "inline-test".into(),
            None,
            Box::new(options),
            None,
            Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
        )
    }

    #[test]
    fn linked_inline_metadata_uses_session_modules_and_typed_intrinsics() {
        let session = session(&[(
            "::inline_test",
            "pub proc add_one(x: felt) -> felt\n push.1 add\nend\npub proc untyped\n nop\nend",
        )]);
        let base = SessionLinkInputs::load(&session, core::iter::empty::<&str>()).unwrap();
        let metadata = LinkedCallables::from_link_inputs(&base, &[]).unwrap();
        let (abi, _) =
            metadata.resolve(&crate::lower::raw::target("::inline_test::add_one")).unwrap();
        assert_eq!((abi.arguments(), abi.results()), (1, 1));
        assert!(
            metadata
                .resolve(&crate::lower::raw::target("::inline_test::untyped"))
                .unwrap_err()
                .to_string()
                .contains("no linked signature")
        );
        assert!(
            metadata
                .resolve(&crate::lower::raw::target("::missing::callee"))
                .unwrap_err()
                .to_string()
                .contains("no linked signature")
        );
        assert!(
            metadata
                .resolve(&crate::lower::raw::target("::intrinsics::mem::heap_base"))
                .is_ok()
        );
        // Core is resolvable, but is not implicitly callable merely because intrinsics need it.
        assert!(
            metadata
                .resolve(&crate::lower::raw::target("::miden::core::sys::truncate_stack"))
                .is_err()
        );
        let mut assembler = Assembler::new(session.source_manager.clone());
        base.apply(&mut assembler).unwrap();
        assembler
            .assemble_program(
                "linked-inline",
                "begin push.41 exec.::inline_test::add_one push.42 assert_eq end",
            )
            .unwrap();
    }

    #[test]
    fn small_aggregate_unused_linked_export_does_not_break_metadata() {
        let session = session(&[(
            "::small_aggregate",
            "pub type S = struct { a: u64, b: u32, c: u32 }\npub proc unused(x: S) -> S\n \
             nop\nend\npub proc used(x: felt) -> felt\n nop\nend",
        )]);
        let metadata = LinkedCallables::for_session(&session).unwrap();
        let (abi, _) =
            metadata.resolve(&crate::lower::raw::target("::small_aggregate::used")).unwrap();
        assert_eq!((abi.arguments(), abi.results()), (1, 1));
    }

    #[test]
    fn linked_inline_unsupported_typed_contracts_are_diagnostics() {
        let session = session(&[("::wide_contract", "pub proc wide(x: [felt; 5])\n nop\nend")]);
        let metadata = LinkedCallables::for_session(&session).unwrap();
        let error = metadata
            .resolve(&crate::lower::raw::target("::wide_contract::wide"))
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("unsupported typed contract") && message.contains("one to four"),
            "{error:?}"
        );
    }

    #[test]
    fn linked_inline_metadata_retains_canonical_imports_and_module_conflicts() {
        let session = session(&[
            (
                "::inline_types",
                "pub type Value = felt\npub proc add_one(x: Value) -> Value\n push.1 add\nend",
            ),
            ("::inline_alias", "pub use {add_one as alias} from ::inline_types"),
        ]);
        let metadata = LinkedCallables::for_session(&session).unwrap();
        assert!(metadata.resolve(&crate::lower::raw::target("::inline_alias::alias")).is_ok());
        let session = self::session(&[
            ("::duplicate", "pub proc p nop end"),
            ("::duplicate", "pub proc q nop end"),
        ]);
        assert!(LinkedCallables::for_session(&session).is_err());
    }
}

#[cfg(test)]
pub(crate) mod package_tests {
    use super::*;
    use crate::plan::CallingConvention;

    pub(crate) fn package(session: &Session, path: &str, text: &str, kernel: bool) -> Arc<Package> {
        let module = miden_assembly::ModuleParser::new(Some(if kernel {
            masm::ModuleKind::Kernel
        } else {
            masm::ModuleKind::Library
        }))
        .parse_str(Some(masm::Path::new(path)), text, session.source_manager.clone())
        .unwrap();
        let mut assembler = Assembler::new(session.source_manager.clone());
        let core = midenc_session::LinkLibrary::core().load(&session.options).unwrap();
        assembler.link_package(core, Linkage::Dynamic).unwrap();
        if kernel {
            assembler.assemble_kernel("kernel", module, None).unwrap().into()
        } else {
            assembler
                .assemble_library("inline-package", module, None::<Box<masm::Module>>)
                .unwrap()
                .into()
        }
    }

    #[test]
    fn linked_inline_package_aliases_cannot_hide_truncate_identity() {
        let session = tests::session(&[]);
        let package = package(
            &session,
            "::alias",
            "pub use {truncate_stack as hidden} from ::miden::core::sys",
            false,
        );
        let base = SessionLinkInputs::load(&session, core::iter::empty::<&str>()).unwrap();
        let metadata = LinkedCallables::from_link_inputs(
            &base,
            &[LinkedPackageInput {
                package,
                linkage: Linkage::Static,
            }],
        )
        .unwrap();
        let error = metadata.resolve(&crate::lower::raw::target("::alias::hidden")).unwrap_err();
        assert!(error.to_string().contains("truncate_stack"), "{error:?}");
    }

    #[test]
    fn linked_inline_direct_kernel_exports_override_source_convention() {
        let session = tests::session(&[]);
        let package =
            package(&session, "::$kernel", "pub proc identity(x: felt) -> felt\n nop\nend", true);
        let base = SessionLinkInputs::load(&session, core::iter::empty::<&str>()).unwrap();
        let metadata = LinkedCallables::from_link_inputs(
            &base,
            &[LinkedPackageInput {
                package,
                linkage: Linkage::Dynamic,
            }],
        )
        .unwrap();
        let (abi, _) = metadata.resolve(&crate::lower::raw::target("::$kernel::identity")).unwrap();
        assert_eq!(abi.convention(), CallingConvention::Kernel);
        assert!(abi.invocation(crate::plan::Invocation::SysCall).is_ok());
        assert!(abi.invocation(crate::plan::Invocation::Exec).is_err());
        assert!(abi.invocation(crate::plan::Invocation::Call).is_err());
    }
}
