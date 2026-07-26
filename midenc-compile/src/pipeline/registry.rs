use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    rc::Rc,
};
use core::fmt;

use midenc_session::{Session, diagnostics::Report};

use super::{Artifact, ArtifactId, CheckpointId, Frontend};
use crate::CompilerResult;

/// Identifies a registered frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrontendId(&'static str);

impl FrontendId {
    /// Construct a frontend id from a static string.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The stable string form of this frontend id.
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for FrontendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What a checkpoint produces, and how to render it.
#[derive(Debug, Clone, Copy)]
pub struct ArtifactDecl {
    /// The checkpoint that produces this artifact.
    pub checkpoint: CheckpointId,
    /// The artifact's stable id.
    pub id: ArtifactId,
    /// Render the artifact to the session's configured destination for its output type.
    ///
    /// The renderer downcasts and delegates to [`Session::emit`], which already resolves
    /// file-versus-stdout from the session's output files.
    ///
    /// The load-bearing point is that this lives on each *registration's own* declaration.
    /// Hoisting it to a global map keyed by [`ArtifactId`] — or by [`CheckpointId`], which
    /// is no better, since one checkpoint can appear on several routes — would make one
    /// shape the only shape. Keeping it here is what lets two routes emit the same artifact
    /// id differently: the MASM route writes one document per module, the Rust/Wasm route
    /// writes a single concatenated component, and the core, which only ever holds a
    /// type-erased [`Artifact`], knows neither shape.
    ///
    /// A plain `fn` pointer rather than a closure or a boxed trait object, so a registration
    /// stays a `&'static` slice built in a `const`.
    pub render: fn(&Artifact, &Session) -> CompilerResult<()>,
}

/// Declares a frontend: which target roots it handles, what route it runs, and how to
/// build it.
///
/// The route is ordered. Checkpoints carry no global ordering, so all comparisons —
/// which checkpoint is furthest, is this stop point reachable — are answered against a
/// single registration's route.
///
/// Built by [`FrontendRegistration::new`] rather than as a struct literal, so that a
/// registration's invariants hold for every value of this type rather than only for those
/// [`FrontendRegistry::register`] has seen. That matters for [`terminal`](Self::terminal),
/// which is reachable from any registration, registered or not.
#[derive(Debug, Clone, Copy)]
pub struct FrontendRegistration {
    /// The frontend's stable identifier.
    id: FrontendId,
    /// Target-root file extensions this frontend handles, without a leading dot.
    extensions: &'static [&'static str],
    /// The checkpoints this frontend can reach, in the order it reaches them. Never empty.
    route: &'static [CheckpointId],
    /// User-facing stop aliases mapped onto this route's checkpoints.
    aliases: &'static [(&'static str, CheckpointId)],
    /// The artifact each checkpoint on this route produces, and how to render it.
    artifacts: &'static [ArtifactDecl],
    /// Construct an instance of this frontend for one compilation request.
    ///
    /// Returns `Rc`, not `Box`: a registration may own several extensions and each needs
    /// its own provider, but they must share one frontend instance so per-target
    /// memoization is not split across extensions.
    make: fn(Rc<Session>) -> Rc<dyn Frontend>,
}

