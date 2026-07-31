use alloc::{format, rc::Rc, sync::Arc};
use core::ops::ControlFlow;

use miden_assembly::{
    ProjectSourceInputs, ProjectSourceProvenanceInputs, ProjectSourceProvider,
    TargetAssemblyContext,
};
use miden_mast_package::Package as MastPackage;
use midenc_hir::Context;
use midenc_session::{
    InputFile, Session,
    diagnostics::Report,
    miden_project::{Package as ProjectPackage, Target, TargetType},
};

use super::{Flow, Frontend, RequestState, TargetContext, TargetRole};

/// The identity of the top-level target selected for a compilation request.
///
/// Recorded during preparation, and the only thing that can answer which of the assembler's
/// callbacks is the root one: [`TargetAssemblyContext`] carries no role, and package
/// identity cannot supply it. For an executable root with an implicit library, the assembler
/// assembles the library target from the *same* `Arc<ProjectPackage>` with the
/// required-library role, so both callbacks report the same package name. Only the target
/// tells them apart.
#[derive(Debug, Clone)]
pub struct RootTarget {
    /// The root project's package, retained for pointer comparison; see
    /// [`RootTarget::role_of`].
    package: Arc<ProjectPackage>,
    /// The effective name of the selected target within that package.
    name: Arc<str>,
    /// The type of the selected target.
    ty: TargetType,
}

impl RootTarget {
    /// Record `package`'s `target` as the selected top-level target.
    pub fn new(package: Arc<ProjectPackage>, target: &Target) -> Self {
        Self {
            package,
            name: target.name.inner().clone(),
            ty: target.ty,
        }
    }

    /// The root project's package.
    pub fn package(&self) -> &Arc<ProjectPackage> {
        &self.package
    }

    /// The effective name of the selected target.
    pub fn name(&self) -> &Arc<str> {
        &self.name
    }

    /// The type of the selected target.
    pub fn target_type(&self) -> TargetType {
        self.ty
    }

    /// Derive the role of the target `context` describes.
    ///
    /// A callback is [`TargetRole::Root`] **iff** its package is the root project *and* its
    /// `(name, type)` pair equals the recorded one. A callback for another target of the
    /// root project is the [`TargetRole::RequiredLibrary`]; anything from another package is
    /// a [`TargetRole::Dependency`].
    ///
    /// The package comparison is `Arc::ptr_eq`, not [`PackageId`](midenc_session::PackageId)
    /// equality: the root and required-library callbacks receive literally
    /// `Arc::clone(&project)` from the assembler, while a dependency callback gets a
    /// separately loaded `Arc`. Pointer equality therefore closes the degenerate case where
    /// a package appears somewhere in its own dependency closure, which name equality would
    /// misreport as the root.
    pub fn role_of(&self, context: &TargetAssemblyContext<'_>) -> TargetRole {
        if !Arc::ptr_eq(&self.package, &context.package) {
            return TargetRole::Dependency;
        }
        if context.target.name.inner() == &self.name && context.target.ty == self.ty {
            TargetRole::Root
        } else {
            TargetRole::RequiredLibrary
        }
    }
}

/// Bridges the assembler's [`ProjectSourceProvider`] callbacks onto one registered
/// [`Frontend`].
///
/// This type owns no language logic. Each callback derives the target's role, wraps the
/// assembler's [`TargetAssemblyContext`] in a pipeline [`TargetContext`], and delegates to
/// the matching [`Frontend`] method.
///
/// # One provider per extension, one frontend for all of them
///
/// [`ProjectSourceProvider::file_type`] returns a single `&'static str`, so a frontend
/// registered for several target-root extensions needs one provider per extension. They must
/// all share a single frontend instance, or per-target memoization would be split across
/// extensions — which is why [`FrontendProvider::new`] takes an already-instantiated
/// `Rc<dyn Frontend>` rather than a registration to instantiate: the caller calls
/// [`FrontendRegistration::instantiate`](super::FrontendRegistration::instantiate) once and
/// clones the `Rc` into each provider.
///
/// # Every field is owned
///
/// `ProjectAssembler::for_project_with_providers` takes
/// `impl IntoIterator<Item = Box<dyn ProjectSourceProvider>>`, and `Box<dyn Trait>` carries
/// an implicit `+ 'static` bound. A provider that *borrowed* its [`RequestState`] could not
/// be boxed into it. Hence `Rc<RequestState>` and not `&RequestState`, and likewise for
/// every other field: the borrow handed to [`TargetContext::new`] is created inside each
/// callback and lives only for that call.
pub struct FrontendProvider {
    /// The target-root extension this provider is registered for, without a leading dot.
    file_type: &'static str,
    /// The frontend every callback delegates to, shared with this frontend's other
    /// providers.
    frontend: Rc<dyn Frontend>,
    /// The session each callback's [`Context`] is built from.
    session: Rc<Session>,
    /// The goal, observers and capture slot shared by every target of this request.
    state: Rc<RequestState>,
    /// The selected target, which is what makes role derivation possible.
    root: RootTarget,
    /// The compiler input this request was given, when the frontend needs it; see
    /// [`FrontendProvider::with_input`].
    input: Option<InputFile>,
}

