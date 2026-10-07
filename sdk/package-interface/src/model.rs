//! The interface model: everything a package exposes, classified once.

use alloc::{collections::BTreeSet, sync::Arc, vec::Vec};
use core::fmt;

use miden_assembly_syntax::ast::{
    AttributeSet, ConstantValue, Path, PathBuf,
    types::{FunctionType, Type},
};
use miden_mast_package::{Package, PackageExport, PackageId, TargetType, Version, Word};

use crate::abi::{LoweredSignature, UnsupportedSignature, lower_signature};

/// The attribute names that mark a procedure as filling a protocol role, in [`Role`] order.
pub const ROLE_ATTRIBUTES: [&str; 4] =
    ["account_procedure", "note_script", "auth_script", "transaction_script"];

/// A protocol role a procedure fills. Role procedures are reached through the WIT path, never
/// through `extern "C"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// An account component procedure.
    AccountProcedure,
    /// A note script entrypoint.
    NoteScript,
    /// An authentication procedure.
    AuthScript,
    /// A transaction script entrypoint.
    TransactionScript,
}

impl Role {
    /// Every role, in declaration order — the order [`Self::from_attributes`] searches, and the
    /// order of [`ROLE_ATTRIBUTES`].
    pub const ALL: [Role; 4] = [
        Role::AccountProcedure,
        Role::NoteScript,
        Role::AuthScript,
        Role::TransactionScript,
    ];

    /// The attribute that marks this role.
    pub const fn attribute(self) -> &'static str {
        match self {
            Role::AccountProcedure => ROLE_ATTRIBUTES[0],
            Role::NoteScript => ROLE_ATTRIBUTES[1],
            Role::AuthScript => ROLE_ATTRIBUTES[2],
            Role::TransactionScript => ROLE_ATTRIBUTES[3],
        }
    }

    /// The first role, in declaration order, whose attribute is present.
    pub fn from_attributes(attributes: &AttributeSet) -> Option<Role> {
        Role::ALL.into_iter().find(|role| attributes.has(role.attribute()))
    }
}

/// Why a procedure gets no `extern "C"` binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The manifest records no typed signature for it.
    Untyped,
    /// The package's `kind` is not [`TargetType::Library`]: an executable, a kernel, an account
    /// component and a transaction script are all equally ineligible, whatever a procedure's
    /// signature says.
    NotALibrary(TargetType),
    /// It has a signature, but the rule set cannot express it.
    Unsupported(UnsupportedSignature),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Untyped => f.write_str("no typed signature in the package manifest"),
            Self::NotALibrary(kind) => write!(
                f,
                "procedures of a {kind} package are not `exec`-bindable; only `Library` packages \
                 are"
            ),
            Self::Unsupported(err) => write!(f, "{err}"),
        }
    }
}

impl core::error::Error for SkipReason {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Unsupported(err) => Some(err),
            Self::Untyped | Self::NotALibrary(_) => None,
        }
    }
}

/// What a procedure is to a foreign caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcedureClass {
    /// Invocable with `exec`; carries its Wasm C ABI shape.
    Bindable(LoweredSignature),
    /// Fills a protocol role; reachable only through the WIT path.
    Role(Role),
    /// Not bindable, and why.
    Skipped(SkipReason),
}

/// One exported procedure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureItem {
    /// Fully qualified path.
    pub path: Arc<Path>,
    /// MAST root digest.
    pub digest: Word,
    /// The typed signature, when the manifest has one.
    pub signature: Option<FunctionType>,
    /// The procedure's attributes.
    pub attributes: AttributeSet,
    /// Its classification.
    pub class: ProcedureClass,
}

impl ProcedureItem {
    /// The procedure's own name, without its module path.
    pub fn name(&self) -> &str {
        self.path.last().expect(
            "`Path::last` is `None` only for an empty or root-only path, and an export path in a \
             manifest is neither",
        )
    }

    /// The module the procedure lives in.
    ///
    /// This is the path's parent, which is empty for a one-component relative path — such a
    /// procedure sits at the root of its package rather than in a module.
    pub fn namespace(&self) -> &Path {
        self.path.parent().expect(
            "`Path::parent` is `None` only for an empty or root-only path, and an export path in \
             a manifest is neither",
        )
    }
}

/// One exported named type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeItem {
    /// Fully qualified path.
    pub path: Arc<Path>,
    /// The type, aliases resolved.
    pub ty: Type,
}

/// One exported constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantItem {
    /// Fully qualified path.
    pub path: Arc<Path>,
    /// The constant's value.
    pub value: ConstantValue,
}