impl FrontendRegistration {
    /// Declare a frontend.
    ///
    /// `route` is ordered; `aliases` and `artifacts` are checked against it by
    /// [`FrontendRegistry::register`], which requires an artifact declaration for every
    /// route checkpoint and rejects declarations for checkpoints that are not on the route.
    /// Artifacts are declared rather than derived so the pipeline core never enumerates
    /// checkpoints.
    ///
    /// # Panics
    ///
    /// Panics if `route` is empty, which would leave [`terminal`](Self::terminal) with
    /// nothing to return. Registrations are `const`, where this is a compile-time error.
    pub const fn new(
        id: FrontendId,
        extensions: &'static [&'static str],
        route: &'static [CheckpointId],
        aliases: &'static [(&'static str, CheckpointId)],
        artifacts: &'static [ArtifactDecl],
        make: fn(Rc<Session>) -> Rc<dyn Frontend>,
    ) -> Self {
        assert!(!route.is_empty(), "a frontend's route must not be empty");
        Self {
            id,
            extensions,
            route,
            aliases,
            artifacts,
            make,
        }
    }

    /// The frontend's stable identifier.
    pub const fn id(&self) -> FrontendId {
        self.id
    }

    /// Target-root file extensions this frontend handles, without a leading dot.
    pub const fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    /// The checkpoints this frontend can reach, in the order it reaches them.
    ///
    /// Never empty, by [`FrontendRegistration::new`].
    pub const fn route(&self) -> &'static [CheckpointId] {
        self.route
    }

    /// User-facing stop aliases mapped onto this route's checkpoints.
    pub const fn aliases(&self) -> &'static [(&'static str, CheckpointId)] {
        self.aliases
    }

    /// The artifact each checkpoint on this route produces, and how to render it.
    pub const fn artifacts(&self) -> &'static [ArtifactDecl] {
        self.artifacts
    }

    /// Construct an instance of this frontend to serve one compilation request.
    pub fn instantiate(&self, session: Rc<Session>) -> Rc<dyn Frontend> {
        (self.make)(session)
    }

    /// The artifact produced at `checkpoint`, if it is on this route.
    pub fn artifact_at(&self, checkpoint: CheckpointId) -> Option<ArtifactId> {
        self.decl_at(checkpoint).map(|decl| decl.id)
    }

    /// The declaration for `checkpoint`, if it is on this route.
    ///
    /// Callers that need to *emit* what a checkpoint produced take this rather than
    /// [`FrontendRegistration::artifact_at`]: the renderer travels with the declaration, so
    /// the shape of the output is settled by the route that produced it.
    pub fn decl_at(&self, checkpoint: CheckpointId) -> Option<&'static ArtifactDecl> {
        self.artifacts.iter().find(|decl| decl.checkpoint == checkpoint)
    }

    /// Resolve a stop alias such as `parse` to this route's corresponding checkpoint.
    pub fn resolve_alias(&self, alias: &str) -> Option<CheckpointId> {
        self.aliases
            .iter()
            .find_map(|(name, checkpoint)| (*name == alias).then_some(*checkpoint))
    }

    /// The position of `checkpoint` in this route, if it is part of it.
    pub fn position(&self, checkpoint: CheckpointId) -> Option<usize> {
        self.route.iter().position(|candidate| *candidate == checkpoint)
    }

    /// Returns true if this route reaches `checkpoint`.
    pub fn supports(&self, checkpoint: CheckpointId) -> bool {
        self.position(checkpoint).is_some()
    }

    /// The final checkpoint of this route.
    ///
    /// # Panics
    ///
    /// Panics if the route is empty, which [`FrontendRegistration::new`] rejects. A
    /// registration can only be built through it, so this is unreachable.
    pub fn terminal(&self) -> CheckpointId {
        *self.route.last().expect("frontend route must not be empty")
    }

    /// The aliases this route supports, for use in diagnostics.
    pub fn alias_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.aliases.iter().map(|(name, _)| *name)
    }
}

/// The set of frontends available to a compilation, keyed by target-root extension.
#[derive(Debug, Default)]
pub struct FrontendRegistry {
    by_extension: BTreeMap<&'static str, FrontendRegistration>,
}

