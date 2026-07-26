use alloc::{format, string::String, vec::Vec};

use midenc_session::{OutputType, OutputTypeSpec, diagnostics::Report};

use super::{ArtifactId, CheckpointId, FrontendRegistration};
use crate::CompilerResult;

/// Map an [`OutputType`] onto the artifact it names.
///
/// `OutputType::Ast` has no producer and no artifact id; it is filtered out of every
/// expansion until the variant is removed.
pub fn artifact_id_for_output(output_type: OutputType) -> Option<ArtifactId> {
    match output_type {
        OutputType::Ast => None,
        OutputType::Wat => Some(ArtifactId::WASM),
        OutputType::Hir => Some(ArtifactId::HIR),
        OutputType::Masm => Some(ArtifactId::MASM),
        OutputType::Mast | OutputType::Masp => Some(ArtifactId::PACKAGE),
    }
}

/// The caller's explicitly requested outputs and optional stop point.
///
/// The specs are the `--emit` selections. They ask for artifacts *in addition to* the usual
/// final package, so they never shorten or extend a compilation; only `--stop-after` does
/// that. They are still validated against the resolved goal, so a named output the run
/// cannot reach is reported rather than silently dropped.
///
/// `Options::with_output_types` inserts an implicit `OutputType::Masp` into
/// `Options::output_types` on every invocation, so that map cannot distinguish explicit
/// from implicit and must never be used for goal resolution.
#[derive(Debug, Clone, Default)]
pub struct OutputRequest {
    specs: Vec<OutputTypeSpec>,
    stop_after: Option<String>,
}

impl OutputRequest {
    /// Construct a request from the explicitly requested output specs.
    pub fn new(specs: Vec<OutputTypeSpec>) -> Self {
        Self {
            specs,
            stop_after: None,
        }
    }

    /// Set the `--stop-after` value: an alias or a fully-qualified checkpoint id.
    pub fn with_stop_after(mut self, stop_after: Option<String>) -> Self {
        self.stop_after = stop_after;
        self
    }

    /// The explicitly requested output specs.
    pub fn specs(&self) -> &[OutputTypeSpec] {
        &self.specs
    }

    /// The raw `--stop-after` value, if one was given.
    pub fn stop_after(&self) -> Option<&str> {
        self.stop_after.as_deref()
    }
}

/// The resolved terminal checkpoint for a compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    checkpoint: CheckpointId,
}

impl Goal {
    /// Construct a goal terminating at `checkpoint`.
    ///
    /// Prefer [`resolve_goal`]; this exists for tests and for callers that already hold a
    /// validated checkpoint.
    pub const fn at(checkpoint: CheckpointId) -> Self {
        Self { checkpoint }
    }

    /// The checkpoint at which compilation should stop.
    pub const fn checkpoint(&self) -> CheckpointId {
        self.checkpoint
    }
}

/// Resolve the terminal checkpoint for `request` against `frontend`'s route.
///
/// * `--stop-after` alone decides how far compilation runs: the goal is that checkpoint if
///   one was given, and otherwise the route's terminal checkpoint.
/// * `--emit` never influences the goal. It asks for artifacts *in addition to* the usual
///   final package, so `--emit=hir` still builds a package; only pairing it with
///   `--stop-after` avoids the work past the stop point.
/// * Requested outputs are still validated against the goal. A *named* output whose
///   artifact no reached checkpoint produces is a usage error, distinguishing "produced
///   only after the stop point" from "this frontend never produces it". An `--emit=all` or
///   subset expansion asks for whatever the run can produce, so an out-of-reach member is
///   skipped instead.
pub fn resolve_goal(
    request: &OutputRequest,
    frontend: &FrontendRegistration,
) -> CompilerResult<Goal> {
    let terminal = frontend.route().len() - 1;
    let goal = match request.stop_after() {
        Some(value) => resolve_stop_after(value, frontend)?,
        None => terminal,
    };

    // Whether some checkpoint in `frontend.route()[..=last]` produces `artifact`, according to
    // the registration's own declaration. Passing the goal position answers "can this run
    // emit it?"; passing the terminal position answers "can this frontend emit it at all?".
    let produces_by = |artifact: ArtifactId, last: usize| {
        frontend.route()[..=last]
            .iter()
            .any(|checkpoint| frontend.artifact_at(*checkpoint) == Some(artifact))
    };

    for spec in request.specs() {
        let (output_types, is_expansion): (Vec<OutputType>, bool) = match spec {
            OutputTypeSpec::All { .. } => (OutputType::all().to_vec(), true),
            OutputTypeSpec::Subset { output_types, .. } => (output_types.to_vec(), true),
            OutputTypeSpec::Typed { output_type, .. } => (alloc::vec![*output_type], false),
        };

        for output_type in output_types {
            let Some(artifact) = artifact_id_for_output(output_type) else {
                continue;
            };
            if produces_by(artifact, goal) {
                continue;
            }
            // An expansion asks for whatever this run can produce, so an artifact this run
            // never reaches is skipped rather than rejected.
            if is_expansion {
                continue;
            }
            // The route may still produce the artifact past the goal; that is a different
            // mistake than asking a frontend for an artifact it never produces at all. It
            // can only arise under `--stop-after`, since an uncapped goal is the terminal
            // checkpoint and therefore reaches everything the route produces.
            return Err(if produces_by(artifact, terminal) {
                Report::msg(format!(
                    "cannot emit '{}': it is produced after the requested stop point '{}'",
                    output_type.extension(),
                    frontend.route()[goal]
                ))
            } else {
                Report::msg(format!(
                    "cannot emit '{}': frontend '{}' does not produce a '{artifact}' artifact",
                    output_type.extension(),
                    frontend.id()
                ))
            });
        }
    }

    Ok(Goal {
        checkpoint: frontend.route()[goal],
    })
}

