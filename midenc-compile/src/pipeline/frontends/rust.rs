//! The frontend for Rust projects driven by cargo.

use alloc::{collections::BTreeMap, format, rc::Rc};
use core::cell::RefCell;

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs};
use miden_mast_package::Package as MastPackage;
use midenc_session::{Session, diagnostics::Report};

use crate::{
    CodegenOutput, CompilerResult,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, TargetContext, TargetKey,
    },
};

/// Declares the frontend that handles targets rooted at a `.rs` file.
///
/// # Why the route holds a single checkpoint
///
/// A route describes the checkpoints a frontend *reaches*, because that is what
/// `--stop-after` and `--emit` are validated against. [`RustProjectFrontend`] reaches none
/// of the intermediate ones: its build is a nested `midenc` invocation driven by cargo,
/// which hands back only a final [`CodegenOutput`], so there is no point in this process at
/// which the initial HIR, the transformed HIR or the lowered Miden Assembly exists to be
/// published. Declaring them anyway would let `--stop-after=lower` resolve to a checkpoint
/// the run never reaches, and the request would then end with an empty capture slot rather
/// than a diagnostic.
///
/// [`CheckpointId::PACKAGE_ASSEMBLED`] is on the route because the *driver* publishes it
/// once the assembler returns a package, as it does for every frontend. So this route is
/// the honest minimum: a Rust build either produces a package or fails.
///
/// Surfacing the nested run's checkpoints — which would grow this route — is a later
/// increment's work; see [`Frontend::compile`] on [`RustProjectFrontend`].
pub const RUST_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("rust"),
    &["rs"],
    &[CheckpointId::PACKAGE_ASSEMBLED],
    &[("assemble", CheckpointId::PACKAGE_ASSEMBLED)],
    // The assembled package is written by [`crate::compile`]; see [`unrendered`].
    &[ArtifactDecl {
        checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
        id: ArtifactId::PACKAGE,
        render: unrendered,
    }],
    make_rust,
);

/// Build the frontend this registration declares.
fn make_rust(session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(RustProjectFrontend::new(session))
}

/// This frontend's renderer, which writes nothing.
///
/// The assembled package is already written by [`crate::compile`], which emits it in both
/// `mast` and `masp` form once the pipeline hands it back; declaring a renderer here would
/// make two writers for one artifact. The `masm` route declares its package artifact the same
/// way, for the same reason.
fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
    Ok(())
}

/// Compiles a target whose sources are Rust, by shelling out to cargo.
///
/// The build is a *nested* compilation: `cargo_build` runs `cargo` with a derived
/// [`midenc_session::Options`], and that inner compiler run performs parsing, rewriting and
/// codegen itself. Nothing of it is observable from here, so this frontend publishes no
/// checkpoints at all — see [`Frontend::compile`].
///
/// [`Frontend::compile`], [`Frontend::provenance`] and [`Frontend::post_process`] are each
/// called for the same target, the last of them against a *fresh* [`TargetContext`], so the
/// build's [`CodegenOutput`] is memoized in `compiled` and every method reaches it by
/// [`TargetContext::target_key`].
pub struct RustProjectFrontend {
    session: Rc<Session>,
    /// The cargo build's output, per target.
    ///
    /// Keyed by [`TargetKey`] rather than by package identity: a project with both a `[lib]`
    /// and a `[[bin]]` has one package id for two targets, so a package-keyed cache serves
    /// the library's codegen output to the executable.
    compiled: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
}

impl RustProjectFrontend {
    /// Construct a frontend that builds within `session`, with an empty cache.
    pub fn new(session: Rc<Session>) -> Self {
        Self {
            session,
            compiled: RefCell::new(BTreeMap::new()),
        }
    }

