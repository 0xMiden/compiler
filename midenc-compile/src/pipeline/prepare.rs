//! Preparation: turning a compiler input into what a compilation request needs.
//!
//! There are two kinds of input, and one [`PreparedProject`] for both:
//!
//! - A **project input** names a manifest — either `miden-project.toml`, or a `Cargo.toml`
//!   standing in for the sibling `miden-project.toml` that `cargo miden` generates next to it.
//!   [`prepare_project`] normalizes that locator, loads the project from it, selects the target
//!   to build, and selects the frontend that handles that target's root.
//! - A **standalone input** is a source file, or bytes on standard input. There is nothing to
//!   load, so [`prepare_standalone`] synthesizes the project instead: one package, one target
//!   rooted at the input, and the frontend that handles that root — which for one extension is
//!   not the registry's; see [`select_standalone_frontend`].
//!
//! # Build profiles
//!
//! Preparation decides the profile *name* a project request carries — the profile itself is
//! resolved from that name by the assembler, once per target it builds — and the rule differs
//! by where the project's profiles come from:
//!
//! - **Synthesized (virtual) projects** — preparation builds the profiles itself, so
//!   profile-affecting flags fold in at synthesis: `--debug none` yields `debug = false`, and
//!   any positive `--debug` yields `debug = true`.
//! - **User-controlled manifests** — the requested profile name passes through untouched, and
//!   `--debug` does not alter the build profile at all. Users select or define a profile with
//!   the configuration they want.
//!
//! The asymmetry is forced: [`ProjectPackage::resolve_profile`] reads the profile out of the
//! package being assembled, and `Package` is neither `Clone` nor mutable through an `Arc`, so
//! the profiles of a loaded manifest cannot be adjusted on the way past. `--debug` continues
//! to govern compiler-side debug behavior in both cases; only its effect on the *build
//! profile* is confined to synthesized projects.
//!
//! Both halves of that rule are implemented here, but only the manifest half is reached: no
//! caller routes a standalone input through [`prepare_standalone`] yet, so standalone builds
//! still run the legacy stage chains, which pass a hardcoded `"dev"` to the assembler (see
//! `stages/assemble.rs`). Until they flip, `cargo miden build --release` builds `release`
//! while `midenc --release foo.wasm` builds `dev`.

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use std::path::{Path, PathBuf};

use miden_assembly::ProjectTargetSelector;
use miden_assembly_syntax::debuginfo::Span;
use midenc_session::{
    DebugInfo, FileType, InputFile, InputType, Options, Session,
    diagnostics::{Report, SourceManager, Uri},
    miden_project::{
        Dependency, DependencyVersionScheme, Linkage, Package as ProjectPackage, Project, Target,
        TargetType, VersionReq, VersionRequirement,
    },
};

use super::{FrontendRegistration, FrontendRegistry};
use crate::CompilerResult;

/// Everything a compilation request needs about the project it was asked to build.
///
/// The [`FrontendRegistration`] is held by value: it is `Copy`, and
/// [`FrontendRegistry::for_extension`] hands back a borrow of the registry rather than a
/// `&'static`, so copying it out keeps this type free of a lifetime parameter it would
/// otherwise carry for nothing.
#[derive(Debug)]
pub struct PreparedProject {
    /// The project's package: loaded from [`manifest_path`](Self::manifest_path) for a project
    /// input, synthesized for a standalone one.
    pub package: Arc<ProjectPackage>,
    /// The normalized manifest locator: always a `miden-project.toml`, never the `Cargo.toml`
    /// that may have named it.
    ///
    /// Empty for a synthesized project, which no locator named; see [`prepare_standalone`] for
    /// why that is a sentinel rather than the input path.
    pub manifest_path: PathBuf,
    /// The target selected for this request.
    pub target: Target,
    /// The name of the build profile to build under, as requested in [`Options::profile`].
    pub profile_name: String,
    /// The frontend registered for the selected target root's extension.
    pub frontend: FrontendRegistration,
}

/// Resolve a standalone `input` — a source file, or bytes read from standard input — into the
/// project, target and frontend a compilation request runs with.
///
/// There is no manifest to load, so the project is synthesized: one package named after the
/// session, holding one target rooted at the input, the core-library dependency every build
/// links against, and the build profile the request asked for. The result is the same
/// [`PreparedProject`] a manifest input produces, so everything downstream of preparation is
/// common to both.
///
/// The target's *namespace* is the one thing not taken from the session: a root file that says
/// what namespace it belongs to is believed, and preparation is the single place that decision
/// is made. `synthesized_target_name` sets out the rule and why it lives here.
///
/// # The synthesized target is not the one on the session
///
/// `Session::new` synthesizes a project of its own for `session.project`, and the target here
/// deliberately differs from that one. Both keep the *requested target type* — `Session::new`
/// through `Target::new(options.target_type.unwrap_or_default(), ..)`, this through
/// `synthesize_target`, which passes it through for all six types — but they disagree about
/// the namespace, in the two cases where the namespace is reserved:
///
/// - **Executables.** `Session::new` derives every namespace from the artifact name, including
///   an executable's. `MasmComponent::source_inputs` builds an executable's root module with
///   `Module::new_executable`, whose path is `$exec`, and `load_target_sources` rejects a root
///   module that does not sit exactly at its target's namespace — so a name-derived namespace
///   fails every standalone executable the moment such a build goes through a source provider.
/// - **Kernels.** Likewise `$kernel`, which is what a manifest-declared kernel gets and what
///   `syscall` resolution addresses.
///
/// They are invisible today for different reasons, and only the first is about that check.
/// A standalone *executable* escapes it because the legacy path hands assembly its sources
/// ready-made, so `load_target_sources` never runs. A *kernel* would be broken either way —
/// `syscall` resolution rewrites every target into `$kernel` whichever route assembled it —
/// but no standalone kernel build reaches codegen at all today: codegen emits only
/// `ModuleKind::Library`, so such a target fails the assembler's root-*kind* check before its
/// namespace is ever compared. `synthesize_target` carries the reasoning in full.
///
/// The package prepared here is therefore the one that gets assembled, and `session.project`
/// is left exactly as it is. It still has readers of its own, none of which want this
/// package: `Session::package_registry` and
/// [`RustProjectFrontend`](super::frontends::rust::RustProjectFrontend) both take the
/// filesystem package-cache directory from its manifest path — which a synthesized project
/// does not have — and
/// [`MasmProjectFrontend`](super::frontends::masm::MasmProjectFrontend) hands it to the
/// disassembler, which reads nothing off it while sources are supplied. Converging the two
/// syntheses means removing `Session`'s, which belongs with its eager project load.
///
/// # `manifest_path` is empty, and nothing reads it
///
/// [`PreparedProject::manifest_path`] is the *normalized locator* a project input named, and a
/// synthesized project was named by no locator at all. It carries an empty [`PathBuf`] — the
/// same sentinel `TargetAssemblyContext::new_virtual` uses for its own `manifest_path` — rather
/// than the input path, which is a source file and would be a plausible-looking lie.
/// [`Pipeline::compile`](super::Pipeline::compile) does not read the field; the assembler takes
/// the manifest path it needs from the package, where it is correctly `None`.
pub fn prepare_standalone(
    input: &InputFile,
    session: &Session,
    registry: &FrontendRegistry,
) -> CompilerResult<PreparedProject> {
    let options = &session.options;

    let target_type = options.target_type.unwrap_or_default();
    let target = synthesize_target(
        &synthesized_target_name(input, session, target_type),
        &standalone_target_root(input),
        target_type,
    )?;
    let frontend = select_standalone_frontend(&target, registry)?;

    let package = ProjectPackage::new(session.name.clone(), target.clone())
        .with_dependencies([core_library_dependency()]);

    // A synthesized project's profiles are preparation's to build, so `--debug` folds into the
    // one the request named — see this module's header for why the manifest case cannot. The
    // fold starts from the profile the package already seeds, so an unknown name fails here with
    // the same diagnostic a manifest project's would, and `dev` and `release` keep everything
    // else they define. `trim_paths` is *not* folded: no compiler flag asks for it, so any value
    // chosen here would be invented rather than requested, and the profile's own default —
    // `false` for `dev`, `true` for `release` — is the only answer the request supports.
    let profile_name = options.profile.clone();
    let mut profile = package.resolve_profile(&profile_name)?.clone();
    profile.enable_debug_info(options.debug != DebugInfo::None);
    let package = package.with_profile(profile);

    Ok(PreparedProject {
        package: Arc::from(package),
        manifest_path: PathBuf::new(),
        target,
        profile_name,
        frontend,
    })
}

/// Construct the target named `name`, rooted at `target_root`, of type `target_type`.
///
/// Shared with [`prepare_standalone`], which synthesizes the target of a real standalone build, so
/// everything below is a correctness requirement rather than a fixture detail.
///
/// # The namespace is decided by the target type
///
/// Two of the six types are rooted at a reserved namespace, and each is reserved because
/// something addresses it by that name:
///
/// - **Executables** are rooted at `$exec`, the path `MasmComponent::source_inputs` gives the
///   module it builds with `Module::new_executable`. `load_target_sources` rejects a root
///   module that does not sit exactly at its target's namespace, so a name-derived namespace
///   fails every executable.
/// - **Kernels** are rooted at `$kernel`, mirroring the manifest, which maps
///   `TargetType::Kernel` to `Path::kernel_path()` unconditionally and never derives a kernel's
///   namespace from a name. It has to: semantic analysis rewrites every `syscall.foo` to
///   `$kernel::foo`, so a kernel assembled elsewhere exports procedures no `syscall` can
///   address, and the linker's `link_with_kernel` asserts that a kernel package contains a
///   module whose path `is_kernel_path()`.
///
/// The remaining four — library, account component, note and transaction script — take the
/// absolutized `name`, and their target *name* is derived from that namespace, so a library
/// named `foo` has the target name `::foo`. That is what distinguishes it from the `foo` an
/// executable of the same package gets, and therefore what lets one package hold both. The two
/// reserved-namespace types keep the caller's `name` instead: their namespace is a sentinel
/// shared by every target of that type and could not identify one.
///
/// # Why not `Target::executable` and `Target::library`
///
/// Both are thin wrappers over [`Target::new`] that hardcode a target type, and `Target::library`
/// hardcodes `TargetType::Library` — so routing the four library-like types through it silently
/// discards the requested type. `assemble_source_package` asserts the assembled package's kind
/// equals its target's, so that would emit a `Library`-kind package for an account component.
/// The wrappers' *shapes* are reproduced exactly; only the type is carried through.
pub(crate) fn synthesize_target(
    name: &str,
    target_root: &Path,
    target_type: TargetType,
) -> CompilerResult<Target> {
    use miden_assembly_syntax::Path as MasmPath;

    let uri = Uri::from(target_root);
    match target_type {
        TargetType::Executable => Ok(Target::new(target_type, name, MasmPath::exec_path(), uri)),
        TargetType::Kernel => Ok(Target::new(target_type, name, MasmPath::kernel_path(), uri)),
        _ => {
            let namespace: Arc<MasmPath> = MasmPath::new(name)
                .to_absolute()
                .map(|path| Arc::from(path.into_owned()))
                .map_err(|err| Report::msg(format!("invalid namespace '{name}': {err}")))?;
            let derived_name: Arc<str> = namespace.as_str().into();
            Ok(Target::new(target_type, derived_name, namespace, uri))
        }
    }
}
/// The path a standalone `input`'s synthesized target is rooted at.
///
/// A file input is rooted at itself. Standard input has no path, and cannot simply be given
/// its name: `TargetAssemblyContext::new_virtual` resolves the target root through
/// `Uri::to_path`, and [`select_frontend`] dispatches on that path's *extension* — so a
/// nameless `stdin` would reach no frontend at all. The input's own file type supplies the
/// extension, giving `stdin.wat` for wasm text piped in under the default name.
///
/// [`Path::with_extension`] rather than a format string, so that a caller-supplied name which
/// already carries the right extension is left as it is instead of gaining a second one.
fn standalone_target_root(input: &InputFile) -> PathBuf {
    match input.as_path() {
        Some(path) => path.to_path_buf(),
        None => {
            PathBuf::from(input.file_name().as_str()).with_extension(input.file_type().to_string())
        }
    }
}

