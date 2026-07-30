//! The language frontends.
//!
//! Each submodule implements [`Frontend`](super::Frontend) for one source language.

pub mod hir;
pub mod masm;
pub mod rust;
pub mod wasm;

pub use self::{
    hir::{HIR_FRONTEND, HirFrontend},
    masm::{MASM_FRONTEND, MasmProjectFrontend},
    rust::{RUST_FRONTEND, RUST_STANDALONE_FRONTEND, RustProjectFrontend, RustStandaloneFrontend},
    wasm::{WASM_FRONTEND, WasmFrontend},
};

#[cfg(test)]
mod synthetic {
    //! A frontend that is entirely foreign to the pipeline core.
    //!
    //! # What this proves
    //!
    //! [`crate::pipeline`] claims to be frontend-neutral: adding a language should require
    //! *registering* it and nothing else — no new variant in a core enum, no new arm in a
    //! core match, no edit to the backend. This module is the mechanical check on that
    //! claim.
    //!
    //! Everything the frontend below is built from is unknown to the core:
    //!
    //! * [`SYNTHETIC_PARSED`], a [`CheckpointId`] the frontend constructs for itself;
    //! * [`SYNTHETIC`], likewise a frontend-constructed [`ArtifactId`];
    //! * [`SyntheticModule`], an artifact *type* named in no file outside this module.
    //!
    //! A weaker fixture — one registering a frontend that only reuses the core's own
    //! checkpoints and artifacts — would prove nothing, because every id it named would
    //! already be one the core enumerates.
    //!
    //! # Neutrality, verified
    //!
    //! Writing this required **no edit** to `checkpoint.rs`, `artifact.rs`, `goal.rs`,
    //! `flow.rs`, `outcome.rs` or `backend.rs`. That claim stays checkable after the fact:
    //! everything foreign this fixture names reaches the core through
    //! [`FrontendRegistration::artifacts`] alone, so moving the checkpoint-to-artifact
    //! mapping back into the core deletes that field and every test below stops compiling —
    //! see *The regression this guards*.
    //!
    //! # The regression this guards
    //!
    //! `goal.rs` used to own the checkpoint-to-artifact mapping as a match over the core's
    //! own constants. A frontend-declared checkpoint fell through it to `None`, so the
    //! artifact produced there could neither be validated against `--emit` nor emitted. The
    //! mapping now lives on [`FrontendRegistration::artifacts`], which is the only thing
    //! that *can* answer for `synthetic.parsed`: no core constant equals either of these
    //! ids, so no match over the core's constants could reach an arm returning
    //! [`SYNTHETIC`].
    //!
    //! (`goal.rs` does name the string `synthetic` in fixtures of its own, so this is a
    //! claim about the core's *constants*, not about the text of core files. It is what
    //! `only_the_registration_can_say_what_the_native_checkpoint_produces` asserts.)
    //!
    //! Restoring the core-owned match therefore removes the field this fixture declares, and
    //! every test here stops compiling. `goal.rs`'s own
    //! `a_frontend_declared_checkpoint_can_satisfy_a_requested_output` covers the narrower
    //! case where the field survives but `--emit` validation consults the core instead.

    use alloc::{
        format,
        rc::Rc,
        string::{String, ToString},
        vec::Vec,
    };
    use core::cell::RefCell;

    use miden_assembly::{
        ModuleParser, ProjectSourceInputs, ProjectSourceProvenanceInputs, SourceFileProvenance,
        ast::ModuleKind,
    };
    use midenc_hir::Context;
    use midenc_session::{Session, diagnostics::Report, miden_project::TargetType};

    use crate::{
        CompilerResult,
        pipeline::{
            Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
            FrontendRegistration, FrontendRegistry, Goal, Observer, Outcome, OutputRequest,
            RecordingObserver, RequestState, TargetContext, TargetRole, resolve_goal,
            testing::{self, VirtualProject},
        },
    };

    // ---------------------------------------------------------------------------------------
    // The frontend's own vocabulary. No constant the core declares equals any of these, so
    // nothing in the core can answer for them by matching over its own ids.
    // ---------------------------------------------------------------------------------------

    /// The frontend's native checkpoint: its source has been parsed into a [`SyntheticModule`].
    const SYNTHETIC_PARSED: CheckpointId = CheckpointId::new("synthetic.parsed");

    /// The artifact a [`SyntheticModule`] is published as.
    const SYNTHETIC: ArtifactId = ArtifactId::new("synthetic");