    /// Construct a frontend whose cache already holds `output` for the target `key` names.
    ///
    /// For the caller that has *already* produced this target's [`CodegenOutput`] in-process:
    /// it hands the output over so that [`Frontend::compile`],
    /// [`Frontend::provenance`] and [`Frontend::post_process`] all serve this target from
    /// the seed, and no nested cargo build is spawned to reproduce work the running process
    /// just did.
    ///
    /// # Building the key
    ///
    /// `key` must be built from the *selector-resolved target*, not from the package. This
    /// is the hazard the switch to target keying introduces: the seed key this replaces was
    /// `(package name, version)`, which is a property of the package, but `compile`,
    /// `provenance` and `post_process` all look up [`TargetContext::target_key`], which
    /// carries the target's name and [`TargetType`](midenc_session::miden_project::TargetType)
    /// too. A package-derived key still type-checks and still inserts, it simply never
    /// matches: the seed is silently ignored, and the target is either rebuilt by cargo or,
    /// in `post_process`, reported as the `internal error` a cache miss raises. Resolve the
    /// target from the selector first and build the key from it.
    pub fn seeded(session: Rc<Session>, key: TargetKey, output: CodegenOutput) -> Self {
        Self {
            session,
            compiled: RefCell::new(BTreeMap::from([(key, output)])),
        }
    }

    /// Run cargo for this target, uncached.
    ///
    /// The build is configured from `self.session`, the session of the project cargo is
    /// invoked *in*, not from `cx.session()`: the package cache lives under the root
    /// project's `target/` directory and the cargo flags come from the root invocation, and
    /// both must be the same for every target of the build.
    fn build(&self, cx: &TargetContext<'_>) -> CompilerResult<CodegenOutput> {
        let assembly = cx.assembly();
        let filesystem_cache_dir = self
            .session
            .project
            .manifest_path()
            .and_then(|p| p.parent())
            .map(|p| p.join("target").join("miden").join("packages"));
        let cargo_opts = crate::cargo::CargoOptions::from_compiler(&self.session.options)?;
        let source_manager = self.session.source_manager.clone();
        crate::cargo::cargo_build(
            assembly.package.clone(),
            assembly.target,
            assembly.manifest_path.with_file_name("Cargo.toml"),
            filesystem_cache_dir.as_deref(),
            &self.session.options,
            &cargo_opts,
            source_manager,
        )
    }
}

impl Frontend for RustProjectFrontend {
    /// Build this target with cargo, returning the Miden Assembly it produced.
    ///
    /// **This publishes no checkpoints, and therefore never returns [`Flow::Stop`].** That
    /// is deliberate, not an oversight. The intermediate artifacts a checkpoint names — the
    /// initial HIR, the analyzed and transformed HIR, the lowered Miden Assembly — are
    /// produced inside the nested `midenc` invocation that `cargo` drives, which returns
    /// only its final [`CodegenOutput`]. This frontend does not call
    /// [`backend::hir_to_masm`](crate::pipeline::backend::hir_to_masm), so there is nothing
    /// here to publish and no point at which it could honour a `--stop-after` short of
    /// assembly.
    ///
    /// Surfacing the nested run's checkpoints means propagating the goal and the observers
    /// into it, which is a later increment's work.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let key = cx.target_key();
        let session = cx.session();
        {
            let compiled = self.compiled.borrow();
            if let Some(found) = compiled.get(&key) {
                return Ok(Flow::Continue(
                    found.component.source_inputs(cx.assembly().target, &session)?,
                ));
            }
        }

