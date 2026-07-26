//! Proves the pipeline contracts are usable from outside the crate.
//!
//! The unit tests live beside the code they cover; this file exists only to catch
//! accidentally-private surface.

#![cfg(feature = "std")]

use midenc_compile::pipeline::{
    Artifact, ArtifactId, CheckpointId, Flow, FrontendId, FrontendRegistration, FrontendRegistry,
    Goal, Outcome, OutputRequest, resolve_goal,
};
use midenc_session::{OutputType, OutputTypeSpec};

const WASM: FrontendRegistration = FrontendRegistration {
    id: FrontendId::new("wasm"),
    extensions: &["wasm", "wat"],
    route: &[
        CheckpointId::HIR_INITIAL,
        CheckpointId::HIR_TRANSFORMED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    aliases: &[
        ("parse", CheckpointId::HIR_INITIAL),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    artifacts: &[
        (CheckpointId::HIR_INITIAL, ArtifactId::HIR),
        (CheckpointId::HIR_TRANSFORMED, ArtifactId::HIR),
        (CheckpointId::MASM_LOWERED, ArtifactId::MASM),
        (CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
    ],
};

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