/// Resolve a `--stop-after` value to a route position.
fn resolve_stop_after(value: &str, frontend: &FrontendRegistration) -> CompilerResult<usize> {
    if let Some(checkpoint) = frontend.resolve_alias(value) {
        return frontend.position(checkpoint).ok_or_else(|| {
            Report::msg(format!(
                "internal error: alias '{value}' maps to '{checkpoint}', which is not on the \
                 route of frontend '{}'",
                frontend.id()
            ))
        });
    }

    if let Some(position) =
        frontend.route().iter().position(|checkpoint| checkpoint.as_str() == value)
    {
        return Ok(position);
    }

    let aliases = frontend.alias_names().collect::<Vec<_>>().join(", ");
    let checkpoints = frontend
        .route()
        .iter()
        .map(|checkpoint| checkpoint.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(Report::msg(format!(
        "'{value}' is not a valid stop point for frontend '{}'; expected one of the aliases \
         [{aliases}] or checkpoints [{checkpoints}]",
        frontend.id()
    )))
}

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString, vec};

    use midenc_session::OutputType;

    use super::*;
    // `MASM`'s route produces no `wasm` artifact at all, which is what separates "this
    // frontend cannot produce it" from "not before the cap".
    // `decl` declares an artifact with a renderer that writes nothing, and `unexercised`
    // builds a frontend that compiles nothing: goal resolution neither emits nor runs a
    // frontend, so nothing here calls either.
    use crate::pipeline::{
        FrontendId,
        registry::tests::{MASM, WASM, decl, unexercised},
    };

    fn typed(output_type: OutputType) -> OutputTypeSpec {
        OutputTypeSpec::Typed {
            output_type,
            path: None,
        }
    }

    /// A `Subset` spec over `output_types`, sharing one destination.
    ///
    /// Increment 1's review established that `Subset` is produced by exactly one input: the
    /// `ir` shorthand in `midenc-session/src/outputs.rs`, which expands to
    /// `OutputType::ir()` — wat, hir, masm. A user naming several outputs writes several
    /// `--emit` flags and gets one `Typed` spec each, never a `Subset`. That is why the
    /// resolver treats `Subset` like `All`, as an *expansion*: it asks for whatever of the
    /// shorthand's members this run can produce, so an unreachable member is skipped rather
    /// than reported. Naming `masm` outright would still be an error.
    fn subset(output_types: &[OutputType]) -> OutputTypeSpec {
        OutputTypeSpec::Subset {
            output_types: output_types.iter().copied().collect(),
            path: None,
        }
    }

    #[test]
    fn output_types_map_onto_artifact_ids() {
        assert_eq!(artifact_id_for_output(OutputType::Wat), Some(ArtifactId::WASM));
        assert_eq!(artifact_id_for_output(OutputType::Hir), Some(ArtifactId::HIR));
        assert_eq!(artifact_id_for_output(OutputType::Masm), Some(ArtifactId::MASM));
        assert_eq!(artifact_id_for_output(OutputType::Mast), Some(ArtifactId::PACKAGE));
        assert_eq!(artifact_id_for_output(OutputType::Masp), Some(ArtifactId::PACKAGE));
        // Ast has no producer; it is filtered out until the variant is deleted.
        assert_eq!(artifact_id_for_output(OutputType::Ast), None);
    }

    #[test]
    fn no_explicit_outputs_means_a_full_build() {
        let goal = resolve_goal(&OutputRequest::new(vec![]), &WASM).expect("should resolve");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    #[test]
    fn stop_after_alone_sets_the_goal() {
        // The case that fails if the implicit Masp inserted by Options::with_output_types
        // is treated as an explicit request.
        let request = OutputRequest::new(vec![]).with_stop_after(Some("parse".to_string()));
        let goal = resolve_goal(&request, &WASM).expect("should resolve");
        assert_eq!(goal.checkpoint(), CheckpointId::WASM_PARSED);
    }

    #[test]
    fn emitting_an_intermediate_artifact_still_builds_the_package() {
        // The motivating case: `--emit=hir` asks for the HIR *in addition to* the usual
        // final package, so it must not stop the run at the last HIR checkpoint.
        let request = OutputRequest::new(vec![typed(OutputType::Hir)]);
        let goal = resolve_goal(&request, &WASM).expect("should resolve");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    #[test]
    fn several_explicit_outputs_still_resolve_to_the_terminal_checkpoint() {
        let request = OutputRequest::new(vec![typed(OutputType::Hir), typed(OutputType::Masm)]);
        let goal = resolve_goal(&request, &WASM).expect("should resolve");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    #[test]
    fn a_bare_hir_request_under_a_cap_resolves_to_the_last_reached_hir_checkpoint() {
        // The successor of `-Canalyze-only --emit=hir=-`: `hir.analyzed` does produce a
        // `hir` artifact, so this must resolve rather than fail as "produced after the
        // requested stop point".
        let request = OutputRequest::new(vec![typed(OutputType::Hir)])
            .with_stop_after(Some("analyze".to_string()));
        let goal = resolve_goal(&request, &WASM).expect("hir.analyzed produces hir");
        assert_eq!(goal.checkpoint(), CheckpointId::HIR_ANALYZED);
    }

    #[test]
    fn a_named_output_beyond_the_cap_is_a_usage_error() {
        let request = OutputRequest::new(vec![typed(OutputType::Masm)])
            .with_stop_after(Some("analyze".to_string()));
        let err = resolve_goal(&request, &WASM).expect_err("masm is past analyze");
        let rendered = format!("{err}");
        assert!(rendered.contains("masm"), "should name the output: {rendered}");
        assert!(rendered.contains("hir.analyzed"), "should name the cap: {rendered}");
    }

    #[test]
    fn an_artifact_the_route_never_produces_is_a_different_usage_error() {
        // No cap at all: the only possible complaint is that the frontend cannot produce it.
        let request = OutputRequest::new(vec![typed(OutputType::Wat)]);
        let err = resolve_goal(&request, &MASM).expect_err("the masm route has no wasm producer");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("does not produce"),
            "an unproducible artifact must not be reported as a stop-point problem: {rendered}"
        );

        // Same artifact, now with a cap: still unproducible, so still the same complaint
        // rather than the beyond-the-stop-point one.
        let request = OutputRequest::new(vec![typed(OutputType::Wat)])
            .with_stop_after(Some("analyze".to_string()));
        let err = resolve_goal(&request, &MASM).expect_err("a cap does not make wasm producible");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("does not produce"),
            "a cap must not disguise an unproducible artifact: {rendered}"
        );
    }

    #[test]
    fn emit_all_never_exceeds_the_cap() {
        let request = OutputRequest::new(vec![OutputTypeSpec::All { path: None }])
            .with_stop_after(Some("analyze".to_string()));
        let goal = resolve_goal(&request, &WASM).expect("all expands within the cap");
        assert_eq!(goal.checkpoint(), CheckpointId::HIR_ANALYZED);
    }

    #[test]
    fn the_ir_subset_expands_within_the_cap() {
        // `--emit=ir --stop-after=analyze` on the wasm route: of wat, hir and masm, only
        // masm is past the cap. As an expansion it is skipped, so this resolves; the same
        // outputs named individually would be a usage error, as the test below pins.
        let request = OutputRequest::new(vec![subset(OutputType::ir())])
            .with_stop_after(Some("analyze".to_string()));
        let goal = resolve_goal(&request, &WASM).expect("the ir subset expands within the cap");
        assert_eq!(goal.checkpoint(), CheckpointId::HIR_ANALYZED);

        // The discriminating half: the same member, named rather than expanded, is rejected.
        let named = OutputRequest::new(vec![typed(OutputType::Masm)])
            .with_stop_after(Some("analyze".to_string()));
        resolve_goal(&named, &WASM).expect_err("a named masm past the cap is still an error");
    }

    #[test]
    fn the_ir_subset_skips_members_the_route_never_produces() {
        // Uncapped, so the only reason to skip `wat` is that the masm route has no wasm
        // producer at all. `an_artifact_the_route_never_produces_is_a_different_usage_error`
        // pins that naming it outright still fails.
        let request = OutputRequest::new(vec![subset(OutputType::ir())]);
        let goal = resolve_goal(&request, &MASM).expect("an unproducible member is skipped");
        assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
    }

    #[test]
    fn an_unknown_alias_lists_the_valid_set() {
        let request = OutputRequest::new(vec![]).with_stop_after(Some("codegen".to_string()));
        let err = resolve_goal(&request, &WASM).expect_err("codegen is not an alias");
        let rendered = format!("{err}");
        assert!(rendered.contains("transform"), "should list valid aliases: {rendered}");
        // The bare `transform` above is also a substring of the `hir.transformed` checkpoint,
        // so it does not pin the alias clause on its own. The full bracketed alias list can
        // only come from the alias clause, so asserting on it fails if that clause is dropped.
        assert!(
            rendered.contains("[parse, analyze, transform, lower, assemble]"),
            "should render the whole alias set: {rendered}"
        );
    }

    #[test]
    fn a_fully_qualified_checkpoint_id_is_accepted_as_a_cap() {
        let request = OutputRequest::new(vec![]).with_stop_after(Some("hir.initial".to_string()));
        let goal = resolve_goal(&request, &WASM).expect("should resolve");
        assert_eq!(goal.checkpoint(), CheckpointId::HIR_INITIAL);
    }

    #[test]
    fn a_checkpoint_outside_the_route_is_rejected() {
        let request = OutputRequest::new(vec![]).with_stop_after(Some("masm.parsed".to_string()));
        let err = resolve_goal(&request, &WASM).expect_err("masm.parsed is off-route");
        assert!(format!("{err}").contains("masm.parsed"));
    }

    #[test]
    fn a_frontend_declared_checkpoint_participates_in_goal_resolution() {
        // The point of moving the mapping onto the registration: a checkpoint the core
        // has never heard of must still resolve as a stop point and validate outputs.
        const NATIVE: CheckpointId = CheckpointId::new("synthetic.parsed");
        const SYNTHETIC: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("synthetic"),
            &["synth"],
            &[NATIVE, CheckpointId::HIR_INITIAL, CheckpointId::PACKAGE_ASSEMBLED],
            &[("parse", NATIVE), ("assemble", CheckpointId::PACKAGE_ASSEMBLED)],
            &[
                decl(NATIVE, ArtifactId::new("synthetic")),
                decl(CheckpointId::HIR_INITIAL, ArtifactId::HIR),
                decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
            ],
            unexercised,
        );

        let request = OutputRequest::new(vec![]).with_stop_after(Some("parse".to_string()));
        let goal = resolve_goal(&request, &SYNTHETIC).expect("should resolve");
        assert_eq!(goal.checkpoint(), NATIVE);

        // And an output the synthetic route cannot produce is still rejected.
        let request = OutputRequest::new(vec![OutputTypeSpec::Typed {
            output_type: OutputType::Masm,
            path: None,
        }]);
        let err = resolve_goal(&request, &SYNTHETIC).expect_err("synthetic route has no masm");
        assert!(format!("{err}").contains("masm"));
    }

    #[test]
    fn a_frontend_declared_checkpoint_can_satisfy_a_requested_output() {
        // The discriminating case for a declared mapping: a core-owned `produces` returns
        // `None` for a checkpoint it has never heard of, so `synthetic.lowered` would
        // produce nothing at all. No checkpoint on this route would then produce `masm` —
        // not by the cap and not by the terminal checkpoint either — so this `--emit` would
        // be rejected as "frontend 'synthetic' does not produce a 'masm' artifact", even
        // though the registration declares exactly that mapping.
        const NATIVE: CheckpointId = CheckpointId::new("synthetic.lowered");
        const SYNTHETIC: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("synthetic"),
            &["synth"],
            &[NATIVE, CheckpointId::PACKAGE_ASSEMBLED],
            &[("lower", NATIVE)],
            &[
                decl(NATIVE, ArtifactId::MASM),
                decl(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE),
            ],
            unexercised,
        );

        let request = OutputRequest::new(vec![typed(OutputType::Masm)])
            .with_stop_after(Some("lower".to_string()));
        let goal = resolve_goal(&request, &SYNTHETIC).expect("synthetic.lowered produces masm");
        assert_eq!(goal.checkpoint(), NATIVE);
    }
}