impl FrontendProvider {
    /// Build the provider that handles target roots with the `file_type` extension.
    ///
    /// `frontend` is shared with this frontend's providers for its other extensions;
    /// `state` is shared with every provider serving the same request.
    pub fn new(
        file_type: &'static str,
        frontend: Rc<dyn Frontend>,
        session: Rc<Session>,
        state: Rc<RequestState>,
        root: RootTarget,
    ) -> Self {
        Self {
            file_type,
            frontend,
            session,
            state,
            root,
            input: None,
        }
    }

    /// Hand `input` to the frontend on every callback this provider serves.
    ///
    /// Nothing supplies one yet: only a standalone preparation will, once standalone inputs
    /// are prepared here. A project target's frontend reads its sources from
    /// `assembly().resolved_target_root`, but a standalone input may be stdin-backed — it
    /// exists only in memory and has no path at all — so the bytes can only reach the frontend
    /// through the request's own [`InputFile`].
    ///
    /// It will therefore be supplied to the provider for the *selected* target's root
    /// extension, which is the target the input backs. Every callback that provider serves is
    /// handed it, including a same-extension dependency's. That is expected to be
    /// unreachable — a standalone request's project is to be synthesized with only the
    /// `miden-core` registry dependency, which has no sources to compile — but the route that
    /// would establish it does not exist yet, so treat it as a property the standalone
    /// preparation owes rather than one already held.
    pub fn with_input(mut self, input: InputFile) -> Self {
        self.input = Some(input);
        self
    }

    /// Wrap one assembler callback's context in the context the frontend expects.
    ///
    /// The HIR [`Context`] is built here rather than once per provider, because the spec
    /// scopes `Options` — and therefore a `Context`, and the session derived from it — per
    /// target, not per provider.
    ///
    /// The [`InputFile`] is whatever this provider was built with; see
    /// [`FrontendProvider::with_input`] for why only some providers carry one. The borrow
    /// handed on is of `self`, which is what lets [`TargetContext::input`] hand back a
    /// reference for the same lifetime as the assembly context's.
    fn target_context<'a>(&'a self, assembly: &'a TargetAssemblyContext<'a>) -> TargetContext<'a> {
        TargetContext::new(
            assembly,
            Rc::new(Context::new(self.session.clone())),
            self.input.as_ref(),
            self.root.role_of(assembly),
            &self.state,
        )
    }
}