/// A package's interface, built from its manifest and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInterface {
    /// Package name.
    pub name: PackageId,
    /// Package version.
    pub version: Version,
    /// What the package is: library, kernel, account component, and so on.
    pub kind: TargetType,
    /// Package digest.
    ///
    /// This is [`Package::dependency_commitment`], the commitment dependency resolution
    /// identifies a package by: it binds the code, the identity and the manifest, and excludes
    /// debug data. So it identifies the compiled code rather than the interface: a purely
    /// internal change — a rewritten procedure body, a different inlining decision — changes it
    /// while every signature, role and classification below stays the same.
    pub digest: Word,
    /// The modules the manifest declares, sorted by path, without duplicates.
    ///
    /// These are only the *declared* modules; [`Self::module_paths`] adds the modules implied by
    /// the export paths.
    pub modules: Vec<PathBuf>,
    /// Every exported procedure, sorted by path.
    pub procedures: Vec<ProcedureItem>,
    /// Every exported type, sorted by path.
    pub types: Vec<TypeItem>,
    /// Every exported constant, sorted by path.
    pub constants: Vec<ConstantItem>,
}

impl PackageInterface {
    /// Build the interface of `package`, classifying every procedure exactly once.
    pub fn from_package(package: &Package) -> Self {
        let mut procedures = Vec::new();
        let mut types = Vec::new();
        let mut constants = Vec::new();

        for export in package.manifest.exports() {
            match export {
                PackageExport::Procedure(proc_export) => {
                    let class = classify(
                        package.kind,
                        &proc_export.attributes,
                        proc_export.signature.as_ref(),
                    );
                    procedures.push(ProcedureItem {
                        path: proc_export.path.clone(),
                        digest: proc_export.digest,
                        signature: proc_export.signature.clone(),
                        attributes: proc_export.attributes.clone(),
                        class,
                    });
                }
                PackageExport::Type(type_export) => types.push(TypeItem {
                    path: type_export.path.clone(),
                    ty: type_export.ty.clone(),
                }),
                PackageExport::Constant(constant_export) => constants.push(ConstantItem {
                    path: constant_export.path.clone(),
                    value: constant_export.value.clone(),
                }),
            }
        }

        let mut modules: Vec<PathBuf> =
            package.manifest.modules().map(|module| module.path.to_path_buf()).collect();

        procedures.sort_by(|a, b| a.path.cmp(&b.path));
        types.sort_by(|a, b| a.path.cmp(&b.path));
        constants.sort_by(|a, b| a.path.cmp(&b.path));
        modules.sort();
        modules.dedup();

        Self {
            name: package.name.clone(),
            version: package.version.clone(),
            kind: package.kind,
            digest: package.dependency_commitment(),
            modules,
            procedures,
            types,
            constants,
        }
    }

    /// The procedures a foreign caller may `exec`, with their Wasm shapes.
    pub fn bindable(&self) -> impl Iterator<Item = (&ProcedureItem, &LoweredSignature)> {
        self.procedures.iter().filter_map(|p| match &p.class {
            ProcedureClass::Bindable(lowered) => Some((p, lowered)),
            _ => None,
        })
    }

    /// The procedures that get no binding, with the reason each was skipped.
    pub fn skipped(&self) -> impl Iterator<Item = (&ProcedureItem, &SkipReason)> {
        self.procedures.iter().filter_map(|p| match &p.class {
            ProcedureClass::Skipped(reason) => Some((p, reason)),
            _ => None,
        })
    }

    /// The procedures that fill a protocol role.
    pub fn roles(&self) -> impl Iterator<Item = (&ProcedureItem, Role)> {
        self.procedures.iter().filter_map(|p| match &p.class {
            ProcedureClass::Role(role) => Some((p, *role)),
            _ => None,
        })
    }

    /// Find a procedure by path. A leading `::` on either side is ignored, so the fully
    /// qualified name a linker stub carries matches the manifest's absolute path.
    pub fn procedure(&self, path: &Path) -> Option<&ProcedureItem> {
        let wanted = path.to_relative();
        self.procedures.iter().find(|p| p.path.to_relative() == wanted)
    }