/// Reject a seeded request whose `input` names no path.
///
/// A seed replaces *reading* the input, not the input itself. Which project is prepared, which
/// target it holds, and therefore which route the seed resumes are all still derived from the
/// input — and for a standalone request that derivation runs through its **path**:
/// `standalone_target_root` roots the synthesized target at it, and `select_frontend`
/// dispatches on that root's extension.
///
/// A stdin-backed input has no path. The one that would be synthesized for it carries the
/// extension of the input's *file type*, which is sniffed from the bytes on standard input —
/// bytes a seeded run never compiles. Accepting one would pick the route a seed resumes from
/// content that has nothing to do with the seed, so it is refused here with a diagnostic
/// instead of being resolved by accident.
///
/// **The path need not exist.** That is the point of separating the two conditions: a caller
/// resuming a build names what its artifact came from, and `tests/support` names a `dummy.wasm`
/// that was never written. `InputFile::from_path` does not touch the filesystem, and a
/// synthesized project resolves its target root with `canonicalize().unwrap_or(..)`
/// (`TargetAssemblyContext::new_virtual`), so an absent path survives preparation intact. Only
/// a manifest-backed project, whose root is resolved with a fallible `canonicalize`, needs the
/// file to be there — and that is true of an unseeded build of it as well.
pub(crate) fn require_input_path_for_seed(input: &InputFile) -> CompilerResult<()> {
    if input.as_path().is_some() {
        return Ok(());
    }
    Err(Report::msg(format!(
        "cannot compile a seeded request from '{}': a seeded request must name the input path it \
         resumes from, because the project, target and frontend route it resumes are all derived \
         from that path. The file need not exist — only be named.",
        input.file_name(),
    )))
}

/// The name a standalone `input`'s synthesized target — and therefore its namespace — is
/// derived from.
///
/// Normally the session's: `Session::new`'s chain runs `--name`, then the output file's stem,
/// then the input's, and ends in a `panic!` — four branches whose second copy could disagree
/// with the first only silently. But when the root file *declares* what namespace it belongs
/// to, that declaration wins, and this is where it is read.
///
/// # Why the file is read here, rather than believed later
///
/// A root that declares a namespace has to be reconciled with the target's, and there are two
/// separate checks to satisfy — which is the whole reason this is decided in preparation:
///
/// - **The parser's.** [`MasmSourceProvider::provide_sources`](miden_assembly::MasmSourceProvider)
///   passes `Some(target.namespace)` **unconditionally**, and semantic analysis raises a
///   namespace conflict when the source declares something else. There is no path through the
///   assembler's own provider that passes `None` and lets the file win, so "believe the file at
///   parse time" is not reachable without displacing upstream or writing a second provider.
/// - **The assembler's.** `load_target_sources` separately compares the parsed root module's
///   path against `target.namespace`. So even a parser that adopted the file's declaration
///   would still be rejected against a target namespace synthesized from the artifact name.
///
/// Deriving the *target* namespace from the file is the only arrangement in which both checks
/// agree. Everything downstream then works as it does for a manifest-declared target, which is
/// the point.
///
/// # `--name` asserts, it does not override
///
/// When `--name` is given it is passed through unconditionally, and a root that declares a
/// *different* namespace then fails semantic analysis. The flag's name implies the stronger
/// meaning, so state it plainly: `--name` only *chooses* the namespace when the file is silent;
/// when the file speaks, `--name` asserts what it must say. True override semantics were
/// considered and rejected — they would mean rewriting the module path after the parse and
/// silently discarding what the source says about itself.
///
/// # Executables and kernels are not consulted about their namespace
///
/// [`synthesize_target`] derives a namespace from this name for the four library-like types
/// only: an executable is rooted at `$exec` and a kernel at `$kernel` whatever they are called.
/// So a `.masm` *program* that declares a namespace of its own is rejected by semantic analysis,
/// and correctly — its root module has to sit at `$exec` for `load_target_sources` to accept it.
/// That such a program compiles today is an artifact of the legacy path handing assembly its
/// sources ready-made and never reaching that check; it is the same delta the `$exec` namespace
/// itself carries, not one this scan introduces.
///
/// **The scan is therefore skipped for those two types outright**, rather than left to be
/// discarded by `synthesize_target`. For a reserved-namespace target this name is not thrown
/// away — it becomes the *target name*, which for an executable folds into the package id as
/// `<project>:<target>`. Almost every declaration would be caught by the namespace conflict
/// before that mattered, but not all of them: a program declaring `namespace $exec` (or a kernel
/// declaring `$kernel`) agrees with the namespace it is given, assembles cleanly, and would ship
/// under a package id derived from the declaration rather than from the artifact name. Matching
/// `synthesize_target`'s own arms here makes "the file has no say" structural.
///
/// # The rule is shared across formats; the reading is not
///
/// "The file says what it is" is also how a standalone `.hir` input is handled: a `.hir` file
/// declares its own component id, codegen roots the Miden Assembly at
/// `ComponentId::to_library_path()`, and no name-derived library namespace can ever equal
/// that. Only the *extraction* differs by format — a `namespace` declaration in Miden Assembly,
/// a component id in HIR — which is why [`declared_namespace`] dispatches on the input's file
/// type and each format gets its own reader: [`masm_namespace_declaration`] and
/// [`hir_declared_namespace`].
///
/// The complementary mechanism is codegen's re-rooting (`MasmComponent::source_inputs` in
/// `codegen/masm`), and the line between the two is *whether the source named its own root*, not
/// which format it is written in. Re-rooting covers the roots nobody wrote: the synthetic wrapper
/// the Wasm frontend builds around every core Wasm module, and a world declaring no component at
/// all, whose root code generation has to invent. Those need no declaration read out of them,
/// because they are moved to whatever namespace the target ends up with.
///
/// What re-rooting deliberately does *not* cover is an authored component id, which is the code's
/// own identity — moving it would rename the procedures every dependent addresses. So for a `.hir`
/// file declaring a component, this scan is the only thing that can make the two namespaces agree,
/// which is why [`hir_declared_namespace`] re-renders the id through `ComponentId` rather than
/// echoing the token it found.
///
/// Two mechanisms, and no third: either the file names its root, and the target is derived from
/// it here, or it does not, and codegen puts the root wherever the target says.
fn synthesized_target_name(
    input: &InputFile,
    session: &Session,
    target_type: TargetType,
) -> String {
    // These two arms are `synthesize_target`'s reserved-namespace arms, restated so that a
    // declaration cannot reach the *target name* of a target whose namespace it has no say in.
    if matches!(target_type, TargetType::Executable | TargetType::Kernel)
        || session.options.name.is_some()
    {
        return session.name.clone();
    }
    declared_namespace(input).unwrap_or_else(|| session.name.clone())
}

/// The namespace `input`'s root source declares for itself, if its format has such a thing and
/// it uses it.
///
/// Dispatch is on the input's own file type rather than on the target root's extension: the two
/// agree by construction ([`standalone_target_root`] derives the latter from the former), and
/// the file type is what says how to *read* the bytes.
///
/// A root that cannot be read is not an error here. The read is a pre-scan, not the compilation:
/// a missing or non-UTF-8 root fails with the parser's own diagnostic a moment later, and
/// reporting it twice — once in a worse form — helps nobody. Seeded requests rely on this too;
/// they name an input path that need not exist.
fn declared_namespace(input: &InputFile) -> Option<String> {
    match input.file_type() {
        FileType::Masm => masm_namespace_declaration(&root_source_text(input)?),
        FileType::Hir => hir_declared_namespace(&root_source_text(input)?),
        // Every other format — WebAssembly, Rust — has nothing to declare, and keeps the
        // artifact name.
        _ => None,
    }
}

/// The text of `input`'s root source, however the input carries it.
fn root_source_text(input: &InputFile) -> Option<String> {
    match &input.file {
        InputType::Real(path) => std::fs::read_to_string(path).ok(),
        InputType::Stdin { input, .. } => core::str::from_utf8(input).ok().map(ToString::to_string),
    }
}

/// The namespace a Miden Assembly module declares, if its first item is a declaration.
///
/// Scanned rather than parsed. Parsing would mean reading and analyzing the whole module tree
/// before the target it is to be parsed *against* exists, and then doing it again for real; the
/// declaration is a single leading item, so a scan is enough to find it.
///
/// The scan is deliberately no more permissive than the grammar. A `namespace` form is only a
/// declaration when it is the module's **first** item — anything else is a
/// `MisplacedNamespaceDeclaration` to semantic analysis — so only the first non-comment,
/// non-blank line is considered. Comments are skipped because they are trivia; a doc comment
/// attached to the following item is not, but a source in which that matters is one semantic
/// analysis rejects whatever namespace it is given, so nothing here can turn a good build bad.
///
/// # What it does not accept
///
/// Being line-oriented where the grammar is token-oriented, it misses two spellings the parser
/// would accept: a declaration split across lines (`namespace\n    foo`), and a quoted path
/// containing whitespace (`namespace "foo bar"`, which truncates at the space and surfaces as
/// `invalid namespace '"foo'`). Both **fail closed** — the namespace this returns is re-checked
/// against the source by semantic analysis, unconditionally, so a mis-scan is a build that
/// fails rather than one that succeeds under the wrong namespace — but they fail with a
/// misdirecting message. Reformatting the declaration onto one line is the fix; widening this
/// to a token scan is the other, if either shape turns out to occur.
fn masm_namespace_declaration(source: &str) -> Option<String> {
    let first_item = source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;
    let declared = first_item.strip_prefix("namespace")?;
    // A keyword only when a separator follows it, or `namespaced_thing` would parse as one.
    if !declared.starts_with(char::is_whitespace) {
        return None;
    }
    // The first token, so that a trailing comment or trailing whitespace is not taken for part
    // of the path.
    declared.split_whitespace().next().map(ToString::to_string)
}

/// The operation whose declaration names a `.hir` root's namespace, when it declares one.
const HIR_COMPONENT_OP: &str = "builtin.component";

/// The operation that names it when no component does; see [`hir_declared_namespace`] for why
/// there are two and in which order they are read.
const HIR_MODULE_OP: &str = "builtin.module";