impl ProjectSourceProvider for FrontendProvider {
    fn file_type(&self) -> &'static str {
        self.file_type
    }

    fn post_process_package(
        &self,
        package: &mut MastPackage,
        context: &TargetAssemblyContext<'_>,
    ) -> Result<(), Report> {
        self.frontend.post_process(package, &self.target_context(context))
    }

    fn provide_source_provenance(
        &self,
        context: &TargetAssemblyContext<'_>,
    ) -> Result<ProjectSourceProvenanceInputs, Report> {
        self.frontend.provenance(&self.target_context(context))
    }

    fn provide_sources(
        &self,
        context: &TargetAssemblyContext<'_>,
    ) -> Result<ProjectSourceInputs, Report> {
        match self.frontend.compile(&self.target_context(context))? {
            Flow::Continue(inputs) => Ok(inputs),
            Flow::Break(stopped) => Err(Report::msg(format!(
                "internal error: compilation of target '{}' of package '{}' stopped at '{}', but \
                 `provide_sources` has no way to report a stop; a request whose goal is short of \
                 assembly must be driven through `provide_sources_interruptible`",
                context.target.name.inner(),
                context.package.name().inner(),
                stopped.checkpoint(),
            ))),
        }
    }

    fn provide_sources_interruptible(
        &self,
        context: &TargetAssemblyContext<'_>,
    ) -> Result<ControlFlow<(), ProjectSourceInputs>, Report> {
        Ok(self.frontend.compile(&self.target_context(context))?.map_break(|_| ()))
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        string::{String, ToString},
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;

    use miden_assembly::{ModuleParser, SourceFileProvenance, ast::ModuleKind};
    use midenc_session::{InputFile, InputType};

    use super::*;
    use crate::{
        CompilerResult,
        pipeline::{
            ArtifactId, CheckpointId, Goal, Observer, RecordingObserver,
            testing::{VirtualProject, wat_fixture},
        },
    };

    /// The checkpoint the fixture frontend publishes at, and the goal these tests run to.
    ///
    /// Short of [`CheckpointId::PACKAGE_ASSEMBLED`], so the root target's publication is
    /// terminal while a non-root target's is not — the distinction the role derivation
    /// exists to make.
    const PUBLISHED: CheckpointId = CheckpointId::MASM_LOWERED;

    /// What a target publishes at [`PUBLISHED`]: which target it was.
    #[derive(Debug, PartialEq, Eq)]
    struct Published(String);

    /// A frontend that publishes once, then hands back trivial Miden Assembly.
    ///
    /// It also records the role and the input it was invoked with, so a test can check what
    /// the *provider* derived and passed on rather than only re-deriving it itself.
    struct PublishingFrontend {
        roles: RefCell<Vec<TargetRole>>,
        inputs: RefCell<Vec<Option<InputFile>>>,
    }

    impl PublishingFrontend {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                roles: RefCell::new(Vec::new()),
                inputs: RefCell::new(Vec::new()),
            })
        }

        /// The inputs this frontend has been handed, in the same order as [`Self::roles`].
        fn inputs(&self) -> Vec<Option<InputFile>> {
            self.inputs.borrow().clone()
        }

        /// Note what `cx` carried for this callback.
        fn observe(&self, cx: &TargetContext<'_>) {
            self.roles.borrow_mut().push(cx.role());
            self.inputs.borrow_mut().push(cx.input().cloned());
        }

        /// The roles this frontend has been invoked with, in order.
        fn roles(&self) -> Vec<TargetRole> {
            self.roles.borrow().clone()
        }

        /// A trivial module in this target's namespace, standing in for compiled output.
        fn sources(cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceInputs> {
            let namespace = cx.assembly().target.namespace.inner().clone();
            let root = ModuleParser::new(Some(ModuleKind::Library)).parse_str(
                Some(namespace.as_ref()),
                "pub proc main\n    push.1\nend\n",
                cx.session().source_manager.clone(),
            )?;
            Ok(ProjectSourceInputs {
                root,
                support: Vec::new(),
            })
        }
    }

    impl Frontend for PublishingFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            self.observe(cx);
            let name = cx.assembly().target.name.inner().to_string();
            match cx.checkpoint(PUBLISHED, ArtifactId::MASM, Published(name))? {
                Flow::Continue(_) => Ok(Flow::Continue(Self::sources(cx)?)),
                Flow::Break(stopped) => Ok(Flow::Break(stopped)),
            }
        }

        fn post_process(
            &self,
            package: &mut MastPackage,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<()> {
            self.observe(cx);
            package.description = Some(format!("post-processed as {:?}", cx.role()));
            Ok(())
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            self.observe(cx);
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: String::from("(module)").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    /// A well-formed assembled package, for [`Frontend::post_process`] to mark.
    ///
    /// A package must export at least one procedure whose MAST root is present in its
    /// forest, so rather than hand-building a forest this borrows the compiler's own
    /// intrinsics package. These tests check that the callback is delegated with the right
    /// role, never what it did to the package's code, so which package it is does not matter.
    fn any_package() -> MastPackage {
        (*midenc_codegen_masm::intrinsics::load()).clone()
    }

    /// A project named `name` with an executable target and a library target.
    fn both_targets(name: &str) -> VirtualProject {
        let exe_root = wat_fixture(name, "main.wat");
        let lib_root = wat_fixture(name, "lib.wat");
        VirtualProject::executable_and_library(name, &exe_root, &lib_root)
            .expect("should build a two-target virtual project")
    }

    /// The library target of `project`.
    fn library_of(project: &VirtualProject) -> &Target {
        project
            .targets()
            .iter()
            .find(|target| target.is_library())
            .expect("the fixture declares a library target")
    }

    /// The `RootTarget` a preparation step would record for `project`'s selected target.
    fn root_of(project: &VirtualProject) -> RootTarget {
        RootTarget::new(project.package(), project.target())
    }

    #[test]
    fn the_selected_target_is_the_root_and_its_sibling_library_is_not() {
        let project = both_targets("role_root");
        let root = root_of(&project);

        let exe_cx = project.assembly_context_for(project.target()).expect("executable context");
        let lib_cx = project.assembly_context_for(library_of(&project)).expect("library context");

        // Precondition: the two callbacks are indistinguishable by package identity, which
        // is what makes this test about the target rather than about the package.
        assert!(Arc::ptr_eq(&exe_cx.package, &lib_cx.package));
        assert_eq!(exe_cx.package.name().inner(), lib_cx.package.name().inner());

        assert_eq!(root.role_of(&exe_cx), TargetRole::Root);
        assert_eq!(root.role_of(&lib_cx), TargetRole::RequiredLibrary);
    }

    #[test]
    fn the_selected_library_is_the_root_and_the_executable_is_not() {
        // The mirror of the previous test: a derivation hard-wired to "executables are
        // root" would pass that one and fail this. Recording the *library* as selected must
        // make the executable non-root.
        let project = both_targets("role_root_lib");
        let library = library_of(&project);
        let root = RootTarget::new(project.package(), library);

        let exe_cx = project.assembly_context_for(project.target()).expect("executable context");
        let lib_cx = project.assembly_context_for(library).expect("library context");

        assert_eq!(root.role_of(&lib_cx), TargetRole::Root);
        assert_eq!(root.role_of(&exe_cx), TargetRole::RequiredLibrary);
    }

    #[test]
    fn a_target_from_another_package_allocation_is_a_dependency() {
        let project = both_targets("role_dependency");
        let root = root_of(&project);

        // A separately built project of the *same name*, with a target of the same name and
        // type as the recorded root. Everything a `PackageId`-based comparison could look at
        // matches; only the allocation differs.
        let twin_root = wat_fixture("role_dependency_twin", "main.wat");
        let twin = VirtualProject::new("role_dependency", &twin_root, TargetType::Executable)
            .expect("should build the twin project");
        let twin_cx = twin.assembly_context().expect("twin context");

        assert!(!Arc::ptr_eq(root.package(), &twin_cx.package), "distinct allocations");
        assert_eq!(
            twin_cx.package.name().inner(),
            root.package().name().inner(),
            "the twin shares the root's package name"
        );
        assert_eq!(twin_cx.target.name.inner(), root.name(), "and its target's name");
        assert_eq!(twin_cx.target.ty, root.target_type(), "and its target's type");

        assert_eq!(
            root.role_of(&twin_cx),
            TargetRole::Dependency,
            "a package that is not the retained allocation is a dependency, however it is named"
        );
    }

    /// Run `provider`'s `provide_sources_interruptible` over `target` of `project`.
    ///
    /// Returns whether the callback interrupted the build.
    fn provide(provider: &FrontendProvider, project: &VirtualProject, target: &Target) -> bool {
        let assembly = project.assembly_context_for(target).expect("assembly context");
        provider
            .provide_sources_interruptible(&assembly)
            .expect("the fixture frontend must not error")
            .is_break()
    }

    #[test]
    fn only_the_root_targets_publication_stops_the_build_and_captures() {
        let project = both_targets("provider_capture");
        let frontend = PublishingFrontend::new();
        let session = Context::default().session_rc();
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = Rc::new(RequestState::new(
            Goal::at(PUBLISHED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        ));
        let provider = FrontendProvider::new(
            "wat",
            frontend.clone() as Rc<dyn Frontend>,
            session,
            state.clone(),
            root_of(&project),
        );

        // The required library runs first, exactly as the assembler orders it.
        let library = library_of(&project);
        assert!(
            !provide(&provider, &project, library),
            "a non-root target publishing at the goal must not interrupt the build"
        );

        assert!(
            provide(&provider, &project, project.target()),
            "the root target publishing at the goal must interrupt the build"
        );

        assert_eq!(
            frontend.roles(),
            vec![TargetRole::RequiredLibrary, TargetRole::Root],
            "the provider must hand each callback the role it derived, not a fixed one"
        );
        assert_eq!(
            observer.borrow().records(),
            &[(PUBLISHED, TargetRole::RequiredLibrary), (PUBLISHED, TargetRole::Root)],
            "both targets' checkpoints are observable; only the role differs"
        );

        let captured = state.take_outcome().expect("the root target's artifact must be captured");
        assert_eq!(captured.checkpoint(), PUBLISHED);
        assert_eq!(
            captured.downcast::<Published>().expect("the captured payload"),
            Published(project.target().name.inner().to_string()),
            "the captured artifact must be the root's, not the library's"
        );
    }

    #[test]
    fn a_non_root_target_alone_captures_nothing() {
        // Isolates the guard: with only the library compiled, the slot stays empty. The
        // paired test above would still pass if capture were merely last-write-wins.
        let project = both_targets("provider_non_root_only");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            frontend as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state.clone(),
            root_of(&project),
        );

        assert!(!provide(&provider, &project, library_of(&project)));

        assert!(
            state.take_outcome().is_none(),
            "a non-root target publishing at the goal must not populate the capture slot"
        );
    }

    #[test]
    fn provide_sources_reports_a_stop_it_cannot_express() {
        // `provide_sources` returns sources or an error, so a stop has nowhere to go. It
        // must be reported rather than silently turned into an empty build.
        let project = both_targets("provider_uninterruptible");
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            PublishingFrontend::new() as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state,
            root_of(&project),
        );

        let assembly = project.assembly_context().expect("assembly context");
        // `ProjectSourceInputs` is not `Debug`, so `expect_err` is unavailable here.
        let Err(err) = provider.provide_sources(&assembly) else {
            panic!("the root target stops at the goal, which `provide_sources` cannot report");
        };
        let rendered = format!("{err}");
        assert!(rendered.contains("masm.lowered"), "should name the checkpoint: {rendered}");
        assert!(
            rendered.contains("provide_sources_interruptible"),
            "should point at the callback that can express a stop: {rendered}"
        );
    }

    #[test]
    fn provenance_is_delegated_with_the_derived_role_and_never_stops() {
        let project = both_targets("provider_provenance");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            frontend.clone() as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state.clone(),
            root_of(&project),
        );

        let library = library_of(&project);
        let assembly = project.assembly_context_for(library).expect("library context");
        let provenance = provider
            .provide_source_provenance(&assembly)
            .expect("provenance should succeed");
        assert_eq!(
            provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("lib.wat"),
            "provenance must describe the target the callback was for"
        );
        assert_eq!(frontend.roles(), vec![TargetRole::RequiredLibrary]);

        assert!(state.take_outcome().is_none(), "provenance must not capture anything");
    }

    #[test]
    fn post_processing_is_delegated_with_the_derived_role() {
        // The assembler builds a *fresh* context for this callback, so the role has to be
        // derived again rather than remembered from `provide_sources` — which is what makes
        // it worth checking that each target still gets its own.
        let project = both_targets("provider_post_process");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            frontend.clone() as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state.clone(),
            root_of(&project),
        );

        let mut root_package = any_package();
        let root_cx = project.assembly_context().expect("root context");
        provider
            .post_process_package(&mut root_package, &root_cx)
            .expect("post-processing should succeed");

        let mut library_package = any_package();
        let library_cx =
            project.assembly_context_for(library_of(&project)).expect("library context");
        provider
            .post_process_package(&mut library_package, &library_cx)
            .expect("post-processing should succeed");

        assert_eq!(
            root_package.description.as_deref(),
            Some("post-processed as Root"),
            "the frontend must have been handed the package and the root role"
        );
        assert_eq!(
            library_package.description.as_deref(),
            Some("post-processed as RequiredLibrary"),
        );
        assert_eq!(frontend.roles(), vec![TargetRole::Root, TargetRole::RequiredLibrary]);

        assert!(state.take_outcome().is_none(), "post-processing must not capture anything");
    }

    #[test]
    fn file_type_is_the_extension_the_provider_was_registered_for() {
        // One frontend instance, several extensions: each provider answers for its own, and
        // they share the frontend so per-target memoization is not split across them.
        let project = both_targets("provider_file_type");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let session = Context::default().session_rc();

        let providers = ["wasm", "wat"].map(|extension| {
            FrontendProvider::new(
                extension,
                frontend.clone() as Rc<dyn Frontend>,
                session.clone(),
                state.clone(),
                root_of(&project),
            )
        });

        assert_eq!(
            providers.iter().map(ProjectSourceProvider::file_type).collect::<Vec<_>>(),
            vec!["wasm", "wat"]
        );

        for provider in &providers {
            assert!(!provide(provider, &project, library_of(&project)));
        }
        assert_eq!(
            frontend.roles(),
            vec![TargetRole::RequiredLibrary; 2],
            "both providers must reach the same frontend instance, each deriving the same role"
        );
    }

    /// Drive all three of `provider`'s callbacks over `project`'s library target.
    ///
    /// Every one of them builds a [`TargetContext`], so a property of that construction —
    /// such as which input it carries — has to hold for all three rather than for the one a
    /// test happened to call.
    fn drive_every_callback(provider: &FrontendProvider, project: &VirtualProject) {
        let library = library_of(project);
        assert!(!provide(provider, project, library));
        let assembly = project.assembly_context_for(library).expect("library context");
        provider
            .provide_source_provenance(&assembly)
            .expect("provenance should succeed");
        provider
            .post_process_package(&mut any_package(), &assembly)
            .expect("post-processing should succeed");
    }

    #[test]
    fn an_input_supplied_to_the_provider_reaches_every_callback() {
        // A stdin-backed input, because that is the case paths cannot serve and therefore
        // the whole reason a context carries an input: `resolved_target_root` names a file
        // that was never written.
        //
        // This is also the test increment 2 owed `TargetContext::input`'s lifetime. The
        // accessor hands back `Option<&'a InputFile>` for the *assembly* context's `'a`,
        // which only type-checks because `target_context<'a>(&'a self, ..)` borrows the
        // provider for that same `'a` — the restrictive direction, and one no caller had
        // exercised.
        let project = both_targets("provider_input");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let input = InputFile::from_bytes(b"(module)".to_vec(), "stdin.wat".into())
            .expect("stdin bytes are a valid compiler input");
        let provider = FrontendProvider::new(
            "wat",
            frontend.clone() as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state,
            root_of(&project),
        )
        .with_input(input.clone());

        drive_every_callback(&provider, &project);

        let mut handed_over = frontend.inputs();
        assert_eq!(
            handed_over,
            vec![Some(input); 3],
            "the request's input must reach the frontend through every callback"
        );
        let reached = handed_over.remove(0).expect("an input was handed over");
        assert!(reached.as_path().is_none(), "a stdin input has no path to be read back from");
        match &reached.file {
            InputType::Stdin { input, .. } => {
                assert_eq!(input.as_slice(), b"(module)", "the stdin bytes must be readable")
            }
            other => panic!("expected a stdin-backed input, got {other:?}"),
        }
    }

    #[test]
    fn a_provider_built_without_an_input_hands_the_frontend_none() {
        // The half that keeps the test above from passing on a provider that fabricates an
        // input: a project target's frontend reads `assembly().resolved_target_root`, and
        // must be told there is nothing else.
        let project = both_targets("provider_no_input");
        let frontend = PublishingFrontend::new();
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            frontend.clone() as Rc<dyn Frontend>,
            Context::default().session_rc(),
            state,
            root_of(&project),
        );

        drive_every_callback(&provider, &project);

        assert_eq!(frontend.inputs(), vec![None; 3]);
    }

    #[test]
    fn a_provider_can_be_boxed_as_the_assembler_requires() {
        // The reason every field is owned: `for_project_with_providers` takes
        // `Box<dyn ProjectSourceProvider>`, whose implicit `'static` bound a borrowed
        // `RequestState` would violate. This is a compile-time claim; the assertion only
        // keeps the value alive.
        let project = both_targets("provider_boxed");
        let state = Rc::new(RequestState::new(Goal::at(PUBLISHED), Vec::new()));
        let boxed: alloc::boxed::Box<dyn ProjectSourceProvider> =
            alloc::boxed::Box::new(FrontendProvider::new(
                "wat",
                PublishingFrontend::new() as Rc<dyn Frontend>,
                Context::default().session_rc(),
                state,
                root_of(&project),
            ));
        assert_eq!(boxed.file_type(), "wat");
    }
}