        let compiled = self.build(cx)?;
        let source_inputs = compiled.component.source_inputs(cx.assembly().target, &session)?;
        self.compiled.borrow_mut().insert(key, compiled);
        Ok(Flow::Continue(source_inputs))
    }

    /// The build provenance of this target's sources.
    ///
    /// Called repeatedly while the assembler hashes the dependency closure, so it shares
    /// `compile`'s cache: the first of the two to run pays for the build.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        let key = cx.target_key();
        {
            let compiled = self.compiled.borrow();
            if let Some(found) = compiled.get(&key) {
                return Ok(found.source_provenance());
            }
        }

        let compiled = self.build(cx)?;
        let provenance = compiled.source_provenance();
        self.compiled.borrow_mut().insert(key, compiled);
        Ok(provenance)
    }

    /// Attach the account-component metadata and rodata this target's build produced.
    ///
    /// The assembler builds a fresh context for this call, so the build is recovered from
    /// the cache by [`TargetContext::target_key`]. A miss means the assembler asked to
    /// post-process a target this frontend never compiled, which is a compiler bug: report
    /// it rather than indexing into the map and panicking.
    fn post_process(
        &self,
        package: &mut MastPackage,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        let key = cx.target_key();
        let compiled = self.compiled.borrow();
        let found = compiled.get(&key).ok_or_else(|| {
            Report::msg(format!(
                "internal error: cannot post-process target '{}' of package '{}': no cargo build \
                 was cached for it, so `compile` never ran for this target",
                key.name(),
                key.package()
            ))
        })?;
        crate::stages::assemble::post_process_package(
            package,
            &found.component,
            found.account_component_metadata_bytes.as_deref(),
            cx.assembly().target,
            cx.assembly().package_registry,
        )
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, sync::Arc, vec::Vec};

    use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
    use midenc_codegen_masm::MasmComponent;
    use midenc_hir::Context;
    use midenc_session::miden_project::TargetType;

    use super::*;
    use crate::pipeline::{
        CheckpointId, FrontendId, FrontendRegistry, Goal, OutputRequest, RequestState, TargetRole,
        resolve_goal,
        testing::{VirtualProject, wat_fixture},
    };

    /// The route declares exactly what this frontend reaches, and no more.
    ///
    /// A route is what `--stop-after` and `--emit` are validated against, so declaring the
    /// checkpoints a Rust build *conceptually* passes through — `wasm.parsed`, the three HIR
    /// points, `masm.lowered` — would make every one of them a stop point that resolves
    /// successfully and is then never reached, leaving the run with an empty capture slot.
    /// The cargo build is a nested `midenc` invocation and publishes nothing back, so the
    /// only checkpoint reachable here is the one the driver itself publishes once the
    /// package is assembled. See [`Frontend::compile`] on this frontend.
    #[test]
    fn the_route_declares_only_the_checkpoint_a_cargo_build_reaches() {
        assert_eq!(RUST_FRONTEND.id(), FrontendId::new("rust"));
        assert_eq!(
            RUST_FRONTEND.extensions(),
            &["rs"],
            "dispatch is by target-root extension, and a Rust target is rooted at a `.rs` file"
        );
        assert_eq!(RUST_FRONTEND.route(), &[CheckpointId::PACKAGE_ASSEMBLED]);
        assert_eq!(RUST_FRONTEND.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        FrontendRegistry::new()
            .register(RUST_FRONTEND)
            .expect("the route, its aliases and its artifacts must be mutually consistent");
    }

    /// An uncapped run reaches the package; a stop point short of it is rejected outright.
    ///
    /// The second half is the point of declaring the route honestly: `--stop-after=lower`
    /// must fail with a diagnostic naming what this frontend *can* stop at, rather than
    /// resolving to a checkpoint no cargo build ever publishes and returning nothing.
    #[test]
    fn a_stop_point_this_frontend_never_reaches_is_rejected() {
        let goal = resolve_goal(&OutputRequest::default(), &RUST_FRONTEND)
            .expect("an uncapped run resolves to the terminal checkpoint");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);

        let capped = OutputRequest::new(Vec::new()).with_stop_after(Some("lower".to_string()));
        let err = resolve_goal(&capped, &RUST_FRONTEND)
            .expect_err("this frontend publishes nothing at `lower`");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("assemble") && rendered.contains("package.assembled"),
            "the rejection must name the stop points this route does offer: {rendered}"
        );
    }

    /// The registration builds *this* frontend, not merely some frontend.
    ///
    /// `Rc<dyn Frontend>` is opaque, so the instance is identified by behaviour: only
    /// [`RustProjectFrontend::post_process`] reports a cache miss as a missing cargo build.
    #[test]
    fn instantiate_builds_a_rust_project_frontend() {
        let project = library("rust_frontend_instantiate");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend = RUST_FRONTEND.instantiate(context.session_rc());
        let err = frontend
            .post_process(&mut any_package(), &cx)
            .expect_err("nothing was built for this target");
        assert!(
            format!("{err}").contains("no cargo build was cached for it"),
            "only the Rust frontend reports a miss this way, got: {err}"
        );
    }

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// A project named `name` with a single target of type `ty`.
    fn project(name: &str, fixture: &str, ty: TargetType) -> VirtualProject {
        let root = wat_fixture(
            fixture,
            if ty.is_executable() {
                "main.wat"
            } else {
                "lib.wat"
            },
        );
        VirtualProject::new(name, &root, ty).expect("should build")
    }

    /// A default HIR context, which is also the source of a target's session.
    fn context() -> Rc<Context> {
        Rc::new(Context::default())
    }

    /// The state of a full build: no observers, and a capture slot nothing reaches. These
    /// tests exercise `post_process`, which publishes no checkpoints.
    fn request() -> RequestState {
        RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new())
    }

    /// Build a target context for `assembly` within `state`.
    fn target_context<'a>(
        assembly: &'a miden_assembly::TargetAssemblyContext<'a>,
        context: Rc<Context>,
        state: &'a RequestState,
    ) -> TargetContext<'a> {
        TargetContext::for_testing(assembly, context, TargetRole::Root, state)
    }

    /// The `[lib]`/`[[bin]]` collision the old `(PackageId, Version)` key could not express.
    ///
    /// One package, two targets: the old provider keyed its cache on the package name and
    /// version, which are identical for both, so the executable read the library's codegen
    /// output. Asserted through the frontend rather than against a hand-built map: a
    /// frontend seeded with the *library* target's build must not serve that build to the
    /// *executable* target, and must say so by naming the target it could not find.
    #[test]
    fn a_library_build_is_not_served_to_the_executable_target() {
        let lib = project("shared", "rust_frontend_key_lib", TargetType::Library);
        let exe = project("shared", "rust_frontend_key_exe", TargetType::Executable);
        let lib_assembly = lib.assembly_context().expect("lib assembly context");
        let exe_assembly = exe.assembly_context().expect("exe assembly context");

        let context = context();
        let state = request();
        let lib_cx = target_context(&lib_assembly, context.clone(), &state);
        let exe_cx = target_context(&exe_assembly, context.clone(), &state);
        assert_eq!(
            lib_cx.target_key().package(),
            exe_cx.target_key().package(),
            "the two targets must belong to one package, or this proves nothing"
        );

        // Exactly the state of an in-process run that has built the library and is now
        // assembling the executable of the same package.
        let frontend = RustProjectFrontend::seeded(
            context.session_rc(),
            lib_cx.target_key(),
            codegen_output(),
        );

        frontend
            .post_process(&mut any_package(), &lib_cx)
            .expect("the seeded library target must be served from the cache");

        let err = frontend
            .post_process(&mut any_package(), &exe_cx)
            .expect_err("the executable must not read the library's codegen output");
        let msg = format!("{err}");
        assert!(msg.contains("internal error"), "a cache miss here is a compiler bug: {msg}");
        assert!(
            msg.contains(&**exe_cx.target_key().name()),
            "the miss must name the executable target it looked for, got: {msg}"
        );
    }

    /// The frontend distinguishes targets in what it reports, not just in what it stores.
    ///
    /// Two contexts for one package over an empty cache: both miss, and each miss must name
    /// its own target. A package-keyed frontend would produce one message twice.
    #[test]
    fn a_cache_miss_names_the_target_it_looked_for() {
        let lib = project("shared", "rust_frontend_miss_lib", TargetType::Library);
        let exe = project("shared", "rust_frontend_miss_exe", TargetType::Executable);
        let lib_assembly = lib.assembly_context().expect("lib assembly context");
        let exe_assembly = exe.assembly_context().expect("exe assembly context");

        let context = context();
        let state = request();
        let lib_cx = target_context(&lib_assembly, context.clone(), &state);
        let exe_cx = target_context(&exe_assembly, context.clone(), &state);

        let frontend = RustProjectFrontend::new(context.session_rc());
        let lib_err = format!(
            "{}",
            frontend
                .post_process(&mut any_package(), &lib_cx)
                .expect_err("nothing was compiled for the library target")
        );
        let exe_err = format!(
            "{}",
            frontend
                .post_process(&mut any_package(), &exe_cx)
                .expect_err("nothing was compiled for the executable target")
        );

        assert_ne!(
            lib_err, exe_err,
            "two targets of one package must not be reported as the same target"
        );
        assert!(
            lib_err.contains(&**lib_cx.target_key().name()),
            "the library's miss must name the library target: {lib_err}"
        );
        assert!(
            exe_err.contains(&**exe_cx.target_key().name()),
            "the executable's miss must name the executable target: {exe_err}"
        );
    }

    /// A seeded frontend serves `post_process` from the seed rather than reporting a miss.
    ///
    /// This is the virtual-project path: the running process already produced this target's
    /// [`CodegenOutput`], so seeding it must spare the nested cargo build entirely.
    #[test]
    fn a_seeded_build_is_served_to_post_process() {
        let project = library("rust_frontend_seeded");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend =
            RustProjectFrontend::seeded(context.session_rc(), cx.target_key(), codegen_output());

        frontend
            .post_process(&mut any_package(), &cx)
            .expect("a seeded target must be post-processed from the seed, not reported missing");
    }

    /// A `post_process` with nothing cached is an internal invariant violation, and must be
    /// reported as one. The provider this frontend replaces indexed with `compiled[&key]`,
    /// which panics on a miss.
    #[test]
    fn post_process_without_a_cached_build_is_an_invariant_error() {
        let project = library("rust_frontend_post_process");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = request();
        let cx = target_context(&assembly, context.clone(), &state);

        let frontend = RustProjectFrontend::new(context.session_rc());
        let mut package = any_package();

        let err = frontend
            .post_process(&mut package, &cx)
            .expect_err("post-processing a target that was never compiled must fail, not panic");
        let msg = format!("{err}");
        assert!(msg.contains("internal error"), "a cache miss here is a compiler bug: {msg}");
    }

    /// A well-formed package to hand to `post_process`.
    ///
    /// A package must export at least one procedure to be constructible, so rather than
    /// hand-building a MAST forest this borrows the compiler's own intrinsics package.
    /// `post_process` must reject the cache miss before it looks at the package at all, so
    /// which package it is does not matter.
    fn any_package() -> MastPackage {
        (*midenc_codegen_masm::intrinsics::load()).clone()
    }

    /// A minimal build result to seed a frontend's cache with.
    ///
    /// `post_process` reads only the component's rodata and the account-component metadata,
    /// both empty here, so this stands in for a real cargo build without running one.
    fn codegen_output() -> CodegenOutput {
        let root = miden_assembly_syntax::Path::new("seeded")
            .to_absolute()
            .map(|path| Arc::from(path.into_owned()))
            .expect("should absolutize the root module path");
        CodegenOutput {
            component: Arc::new(MasmComponent {
                id: None,
                root,
                init: None,
                entrypoint: None,
                rodata: Vec::new(),
                heap_base: 0,
                stack_pointer: None,
                modules: Vec::new(),
            }),
            account_component_metadata_bytes: None,
            source_provenance: ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: std::path::PathBuf::from("seeded.wat").into_boxed_path(),
                    content: "(module)".into(),
                },
                support: Vec::new(),
            },
        }
    }
}