    /// Every module in the package's module tree: the manifest's declared modules plus every
    /// ancestor of an export.
    ///
    /// An export at `::a::b::c` implies both `::a::b` and `::a`, whether or not the manifest
    /// declares them, because a binding generator has to emit the whole path down to the export.
    /// Sorted, without duplicates.
    pub fn module_paths(&self) -> Vec<PathBuf> {
        let mut modules: BTreeSet<PathBuf> = self.modules.iter().cloned().collect();
        for path in self
            .procedures
            .iter()
            .map(|p| &p.path)
            .chain(self.types.iter().map(|t| &t.path))
            .chain(self.constants.iter().map(|c| &c.path))
        {
            let mut ancestor = path.parent();
            while let Some(module) = ancestor.filter(|module| !module.is_empty()) {
                modules.insert(module.to_path_buf());
                ancestor = module.parent();
            }
        }
        modules.into_iter().collect()
    }

    /// The first component of every path in the package's module tree — the namespaces a path
    /// into this package can start with. Sorted, without duplicates.
    ///
    /// This is the cheap test for "could this name be a binding to this package at all?", which
    /// is what the Wasm frontend asks before it treats a diverging function as a linker stub: a
    /// name rooted elsewhere is some other language's symbol, not a stub whose procedure is
    /// missing. Derived from the same paths as [`Self::module_paths`], so a package that declares
    /// no modules still contributes the namespaces its exports imply.
    pub fn root_namespaces(&self) -> BTreeSet<&str> {
        self.modules
            .iter()
            .map(|module| &**module)
            .chain(self.procedures.iter().map(|p| &*p.path))
            .chain(self.types.iter().map(|t| &*t.path))
            .chain(self.constants.iter().map(|c| &*c.path))
            .filter_map(|path| path.first())
            .collect()
    }
}