    /// The declaration that is, by the neutrality claim, the *only* thing adding a frontend
    /// should require.
    ///
    /// The route mixes the frontend's own checkpoint with two of the core's: a `.synth`
    /// target is parsed into a [`SyntheticModule`], lowered straight to Miden Assembly, and
    /// then assembled by the orchestrator — which is what publishes
    /// [`CheckpointId::PACKAGE_ASSEMBLED`], for this frontend as for every other.
    ///
    /// A route must describe what the frontend actually does, since `--stop-after` and
    /// `--emit` are validated against it. This one names [`CheckpointId::MASM_LOWERED`]
    /// rather than [`CheckpointId::HIR_INITIAL`] for that reason: a route claiming HIR would
    /// oblige the fixture either to build a real `MidenComponent`, or to delegate to
    /// [`backend::hir_to_masm`](crate::pipeline::backend::hir_to_masm), whose own three
    /// checkpoints would then be reached but undeclared. Going straight to Miden Assembly is
    /// both smaller and honest, and still exercises a foreign checkpoint feeding a core one.
    const SYNTHETIC_FRONTEND: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("synthetic"),
        &["synth"],
        &[SYNTHETIC_PARSED, CheckpointId::MASM_LOWERED, CheckpointId::PACKAGE_ASSEMBLED],
        &[
            ("parse", SYNTHETIC_PARSED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
        &[
            ArtifactDecl {
                checkpoint: SYNTHETIC_PARSED,
                id: SYNTHETIC,
                render: unrendered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: unrendered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: unrendered,
            },
        ],
        make_synthetic,
    );

    /// Build the frontend this registration declares.
    ///
    /// The `.synth` frontend holds no per-target state, so it ignores the session; a real
    /// frontend keeps it for the memoization [`Frontend::provenance`] requires.
    fn make_synthetic(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(SyntheticFrontend)
    }

    /// This frontend's renderer, which writes nothing.
    ///
    /// Emission is not exercised here: these tests run the frontend to a goal and inspect
    /// the captured artifact directly, so no `--emit` destination is ever resolved. Rendering
    /// a [`SyntheticModule`] would mean giving it an
    /// [`OutputType`](midenc_session::OutputType), which is a session-level vocabulary this
    /// fixture deliberately stays out of.
    fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // The frontend itself.
    // ---------------------------------------------------------------------------------------

    /// The parsed form of a `.synth` source file.
    ///
    /// A `.synth` file is one `push <integer>` per line, and this is the whole of the
    /// language's AST. It is deliberately a type the pipeline core has never seen: the
    /// artifact envelope is type-erased, so nothing between `cx.checkpoint` and the caller's
    /// `Outcome::downcast` needs to name it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SyntheticModule {
        pushes: Vec<u32>,
    }

    impl SyntheticModule {
        /// Parse `source`, rejecting anything that is not a `push`.
        fn parse(source: &str) -> CompilerResult<Self> {
            let mut pushes = Vec::new();
            for (number, line) in source.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let operand = line.strip_prefix("push ").ok_or_else(|| {
                    Report::msg(format!(
                        "line {}: expected `push <integer>`, got '{line}'",
                        number + 1
                    ))
                })?;
                let operand = operand.trim().parse::<u32>().map_err(|err| {
                    Report::msg(format!("line {}: invalid operand '{operand}': {err}", number + 1))
                })?;
                pushes.push(operand);
            }
            Ok(Self { pushes })
        }

        /// Render this module as the Miden Assembly it lowers to.
        fn to_masm(&self) -> String {
            let mut rendered = String::from("pub proc main\n");
            for operand in &self.pushes {
                rendered.push_str(&format!("    push.{operand}\n"));
            }
            rendered.push_str("end\n");
            rendered
        }

        /// Lower to assembly-ready Miden Assembly.
        fn lower(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceInputs> {
            let session = cx.session();
            let namespace = cx.assembly().target.namespace.inner().clone();
            let root = ModuleParser::new(Some(ModuleKind::Library)).parse_str(
                Some(namespace.as_ref()),
                self.to_masm(),
                session.source_manager.clone(),
            )?;
            Ok(ProjectSourceInputs {
                root,
                support: Vec::new(),
            })
        }
    }

    /// Compiles targets whose root is a `.synth` file.
    struct SyntheticFrontend;

    impl SyntheticFrontend {
        /// Read this target's root source.
        ///
        /// A real frontend would prefer [`TargetContext::input`] when one is present, since a
        /// stdin-backed input has no path; the fixture is always file-backed.
        fn read(cx: &TargetContext<'_>) -> CompilerResult<String> {
            let path = &cx.assembly().resolved_target_root;
            std::fs::read_to_string(path)
                .map_err(|err| Report::msg(format!("unable to read '{}': {err}", path.display())))
        }
    }