impl FrontendRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `registration` for each of its extensions.
    ///
    /// Returns an error if an alias names a checkpoint outside the route, if the declared
    /// artifacts do not cover the route exactly, if one checkpoint is declared twice, or if
    /// any extension is already claimed by another frontend. A registration's route is
    /// non-empty by [`FrontendRegistration::new`], so there is nothing to check here.
    pub fn register(&mut self, registration: FrontendRegistration) -> CompilerResult<()> {
        for (alias, checkpoint) in registration.aliases() {
            if !registration.supports(*checkpoint) {
                return Err(Report::msg(format!(
                    "frontend '{}' maps alias '{alias}' to '{checkpoint}', which is not on its \
                     route",
                    registration.id()
                )));
            }
        }
        for checkpoint in registration.route() {
            if registration.artifact_at(*checkpoint).is_none() {
                return Err(Report::msg(format!(
                    "frontend '{}' declares '{checkpoint}' on its route, but no artifact for it",
                    registration.id()
                )));
            }
        }
        for (position, decl) in registration.artifacts().iter().enumerate() {
            if !registration.supports(decl.checkpoint) {
                return Err(Report::msg(format!(
                    "frontend '{}' declares artifact '{}' at '{}', which is not on its route",
                    registration.id(),
                    decl.id,
                    decl.checkpoint
                )));
            }
            // Only the first declaration for a checkpoint is ever consulted, so a second
            // one strands its renderer rather than adding anything.
            if let Some(first) = registration.artifacts()[..position]
                .iter()
                .find(|earlier| earlier.checkpoint == decl.checkpoint)
            {
                return Err(Report::msg(format!(
                    "frontend '{}' declares two artifacts at '{}': '{}' and '{}'",
                    registration.id(),
                    decl.checkpoint,
                    first.id,
                    decl.id
                )));
            }
        }
        for extension in registration.extensions() {
            if let Some(existing) = self.by_extension.get(extension) {
                return Err(Report::msg(format!(
                    "cannot register frontend '{}' for extension '{extension}': already \
                     registered by frontend '{}'",
                    registration.id(),
                    existing.id()
                )));
            }
        }
        for extension in registration.extensions() {
            self.by_extension.insert(extension, registration);
        }
        Ok(())
    }

    /// The frontend registered for `extension`, if any.
    pub fn for_extension(&self, extension: &str) -> Option<&FrontendRegistration> {
        self.by_extension.get(extension)
    }

    /// Every registered extension, in sorted order, for use in diagnostics.
    pub fn extensions(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_extension.keys().copied()
    }

    /// Every registered frontend, once each, ordered by its first extension.
    ///
    /// A registration is stored once per extension it claims, so the extension map yields a
    /// frontend claiming two extensions twice. Callers that must act *per frontend* rather
    /// than per extension want this: the driver instantiates one frontend per registration
    /// and shares that instance across the registration's extensions, because a second
    /// [`FrontendRegistration::instantiate`] would split the frontend's per-target
    /// memoization across its own providers.
    pub fn registrations(&self) -> impl Iterator<Item = FrontendRegistration> + '_ {
        let mut seen = BTreeSet::new();
        self.by_extension
            .values()
            .filter(move |registration| seen.insert(registration.id()))
            .copied()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use alloc::{
        format,
        string::{String, ToString},
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;

    use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs};
    use midenc_hir::Context;

    use super::*;
    use crate::pipeline::{Flow, TargetContext};

    /// A renderer for fixtures whose emission is never exercised.
    ///
    /// The fixtures below exist to test route and artifact bookkeeping — dispatch, alias
    /// resolution, route ordering, and the both-directions validation in
    /// [`FrontendRegistry::register`]. Nothing in those tests calls `render`, so there is
    /// nothing for it to write. The one test that *does* exercise rendering,
    /// [`two_routes_may_render_one_artifact_id_in_different_shapes`], declares its own
    /// renderers.
    pub(crate) fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        Ok(())
    }

    /// The frontend the fixtures below instantiate to, which compiles nothing.
    ///
    /// A registration cannot be built without a `make`, but these fixtures test route and
    /// artifact bookkeeping and never run a compilation. Reporting rather than panicking
    /// keeps a test that reaches one of these methods by mistake a failed assertion rather
    /// than an unexplained abort. The tests that do run a frontend use a real one — see
    /// [`crate::pipeline::frontends`].
    struct UnexercisedFrontend;

    impl Frontend for UnexercisedFrontend {
        fn compile(&self, _cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            Err(Report::msg("this fixture's frontend is never compiled with"))
        }

        fn provenance(
            &self,
            _cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            Err(Report::msg("this fixture's frontend has no sources"))
        }
    }

    /// Instantiate the frontend that compiles nothing; see [`UnexercisedFrontend`].
    pub(crate) fn unexercised(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(UnexercisedFrontend)
    }

    /// Declare `id` at `checkpoint` with a renderer that writes nothing; see [`unrendered`].
    pub(crate) const fn decl(checkpoint: CheckpointId, id: ArtifactId) -> ArtifactDecl {
        ArtifactDecl {
            checkpoint,
            id,
            render: unrendered,
        }
    }

    std::thread_local! {
        /// What the `fn` pointers on a registration have done when called: the documents
        /// written by the renderers in
        /// `two_routes_may_render_one_artifact_id_in_different_shapes`, and the frontends
        /// built by the `make`s in `instantiate_builds_the_frontend_this_registration_declares`.
        ///
        /// Both are plain `fn` pointers, so neither carries state to inspect and neither has
        /// a reliable address — the compiler may merge two functions with identical bodies.
        /// Recording what they *did* is what distinguishes them. Tests each run on their own
        /// thread, so a thread-local log is per-test.
        static RECORDED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Note that `event` happened.
    fn record(event: String) {
        RECORDED.with_borrow_mut(|recorded| recorded.push(event));
    }

    /// Take everything recorded since the last call.
    fn recorded() -> Vec<String> {
        RECORDED.with_borrow_mut(core::mem::take)
    }

    /// The modules carried by a renderable artifact, standing in for a real Miden Assembly
    /// program: the point is only that both renderers start from the same value.
    fn modules(artifact: &Artifact) -> CompilerResult<&Vec<String>> {
        artifact
            .downcast_ref::<Vec<String>>()
            .ok_or_else(|| Report::msg("expected a list of modules"))
    }

    pub(crate) const WASM: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("wasm"),
        &["wasm", "wat"],
        &[
            CheckpointId::WASM_PARSED,
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[
            ("parse", CheckpointId::WASM_PARSED),
            ("analyze", CheckpointId::HIR_ANALYZED),
            ("transform", CheckpointId::HIR_TRANSFORMED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
        &[
            decl(CheckpointId::WASM_PARSED, ArtifactId::WASM),
            decl(CheckpointId::HIR_INITIAL, ArtifactId::HIR),
            decl(CheckpointId::HIR_ANALYZED, ArtifactId::HIR),
            decl(CheckpointId::HIR_TRANSFORMED, ArtifactId::HIR),
            decl(CheckpointId::MASM_LOWERED, ArtifactId::MASM),
            decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
        ],
        unexercised,
    );

    /// A route with no `wasm` producer, and no `hir.transformed`, so tests can tell
    /// "this frontend never produces it" from "not before the cap".
    pub(crate) const MASM: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("masm"),
        &["masm"],
        &[
            CheckpointId::MASM_PARSED,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[
            ("parse", CheckpointId::MASM_PARSED),
            ("analyze", CheckpointId::HIR_ANALYZED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
        &[
            decl(CheckpointId::MASM_PARSED, ArtifactId::MASM),
            decl(CheckpointId::HIR_ANALYZED, ArtifactId::HIR),
            decl(CheckpointId::MASM_LOWERED, ArtifactId::MASM),
            decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
        ],
        unexercised,
    );

    fn registry() -> FrontendRegistry {
        let mut registry = FrontendRegistry::new();
        registry.register(WASM).expect("wasm should register");
        registry.register(MASM).expect("masm should register");
        registry
    }

    #[test]
    fn dispatch_is_by_target_root_extension() {
        let registry = registry();
        assert_eq!(registry.for_extension("wat").map(|r| r.id()), Some(FrontendId::new("wasm")));
        assert_eq!(registry.for_extension("wasm").map(|r| r.id()), Some(FrontendId::new("wasm")));
        assert_eq!(registry.for_extension("masm").map(|r| r.id()), Some(FrontendId::new("masm")));
        assert!(registry.for_extension("rs").is_none());
    }

    /// The declaration a registration was built from is readable back off it, in order.
    ///
    /// Everything else here goes through a method that interprets one of these — resolving
    /// an alias, ordering a route — so this is what pins that the constructor stores each
    /// argument where the corresponding accessor reads it, and not, say, `aliases` and
    /// `artifacts` transposed.
    #[test]
    fn a_registration_reports_what_it_was_constructed_with() {
        assert_eq!(WASM.id(), FrontendId::new("wasm"));
        assert_eq!(WASM.extensions(), &["wasm", "wat"]);
        assert_eq!(WASM.route().first(), Some(&CheckpointId::WASM_PARSED));
        assert_eq!(WASM.aliases().first(), Some(&("parse", CheckpointId::WASM_PARSED)));
        assert_eq!(WASM.artifacts().first().map(|decl| decl.id), Some(ArtifactId::WASM));
    }

    #[test]
    fn instantiate_builds_the_frontend_this_registration_declares() {
        // A registration's `make` is the only route to its frontend, so what matters is
        // that `instantiate` runs *this* registration's own: with several frontends in one
        // registry, dispatching to the wrong one would still hand back a working frontend,
        // just the wrong language's.
        fn make_alpha(_session: Rc<Session>) -> Rc<dyn Frontend> {
            record("alpha".to_string());
            Rc::new(UnexercisedFrontend)
        }

        fn make_beta(_session: Rc<Session>) -> Rc<dyn Frontend> {
            record("beta".to_string());
            Rc::new(UnexercisedFrontend)
        }

        const ALPHA: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("alpha"),
            &["alpha"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE)],
            make_alpha,
        );
        const BETA: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("beta"),
            &["beta"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE)],
            make_beta,
        );

        let mut registry = FrontendRegistry::new();
        registry.register(ALPHA).expect("alpha should register");
        registry.register(BETA).expect("beta should register");

        let context = Context::default();
        let beta = registry.for_extension("beta").expect("beta is registered");
        drop(beta.instantiate(context.session_rc()));
        drop(beta.instantiate(context.session_rc()));
        assert_eq!(
            recorded(),
            vec!["beta".to_string(), "beta".to_string()],
            "every call must go through this registration's own make: a registration is \
             `&'static` and `Copy`, so it has nowhere to keep an instance, and sharing one across \
             a frontend's extensions is the provider's job"
        );

        let alpha = registry.for_extension("alpha").expect("alpha is registered");
        drop(alpha.instantiate(context.session_rc()));
        assert_eq!(recorded(), vec!["alpha".to_string()], "alpha's own make, not beta's");
    }

    /// Every registered frontend appears once, however many extensions it claims.
    ///
    /// The driver builds one provider per *extension* but instantiates one frontend per
    /// *registration*, and shares that instance across the registration's extensions —
    /// a second `instantiate` would split the frontend's per-target memoization in two.
    /// Iterating the extension map directly would hand out `wasm` twice, so the dedup is
    /// what makes "instantiate once per registration" expressible at all.
    #[test]
    fn registrations_yields_each_frontend_once_however_many_extensions_it_claims() {
        let registry = registry();
        assert_eq!(
            registry.extensions().collect::<Vec<_>>(),
            vec!["masm", "wasm", "wat"],
            "the fixture must register one frontend under two extensions, or this proves nothing"
        );
        assert_eq!(
            registry
                .registrations()
                .map(|registration| registration.id())
                .collect::<Vec<_>>(),
            vec![FrontendId::new("masm"), FrontendId::new("wasm")],
            "the wasm frontend claims both `wasm` and `wat`, and must still be yielded once"
        );
    }

    #[test]
    fn duplicate_extension_registration_is_an_error() {
        const CONFLICT: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("other"),
            &["masm"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE)],
            unexercised,
        );
        let err = registry().register(CONFLICT).expect_err("masm is already registered");
        let rendered = format!("{err}");
        assert!(rendered.contains("masm"), "diagnostic should name the extension: {rendered}");
    }

    /// An empty route cannot be constructed, so [`FrontendRegistration::terminal`] has
    /// nothing left to be wrong about.
    ///
    /// Built at runtime deliberately: every registration in the tree is a `const`, where
    /// this is a compile-time error — the stronger guarantee, but not one a test can
    /// observe as anything but a build failure.
    #[test]
    #[should_panic(expected = "route must not be empty")]
    fn an_empty_route_is_rejected_at_construction() {
        let empty: &'static [CheckpointId] = &[];
        FrontendRegistration::new(
            FrontendId::new("empty"),
            &["nothing"],
            empty,
            &[],
            &[],
            unexercised,
        );
    }

    #[test]
    fn an_alias_outside_the_route_is_rejected() {
        const BAD: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("bad"),
            &["bad"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[("parse", CheckpointId::HIR_INITIAL)],
            &[decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE)],
            unexercised,
        );
        let err = FrontendRegistry::new().register(BAD).expect_err("alias off-route");
        assert!(format!("{err}").contains("hir.initial"));
    }

    #[test]
    fn artifact_at_maps_route_checkpoints_to_artifacts() {
        assert_eq!(WASM.artifact_at(CheckpointId::HIR_TRANSFORMED), Some(ArtifactId::HIR));
        assert_eq!(WASM.artifact_at(CheckpointId::PACKAGE_ASSEMBLED), Some(ArtifactId::PACKAGE));
        assert_eq!(WASM.artifact_at(CheckpointId::MASM_PARSED), None);
    }

    #[test]
    fn decl_at_returns_the_declaration_for_a_route_checkpoint() {
        let decl = WASM.decl_at(CheckpointId::MASM_LOWERED).expect("masm.lowered is on the route");
        assert_eq!(decl.checkpoint, CheckpointId::MASM_LOWERED);
        assert_eq!(decl.id, ArtifactId::MASM);
        // A checkpoint this route never reaches has no declaration to return.
        assert!(WASM.decl_at(CheckpointId::MASM_PARSED).is_none());
    }

    #[test]
    fn two_routes_may_render_one_artifact_id_in_different_shapes() {
        // The property the renderer table exists for: `masm` at `masm.lowered` means the
        // same artifact id at the same checkpoint on both routes, but the two routes write
        // it out differently — one document per module, versus a single concatenated
        // component. A mapping keyed by artifact id alone could not express this, and the
        // core is never told which shape it is dealing with.
        fn render_per_module(artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
            for module in modules(artifact)? {
                record(format!("document: {module}"));
            }
            Ok(())
        }

        fn render_concatenated(artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
            record(format!("document: {}", modules(artifact)?.join(" ")));
            Ok(())
        }

        const PER_MODULE: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("per_module"),
            &["per_module"],
            &[CheckpointId::MASM_LOWERED],
            &[],
            &[ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: render_per_module,
            }],
            unexercised,
        );
        const CONCATENATED: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("concatenated"),
            &["concatenated"],
            &[CheckpointId::MASM_LOWERED],
            &[],
            &[ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: render_concatenated,
            }],
            unexercised,
        );

        let mut registry = FrontendRegistry::new();
        registry.register(PER_MODULE).expect("per_module should register");
        registry.register(CONCATENATED).expect("concatenated should register");

        let per_module = registry
            .for_extension("per_module")
            .and_then(|frontend| frontend.decl_at(CheckpointId::MASM_LOWERED))
            .expect("per_module declares masm.lowered");
        let concatenated = registry
            .for_extension("concatenated")
            .and_then(|frontend| frontend.decl_at(CheckpointId::MASM_LOWERED))
            .expect("concatenated declares masm.lowered");
        assert_eq!(per_module.id, concatenated.id, "both routes declare the same artifact id");
        assert_eq!(per_module.checkpoint, concatenated.checkpoint, "at the same checkpoint");

        // The renderers are told apart by what they write, not by their identity: two
        // declarations that named one renderer would produce the same output twice.
        let context = Context::default();
        let session = context.session();
        let artifact = Artifact::new(ArtifactId::MASM, vec!["a".to_string(), "b".to_string()]);

        (per_module.render)(&artifact, session).expect("per-module render should succeed");
        assert_eq!(recorded(), vec!["document: a".to_string(), "document: b".to_string()]);

        (concatenated.render)(&artifact, session).expect("concatenated render should succeed");
        assert_eq!(recorded(), vec!["document: a b".to_string()]);
    }

    #[test]
    fn a_route_checkpoint_with_no_declared_artifact_is_rejected() {
        const MISSING: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("missing"),
            &["missing"],
            &[CheckpointId::HIR_INITIAL, CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[decl(CheckpointId::HIR_INITIAL, ArtifactId::HIR)],
            unexercised,
        );
        let err = FrontendRegistry::new().register(MISSING).expect_err("incomplete artifacts");
        assert!(format!("{err}").contains("package.assembled"));
    }

    #[test]
    fn an_artifact_declared_off_route_is_rejected() {
        const OFF_ROUTE: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("off"),
            &["off"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[
                decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
                decl(CheckpointId::HIR_INITIAL, ArtifactId::HIR),
            ],
            unexercised,
        );
        let err = FrontendRegistry::new().register(OFF_ROUTE).expect_err("artifact off-route");
        assert!(format!("{err}").contains("hir.initial"));
    }

    #[test]
    fn two_artifacts_declared_for_one_checkpoint_are_rejected() {
        // `decl_at` answers with the first match, so a second declaration for the same
        // checkpoint strands its renderer: the artifact is emitted in the first
        // declaration's shape, and nothing reports the renderer that never runs. Harmless
        // when the declaration was a bare checkpoint-to-artifact mapping; not once it
        // carries behaviour.
        const DUPLICATE: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("duplicate"),
            &["duplicate"],
            &[CheckpointId::MASM_LOWERED],
            &[],
            &[
                decl(CheckpointId::MASM_LOWERED, ArtifactId::MASM),
                decl(CheckpointId::MASM_LOWERED, ArtifactId::HIR),
            ],
            unexercised,
        );
        let err = FrontendRegistry::new()
            .register(DUPLICATE)
            .expect_err("checkpoint declared twice");
        let rendered = format!("{err}");
        assert!(rendered.contains("'duplicate'"), "should name the frontend: {rendered}");
        assert!(rendered.contains("'masm.lowered'"), "should name the checkpoint: {rendered}");
        assert!(
            rendered.contains("'masm'") && rendered.contains("'hir'"),
            "should name both artifacts, since which one is stranded is the point: {rendered}"
        );
    }

    #[test]
    fn aliases_resolve_within_a_route() {
        assert_eq!(WASM.resolve_alias("parse"), Some(CheckpointId::WASM_PARSED));
        assert_eq!(MASM.resolve_alias("parse"), Some(CheckpointId::MASM_PARSED));
        assert_eq!(WASM.resolve_alias("transform"), Some(CheckpointId::HIR_TRANSFORMED));
    }

    #[test]
    fn masm_has_no_transform_alias() {
        assert_eq!(MASM.resolve_alias("transform"), None);
        assert!(!MASM.supports(CheckpointId::HIR_TRANSFORMED));
    }

    #[test]
    fn route_position_orders_checkpoints_within_a_route() {
        assert!(
            WASM.position(CheckpointId::HIR_INITIAL) < WASM.position(CheckpointId::MASM_LOWERED),
            "hir.initial precedes masm.lowered on the wasm route"
        );
        assert_eq!(WASM.position(CheckpointId::MASM_PARSED), None);
        assert_eq!(WASM.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
    }
}