/// Classify one export, in the mandated order: role, then library kind, then untyped, then
/// lowering.
///
/// The order matters because the tests are not disjoint. A role attribute wins over everything:
/// a role procedure is reached through the WIT path even when it is typed and lowerable, and
/// even in a package that is not a library. The library-kind test comes next because nothing
/// outside a [`TargetType::Library`] package is `exec`-invocable, whatever its signature says.
///
/// The gate is `kind == TargetType::Library` exactly — not [`TargetType::is_library`] (which is
/// `!is_executable()` and so is also true for a kernel), and not "not a kernel": an executable, a
/// kernel, an account component and a transaction script are all equally ineligible. Only a
/// plain library's typed, role-free exports are `exec`-bindable.
fn classify(
    kind: TargetType,
    attributes: &AttributeSet,
    signature: Option<&FunctionType>,
) -> ProcedureClass {
    if let Some(role) = Role::from_attributes(attributes) {
        return ProcedureClass::Role(role);
    }
    if kind != TargetType::Library {
        return ProcedureClass::Skipped(SkipReason::NotALibrary(kind));
    }
    let Some(signature) = signature else {
        return ProcedureClass::Skipped(SkipReason::Untyped);
    };
    match lower_signature(signature) {
        Ok(lowered) => ProcedureClass::Bindable(lowered),
        Err(err) => ProcedureClass::Skipped(SkipReason::Unsupported(err)),
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec::Vec};

    use miden_assembly_syntax::ast::{
        Path,
        types::{AddressSpace, Type},
    };

    use super::*;
    use crate::{
        abi::{ReturnStrategy, WasmScalar},
        testing::{FIXTURE_SOURCE, assemble_fixture},
    };

    fn fixture() -> PackageInterface {
        PackageInterface::from_package(&assemble_fixture("fixture", "fixture", FIXTURE_SOURCE))
    }

    fn class_of<'a>(iface: &'a PackageInterface, name: &str) -> &'a ProcedureClass {
        &iface
            .procedures
            .iter()
            .find(|p| p.name() == name)
            .unwrap_or_else(|| panic!("no procedure named {name}"))
            .class
    }

    #[test]
    fn identity_and_kind_come_from_the_package() {
        let package = assemble_fixture("fixture", "fixture", FIXTURE_SOURCE);
        let iface = PackageInterface::from_package(&package);
        assert_eq!(AsRef::<str>::as_ref(&iface.name), "fixture");
        assert_eq!(iface.kind, miden_mast_package::TargetType::Library);
        assert_eq!(iface.version, package.version);
        assert_eq!(iface.digest, package.dependency_commitment());
        assert_eq!(
            iface.modules.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
            ["::fixture"],
            "the manifest's declared modules are carried through verbatim"
        );
    }

    #[test]
    fn typed_library_procedures_without_a_role_are_bindable() {
        let iface = fixture();
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "id") else {
            panic!("`id` must be bindable");
        };
        assert_eq!(lowered.ret, ReturnStrategy::Direct(WasmScalar::Felt));
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "make_pair") else {
            panic!("`make_pair` must be bindable");
        };
        assert_eq!(lowered.ret.scalars(), alloc::vec![WasmScalar::Felt; 2]);
        // `make_pair(..) -> Pair` with `Pair = struct { a: felt, b: felt }`: the out-pointer
        // results keep the declared field names, so a generator can name the wrapper's fields.
        let ReturnStrategy::OutPointer(results) = &lowered.ret else {
            panic!("two result scalars go through an out pointer, got {:?}", lowered.ret);
        };
        let names: Vec<Option<&str>> = results.iter().map(|f| f.path[0].name.as_deref()).collect();
        assert_eq!(names, [Some("a"), Some("b")]);
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "hash") else {
            panic!("`hash` must be bindable");
        };
        assert_eq!(lowered.params.len(), 4);
        // `take_pair(p: Pair)` with `Pair = struct { a: felt, b: felt }`: the declared field
        // names travel with the flattened parameters (the generator names Rust params from them).
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "take_pair") else {
            panic!("`take_pair` must be bindable");
        };
        let names: Vec<Option<&str>> =
            lowered.params.iter().map(|p| p.path[0].name.as_deref()).collect();
        assert_eq!(names, [Some("a"), Some("b")]);
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "tag_of") else {
            panic!("`tag_of` must be bindable");
        };
        assert_eq!(lowered.ret, ReturnStrategy::Direct(WasmScalar::U16));
        let bindable: Vec<&str> = iface.bindable().map(|(p, _)| p.name()).collect();
        assert_eq!(
            bindable,
            ["hash", "id", "make_pair", "tag_of", "take_pair", "write"],
            "sorted by path"
        );
    }

    #[test]
    fn a_pointer_parameter_lowers_to_a_pointer_scalar() {
        // `pub proc write(p: ptr<u32, addrspace(felt)>, n: u32)`
        let iface = fixture();
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "write") else {
            panic!("`write` must be bindable");
        };
        assert_eq!(lowered.ret, ReturnStrategy::Void);
        assert_eq!(lowered.params.len(), 2);
        let WasmScalar::Ptr(ptr) = &lowered.params[0].scalar else {
            panic!("first param must lower to a pointer, got {:?}", lowered.params[0].scalar);
        };
        assert_eq!(ptr.pointee(), &Type::U32);
        assert_eq!(lowered.params[1].scalar, WasmScalar::U32);
        assert_eq!(lowered.stub_signature().params(), &[Type::I32, Type::I32]);
    }

    /// The declared `addrspace(felt)` must survive into the manifest: a generated wrapper converts
    /// a byte address to an element address only for element-space pointers, so a dropped
    /// annotation would hand the callee a byte address where it expects an element address.
    /// (`miden-assembly-syntax` 0.29 dropped it on resolution; this pins the fixed behaviour.)
    #[test]
    fn an_element_space_pointer_parameter_keeps_its_address_space() {
        let iface = fixture();
        let ProcedureClass::Bindable(lowered) = class_of(&iface, "write") else {
            panic!("`write` must be bindable");
        };
        let WasmScalar::Ptr(ptr) = &lowered.params[0].scalar else {
            panic!("first param must lower to a pointer, got {:?}", lowered.params[0].scalar);
        };
        assert_eq!(ptr.addrspace(), AddressSpace::Element);
    }

    #[test]
    fn a_procedure_without_a_signature_is_skipped_as_untyped() {
        let iface = fixture();
        assert_eq!(class_of(&iface, "untyped"), &ProcedureClass::Skipped(SkipReason::Untyped));
        let skipped: Vec<(&str, &SkipReason)> =
            iface.skipped().map(|(p, r)| (p.name(), r)).collect();
        assert_eq!(skipped, [("untyped", &SkipReason::Untyped)]);
        assert_eq!(SkipReason::Untyped.to_string(), "no typed signature in the package manifest");
    }

    #[test]
    fn a_skip_reason_is_an_error_that_chains_to_the_lowering_failure() {
        use core::error::Error;

        let inner = UnsupportedSignature::CallingConvention(
            miden_assembly_syntax::ast::types::CallConv::ComponentModel,
        );
        let reason = SkipReason::Unsupported(inner.clone());
        assert_eq!(reason.to_string(), inner.to_string());
        let source = reason.source().expect("an unsupported signature chains to its cause");
        assert_eq!(source.to_string(), inner.to_string());
        assert!(SkipReason::Untyped.source().is_none());
        assert!(SkipReason::NotALibrary(TargetType::Kernel).source().is_none());
    }

    #[test]
    fn a_role_attribute_makes_a_role_procedure_even_when_typed() {
        let iface = fixture();
        assert_eq!(class_of(&iface, "role"), &ProcedureClass::Role(Role::AccountProcedure));
        let roles: Vec<(&str, Role)> = iface.roles().map(|(p, r)| (p.name(), r)).collect();
        assert_eq!(roles, [("role", Role::AccountProcedure)]);
    }

    #[test]
    fn types_and_constants_are_carried_through() {
        let iface = fixture();
        let names: Vec<&str> = iface.types.iter().map(|t| t.path.last().unwrap()).collect();
        assert_eq!(names, ["Pair", "Tag"]);
        assert_eq!(iface.types[1].ty, Type::U16);
        assert!(matches!(&iface.types[0].ty, Type::Struct(s) if s.get().fields().len() == 2));
        let constants: Vec<&str> = iface.constants.iter().map(|c| c.path.last().unwrap()).collect();
        assert_eq!(constants, ["LIMIT"]);
    }

    #[test]
    fn lookup_by_path_ignores_the_leading_separator() {
        let iface = fixture();
        assert!(iface.procedure(Path::new("fixture::id")).is_some());
        assert!(iface.procedure(Path::new("::fixture::id")).is_some());
        assert!(iface.procedure(Path::new("fixture::missing")).is_none());
        assert_eq!(
            iface.module_paths().iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            ["::fixture"]
        );
    }

    #[test]
    fn module_paths_are_the_declared_modules_plus_every_ancestor_of_an_export() {
        fn procedure(path: &str) -> ProcedureItem {
            ProcedureItem {
                path: Path::new(path).into(),
                digest: Word::default(),
                signature: None,
                attributes: AttributeSet::default(),
                class: ProcedureClass::Skipped(SkipReason::Untyped),
            }
        }
        fn paths(iface: &PackageInterface) -> Vec<alloc::string::String> {
            iface.module_paths().iter().map(|p| p.to_string()).collect()
        }

        let mut iface = PackageInterface {
            name: PackageId::from("p"),
            version: Version::new(0, 0, 0),
            kind: TargetType::Library,
            digest: Word::default(),
            modules: Vec::new(),
            procedures: alloc::vec![procedure("::a::b::c")],
            types: Vec::new(),
            constants: Vec::new(),
        };
        assert_eq!(
            paths(&iface),
            ["::a", "::a::b"],
            "a nested export contributes every one of its ancestors, not just its own module"
        );

        // A declared module with no exports of its own is still part of the module tree.
        iface.modules = alloc::vec![Path::new("::a::b::d").to_path_buf()];
        assert_eq!(paths(&iface), ["::a", "::a::b", "::a::b::d"], "sorted, and deduped");

        // The root namespaces are the first components of that same tree.
        assert_eq!(iface.root_namespaces().into_iter().collect::<Vec<_>>(), ["a"]);
        iface.modules.push(Path::new("::z").to_path_buf());
        assert_eq!(iface.root_namespaces().into_iter().collect::<Vec<_>>(), ["a", "z"]);
    }

    #[test]
    fn only_a_library_package_is_exec_bindable() {
        // "library package" means `kind == TargetType::Library` exactly: a kernel's procedures
        // are `syscall` targets, and an account component's are reached through its own protocol,
        // so neither is `exec`-bindable, whatever its signatures say. Role procedures are
        // unaffected either way.
        for kind in [TargetType::Kernel, TargetType::AccountComponent] {
            let mut package = (*assemble_fixture("fixture", "fixture", FIXTURE_SOURCE)).clone();
            package.kind = kind;
            let iface = PackageInterface::from_package(&package);
            assert!(iface.bindable().next().is_none());
            assert_eq!(
                class_of(&iface, "id"),
                &ProcedureClass::Skipped(SkipReason::NotALibrary(kind)),
                "{kind} package"
            );
            assert_eq!(class_of(&iface, "role"), &ProcedureClass::Role(Role::AccountProcedure));
        }
        assert_eq!(
            SkipReason::NotALibrary(TargetType::Kernel).to_string(),
            "procedures of a kernel package are not `exec`-bindable; only `Library` packages are"
        );
    }

    #[test]
    fn role_lookup_follows_the_attribute_table() {
        assert_eq!(Role::AccountProcedure.attribute(), "account_procedure");
        assert_eq!(Role::NoteScript.attribute(), "note_script");
        assert_eq!(Role::AuthScript.attribute(), "auth_script");
        assert_eq!(Role::TransactionScript.attribute(), "transaction_script");
        assert_eq!(ROLE_ATTRIBUTES, Role::ALL.map(Role::attribute));
    }
}