/// The namespace a file of HIR text declares for itself, rendered the way codegen roots that
/// file's Miden Assembly.
///
/// Scanned rather than parsed, for the reason [`masm_namespace_declaration`] is: this runs
/// *before* the target the HIR will be parsed against exists, and the real parse follows a
/// moment later. Here there is a second reason, which is the seam's shape. [`declared_namespace`]
/// returns an [`Option`] with no error channel, so a parse failure could only be swallowed into
/// `None` — and would then resurface downstream as a namespace mismatch instead of the syntax
/// error it is. A scan has nothing to swallow.
///
/// # Two shapes, because codegen roots two ways
///
/// What the target's namespace must equal is whatever `codegen/masm` roots the lowered Miden
/// Assembly at, because `load_target_sources` rejects a root module that does not sit exactly at
/// its target's namespace. Codegen decides that in one of two places, and this mirrors both:
///
/// - **A file declaring a component** is rooted at that component's *id*
///   (`ToMasmComponent for builtin::Component`). This is the case whether the component stands
///   alone or is nested in a `builtin.world`, because a world holding one component is lowered by
///   lowering that component.
/// - **A file declaring no component** is rooted at the *target's own namespace*
///   (`MasmComponent::source_inputs`), because such a world has no identity beyond the namespace
///   its modules sit in. Reading its single top-level module's name is therefore not a second
///   rule to keep in step but the way this scan *chooses* that namespace: what it returns is what
///   codegen then roots at, and codegen's equality guard makes the re-rooting a no-op. A world of
///   modules is what `hir-opt` fixtures and `--emit=hir` output for a module-only program look
///   like, so this is not an exotic shape.
///
/// The two are tried in that order, and the fall-through is on *nothing being declared*, never on
/// a component being unreadable — which is what keeps the modules **inside** a component, the
/// commonest shape of all, from being mistaken for a top-level one.
///
/// # The names are re-rendered, not echoed
///
/// A component's id is handed to the very same
/// [`ComponentId`](midenc_hir::dialects::builtin::ComponentId) parser the
/// [`Component`](midenc_hir::dialects::builtin::Component) op uses, and rendered by the very same
/// `to_library_path` codegen calls. That is not cosmetic: a component id may omit its version,
/// and `ComponentId` supplies `1.0.0`, so `@"a:b"` must become `"a:b@1.0.0"` — echoing the token
/// would produce a namespace codegen never roots anything at. A module's name needs no such
/// interpretation, and both sides normalize it identically: [`synthesize_target`] absolutizes it
/// through `Path::to_absolute` while codegen builds `PathBuf::new("::{module}")`, and both route
/// every component through `PathBuf::push_component`, which decides quoting.
///
/// This leaves the scan responsible for exactly one thing: *locating* the name. Everything after
/// that is shared code, which is what makes a mis-scan fail rather than mislead.
///
/// # A component id is always quoted; a module name need not be
///
/// `namespace:name@version` is **one** symbol-path component, and neither `:` nor `@` is an
/// identifier character — so the lexer only produces such a name from a string literal, and the
/// op's parser rejects a bare `@name` with "invalid component id: missing namespace identifier".
/// Both spellings are therefore scanned and the difference is left to `ComponentId`: a component
/// declaring a bare name is rejected here because it is rejected there.
///
/// # What it does not accept
///
/// - **A file declaring neither a component nor a module** — nothing was declared, so the
///   artifact name stands, as it does for a `.wasm`.
/// - **A file declaring more than one component.** There is no single id to be rooted at, and
///   picking one would be an invention. Saying nothing agrees with codegen, which rejects such a
///   world outright naming the package-metadata limitation (`too_many_components` in
///   `codegen/masm`), so the build fails on the limitation rather than on a namespace this
///   guessed at.
/// - **A file declaring more than one top-level module**, which declares no single namespace and
///   for which none has to be guessed: codegen roots a component-less world at whatever namespace
///   the target ends up with, so the artifact name is as good an answer as any and the root-module
///   check passes either way. (Assembly may still fail further on — the modules such a world
///   declares are siblings of its root rather than children of it, and so sit outside the
///   namespace — but that is a shape question, not a namespace one.)
/// - **Declarations nested where codegen does not count them.** Both counts are of declarations
///   *anywhere* in the file, while codegen counts only a world's direct children — so a component
///   inside a component, or a module inside a module, is counted here and is not there. Such a
///   file falls back to the artifact name.
/// - **A declaration split across lines**, one whose visibility is joined to its name without
///   whitespace (`private@"a:b"`, which the lexer does tokenize), and **a name containing an
///   escaped quote or the sequence `//`** — being line-oriented and delimiter-driven where the
///   lexer is neither.
///
/// All of these **fail closed**, in the same sense [`masm_namespace_declaration`]'s misses do:
/// the namespace this returns is compared against what codegen produces from the same file, so
/// anything mis-scanned is a build that fails rather than one that assembles under a namespace
/// the source never claimed. What is lost is the quality of the message, not the outcome.
fn hir_declared_namespace(source: &str) -> Option<String> {
    use midenc_hir::dialects::builtin::ComponentId;

    let components = hir_declarations(source, HIR_COMPONENT_OP)?;
    if !components.is_empty() {
        let [id] = components[..] else {
            return None;
        };
        let id = id.parse::<ComponentId>().ok()?;
        return Some(id.to_library_path().to_string());
    }

    let [module] = hir_declarations(source, HIR_MODULE_OP)?[..] else {
        return None;
    };
    Some(module.to_string())
}

/// The names declared by every `op_name` declaration in `source`, in order.
///
/// `None` — rather than a shorter list — when any occurrence of `op_name` cannot be read as a
/// declaration, because a scan that skipped what it could not understand would report a *count*
/// the file does not have, and the count is what decides whether anything is declared at all.
fn hir_declarations<'a>(source: &'a str, op_name: &str) -> Option<Vec<&'a str>> {
    let mut declared = Vec::new();
    for line in source.lines() {
        // `//` runs to the end of the line, so everything after it is trivia — as it is to the
        // lexer, which skips a comment without producing a token. Truncating there is what keeps
        // a commented-out declaration from being read as one. It also truncates a name that
        // *contains* `//`, whose quote then never closes and which is therefore not read as a
        // declaration either: both are the closed direction.
        let mut rest = match line.split_once("//") {
            Some((code, _comment)) => code,
            None => line,
        };
        while let Some(at) = rest.find(op_name) {
            let (before, found) = rest.split_at(at);
            rest = &found[op_name.len()..];
            // An operation name only when nothing joins it to what precedes it, or the tail of a
            // longer identifier — both of these are legal *prefixes* of one — would be read as a
            // declaration.
            if before.chars().next_back().is_some_and(is_hir_identifier_char) {
                continue;
            }
            declared.push(hir_declared_name(rest)?);
        }
    }
    Some(declared)
}

/// The symbol name declared by `rest`, the text following an operation name.
///
/// One function for both operations because they are written alike: `builtin.component` and
/// `builtin.module` each parse a visibility keyword and then a symbol name, in that order.
fn hir_declared_name(rest: &str) -> Option<&str> {
    // A keyword only when a separator follows it, exactly as in Miden Assembly.
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    // The visibility is not optional in either grammar, so it is not optional here.
    let rest = ["public", "private", "internal"]
        .into_iter()
        .find_map(|visibility| {
            rest.strip_prefix(visibility)
                .filter(|rest| rest.starts_with(char::is_whitespace))
        })?
        .trim_start();

    // `symbol-ref-id ::= '@' (bare-id | string-literal)`, which is what the lexer accepts.
    let name = rest.strip_prefix('@')?;
    match name.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').map(|(name, _rest)| name),
        None => {
            let end = name.find(|c| !is_hir_identifier_char(c)).unwrap_or(name.len());
            Some(&name[..end]).filter(|name| !name.is_empty())
        }
    }
}

/// Whether `c` may appear in a HIR identifier, and so cannot end one thing and begin another.
///
/// The lexer's own continuation set for a bare identifier (`lex_keyword_or_ident`), which is
/// wider than an ASCII word: `.` is what makes `builtin.component` a single token in the first
/// place.
fn is_hir_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$' | '.')
}

/// The core library dependency every standalone build links against.
///
/// A manifest declares its own dependencies; a synthesized project has only this one, and
/// without it the dependency graph has nothing to resolve `miden-core` from and nothing links.
/// `Session::new` adds the same dependency to the project it synthesizes, in the same shape:
/// any version, resolved from the registry, linked dynamically.
fn core_library_dependency() -> Dependency {
    Dependency::new(
        Span::unknown("miden-core".to_string().into()),
        DependencyVersionScheme::Registry(VersionRequirement::Semantic(Span::unknown(
            VersionReq::STAR.clone(),
        ))),
        Linkage::Dynamic,
    )
}

/// Resolve `input` into the project, target and frontend a compilation request runs with.
///
/// The locator is normalized first, then the project is loaded from it, then the requested
/// target is selected, and finally the frontend is chosen from that target's root.
pub fn prepare_project(
    input: &InputFile,
    options: &Options,
    registry: &FrontendRegistry,
    source_manager: &dyn SourceManager,
) -> CompilerResult<PreparedProject> {
    let manifest_path = normalize_locator(input)?;

    // The project is loaded here rather than taken from the session, and that is
    // load-bearing. `Session::new` loads this same manifest, but for a `Cargo.toml` input
    // whose target type is executable it then replaces the package with the one
    // `fixup_cargo_target` rebuilds — which rewrites library targets' namespaces, and, being
    // built by `Package::new`, has no manifest path. That fixed-up package is not what a
    // project build has ever compiled: the stage this preparation replaced assembled through
    // `for_project_at_path_with_providers`, which loads the manifest itself, so the package it
    // built is this one — manifest-backed and un-fixed-up. Substituting
    // `session.project.package()` here would look like a simplification and would change two
    // things at once: the required library would be assembled under the rewritten namespace,
    // and `DependencyGraph::from_project` branches on the package's manifest path, so a missing
    // one takes the virtual path and yields a different dependency graph altogether.
    //
    // What that costs is a second load of the same manifest in every project build: `Session`
    // loaded it once already (`midenc-session/src/lib.rs`, `Session::new`). That is not new —
    // the legacy path also loaded twice, because `for_project_at_path_with_providers` re-loaded
    // the manifest internally — and the way to converge on one load is to remove `Session`'s own
    // Toml branch, not to drop this one for a package it does not build.
    let project = Project::load(&manifest_path, source_manager).map_err(|err| {
        err.wrap_err(format!("failed to load Miden project from {}", manifest_path.display()))
    })?;
    let package = project.package();

    // `Session` derives the artifact name from `--name` if given, and otherwise from the
    // loaded package's name (see `Session::new`). Preparation takes only `Options`, so that
    // rule is restated here rather than read off a session — and
    // `the_selected_executable_is_the_one_the_session_names` runs both, so the two cannot
    // diverge in silence.
    let name = options.name.clone().unwrap_or_else(|| package.name().inner().to_string());
    let selector = if options.target_type.unwrap_or_default().is_executable() {
        ProjectTargetSelector::Executable(name.as_str())
    } else {
        ProjectTargetSelector::Library
    };
    let target = selector.select_target(&package)?;
    let frontend = select_frontend(&target, registry)?;

    // The requested profile is carried by name, not by value, because the assembler resolves
    // it again from the package for each target it builds. Resolving it once here anyway, and
    // discarding the result, is what turns an unknown name into a diagnostic before any work
    // is done — and the diagnostic is the assembler's own, so both paths report it alike. The
    // resolved profile is not worth keeping: it borrows `package`, which would put a lifetime
    // on `PreparedProject` for a value the assembler does not accept.
    let profile_name = options.profile.clone();
    package.resolve_profile(&profile_name)?;

    Ok(PreparedProject {
        package,
        manifest_path,
        target,
        profile_name,
        frontend,
    })
}