    impl Frontend for SyntheticFrontend {
        /// Parse, publish, and lower — honouring [`Flow::Break`] at the native checkpoint.
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let module = SyntheticModule::parse(&Self::read(cx)?)?;
            let module = match cx.checkpoint(SYNTHETIC_PARSED, SYNTHETIC, module)? {
                Flow::Continue(module) => module,
                Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
            };

            let sources = module.lower(cx)?;
            cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, sources)
        }

        /// This target's build provenance: its single source file.
        ///
        /// Real frontends memoize this by [`TargetContext::target_key`], because the assembler
        /// calls it repeatedly while hashing the dependency closure. Re-reading one small
        /// fixture file is cheaper here than the cache would be.
        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: Self::read(cx)?.into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    // ---------------------------------------------------------------------------------------
    // Fixtures.
    // ---------------------------------------------------------------------------------------

    /// Two pushes, so a downcast of the captured artifact has something to be wrong about.
    const SOURCE: &str = "push 1\npush 2\n";

    /// The [`SyntheticModule`] [`SOURCE`] parses to.
    fn expected_module() -> SyntheticModule {
        SyntheticModule {
            pushes: alloc::vec![1, 2],
        }
    }

    /// A single-target virtual project whose root is a `.synth` file holding [`SOURCE`].
    fn project(name: &str) -> VirtualProject {
        let root = testing::fixture_source(name, "lib.synth", SOURCE);
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// The goal a `--stop-after=<alias>` run of this frontend resolves to.
    ///
    /// Goes through [`resolve_goal`] rather than [`Goal::at`] so the frontend's own alias
    /// table is what selects the checkpoint, and the run below is driven by the result.
    fn goal_for_stop_after(alias: &str) -> Goal {
        let request = OutputRequest::new(Vec::new()).with_stop_after(Some(alias.to_string()));
        resolve_goal(&request, &SYNTHETIC_FRONTEND)
            .expect("the frontend's own aliases must resolve against its own route")
    }

    /// Run the synthetic frontend over a fresh project to `goal`.
    ///
    /// The frontend comes from [`FrontendRegistration::instantiate`] rather than being
    /// constructed here, so what these tests exercise is what a caller holding only the
    /// registration would get.
    ///
    /// Returns whether the run stopped, the observed checkpoint trace, and whatever the
    /// target captured.
    fn run(name: &str, goal: Goal) -> (bool, Vec<CheckpointId>, Option<Outcome>) {
        let project = project(name);
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state =
            RequestState::new(goal, alloc::vec![observer.clone() as Rc<RefCell<dyn Observer>>]);

        let cx = TargetContext::for_testing(
            &assembly,
            Rc::new(Context::default()),
            TargetRole::Root,
            &state,
        );

        let frontend = SYNTHETIC_FRONTEND.instantiate(cx.session());
        let flow = frontend.compile(&cx).expect("the synthetic frontend should compile");
        let trace = observer.borrow().records().iter().map(|(c, _)| *c).collect();
        (flow.is_break(), trace, state.take_outcome())
    }

    // ---------------------------------------------------------------------------------------
    // The conformance tests.
    // ---------------------------------------------------------------------------------------

    /// Registering is all it takes: the registry accepts a route built around a checkpoint
    /// and an artifact it has never heard of, and dispatches to it by extension.
    #[test]
    fn a_foreign_frontend_needs_only_to_be_registered() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(SYNTHETIC_FRONTEND)
            .expect("a frontend built from its own ids must register like any other");

        let found = registry.for_extension("synth").expect("dispatch is by target-root extension");
        assert_eq!(found.id(), FrontendId::new("synthetic"));
        assert!(found.supports(SYNTHETIC_PARSED), "the native checkpoint is on the route");
        assert_eq!(found.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    /// A checkpoint the core has never heard of still resolves as a stop point, both by the
    /// frontend's alias for it and by its fully-qualified id.
    #[test]
    fn a_native_checkpoint_resolves_as_a_stop_point() {
        let by_alias = OutputRequest::new(Vec::new()).with_stop_after(Some("parse".to_string()));
        let goal = resolve_goal(&by_alias, &SYNTHETIC_FRONTEND).expect("`parse` is an alias");
        assert_eq!(goal.checkpoint(), SYNTHETIC_PARSED);

        let by_id =
            OutputRequest::new(Vec::new()).with_stop_after(Some("synthetic.parsed".to_string()));
        let goal = resolve_goal(&by_id, &SYNTHETIC_FRONTEND).expect("the id names a route entry");
        assert_eq!(goal.checkpoint(), SYNTHETIC_PARSED);

        // And an uncapped run still reaches the end of the route.
        let goal = resolve_goal(&OutputRequest::new(Vec::new()), &SYNTHETIC_FRONTEND)
            .expect("an uncapped run resolves to the terminal checkpoint");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    /// Stopping at the native checkpoint captures the frontend's *own* artifact type, and the
    /// observer sees the native checkpoint go by.
    ///
    /// The goal comes from [`resolve_goal`], so one unbroken chain runs here: the frontend's
    /// own alias `parse` selects its own checkpoint, the run stops there, and what comes back
    /// is the artifact its registration declares, downcast to the type only it knows.
    #[test]
    fn stopping_at_a_native_checkpoint_captures_the_native_artifact() {
        let goal = goal_for_stop_after("parse");
        assert_eq!(goal.checkpoint(), SYNTHETIC_PARSED, "`parse` is the frontend's own alias");
        let (stopped, trace, captured) = run("synthetic_stop", goal);

        assert!(stopped, "the goal is the frontend's own checkpoint, so it must stop there");
        assert_eq!(
            trace,
            alloc::vec![SYNTHETIC_PARSED],
            "the observer must see the native checkpoint, and lowering must not have run"
        );

        let captured = captured.expect("stopping at the goal must capture an artifact");
        assert_eq!(captured.checkpoint(), SYNTHETIC_PARSED);
        assert_eq!(
            captured.artifact().id(),
            SYNTHETIC_FRONTEND
                .artifact_at(SYNTHETIC_PARSED)
                .expect("the native checkpoint is on the route"),
            "the published artifact must be the one the registration declares for this \
             checkpoint, not merely the id the frontend happened to pass to `checkpoint`"
        );
        assert_eq!(
            captured
                .downcast::<SyntheticModule>()
                .expect("must downcast to the native type"),
            expected_module(),
            "the artifact must survive type erasure intact, not merely as the right variant"
        );
    }

    /// The mirror of the previous test: with a goal further along the route, the same
    /// publication continues, lowering runs, and the run yields Miden Assembly.
    ///
    /// Without this, a frontend that stopped unconditionally would pass the test above.
    #[test]
    fn continuing_past_the_native_checkpoint_reaches_the_core_route() {
        let (stopped, trace, captured) =
            run("synthetic_continue", Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        assert!(
            !stopped,
            "package.assembled is the orchestrator's to publish, not the frontend's"
        );
        assert_eq!(
            trace,
            alloc::vec![SYNTHETIC_PARSED, CheckpointId::MASM_LOWERED],
            "a native checkpoint and a core one must be observable from one route"
        );
        assert!(captured.is_none(), "nothing may be captured before the goal is reached");
    }

    /// A native checkpoint's artifact can only be known from the registration.
    ///
    /// This is the assertion that fails if the checkpoint-to-artifact mapping moves back into
    /// the core: `synthetic.parsed` and `synthetic` are not among the core's constants, so no
    /// match over them could answer.
    #[test]
    fn only_the_registration_can_say_what_the_native_checkpoint_produces() {
        assert_eq!(SYNTHETIC_FRONTEND.artifact_at(SYNTHETIC_PARSED), Some(SYNTHETIC));

        for known in [ArtifactId::HIR, ArtifactId::MASM, ArtifactId::PACKAGE, ArtifactId::WASM] {
            assert_ne!(SYNTHETIC, known, "the synthetic artifact must be foreign to the core");
        }
        for known in [
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_PARSED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
            CheckpointId::WASM_PARSED,
        ] {
            assert_ne!(SYNTHETIC_PARSED, known, "the native checkpoint must be foreign too");
        }
    }

    /// The frontend's provenance contract holds for a foreign frontend as well: it reports
    /// its own sources without publishing anything.
    #[test]
    fn provenance_reports_the_targets_sources_without_publishing() {
        let project = project("synthetic_provenance");
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(SYNTHETIC_PARSED),
            alloc::vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(
            &assembly,
            Rc::new(Context::default()),
            TargetRole::Root,
            &state,
        );

        let frontend = SYNTHETIC_FRONTEND.instantiate(cx.session());
        let provenance = frontend.provenance(&cx).expect("provenance should succeed");
        assert_eq!(&*provenance.root.content, SOURCE);
        assert!(provenance.support.is_empty());
        assert!(
            observer.borrow().records().is_empty(),
            "provenance must not publish checkpoints, even at the goal"
        );
    }
}
