//! Proves the pipeline contracts are usable from outside the crate.
//!
//! The unit tests live beside the code they cover; this file exists only to catch
//! accidentally-private surface.

#![cfg(feature = "std")]

use std::rc::Rc;

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs};
use midenc_compile::{
    CompilerResult,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, FrontendRegistry, Goal, Outcome, OutputRequest, TargetContext,
        resolve_goal,
    },
};
use midenc_session::{OutputType, OutputTypeSpec, Session, diagnostics::Report};

/// This fixture's renderer, which writes nothing.
///
/// These tests only check that the registration and goal contracts are reachable from
/// outside the crate; nothing here emits, so there is no destination to write to. The
/// renderers that do write live in the unit tests beside the code they cover.
fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
    Ok(())
}

/// Declare `id` at `checkpoint` with a renderer that writes nothing; see [`unrendered`].
const fn decl(checkpoint: CheckpointId, id: ArtifactId) -> ArtifactDecl {
    ArtifactDecl {
        checkpoint,
        id,
        render: unrendered,
    }
}

/// This fixture's frontend, which compiles nothing.
///
/// A registration cannot be built without one, and these tests never run a compilation —
/// but implementing the trait here is itself part of what this file checks, since a
/// third-party frontend has to be able to. Reporting rather than panicking keeps a
/// mistaken call a failed assertion rather than an unexplained abort.
struct UnexercisedFrontend;

impl Frontend for UnexercisedFrontend {
    fn compile(&self, _cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        Err(Report::msg("this fixture's frontend is never compiled with"))
    }

    fn provenance(&self, _cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        Err(Report::msg("this fixture's frontend has no sources"))
    }
}

/// Instantiate the frontend that compiles nothing; see [`UnexercisedFrontend`].
fn unexercised(_session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(UnexercisedFrontend)
}

const WASM: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("wasm"),
    &["wasm", "wat"],
    &[
        CheckpointId::HIR_INITIAL,
        CheckpointId::HIR_TRANSFORMED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    &[
        ("parse", CheckpointId::HIR_INITIAL),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        decl(CheckpointId::HIR_INITIAL, ArtifactId::HIR),
        decl(CheckpointId::HIR_TRANSFORMED, ArtifactId::HIR),
        decl(CheckpointId::MASM_LOWERED, ArtifactId::MASM),
        decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
    ],
    unexercised,
);

#[test]
fn an_external_crate_can_register_a_frontend_and_resolve_a_goal() {
    let mut registry = FrontendRegistry::new();
    registry.register(WASM).expect("should register");

    let frontend = registry.for_extension("wat").expect("wat is registered");
    let request = OutputRequest::new(vec![OutputTypeSpec::Typed {
        output_type: OutputType::Hir,
        path: None,
    }]);
    // `--emit` requests an *additional* artifact; it never shortens the build, so the goal
    // is still the route's terminal checkpoint.
    let goal = resolve_goal(&request, frontend).expect("should resolve");
    assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
}

#[test]
fn an_external_crate_can_build_and_unwrap_an_outcome() {
    let outcome = Outcome::new(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, 7u32));
    assert_eq!(outcome.checkpoint(), CheckpointId::HIR_INITIAL);
    assert_eq!(outcome.downcast::<u32>().expect("should downcast"), 7);

    // Goal and Flow are constructible externally too.
    let _ = Goal::at(CheckpointId::MASM_LOWERED);
    assert!(!Flow::Continue(0u32).is_stop());
}