/// Resolve the project locator `input` names to the `miden-project.toml` it stands for.
///
/// A `Cargo.toml` locates the `miden-project.toml` beside it, which is where `cargo miden`
/// writes the Miden manifest for a crate. This is the same normalization `Session::new`
/// performs, and the two must agree: they load the same project.
fn normalize_locator(input: &InputFile) -> CompilerResult<PathBuf> {
    let file_name = input.file_name();
    match file_name.file_name() {
        Some(name) if name.eq_ignore_ascii_case("Cargo.toml") => {
            let cargo_manifest_path = file_name.as_path();
            reject_unselected_workspace_root(cargo_manifest_path)?;
            Ok(cargo_manifest_path.with_file_name("miden-project.toml"))
        }
        Some(name) if name.eq_ignore_ascii_case("miden-project.toml") => {
            Ok(file_name.as_path().to_path_buf())
        }
        _ => Err(Report::msg(
            "unsupported toml input: expected either `miden-project.toml` or `Cargo.toml`",
        )),
    }
}

/// Reject `manifest_path` if it is a Cargo workspace root that selects no package.
///
/// A workspace root names members but no package of its own, so there is nothing to build;
/// which member was meant has to come from the caller.
///
/// `manifest_path` is a Cargo manifest by construction: [`normalize_locator`] is the only
/// caller, and it calls this from the arm that has just matched the file name. Nothing is
/// re-checked here. The version carried over from the deleted `stages/project.rs` did
/// re-check, with a case-*sensitive* comparison — so on a case-insensitive filesystem a
/// `cargo.toml` workspace root skipped the rejection entirely and failed later as a missing
/// Miden project.
fn reject_unselected_workspace_root(manifest_path: &Path) -> CompilerResult<()> {
    use toml_edit::DocumentMut;

    let manifest = std::fs::read_to_string(manifest_path).map_err(|err| {
        Report::msg(format!("failed to read Cargo manifest '{}': {err}", manifest_path.display()))
    })?;
    let manifest = manifest.parse::<DocumentMut>().map_err(|err| {
        Report::msg(format!("failed to parse Cargo manifest '{}': {err}", manifest_path.display()))
    })?;
    if manifest.get("workspace").is_some() && manifest.get("package").is_none() {
        Err(Report::msg(
            "unable to determine package from Cargo workspace root; run `miden build` from a \
             workspace member or select a member package explicitly with --manifest-path",
        ))
    } else {
        Ok(())
    }
}

/// Select the frontend that compiles `target`'s root for a **standalone** request.
///
/// Dispatch is the registry's, as it is for a project request — and then one extension is
/// answered differently, because for `rs` the registry's answer is the wrong one *for this
/// request only*.
///
/// A Rust target has two entry points. The registry holds
/// [`RUST_FRONTEND`](super::frontends::RUST_FRONTEND), which builds a manifest-declared target
/// by running `cargo` over the manifest; that is what a Rust *dependency* of any project is, and
/// it is what the registry must keep answering for every target this request builds beyond the
/// root. A standalone `.rs` file is not that: there is no manifest, and the file is compiled by
/// `rustc` — or by a temporary Cargo project synthesized around it — in this process. So the
/// request has to carry
/// [`RUST_STANDALONE_FRONTEND`](super::frontends::RUST_STANDALONE_FRONTEND) instead, and it
/// cannot get it from the registry, which rejects a second claim on an extension.
///
/// The two declare the *same* route — past the WebAssembly both entry points run the same shared
/// tail — so what the substitution decides is which build runs, not what `--stop-after` and
/// `--emit` may name. It was once both; see [`RUST_STANDALONE_FRONTEND`].
///
/// Only the *selected* registration is substituted, never the registry's own entry, so this
/// decides how the root target is compiled and nothing else. The driver installs a provider
/// built from what comes back for the root's extension, for this request alone.
///
/// The substitution is keyed on the registration the registry answered with rather than on the
/// `rs` extension: a registry configured with some other frontend for `rs` gets that frontend,
/// which is what registering it asked for.
fn select_standalone_frontend(
    target: &Target,
    registry: &FrontendRegistry,
) -> CompilerResult<FrontendRegistration> {
    use super::frontends::{RUST_FRONTEND, RUST_STANDALONE_FRONTEND};

    let selected = select_frontend(target, registry)?;
    Ok(if selected.id() == RUST_FRONTEND.id() {
        RUST_STANDALONE_FRONTEND
    } else {
        selected
    })
}

/// Select the frontend that handles `target`'s root.
///
/// Dispatch is on the extension of the target root, never on the manifest that declared it: a
/// `.toml` is a project locator, and no frontend compiles one.
fn select_frontend(
    target: &Target,
    registry: &FrontendRegistry,
) -> CompilerResult<FrontendRegistration> {
    let root = target.path.inner();
    let extension = target_root_extension(target);
    let Some(extension) = extension.as_deref() else {
        return Err(Report::msg(format!(
            "cannot select a frontend for target '{}': its root '{root}' has no file extension; \
             registered extensions: [{}]",
            target.name.inner(),
            registered_extensions(registry)
        )));
    };
    registry.for_extension(extension).copied().ok_or_else(|| {
        Report::msg(format!(
            "cannot select a frontend for target '{}': no frontend is registered for the \
             '{extension}' extension of its root '{root}'; registered extensions: [{}]",
            target.name.inner(),
            registered_extensions(registry)
        ))
    })
}

/// The extension of `target`'s root, which is what everything dispatches on.
///
/// Owned rather than borrowed, because the path is reconstructed from the target's `Uri` and so
/// lives no longer than this call. It is the one derivation of "what kind of file is this target
/// rooted at?" in the crate, and it must stay that way: [`select_frontend`] uses it to choose a
/// frontend and `seed.rs` uses it to choose the provider key a seed is installed under, so a
/// second copy that grew, say, case folding would make the two disagree — and the disagreement
/// would surface as an internal error on a project that is perfectly valid.
pub(crate) fn target_root_extension(target: &Target) -> Option<String> {
    target
        .path
        .inner()
        .to_path()
        .as_deref()
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .map(ToString::to_string)
}

/// The key a request-scoped provider for `prepared`'s **selected** frontend is installed under.
///
/// [`SourceProviderRegistry`](miden_assembly::SourceProviderRegistry) is keyed by the
/// `&'static str` a registration declares and by nothing else, so an override cannot be
/// installed under the target root's own extension, which is a runtime `Cow<str>`. This matches
/// the two, and hands back the registration's copy.
///
/// Both callers install a provider that must win for the root target's extension: `seed.rs`,
/// which resumes it from an artifact in hand, and [`Pipeline::compile`](super::Pipeline::compile),
/// which hands a standalone request's own input — and, for a standalone `.rs` root, a different
/// registration entirely — to that extension.
pub(crate) fn selected_provider_extension(
    prepared: &PreparedProject,
) -> CompilerResult<&'static str> {
    let root = prepared.target.path.inner();
    let extension = target_root_extension(&prepared.target);
    let Some(extension) = extension.as_deref() else {
        return Err(Report::msg(format!(
            "cannot compile target '{}': its root '{root}' has no file extension, so there is no \
             provider key to install its frontend under",
            prepared.target.name.inner(),
        )));
    };
    prepared
        .frontend
        .extensions()
        .iter()
        .copied()
        .find(|candidate| *candidate == extension)
        .ok_or_else(|| {
            Report::msg(format!(
                "internal error: frontend '{}' was selected for target root '{root}', but claims \
                 none of its extension '{extension}'",
                prepared.frontend.id(),
            ))
        })
}

/// The registry's extensions, in sorted order, for use in diagnostics.
fn registered_extensions(registry: &FrontendRegistry) -> String {
    registry.extensions().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc, string::ToString, sync::Arc};
    use std::path::Path;

    use miden_assembly::ProjectSourceInputs;
    use midenc_hir::Context;
    use midenc_session::{
        DebugInfo, Session,
        diagnostics::{DefaultSourceManager, SourceManager},
        miden_project::{Profile, TargetType},
    };

    use super::*;
    use crate::pipeline::{
        CheckpointId,
        FrontendId,
        Goal,
        RequestState,
        TargetContext,
        TargetRole,
        // The shipped HIR frontend, which is instantiated and run: the `.hir` half of the
        // pre-scan is only meaningful if preparation and that frontend agree, and nothing
        // short of running both can show that they do.
        frontends::{
            HIR_FRONTEND,
            // And its own fixtures, so that the scan cannot be tested against HIR text the
            // frontend has never been asked to compile.
            hir::tests::{COMPONENT, MODULE, WORLD},
        },
        // `WASM` is a registration for `.wasm` and `.wat` target roots whose frontend is
        // never run: preparation selects a frontend, and never instantiates one.
        registry::tests::WASM,
        testing::{VirtualProject, fixture_source, wat_fixture},
    };

    /// A library project whose target root is a `.wat` file, which [`registry`] handles.
    const LIBRARY_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[lib]
namespace = "prepare_fixture"
path = "lib.wat"
"#;

    /// The `Cargo.toml` that sits beside [`LIBRARY_MANIFEST`] in a `cargo miden` project.
    ///
    /// Deliberately not a valid Miden manifest: if preparation loaded the locator it was
    /// given instead of normalizing it, it would not quietly succeed.
    const CARGO_MANIFEST: &str = r#"
[package]
name = "prepare-fixture-crate"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
"#;

    /// A Cargo workspace root: it declares members, but names no package of its own.
    const CARGO_WORKSPACE_ROOT: &str = r#"
[workspace]
members = ["member"]
"#;

    /// The profile a project defines for itself, over and above the two every package is
    /// seeded with.
    const CUSTOM_PROFILE: &str = "checked";

    /// [`LIBRARY_MANIFEST`] plus a build profile of the project's own.
    ///
    /// [`CUSTOM_PROFILE`] is deliberately not one of the two profiles every package is seeded
    /// with, and it inherits `release` while re-enabling debug info — so a profile resolved
    /// under that name can only have come from the manifest.
    const CUSTOM_PROFILE_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[lib]
namespace = "prepare_fixture"
path = "lib.wat"

[profile.checked]
inherits = "release"
debug = true
"#;

    /// A project with two executable targets, one of them named after the package.
    ///
    /// Which of the two is selected is decided by the name, which is what
    /// [`the_selected_executable_is_the_one_the_session_names`] pins.
    const EXECUTABLE_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[[bin]]
name = "prepare_fixture"
path = "main.wat"

[[bin]]
name = "other"
path = "other.wat"
"#;

    /// A registry that handles `.wasm` and `.wat` target roots, and nothing else.
    fn registry() -> FrontendRegistry {
        let mut registry = FrontendRegistry::new();
        registry.register(WASM).expect("wasm should register");
        registry
    }

    /// The same, plus the shipped Rust *project* frontend under `rs`.
    ///
    /// Deliberately the project registration and not the standalone one: that is what the
    /// registry holds in a real build, because a Rust *dependency* of any project is a cargo
    /// build. Which of the two a standalone request runs is [`prepare_standalone`]'s to decide.
    fn registry_with_rust() -> FrontendRegistry {
        let mut registry = registry();
        registry
            .register(crate::pipeline::frontends::RUST_FRONTEND)
            .expect("the Rust project frontend should register");
        registry
    }

    /// The compiler input naming `path`, as the driver builds it from the command line.
    ///
    /// Both halves of preparation take one: a project locator for [`prepare_project`], a source
    /// file for [`prepare_standalone`].
    fn input(path: &Path) -> InputFile {
        InputFile::from_path(path).expect("a manifest or source file is a valid compiler input")
    }

    /// The package name of a prepared project, for comparing two preparations.
    fn package_name(prepared: &PreparedProject) -> String {
        prepared.package.name().inner().to_string()
    }

    /// The options a request that asked for build profile `profile` arrives with.
    fn requesting_profile(profile: &str) -> Options {
        Options {
            profile: profile.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_cargo_locator_prepares_the_project_its_sibling_manifest_names() {
        let dir = "prepare_locator";
        let miden_manifest = fixture_source(dir, "miden-project.toml", LIBRARY_MANIFEST);
        let cargo_manifest = fixture_source(dir, "Cargo.toml", CARGO_MANIFEST);
        let registry = registry();
        let options = Options::default();
        let source_manager = DefaultSourceManager::default();

        let from_cargo =
            prepare_project(&input(&cargo_manifest), &options, &registry, &source_manager)
                .expect("a Cargo locator should prepare the sibling Miden project");
        let from_miden =
            prepare_project(&input(&miden_manifest), &options, &registry, &source_manager)
                .expect("a Miden manifest locator should prepare the project it names");

        assert_eq!(
            from_cargo.manifest_path, miden_manifest,
            "a Cargo locator must be normalized to its sibling miden-project.toml"
        );
        assert_eq!(from_miden.manifest_path, miden_manifest, "a Miden manifest is used as given");
        assert_eq!(
            package_name(&from_cargo),
            package_name(&from_miden),
            "both locators name one project, so both must load one package"
        );
        assert_eq!(package_name(&from_cargo), "prepare_fixture");
        assert_eq!(
            from_cargo.target, from_miden.target,
            "both locators must select the same target"
        );
        assert_eq!(
            from_cargo.target.path.inner().as_str(),
            "lib.wat",
            "the library target of the Miden manifest, not anything derived from the Cargo one"
        );
        assert_eq!(
            from_cargo.frontend.id(),
            FrontendId::new("wasm"),
            "the frontend follows the target root's extension; a `.toml` locator is not a \
             frontend format"
        );
        assert_eq!(from_miden.frontend.id(), from_cargo.frontend.id());
        assert!(
            from_cargo.package.manifest_path().is_some(),
            "the prepared package must be the manifest-backed one: a package rebuilt in memory \
             has no manifest path, and the dependency graph takes its virtual path when the \
             manifest path is missing"
        );
    }

    #[test]
    fn an_unselected_cargo_workspace_root_is_rejected() {
        // Nothing else in this directory: were the workspace root accepted, preparation would
        // fail on the missing miden-project.toml instead, which the message check separates.
        let cargo_manifest =
            fixture_source("prepare_workspace_root", "Cargo.toml", CARGO_WORKSPACE_ROOT);

        let err = prepare_project(
            &input(&cargo_manifest),
            &Options::default(),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("a Cargo workspace root selects no package to build");

        let rendered = format!("{err}");
        assert!(
            rendered.contains("unable to determine package from Cargo workspace root"),
            "the workspace root must be rejected on its own terms, not as a missing manifest: \
             {rendered}"
        );
    }

    #[test]
    fn a_target_root_with_an_unregistered_extension_is_reported() {
        let manifest = fixture_source(
            "prepare_unregistered_extension",
            "miden-project.toml",
            &LIBRARY_MANIFEST.replace("lib.wat", "lib.masm"),
        );

        let err = prepare_project(
            &input(&manifest),
            &Options::default(),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("no frontend handles `.masm` target roots in this registry");

        let rendered = format!("{err}");
        assert!(
            rendered.contains("'masm'"),
            "the diagnostic must name the extension it could not dispatch on: {rendered}"
        );
        assert!(
            rendered.contains("wasm, wat"),
            "the diagnostic must list the registered extensions, in sorted order: {rendered}"
        );
    }

    #[test]
    fn the_requested_profile_name_reaches_the_prepared_project_unchanged() {
        let manifest = fixture_source(
            "prepare_profile_passthrough",
            "miden-project.toml",
            CUSTOM_PROFILE_MANIFEST,
        );

        // The two seeded profiles and one the manifest defines itself: preparation must not
        // substitute a default for any of them, which is what hardcoding `"dev"` did.
        for requested in ["dev", "release", CUSTOM_PROFILE] {
            let prepared = prepare_project(
                &input(&manifest),
                &requesting_profile(requested),
                &registry(),
                &DefaultSourceManager::default(),
            )
            .unwrap_or_else(|err| panic!("the manifest defines a '{requested}' profile: {err}"));

            assert_eq!(
                prepared.profile_name, requested,
                "the requested profile name is what the assembler resolves per target, so \
                 preparation must carry it through untouched"
            );
        }
    }

    #[test]
    fn a_manifest_projects_profile_is_not_rewritten_by_the_debug_level() {
        let manifest =
            fixture_source("prepare_profile_debug", "miden-project.toml", CUSTOM_PROFILE_MANIFEST);

        // `release` emits no debug info; `checked` inherits it and turns debug info back on.
        // Neither answer may move with `--debug`: a user-controlled manifest owns its profiles.
        for (requested, emits_debug_info) in [("release", false), (CUSTOM_PROFILE, true)] {
            for debug in [DebugInfo::None, DebugInfo::Line, DebugInfo::Full] {
                let options = Options {
                    debug,
                    ..requesting_profile(requested)
                };
                let prepared = prepare_project(
                    &input(&manifest),
                    &options,
                    &registry(),
                    &DefaultSourceManager::default(),
                )
                .unwrap_or_else(|err| {
                    panic!("'{requested}' should prepare under {debug:?}: {err}")
                });

                assert_eq!(prepared.profile_name, requested, "--debug must not select a profile");
                let profile = prepared
                    .package
                    .resolve_profile(&prepared.profile_name)
                    .expect("a prepared profile name resolves against its own package");
                assert_eq!(
                    profile.should_emit_debug_info(),
                    emits_debug_info,
                    "--debug {debug:?} must not fold into the '{requested}' profile of a \
                     manifest-backed project"
                );
            }
        }
    }

    #[test]
    fn a_profile_the_manifest_does_not_define_is_rejected() {
        let manifest =
            fixture_source("prepare_profile_unknown", "miden-project.toml", LIBRARY_MANIFEST);

        let err = prepare_project(
            &input(&manifest),
            &requesting_profile("nonexistent"),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("the manifest defines no 'nonexistent' build profile");

        assert_eq!(
            format!("{err}"),
            "project 'prepare_fixture' does not define a 'nonexistent' build profile",
            "the profile is resolved against the package so that an unknown name fails here with \
             the assembler's own diagnostic, rather than deep inside assembly"
        );
    }

    #[test]
    fn the_selected_executable_is_the_one_the_session_names() {
        let manifest =
            fixture_source("prepare_executable_name", "miden-project.toml", EXECUTABLE_MANIFEST);

        // Preparation restates `Session`'s naming rule from `Options` alone, so this runs both
        // and holds them to the same answer: were either side to change how the name is
        // derived, the two would pick different executable targets.
        for requested_name in [None, Some("other")] {
            let mut options = Box::new(Options::default());
            options.name = requested_name.map(ToString::to_string);
            let source_manager: Arc<dyn SourceManager + Send + Sync> =
                Arc::new(DefaultSourceManager::default());
            let session = Session::new(input(&manifest), options, None, source_manager.clone())
                .expect("a Miden manifest with executable targets should open a compiler session");

            let prepared = prepare_project(
                &input(&manifest),
                &session.options,
                &registry(),
                source_manager.as_ref(),
            )
            .expect("an executable project should prepare");

            assert_eq!(
                prepared.target.name.inner().as_ref(),
                session.name.as_str(),
                "the selected executable is the one named by the session"
            );
            assert_eq!(
                prepared.target.name.inner().as_ref(),
                requested_name.unwrap_or("prepare_fixture"),
                "an explicit --name selects that executable; without one, the package's own name \
                 does"
            );
        }
    }

    // -------------------------------------------------------------------------------------
    // Standalone inputs.
    // -------------------------------------------------------------------------------------

    /// The artifact name every standalone fixture below builds under.
    ///
    /// Set explicitly with `--name`, because `Session`'s derivation for a stdin input falls
    /// back to the *current directory's* base name, which no assertion here could spell.
    const STANDALONE_NAME: &str = "standalone_fixture";

    /// A session over `input`, named [`STANDALONE_NAME`] and otherwise configured by
    /// `configure`.
    fn standalone_session(input: InputFile, configure: impl FnOnce(&mut Options)) -> Session {
        let mut options = Box::new(Options::default());
        options.name = Some(STANDALONE_NAME.to_string());
        configure(&mut options);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        Session::new(input, options, None, source_manager)
            .expect("a source file input should open a compiler session")
    }

    /// Prepare `input` as a standalone request whose options `configure` set up.
    ///
    /// The session is built from the very same input, because that is the only shape
    /// [`prepare_standalone`] is ever called in: the name it derives the project from is
    /// `Session`'s, and pairing a session with a different input would prepare a project no
    /// run could produce.
    fn prepare_standalone_input(
        input: InputFile,
        configure: impl FnOnce(&mut Options),
    ) -> CompilerResult<PreparedProject> {
        prepare_standalone_input_with(input, &registry(), configure)
    }

    /// [`prepare_standalone_input`], against a registry the caller chooses.
    fn prepare_standalone_input_with(
        input: InputFile,
        registry: &FrontendRegistry,
        configure: impl FnOnce(&mut Options),
    ) -> CompilerResult<PreparedProject> {
        let session = standalone_session(input.clone(), configure);
        prepare_standalone(&input, &session, registry)
    }

    /// The build profile a prepared project's own package resolves its profile name to.
    fn resolved_profile(prepared: &PreparedProject) -> &Profile {
        prepared
            .package
            .resolve_profile(&prepared.profile_name)
            .expect("a prepared profile name resolves against its own package")
    }

    #[test]
    fn a_file_input_synthesizes_a_library_target_rooted_at_that_file() {
        let root = wat_fixture("prepare_standalone_library", "lib.wat");

        let prepared = prepare_standalone_input(input(&root), |_| {})
            .expect("a `.wat` file is a standalone input this registry's frontend handles");

        assert_eq!(
            prepared.target.ty,
            TargetType::Library,
            "a request that named no target type builds a library"
        );
        assert_eq!(
            prepared.target.path.inner().to_path().as_deref(),
            Some(root.as_path()),
            "the synthesized target is rooted at the input file itself"
        );
        assert_eq!(
            prepared.target.namespace.inner().as_str(),
            "::standalone_fixture",
            "a library target's namespace is the absolutized artifact name"
        );
        assert_eq!(
            package_name(&prepared),
            STANDALONE_NAME,
            "the package is named after the session, whose derivation is not restated here"
        );
        assert_eq!(
            prepared.frontend.id(),
            FrontendId::new("wasm"),
            "the frontend follows the synthesized target root's extension, exactly as it does for \
             a manifest-backed target"
        );
        assert!(
            prepared.package.manifest_path().is_none(),
            "a synthesized package has no manifest, which is what makes the dependency graph \
             treat it as virtual"
        );
        assert_eq!(
            prepared.manifest_path,
            PathBuf::new(),
            "and there is no locator to normalize, so the field carries the same empty sentinel \
             `TargetAssemblyContext::new_virtual` uses"
        );
    }

    #[test]
    fn an_executable_standalone_target_uses_the_exec_namespace() {
        let root = wat_fixture("prepare_standalone_executable", "main.wat");

        let prepared = prepare_standalone_input(input(&root), |options| {
            options.target_type = Some(TargetType::Executable);
        })
        .expect("a `.wat` file is a standalone input this registry's frontend handles");

        assert_eq!(
            prepared.target.namespace.inner().as_ref(),
            miden_assembly_syntax::Path::exec_path(),
            "codegen roots an executable at `$exec`, and `load_target_sources` rejects a root \
             module that does not sit at the target's own namespace — so a name-derived namespace \
             would fail every standalone executable"
        );
        assert_eq!(
            prepared.target.name.inner().as_ref(),
            STANDALONE_NAME,
            "the executable is named after the session, which is what the driver selects it by"
        );
    }

    #[test]
    fn a_standalone_targets_type_survives_synthesis() {
        let root = wat_fixture("prepare_standalone_target_types", "lib.wat");

        // The four library-*like* types. Each is reachable from the command line —
        // `--target-type account-component` and friends parse straight into `TargetType` — and
        // each must arrive at the assembler intact: `assemble_source_package` asserts the
        // assembled package's kind equals the target's, so a type quietly rewritten to
        // `Library` here emits a `Library`-kind `.masp` for an account component.
        for target_type in [
            TargetType::Library,
            TargetType::AccountComponent,
            TargetType::Note,
            TargetType::TransactionScript,
        ] {
            let prepared = prepare_standalone_input(input(&root), |options| {
                options.target_type = Some(target_type);
            })
            .unwrap_or_else(|err| panic!("a {target_type} target should prepare: {err}"));

            assert_eq!(
                prepared.target.ty, target_type,
                "the requested target type reaches the synthesized target unchanged"
            );
            assert_eq!(
                prepared.target.namespace.inner().as_str(),
                "::standalone_fixture",
                "and every library-like type keeps the name-derived namespace `Library` has"
            );
        }
    }

    #[test]
    fn a_kernel_target_is_rooted_at_the_kernel_namespace() {
        let root = wat_fixture("prepare_standalone_kernel", "lib.wat");

        let prepared = prepare_standalone_input(input(&root), |options| {
            options.target_type = Some(TargetType::Kernel);
        })
        .expect("a kernel target should prepare");

        assert_eq!(prepared.target.ty, TargetType::Kernel);
        assert_eq!(
            prepared.target.namespace.inner().as_ref(),
            miden_assembly_syntax::Path::kernel_path(),
            "`syscall` targets are rewritten to `$kernel::<name>`, so a kernel assembled under a \
             name-derived namespace exports procedures no syscall can address"
        );
        assert_eq!(
            prepared.target.name.inner().as_ref(),
            STANDALONE_NAME,
            "the name comes from the session, as it does for an executable: `$kernel` is a \
             sentinel shared by every kernel and cannot identify this target"
        );
    }

    #[test]
    fn a_stdin_input_synthesizes_a_target_root_carrying_its_extension() {
        let input = InputFile::from_bytes(b"(module)".to_vec(), "stdin".into())
            .expect("wasm text is a recognized standalone input");

        let prepared = prepare_standalone_input(input, |_| {})
            .expect("bytes on stdin are a standalone input like any other");

        assert_eq!(
            prepared.target.path.inner().as_str(),
            "stdin.wat",
            "`TargetAssemblyContext::new_virtual` resolves the target root through \
             `Uri::to_path`, so a stdin target must carry a path-shaped value"
        );
        assert_eq!(
            prepared.frontend.id(),
            FrontendId::new("wasm"),
            "and it must carry the input's own extension, because that is what frontend selection \
             dispatches on"
        );
    }

    #[test]
    fn the_debug_level_folds_into_the_synthesized_profile() {
        let root = wat_fixture("prepare_standalone_debug", "lib.wat");

        for (debug, emits_debug_info) in
            [(DebugInfo::None, false), (DebugInfo::Line, true), (DebugInfo::Full, true)]
        {
            let prepared = prepare_standalone_input(input(&root), |options| options.debug = debug)
                .unwrap_or_else(|err| panic!("a standalone build under {debug:?} prepares: {err}"));

            assert_eq!(prepared.profile_name, "dev", "--debug must not select a profile");
            let profile = resolved_profile(&prepared);
            assert_eq!(
                profile.should_emit_debug_info(),
                emits_debug_info,
                "preparation owns a synthesized project's profiles, so `--debug {debug:?}` folds \
                 into the one the request named"
            );
            assert!(
                !profile.should_trim_paths(),
                "and nothing else folds: `trim_paths` stays at the `dev` profile's own default"
            );
        }
    }

    #[test]
    fn a_release_request_synthesizes_the_release_profile_with_the_folded_debug() {
        let root = wat_fixture("prepare_standalone_release", "lib.wat");

        // `--debug` defaults to `line`, so `--release` alone is the interesting row: it yields
        // a profile that emits debug info *and* trims paths, which no standalone build saw
        // while `assemble_virtual_project_with_registry` hardcoded `dev`.
        for (debug, emits_debug_info) in [(DebugInfo::default(), true), (DebugInfo::None, false)] {
            let prepared = prepare_standalone_input(input(&root), |options| {
                options.profile = "release".to_string();
                options.debug = debug;
            })
            .unwrap_or_else(|err| panic!("a release build under {debug:?} prepares: {err}"));

            assert_eq!(
                prepared.profile_name, "release",
                "the requested profile name is what the assembler resolves per target"
            );
            let profile = resolved_profile(&prepared);
            assert_eq!(
                profile.should_emit_debug_info(),
                emits_debug_info,
                "the fold applies to whichever profile was named, not only to `dev`"
            );
            assert!(
                profile.should_trim_paths(),
                "`trim_paths` comes from the `release` profile itself, unfolded — there is no \
                 flag that asks for it"
            );
        }
    }

    #[test]
    fn the_synthesized_project_requires_the_core_library() {
        let root = wat_fixture("prepare_standalone_core", "lib.wat");

        let prepared = prepare_standalone_input(input(&root), |_| {})
            .expect("a `.wat` file is a standalone input this registry's frontend handles");

        let dependencies = prepared.package.dependencies();
        assert_eq!(
            dependencies.iter().map(|dep| dep.name().as_ref()).collect::<Vec<_>>(),
            vec!["miden-core"],
            "every standalone build links the core library; without the dependency the dependency \
             graph never resolves it and nothing links"
        );
    }

    #[test]
    fn a_standalone_rust_input_prepares_the_standalone_frontend() {
        use crate::pipeline::frontends::{RUST_FRONTEND, RUST_STANDALONE_FRONTEND};

        let root = fixture_source("prepare_standalone_rust", "lib.rs", "");

        let prepared = prepare_standalone_input_with(input(&root), &registry_with_rust(), |_| {})
            .expect("a `.rs` file is a standalone input like any other");

        assert_eq!(
            prepared.frontend.id(),
            RUST_STANDALONE_FRONTEND.id(),
            "a standalone `.rs` file has no manifest for cargo to build, so the request runs the \
             registration that compiles one file in this process — not the registry's, which runs \
             cargo over a manifest"
        );
        assert_eq!(
            registry_with_rust().for_extension("rs").map(FrontendRegistration::id),
            Some(RUST_FRONTEND.id()),
            "and the registry's `rs` entry is untouched, because that is what a Rust dependency \
             of any project is"
        );
        assert_eq!(
            prepared.frontend.resolve_alias("parse"),
            RUST_STANDALONE_FRONTEND.resolve_alias("parse"),
            "so `--stop-after=parse` resolves against the route the run actually takes"
        );
    }

    #[test]
    fn a_manifest_backed_rust_target_prepares_the_project_frontend() {
        use crate::pipeline::frontends::RUST_FRONTEND;

        // The discriminating half: the substitution above belongs to *standalone* preparation
        // alone. A manifest-backed Rust target is a cargo build whatever else is in the
        // request, and must keep the registry's own registration.
        let manifest = fixture_source(
            "prepare_project_rust",
            "miden-project.toml",
            &LIBRARY_MANIFEST.replace("lib.wat", "lib.rs"),
        );

        let prepared = prepare_project(
            &input(&manifest),
            &Options::default(),
            &registry_with_rust(),
            &DefaultSourceManager::default(),
        )
        .expect("a manifest naming a `.rs` target root should prepare");

        assert_eq!(
            prepared.frontend.id(),
            RUST_FRONTEND.id(),
            "a manifest-backed Rust target is built by cargo over its manifest, whatever else is \
             in the request"
        );
    }

    // -------------------------------------------------------------------------------------
    // The namespace pre-scan.
    // -------------------------------------------------------------------------------------

    /// A `.masm` root that declares a namespace of its own.
    ///
    /// Shaped like `tests/lit/midenc/unconstrained_advice_interprocedural.masm`, which is the
    /// only standalone `.masm` fixture in the tree: a leading comment, a blank line, and then
    /// the declaration.
    const DECLARED_MASM: &str = "# RUN: midenc -Zlint -Canalyze-only %s\n\nnamespace \
                                 declared_ns\n\npub proc entry\n    push.1\nend\n";

    /// The same root, declaring nothing.
    const SILENT_MASM: &str = "pub proc entry\n    push.1\nend\n";

    /// A `.masm` root whose `namespace` is not its first item.
    ///
    /// Semantic analysis rejects such a declaration as misplaced, so preparation must not
    /// treat it as one either: adopting it would synthesize a target the parser then refuses
    /// to root a module at.
    const LATE_MASM: &str = "pub proc entry\n    push.1\nend\n\nnamespace too_late\n";

    /// A registry that also dispatches `.masm` target roots, to the shipped MASM frontend.
    fn registry_with_masm() -> FrontendRegistry {
        let mut registry = registry();
        registry
            .register(crate::pipeline::frontends::MASM_FRONTEND)
            .expect("the MASM registration should register");
        registry
    }

    /// Prepare the file `<dir>/<file>` holding `contents` as a standalone input.
    ///
    /// Unlike [`prepare_standalone_input`] the session is built **without** `--name`, because
    /// that flag is precisely what suppresses the pre-scan. The artifact name therefore falls
    /// back to `Session`'s own derivation from the input, which for a file input is its file
    /// stem — so a fixture written to `lib.masm` is named `lib`, and that is what the
    /// fallback assertions below spell.
    fn prepare_standalone_source(
        dir: &str,
        file: &str,
        contents: &str,
        registry: &FrontendRegistry,
        configure: impl FnOnce(&mut Options),
    ) -> CompilerResult<PreparedProject> {
        let root = fixture_source(dir, file, contents);
        let input = input(&root);
        let mut options = Box::new(Options::default());
        configure(&mut options);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        let session = Session::new(input.clone(), options, None, source_manager)
            .expect("a source file input should open a compiler session");
        prepare_standalone(&input, &session, registry)
    }

    /// The namespace of the target a standalone `.masm` file prepares to.
    fn masm_namespace(dir: &str, contents: &str, configure: impl FnOnce(&mut Options)) -> String {
        let prepared =
            prepare_standalone_source(dir, "lib.masm", contents, &registry_with_masm(), configure)
                .expect("a `.masm` file is a standalone input the MASM frontend handles");
        prepared.target.namespace.inner().as_str().to_string()
    }

    #[test]
    fn a_standalone_masm_targets_namespace_is_the_one_its_root_declares() {
        // The assembler checks the root module's path against the target's namespace *and*
        // the parser rejects a source whose declaration disagrees with the path it is given.
        // Deriving the target namespace from the file is the only arrangement in which both
        // checks can pass for a root that declares one.
        assert_eq!(
            masm_namespace("prepare_standalone_masm_declared", DECLARED_MASM, |_| {}),
            "::declared_ns",
            "the root declares its own namespace, and that is what the target must be rooted at"
        );
    }

    #[test]
    fn a_standalone_masm_root_that_declares_nothing_keeps_the_session_name() {
        assert_eq!(
            masm_namespace("prepare_standalone_masm_silent", SILENT_MASM, |_| {}),
            "::lib",
            "a silent root leaves the namespace where it has always been: the artifact name"
        );
    }

    #[test]
    fn a_misplaced_namespace_is_not_a_declaration() {
        assert_eq!(
            masm_namespace("prepare_standalone_masm_late", LATE_MASM, |_| {}),
            "::lib",
            "a `namespace` that is not the root's first item is one semantic analysis rejects, so \
             preparation must not adopt it either"
        );
    }

    #[test]
    fn an_explicit_name_is_passed_through_over_the_roots_declaration() {
        // `--name` asserts rather than overrides: the namespace it names is what the target
        // gets, and a root declaring something else then fails semantic analysis. What must
        // not happen is the flag being quietly ignored in favour of the file.
        assert_eq!(
            masm_namespace("prepare_standalone_masm_named", DECLARED_MASM, |options| {
                options.name = Some("chosen".to_string());
            }),
            "::chosen",
            "an explicit --name is passed through unconditionally"
        );
    }

    #[test]
    fn a_reserved_namespace_target_is_never_named_after_its_root() {
        // `synthesize_target` discards this name for the two reserved-namespace types — but only
        // for the *namespace*. It keeps it as the target's **name**, which for an executable
        // becomes half the package id, `<project>:<target>`. A declaration that disagrees with
        // `$exec` is caught by the namespace conflict long before that matters; one that agrees
        // is not, and would ship the package under an id derived from the file. So the scan is
        // skipped for these types rather than left to be discarded downstream.
        for (target_type, reserved) in [
            (TargetType::Executable, miden_assembly_syntax::Path::exec_path()),
            (TargetType::Kernel, miden_assembly_syntax::Path::kernel_path()),
        ] {
            let prepared = prepare_standalone_source(
                &format!("prepare_standalone_masm_reserved_{target_type}"),
                "main.masm",
                &format!("namespace {reserved}\n\npub proc entry\n    push.1\nend\n"),
                &registry_with_masm(),
                |options| options.target_type = Some(target_type),
            )
            .unwrap_or_else(|err| panic!("a {target_type} target should prepare: {err}"));

            assert_eq!(
                prepared.target.namespace.inner().as_ref(),
                reserved,
                "the namespace is reserved for this target type whatever the root says"
            );
            assert_eq!(
                prepared.target.name.inner().as_ref(),
                "main",
                "and so is the name: it comes from the artifact, not from a declaration this type \
                 has no say in"
            );
        }
    }

    #[test]
    fn the_pre_scan_is_per_format() {
        // The rule — the file says what it is — is shared, but *what* is read differs by
        // format: a `namespace` declaration is Miden Assembly syntax and means nothing in a
        // `.wat`. A scan that ran over every standalone input alike would root this target at
        // `::not_a_wat_thing`.
        let prepared = prepare_standalone_source(
            "prepare_standalone_wat_namespace",
            "lib.wat",
            "namespace not_a_wat_thing\n(module)\n",
            &registry(),
            |_| {},
        )
        .expect("a `.wat` file is a standalone input this registry's frontend handles");

        assert_eq!(
            prepared.target.namespace.inner().as_str(),
            "::lib",
            "a `.wat` root has no `namespace` declaration to find, whatever its text says"
        );
    }

    #[test]
    fn what_counts_as_a_miden_assembly_namespace_declaration() {
        // The scan stands in for a parse, so what it accepts has to be what the grammar
        // accepts — no more, because a namespace nothing declared is one the parser will
        // reject the target for, and no less, because a declaration missed is a build that
        // fails for a reason the user cannot see in their own file.
        for (source, declared) in [
            ("namespace foo\n", Some("foo")),
            ("namespace foo::bar\n", Some("foo::bar")),
            ("  namespace foo  \n", Some("foo")),
            ("namespace foo # a trailing comment is trivia\n", Some("foo")),
            ("# a leading comment is too\n\n#! and a docstring\nnamespace foo\n", Some("foo")),
            // Not a declaration: no separator after the keyword.
            ("namespaced_thing\n", None),
            // Not a declaration: nothing is declared.
            ("namespace\n", None),
            // Not a declaration: `namespace` is only a form as a module's *first* item.
            ("pub proc entry\nend\n\nnamespace late\n", None),
            // Nothing to find at all.
            ("", None),
            ("begin\n    push.1\nend\n", None),
        ] {
            assert_eq!(
                masm_namespace_declaration(source).as_deref(),
                declared,
                "scanning {source:?}"
            );
        }
    }

    // -------------------------------------------------------------------------------------
    // The `.hir` half of the pre-scan.
    // -------------------------------------------------------------------------------------

    /// The namespace a target rooted at [`WORLD`] or [`COMPONENT`] must be given.
    ///
    /// One **quoted** path component, because that is what `ComponentId::to_library_path`
    /// produces and therefore where codegen roots the Miden Assembly: the `:` and the `@` are
    /// part of the name, not path separators. Preparation absolutizes what it scanned, so the
    /// target's namespace carries the `::` prefix that codegen's own `to_absolute` adds.
    const COMPONENT_NAMESPACE: &str = "::\"hir_ns:test@1.0.0\"";

    /// A registry that also dispatches `.hir` target roots, to the shipped HIR frontend.
    fn registry_with_hir() -> FrontendRegistry {
        let mut registry = registry();
        registry.register(HIR_FRONTEND).expect("the HIR registration should register");
        registry
    }

    /// The namespace of the target a standalone `.hir` file holding `contents` prepares to.
    fn hir_namespace(dir: &str, contents: &str, configure: impl FnOnce(&mut Options)) -> String {
        let prepared =
            prepare_standalone_source(dir, "lib.hir", contents, &registry_with_hir(), configure)
                .expect("a `.hir` file is a standalone input the HIR frontend handles");
        prepared.target.namespace.inner().as_str().to_string()
    }

    /// A world declaring **two** components, built by doubling the shared [`COMPONENT`] fixture.
    ///
    /// Derived rather than written out so that it cannot drift from the fixture the frontend is
    /// tested against: it is that component, plus the same component under a second id. The
    /// second id differs only in its *name*, which is enough to make the two distinct components
    /// and keeps the substitution a single unambiguous replacement.
    fn two_component_world() -> String {
        format!(
            "builtin.world {{{}{}}};\n",
            COMPONENT,
            COMPONENT.replace("hir_ns:test", "hir_ns:other")
        )
    }

    /// Lower the target `prepared` synthesized, with the shipped HIR frontend.
    ///
    /// Stops at `masm.lowered`, which is where the Miden Assembly module tree first exists and
    /// therefore where its **root module's path** — the value `load_target_sources` compares
    /// against the target's namespace — is decided.
    fn lower_prepared_hir(prepared: &PreparedProject) -> CompilerResult<ProjectSourceInputs> {
        let project = VirtualProject::for_prepared_target(prepared)?;
        let assembly = project.assembly_context()?;
        let state = RequestState::new(Goal::at(CheckpointId::MASM_LOWERED), Vec::new());
        let cx = TargetContext::for_testing(
            &assembly,
            Rc::new(Context::default()),
            TargetRole::Root,
            &state,
        );

        // The returned `ControlFlow` is dropped rather than asserted on: it is not `Debug`, and
        // what this needs is the artifact captured at the goal.
        let _ = HIR_FRONTEND.instantiate(cx.session()).compile(&cx)?;
        Ok(state
            .take_outcome()
            .expect("stopping at masm.lowered must capture the lowered sources")
            .downcast::<ProjectSourceInputs>()
            .expect("the lowered artifact is the assembler's own source inputs"))
    }

    #[test]
    fn a_standalone_hir_targets_namespace_is_the_component_id_its_root_declares() {
        // Both shapes a `.hir` file may take, because the component may or may not be nested in
        // a world and codegen roots at its id either way — a world holding one component is
        // lowered by lowering that component.
        for (dir, contents) in [
            ("prepare_standalone_hir_world", WORLD),
            ("prepare_standalone_hir_component", COMPONENT),
        ] {
            assert_eq!(
                hir_namespace(dir, contents, |_| {}),
                COMPONENT_NAMESPACE,
                "a `.hir` root declares its own component id, and codegen roots the Miden \
                 Assembly at that id — so no name-derived namespace can ever be right for one"
            );
        }
    }

    /// A file declaring no component is rooted at the module it does declare.
    ///
    /// The discriminating half is the **renamed** row: the shared [`MODULE`] fixture is named
    /// after the file that holds it, so a scan that read nothing would still produce `::lib` for
    /// it and this test would pass for the wrong reason. Renaming the module separates the two
    /// answers — the artifact name is still `lib`, and only reading the declaration gives
    /// `::renamed`, which is where codegen roots it.
    #[test]
    fn a_hir_root_that_declares_no_component_is_rooted_at_its_module() {
        assert_eq!(
            hir_namespace("prepare_standalone_hir_module", MODULE, |_| {}),
            "::lib",
            "a world of modules is rooted at its single top-level module's own name"
        );
        assert_eq!(
            hir_namespace(
                "prepare_standalone_hir_module_renamed",
                &MODULE.replace("@lib", "@renamed"),
                |_| {}
            ),
            "::renamed",
            "and that name comes from the file, not from the artifact it is being built as"
        );
    }

    #[test]
    fn a_hir_root_declaring_neither_a_component_nor_a_module_keeps_the_session_name() {
        // The fallback is meaningful only if it applies when the file declares nothing at all.
        assert_eq!(
            hir_namespace("prepare_standalone_hir_silent", "builtin.world {\n};\n", |_| {}),
            "::lib",
            "nothing was declared, so the namespace stays where it has always been: the artifact \
             name"
        );
    }

    /// A file declaring several top-level modules declares no namespace, and codegen agrees.
    ///
    /// The scan says nothing because there is no single declaration to read, so the artifact name
    /// stands. That used to *disagree* with codegen, whose `_` arm in
    /// `world_body_to_masm_component`'s `match toplevel_namespaces.len()` roots at the constant
    /// `::init` — a name no file declares and no synthesized namespace can equal. Mirroring that
    /// constant here was refused, because it would put one value in two places that must agree
    /// and would let such a build assemble under a namespace no source claimed.
    ///
    /// Codegen closed it instead: `MasmComponent::source_inputs` re-roots a component-less world
    /// at its target's namespace, exactly as it does the synthetic wrapper. The second assertion
    /// is what makes this pair an oracle rather than two independent claims — it lowers the very
    /// project preparation synthesized, and requires the path codegen hands the assembler to be
    /// the namespace preparation chose, which is the equality `load_target_sources` enforces.
    ///
    /// It does not follow that such a file *builds*: the modules it declares are siblings of the
    /// re-rooted placeholder rather than children of it, so they stay outside the namespace and
    /// assembly still fails — on that, now, rather than on a namespace mismatch. See
    /// `a_world_of_several_modules_is_rooted_at_the_target_namespace` in `codegen/masm`.
    #[test]
    fn a_hir_root_declaring_several_top_level_modules_declares_nothing() {
        let source =
            format!("builtin.world {{{}{}}};\n", MODULE, MODULE.replace("@lib", "@second"));
        let prepared = prepare_standalone_source(
            "prepare_standalone_hir_two_modules",
            "lib.hir",
            &source,
            &registry_with_hir(),
            |_| {},
        )
        .expect("a world of two modules is a standalone input the HIR frontend handles");

        assert_eq!(
            prepared.target.namespace.inner().as_str(),
            "::lib",
            "two modules declare no single namespace, so the artifact name stands"
        );
        assert_eq!(
            lower_prepared_hir(&prepared)
                .expect("the fixture should lower")
                .root
                .path()
                .as_str(),
            prepared.target.namespace.inner().as_str(),
            "and codegen roots a world declaring no component at its target's namespace, so \
             preparation's answer is the one the assembler is handed"
        );
    }

    #[test]
    fn a_hir_root_declaring_more_than_one_component_declares_no_namespace() {
        // Not "the first component's id": a world with two components has no single id to be
        // rooted at, and choosing one would be an invention. Codegen rejects such a world
        // outright — see `too_many_components` in `codegen/masm` — which is the answer the user
        // gets either way, so the scan says nothing rather than disagreeing with it.
        assert_eq!(
            hir_namespace("prepare_standalone_hir_two", &two_component_world(), |_| {}),
            "::lib",
            "a root declaring two components declares no namespace for its target"
        );
    }

    #[test]
    fn a_hir_root_declaring_more_than_one_component_is_rejected_by_codegen() {
        // The other half of the decision above: whatever the scan says, such a build fails, and
        // it fails naming the package-metadata limitation rather than a namespace mismatch the
        // user cannot act on.
        //
        // Be precise about what this test can and cannot carry. It would pass just as well if
        // the scan had picked the *first* component's id, because codegen refuses before any
        // namespace is compared — so it does not discriminate the scan's answer at all. That is
        // its sibling's job: `…_declares_no_namespace` pins the answer, and this pins that the
        // answer costs nothing, because the diagnostic the user sees is the same either way.
        // The pair is what settles the decision; neither test settles it alone.
        let prepared = prepare_standalone_source(
            "prepare_standalone_hir_two_lowered",
            "lib.hir",
            &two_component_world(),
            &registry_with_hir(),
            |_| {},
        )
        .expect("preparation succeeds; it is codegen that refuses");

        let err = lower_prepared_hir(&prepared)
            .err()
            .expect("a world declaring two components is not something codegen can lower");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("world containing 2 components"),
            "the failure must be codegen's own report of the package-metadata limitation, not a \
             namespace the scan guessed at: {rendered}"
        );
    }

    #[test]
    fn an_explicit_name_is_passed_through_over_the_hir_roots_component_id() {
        // As for `.masm`: `--name` asserts rather than overrides. The namespace it names is what
        // the target gets, and a root whose component id says otherwise then fails the
        // assembler's root-module check. What must not happen is the flag being quietly ignored.
        assert_eq!(
            hir_namespace("prepare_standalone_hir_named", WORLD, |options| {
                options.name = Some("chosen".to_string());
            }),
            "::chosen",
            "an explicit --name is passed through unconditionally"
        );
    }

    #[test]
    fn what_counts_as_a_hir_namespace_declaration() {
        // The scan stands in for a parse, so what it accepts has to be what the grammar
        // accepts. Every row here is a claim about the *parser*: a name this rejects is one the
        // parser rejects too, and an id it renders differently from the source spelling is one
        // `ComponentId` itself renders that way.
        for (source, declared) in [
            (
                "builtin.component private @\"hir_ns:test@1.0.0\" {\n};\n",
                Some("\"hir_ns:test@1.0.0\""),
            ),
            // Nested in a world and holding a module, which is the shape `--emit=hir` writes —
            // and the shape that decides the *order* of the two scans: the component's id wins,
            // and the module inside it is never read as a top-level one.
            (
                "builtin.world {\n  builtin.component public @\"a:b@2.1.0\" {\n    builtin.module \
                 private @inner {\n    };\n  };\n};\n",
                Some("\"a:b@2.1.0\""),
            ),
            // A component id may omit the version, and `ComponentId` supplies `1.0.0` — so the
            // namespace is *not* the text that was scanned.
            ("builtin.component internal @\"a:b\" {\n};\n", Some("\"a:b@1.0.0\"")),
            // A file declaring no component is rooted at the module it does declare, bare name
            // or quoted: `synthesize_target` and codegen both normalize it the same way.
            ("builtin.module public @lib {\n};\n", Some("lib")),
            ("builtin.world {\n  builtin.module public @lib {\n  };\n};\n", Some("lib")),
            ("builtin.module public @\"lib.rs\" {\n};\n", Some("lib.rs")),
            // Not a declaration: `ComponentId` requires a namespace, and the parser rejects a
            // bare name with "invalid component id: missing namespace identifier". The module
            // inside it must *not* be read instead — a component was declared, so its id is the
            // only answer, and there is no fall-through to the second scan.
            (
                "builtin.component private @test {\n  builtin.module public @m {\n  };\n};\n",
                None,
            ),
            // Not a declaration: nothing separates the operation name from what follows it, so
            // this is some other operation whose name merely begins the same way. What that
            // costs is worth stating exactly, because the outcome looks like a skip and is not
            // one: no name can be read here, so `hir_declarations` abandons the **whole file**
            // and nothing is declared at all. The two are indistinguishable in a fixture holding
            // one operation, and only the abandoning rule is fail-closed — a scan that skipped
            // what it could not read would report a count the file does not have.
            ("builtin.components private @\"a:b@1.0.0\" {\n};\n", None),
            // Not a declaration: the visibility keyword is not optional in either grammar.
            ("builtin.component @\"a:b@1.0.0\" {\n};\n", None),
            ("builtin.module @lib {\n};\n", None),
            // Not a declaration: a commented-out one is trivia, as it is to the lexer —
            // whether the comment is the whole line or follows something on it.
            ("// builtin.component private @\"a:b@1.0.0\" {\n", None),
            ("builtin.world {\n}; // builtin.component private @\"a:b@1.0.0\"\n", None),
            // Two components: no single id to be rooted at.
            (
                "builtin.world {\n  builtin.component private @\"a:b@1.0.0\" {\n  };\n  \
                 builtin.component private @\"c:d@1.0.0\" {\n  };\n};\n",
                None,
            ),
            // Two modules: codegen roots those at the constant `::init`, which is not something
            // the file says.
            (
                "builtin.world {\n  builtin.module public @a {\n  };\n  builtin.module public @b \
                 {\n  };\n};\n",
                None,
            ),
            // Nothing to find at all.
            ("", None),
            ("builtin.world {\n};\n", None),
        ] {
            assert_eq!(hir_declared_namespace(source).as_deref(), declared, "scanning {source:?}");
        }
    }

    #[test]
    fn a_declared_namespace_reaches_the_root_module_codegen_lowers_to() {
        // The oracle for the whole arrangement, and the only test that runs both halves.
        // Preparation derives the target's namespace from a scan of the root file; codegen
        // derives the root Miden Assembly module's path from the parsed IR. The assembler
        // compares the two (`load_target_sources`), and a build in which they differ is
        // rejected — so a scan that missed a declaration would root its target at `::lib` and no
        // standalone `.hir` build of that shape would assemble at all.
        //
        // Every shape a `.hir` file can declare a namespace in, and each is rooted by different
        // code: a component's id (`ToMasmComponent for builtin::Component`) whether or not it is
        // nested in a world, and a module's own name (`world_body_to_masm_component`). The
        // module row is renamed away from the file stem deliberately — with a module called
        // `lib` in `lib.hir`, both a working scan and a scan that read nothing produce `::lib`,
        // and the test would pass either way.
        for (dir, contents) in [
            ("prepare_standalone_hir_oracle_world", WORLD.to_string()),
            ("prepare_standalone_hir_oracle_component", COMPONENT.to_string()),
            ("prepare_standalone_hir_oracle_module", MODULE.replace("@lib", "@renamed")),
        ] {
            let prepared =
                prepare_standalone_source(dir, "lib.hir", &contents, &registry_with_hir(), |_| {})
                    .expect("a `.hir` file is a standalone input the HIR frontend handles");
            assert_ne!(
                prepared.target.namespace.inner().as_str(),
                "::lib",
                "the artifact name here is the file stem, so a target rooted at `::lib` would \
                 mean the declaration was never read"
            );

            let inputs = lower_prepared_hir(&prepared).expect("the fixture should lower");
            assert_eq!(
                inputs.root.path(),
                prepared.target.namespace.inner().as_ref(),
                "the namespace preparation synthesized is where codegen roots this target's Miden \
                 Assembly, or the assembler rejects the build"
            );
        }
    }

    #[test]
    fn a_seeded_request_must_name_an_input_path_though_it_need_not_exist() {
        // The two conditions are separated deliberately: a caller resuming a build names what
        // its artifact came from, and `tests/support` names a `dummy.wasm` that was never
        // written. What cannot be accepted is an input with no path at all, whose synthesized
        // target root — and therefore the route a seed would resume — is derived from a sniff
        // of bytes the seeded run never compiles.
        let absent = Path::new("this-file-was-never-written.wasm");
        assert!(!absent.exists(), "the fixture must name a file that does not exist");
        require_input_path_for_seed(&input(absent))
            .expect("a seeded request may name an input path that does not exist");

        let stdin = InputFile::from_bytes(b"(module)".to_vec(), "stdin".into())
            .expect("wasm text is a recognized input");
        let err = require_input_path_for_seed(&stdin)
            .expect_err("a seeded request must name the input it resumes from");

        let rendered = format!("{err}");
        assert!(rendered.contains("seeded"), "the rejection must be the seed's own: {rendered}");
        assert!(
            rendered.contains("path"),
            "and must say what is missing, or a caller cannot act on it: {rendered}"
        );
    }

    #[test]
    fn a_profile_a_synthesized_project_does_not_define_is_rejected() {
        let root = wat_fixture("prepare_standalone_unknown_profile", "lib.wat");

        let err = prepare_standalone_input(input(&root), |options| {
            options.profile = CUSTOM_PROFILE.to_string();
        })
        .expect_err("a synthesized project has only the two profiles every package is seeded with");

        assert_eq!(
            format!("{err}"),
            format!(
                "project '{STANDALONE_NAME}' does not define a '{CUSTOM_PROFILE}' build profile"
            ),
            "a standalone request naming a profile nobody defined fails with the assembler's own \
             diagnostic, rather than silently building `dev` as the legacy path did"
        );
    }
}
