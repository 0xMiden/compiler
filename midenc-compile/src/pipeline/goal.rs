use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use midenc_session::{Options, OutputType, OutputTypeSpec, diagnostics::Report};

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

/// A request to end compilation early, as expressed by a `-C` stop flag.
///
/// # These are stop points, not switches
///
/// Each variant names a *phase*, and asking for it means "run up to and including that phase,
/// then exit". They are therefore resolved into a [`Goal`] like any `--stop-after` value, and
/// the existing stop-at-goal machinery does the exiting. No frontend checks these flags: a
/// frontend-local exit would be a second mechanism, disagreeing with the goal on when the
/// checkpoint gets published and on what — if anything — the run captures.
///
/// # Why a flag maps to a checkpoint rather than to a route alias
///
/// The mapping is deliberately not "`-Cparse-only` means `--stop-after=parse`". On the
/// Wasm-derived routes the `parse` alias is `wasm.parsed`, while the flag has always stopped
/// *after* the WebAssembly had been translated to HIR — one checkpoint later. Naming
/// checkpoints keeps the flags meaning what they have always meant.
///
/// # And why the mapping is a preference list rather than a single checkpoint
///
/// Routes differ in which phases they have. Rather than matching on frontend identity — which
/// would put language knowledge back into the frontend-neutral core — each flag names the
/// checkpoints that could serve it, most preferred first, and `stop_checkpoint` takes the
/// first one the route declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopFlag {
    /// `-Cparse-only`: stop once the input has become the route's first intermediate form.
    ///
    /// For a route that produces HIR that is `hir.initial`, the point the legacy
    /// `ParseWasmStage` and `ParseHirStage` stopped at — after translation, not merely after
    /// the source had been read. A route that produces no HIR stops at `masm.parsed`.
    ParseOnly,
    /// `-Canalyze-only`: stop once the analysis phase has run.
    ///
    /// The owner's framing, and the reason this is not conditional on `-Zlint`: the flag names
    /// the analysis *step*, of which the advice-taint lint is today the only member. A run
    /// that asked to stop after analysis stops there whether or not any lint was enabled.
    AnalyzeOnly,
    /// `-Clink-only`: stop once the inputs have been linked, without generating Miden Assembly.
    ///
    /// Linking is what the wasm-to-HIR translation does, so this lands on `hir.initial` like
    /// `-Cparse-only`; the two flags differ in what they *suppress* downstream, not in where
    /// they stop. That is where `ParseWasmStage` stopped for it
    /// (`stages/parse/wasm.rs`), and it is the same answer for a `.hir` input, whose
    /// `hir.initial` *is* already-linked HIR.
    ///
    /// The `.hir` route is worth a note, because the obvious reading of the legacy chain says
    /// `masm.lowered`: `CodegenStage::run` raises `CompilerStopped("link-only=true")` *after*
    /// lowering. That check is unreachable. `CodegenStage::enabled` returns
    /// `Session::should_codegen()`, which is `false` whenever `link_only` is set, and
    /// `Chain::run` refuses to run a disabled stage — so the legacy `.hir` chain under
    /// `-Clink-only` never reached codegen at all and exited from `Chain::run` with the
    /// unhelpful "second stage of chain is disabled". Stopping at `hir.initial` is what the
    /// flag says and what the Wasm route already did.
    ///
    /// This is also the one flag a route may legitimately not have: a target that is already
    /// Miden Assembly is assembled, never linked into HIR, so there is no phase to stop after
    /// and the flag is inert rather than rejected.
    LinkOnly,
    /// The derived rewrite-only mode: the request asked for no output needing linking or
    /// codegen, so compilation stops after the rewrites.
    ///
    /// This is not a flag. It is [`Session::rewrite_only`](midenc_session::Session::rewrite_only),
    /// restated over `Options` by `rewrite_only`, and it is unreachable from the command
    /// line: every command line goes through
    /// [`Options::with_output_types`](midenc_session::Options::with_output_types), which
    /// inserts the implicit `masp`, and clap rejects `-Clink-only` together with `-Cno-link`.
    /// It is mapped because a caller building `Options` directly can still express it.
    RewriteOnly,
}

impl StopFlag {
    /// How this stop point is named in diagnostics.
    pub const fn flag(self) -> &'static str {
        match self {
            Self::ParseOnly => "-Cparse-only",
            Self::AnalyzeOnly => "-Canalyze-only",
            Self::LinkOnly => "-Clink-only",
            Self::RewriteOnly => "rewrite-only mode",
        }
    }

    /// The reason a [`CompilerStopped`](crate::CompilerStopped) raised for this stop carries.
    ///
    /// These are the strings the legacy stages used, so a run stopped by a flag reports what
    /// it has always reported.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ParseOnly => "parse-only",
            Self::AnalyzeOnly => "analyze-only",
            Self::LinkOnly => "link-only",
            Self::RewriteOnly => "rewrite-only",
        }
    }

    /// The checkpoints that can serve this stop, most preferred first.
    const fn checkpoints(self) -> &'static [CheckpointId] {
        match self {
            Self::ParseOnly => &[CheckpointId::HIR_INITIAL, CheckpointId::MASM_PARSED],
            Self::AnalyzeOnly => &[CheckpointId::HIR_ANALYZED],
            Self::LinkOnly => &[CheckpointId::HIR_INITIAL],
            Self::RewriteOnly => &[CheckpointId::HIR_TRANSFORMED],
        }
    }

    /// Whether a route serving none of [`Self::checkpoints`] is a usage error.
    ///
    /// True for every stop that names a phase each compilation route performs somewhere, so
    /// that a route unable to express it is reported rather than silently ignored — which is
    /// what a manifest-backed Rust target did with `-Canalyze-only` before these flags became
    /// goals.
    ///
    /// False for [`Self::LinkOnly`] alone, whose phase a Miden Assembly route genuinely does
    /// not have; see that variant.
    const fn is_required(self) -> bool {
        !matches!(self, Self::LinkOnly)
    }
}

/// The derived rewrite-only mode: neither linking nor codegen was asked for.
///
/// This restates [`Session::rewrite_only`](midenc_session::Session::rewrite_only) over
/// `Options` alone, because goal resolution never sees a session.
/// `the_derived_rewrite_only_mode_agrees_with_the_session_predicate` runs both over the shapes
/// that make the predicate interesting, so the two cannot drift in silence.
fn rewrite_only(options: &Options) -> bool {
    let should_link = options.output_types.should_link() && !options.no_link;
    let should_codegen = options.output_types.should_codegen() && !options.link_only;
    !options.parse_only && !options.analyze_only && !(should_link || should_codegen)
}

/// Which stop `options` asks for, if any.
///
/// A request naming two of the `-C` flags is a usage error reporting both, rather than a
/// silent precedence rule. It is not reachable from the command line — `CodegenOptions`
/// declares each flag `conflicts_with_all` the others (`compiler.rs`) — but an `Options` built
/// programmatically can carry two.
///
/// [`StopFlag::RewriteOnly`] is consulted only when none of the three flags is set, which
/// keeps it from competing with them. For `-Cparse-only` and `-Canalyze-only` that is not a new
/// rule at all — `rewrite_only()` excludes both by construction.
///
/// For `-Clink-only` it *is* a choice, and it is only half-precedented: the legacy wasm chain
/// stopped on `link_only` in `ParseWasmStage`, ahead of `ApplyRewritesStage`'s rewrite-only
/// exit, but the `.hir` chain had no `link_only` check in `ParseHirStage`, so there the
/// rewrite-only exit would have come first. Preferring the named flag over the derived mode is
/// the choice made here, and nothing observes it: the pair is unreachable from the command
/// line, because `Options::with_output_types` inserts the implicit `masp` and therefore makes
/// `should_link()` true, which makes `rewrite_only()` false whenever `-Clink-only` is given.
pub fn stop_flag(options: &Options) -> CompilerResult<Option<StopFlag>> {
    let named = [
        (options.parse_only, StopFlag::ParseOnly),
        (options.analyze_only, StopFlag::AnalyzeOnly),
        (options.link_only, StopFlag::LinkOnly),
    ]
    .into_iter()
    .filter_map(|(is_set, flag)| is_set.then_some(flag))
    .collect::<Vec<_>>();

    match named.as_slice() {
        [] => Ok(rewrite_only(options).then_some(StopFlag::RewriteOnly)),
        [flag] => Ok(Some(*flag)),
        several => Err(Report::msg(format!(
            "{} name different stop points; give at most one",
            several
                .iter()
                .map(|flag| format!("'{}'", flag.flag()))
                .collect::<Vec<_>>()
                .join(" and ")
        ))),
    }
}

/// Fold the stop `options` asks for into `request`, as a `--stop-after` value.
///
/// The result goes through [`resolve_goal`] like any other request, so the flags and
/// `--stop-after` share one resolution path and one set of diagnostics.
///
/// # Three ways to name a stop point, reconciled in one place
///
/// [`Options::stop_after`] is `--stop-after` as the user typed it, and it is folded in here
/// rather than by whoever built the request, so that *every* entry point honours it — the
/// driver's, `tests/support`'s, and any other caller that assembles a request of its own. A
/// caller may also set [`OutputRequest::with_stop_after`] directly, which is the programmatic
/// form and needs no command line.
///
/// Naming a stop point twice is refused rather than resolved by precedence, whichever pair it
/// is and even when the two agree: a request that says `parse` twice is a request whose author
/// believed one of them was doing something, and picking a winner hides that.
pub fn apply_stop_flags(
    request: OutputRequest,
    options: &Options,
    frontend: &FrontendRegistration,
) -> CompilerResult<OutputRequest> {
    let request = match (options.stop_after.as_deref(), request.stop_after()) {
        (Some(requested), None) => request.with_stop_after(Some(requested.to_string())),
        (Some(from_options), Some(on_request)) => {
            return Err(Report::msg(format!(
                "'--stop-after={from_options}' and the requested stop point '{on_request}' name \
                 different stop points; give at most one"
            )));
        }
        (None, _) => request,
    };

    let Some(flag) = stop_flag(options)? else {
        return Ok(request);
    };
    if let Some(stop_after) = request.stop_after() {
        return Err(Report::msg(format!(
            "'--stop-after={stop_after}' and '{}' name different stop points; give at most one",
            flag.flag()
        )));
    }
    match stop_checkpoint(flag, frontend)? {
        Some(checkpoint) => Ok(request.with_stop_after(Some(checkpoint.as_str().to_string()))),
        None => Ok(request),
    }
}

/// The checkpoint `flag` stops at on `frontend`'s route.
///
/// `None` means the flag names a phase this route does not have and is therefore inert; see
/// [`StopFlag::is_required`]. Otherwise a route that cannot serve the flag is reported as a
/// limitation of the compiler, because that is what it is: the phase happens, but not
/// anywhere this request can observe.
///
/// No shipped route is in that position today. A manifest-backed Rust target was the live case —
/// it compiled by recursing with a fresh `Session` and `Context`, whose checkpoints this
/// request's observers never saw, so its route was `package.assembled` alone — and it no longer
/// is: its root target now runs the shared WebAssembly tail in this process and publishes every
/// checkpoint on that route. The branch stays because it is the honest answer for any future
/// route that compiles somewhere this request cannot see, and because leaving such a flag
/// *ignored* is what it replaced.
fn stop_checkpoint(
    flag: StopFlag,
    frontend: &FrontendRegistration,
) -> CompilerResult<Option<CheckpointId>> {
    if let Some(checkpoint) = flag
        .checkpoints()
        .iter()
        .copied()
        .find(|checkpoint| frontend.supports(*checkpoint))
    {
        return Ok(Some(checkpoint));
    }
    if !flag.is_required() {
        return Ok(None);
    }

    let wanted = flag
        .checkpoints()
        .iter()
        .map(|checkpoint| checkpoint.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let route = frontend
        .route()
        .iter()
        .map(|checkpoint| checkpoint.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(Report::msg(format!(
        "'{}' is not supported for '{}' targets yet: it stops compilation at whichever of \
         [{wanted}] the route reaches, and this one reaches none of them — its checkpoints are \
         [{route}]. This is a known limitation of the compiler rather than a problem with your \
         project.",
        flag.flag(),
        frontend.id(),
    )))
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

    use midenc_session::{Options, OutputType};

    use super::*;
    use crate::pipeline::frontends::{
        HIR_FRONTEND, MASM_FRONTEND, RUST_FRONTEND, RUST_STANDALONE_FRONTEND, WASM_FRONTEND,
    };
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

    // -------------------------------------------------------------------------------------
    // The `-C` stop flags.
    //
    // These run against the *shipped* registrations rather than the fixtures above, because
    // what a flag means is a claim about the routes users actually compile on. A fixture
    // route could be given whatever checkpoints made the mapping look right.
    // -------------------------------------------------------------------------------------

    /// The options a command line carrying the flags `configure` sets arrives with.
    ///
    /// `with_output_types` is not decoration: every command line goes through it (see
    /// `compiler.rs`), and it inserts the implicit `masp`. Without it `output_types` is empty,
    /// which makes the derived [`StopFlag::RewriteOnly`] true and every case below ambiguous.
    fn options_with(configure: impl FnOnce(&mut Options)) -> Options {
        let mut options = alloc::boxed::Box::new(Options::default());
        configure(&mut options);
        *options.with_output_types(Default::default(), None)
    }

    /// The checkpoint `options` stop compilation at on `frontend`'s route, if any.
    fn resolved_stop(options: &Options, frontend: &FrontendRegistration) -> Option<CheckpointId> {
        let request =
            apply_stop_flags(OutputRequest::default(), options, frontend).unwrap_or_else(|err| {
                panic!("the flags must map on the '{}' route: {err}", frontend.id())
            });
        request
            .stop_after()
            .map(|value| resolve_goal(&request, frontend).expect(value).checkpoint())
    }

    #[test]
    fn each_stop_flag_maps_onto_the_checkpoint_its_route_reaches() {
        // The whole table, per route. `-Cparse-only` and `-Clink-only` name checkpoints that
        // are *not* the route's `parse` alias on the Wasm-derived routes: the legacy stages
        // stopped after the wasm -> HIR translation, not after the wasm was parsed.
        let cases: &[(&str, FrontendRegistration, Option<CheckpointId>, Option<CheckpointId>)] = &[
            (
                "wasm",
                WASM_FRONTEND,
                Some(CheckpointId::HIR_INITIAL),
                Some(CheckpointId::HIR_INITIAL),
            ),
            (
                "rust-standalone",
                RUST_STANDALONE_FRONTEND,
                Some(CheckpointId::HIR_INITIAL),
                Some(CheckpointId::HIR_INITIAL),
            ),
            (
                "hir",
                HIR_FRONTEND,
                Some(CheckpointId::HIR_INITIAL),
                Some(CheckpointId::HIR_INITIAL),
            ),
            // The MASM route has no HIR of its own to stop at, and no link phase at all.
            ("masm", MASM_FRONTEND, Some(CheckpointId::MASM_PARSED), None),
        ];

        for (name, frontend, parse_stop, link_stop) in cases {
            assert_eq!(
                resolved_stop(&options_with(|options| options.parse_only = true), frontend),
                *parse_stop,
                "-Cparse-only on the '{name}' route"
            );
            assert_eq!(
                resolved_stop(&options_with(|options| options.analyze_only = true), frontend),
                Some(CheckpointId::HIR_ANALYZED),
                "-Canalyze-only on the '{name}' route"
            );
            assert_eq!(
                resolved_stop(&options_with(|options| options.link_only = true), frontend),
                *link_stop,
                "-Clink-only on the '{name}' route"
            );
        }
    }

    #[test]
    fn the_derived_rewrite_only_mode_stops_after_the_rewrites() {
        // `rewrite_only()` is not a flag: it holds when the request asked for no output that
        // needs linking or codegen, which is why the fixture skips `with_output_types`.
        let options = Options::default();
        assert_eq!(
            stop_flag(&options).expect("no two flags are set"),
            Some(StopFlag::RewriteOnly),
            "an empty output-type set is what makes the derived mode hold"
        );
        for frontend in [WASM_FRONTEND, RUST_STANDALONE_FRONTEND, HIR_FRONTEND] {
            assert_eq!(
                resolved_stop(&options, &frontend),
                Some(CheckpointId::HIR_TRANSFORMED),
                "the rewrite-only mode stops after the rewrites on the '{}' route",
                frontend.id()
            );
        }

        // And it is rejected on the MASM route, which never reaches `hir.transformed` — the
        // same reason `--stop-after=transform` is rejected there.
        let err = apply_stop_flags(OutputRequest::default(), &options, &MASM_FRONTEND)
            .expect_err("the masm route has no rewrite phase");
        assert!(format!("{err}").contains("hir.transformed"), "{err}");
    }

    /// Every `-C` stop flag lands on a manifest-backed Rust target exactly where it lands on a
    /// standalone one.
    ///
    /// This is the user-visible half of propagating the goal into the root Rust build. Until
    /// that landed, `RUST_FRONTEND`'s route was `package.assembled` alone — its target was
    /// compiled by recursing with a fresh `Session`/`Context` whose checkpoints this request's
    /// observers never saw — and each of these flags was rejected as a known limitation of the
    /// compiler. The root target now runs the shared WebAssembly tail in this process, so there
    /// is nothing left to be limited about.
    ///
    /// Asserted against `RUST_STANDALONE_FRONTEND`'s answers rather than against literals: the
    /// claim is that the two Rust entry points stop in the same places, and a literal would
    /// still pass if both drifted together.
    #[test]
    fn every_stop_flag_lands_on_a_manifest_backed_rust_target_where_it_lands_on_a_standalone_one() {
        /// One `-C` stop flag: how it is spelled, and how it is set.
        type Flag = (&'static str, fn(&mut Options));

        let flags: [Flag; 3] = [
            ("-Cparse-only", |options| options.parse_only = true),
            ("-Canalyze-only", |options| options.analyze_only = true),
            ("-Clink-only", |options| options.link_only = true),
        ];

        for (name, set) in flags {
            let options = options_with(set);
            let standalone = resolved_stop(&options, &RUST_STANDALONE_FRONTEND);
            assert!(
                standalone.is_some(),
                "{name} must resolve on the standalone route, or this proves nothing"
            );
            assert_eq!(
                resolved_stop(&options, &RUST_FRONTEND),
                standalone,
                "{name} must stop in the same place on both Rust routes"
            );
        }

        // And the derived rewrite-only mode, which is not a flag and is reached by a different
        // branch of `stop_flag`.
        let options = Options::default();
        assert_eq!(
            resolved_stop(&options, &RUST_FRONTEND),
            Some(CheckpointId::HIR_TRANSFORMED),
            "the rewrite-only mode stops after the rewrites on the rust project route too"
        );
    }

    #[test]
    fn analyze_only_stops_at_the_analysis_checkpoint_without_the_lint_flag() {
        // The discriminating pair for the question `-Zlint` raises: `-Canalyze-only` names a
        // stop point, so it must resolve identically whether or not any lint was enabled.
        // Nothing else pins this — most lit fixtures use the flag *without* `-Zlint`.
        for frontend in [WASM_FRONTEND, RUST_STANDALONE_FRONTEND, HIR_FRONTEND, MASM_FRONTEND] {
            for lint in [false, true] {
                let options = options_with(|options| {
                    options.analyze_only = true;
                    options.lint = lint;
                });
                assert_eq!(
                    resolved_stop(&options, &frontend),
                    Some(CheckpointId::HIR_ANALYZED),
                    "-Canalyze-only with lint={lint} on the '{}' route",
                    frontend.id()
                );
            }
        }
    }

    #[test]
    fn two_stop_flags_are_a_usage_error_naming_both() {
        // Not reachable from the command line: `CodegenOptions` declares each of the three
        // with `conflicts_with_all` over the other two (`compiler.rs`), so clap rejects the
        // pair first. Only an `Options` built programmatically can carry two, and a silent
        // precedence rule is the wrong answer for it.
        let options = options_with(|options| {
            options.parse_only = true;
            options.link_only = true;
        });
        let err = stop_flag(&options).expect_err("two flags name two stop points");
        let rendered = format!("{err}");
        assert!(rendered.contains("-Cparse-only"), "must name the first flag: {rendered}");
        assert!(rendered.contains("-Clink-only"), "must name the second flag: {rendered}");
    }

    #[test]
    fn a_stop_flag_and_an_explicit_stop_after_are_a_usage_error() {
        let options = options_with(|options| options.analyze_only = true);
        let request = OutputRequest::default().with_stop_after(Some("parse".to_string()));
        let err = apply_stop_flags(request, &options, &WASM_FRONTEND)
            .expect_err("two stop points were named");
        let rendered = format!("{err}");
        assert!(rendered.contains("--stop-after=parse"), "{rendered}");
        assert!(rendered.contains("-Canalyze-only"), "{rendered}");
    }

    #[test]
    fn a_request_carrying_no_stop_flag_is_left_alone() {
        // The half without which every assertion above could be satisfied by a mapping that
        // stopped every build: the usual command line names no stop point at all.
        let options = options_with(|_| {});
        assert_eq!(stop_flag(&options).expect("no flags are set"), None);
        for frontend in [
            WASM_FRONTEND,
            RUST_STANDALONE_FRONTEND,
            HIR_FRONTEND,
            MASM_FRONTEND,
            RUST_FRONTEND,
        ] {
            assert_eq!(
                resolved_stop(&options, &frontend),
                None,
                "on the '{}' route",
                frontend.id()
            );
            let goal = resolve_goal(
                &apply_stop_flags(OutputRequest::default(), &options, &frontend)
                    .expect("no flag, nothing to map"),
                &frontend,
            )
            .expect("an unflagged request resolves to the terminal checkpoint");
            assert_eq!(goal.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
        }
    }

    #[test]
    fn the_derived_rewrite_only_mode_agrees_with_the_session_predicate() {
        // `rewrite_only` restates `Session::rewrite_only` against `Options` alone, because
        // goal resolution never sees a session. The two must not drift, so both are run over
        // the shapes that make the predicate interesting.
        use alloc::{boxed::Box, sync::Arc};

        use midenc_session::{
            InputFile, Session, Verbosity,
            diagnostics::{DefaultSourceManager, SourceManager},
        };

        /// A named options shape: what to call it, and how to build it.
        type Shape = (&'static str, fn(&mut Options));

        let shapes: &[Shape] = &[
            ("the usual command line", |_| {}),
            ("no requested outputs", |options| options.output_types = Default::default()),
            ("-Cparse-only", |options| options.parse_only = true),
            ("-Canalyze-only", |options| options.analyze_only = true),
            ("-Clink-only with no outputs", |options| {
                options.link_only = true;
                options.output_types = Default::default();
            }),
            ("-Cno-link with no outputs", |options| {
                options.no_link = true;
                options.output_types = Default::default();
            }),
        ];

        for (name, configure) in shapes {
            let mut options =
                Box::new(Options::default()).with_output_types(Default::default(), None);
            options.diagnostics.verbosity = Verbosity::Silent;
            configure(&mut options);
            let expected = rewrite_only(&options);

            let source_manager: Arc<dyn SourceManager + Send + Sync> =
                Arc::new(DefaultSourceManager::default());
            let session = Session::new(InputFile::empty(), options, None, source_manager)
                .expect("should build a session");
            assert_eq!(
                session.rewrite_only(),
                expected,
                "the Options-only predicate must agree with the session's for {name}"
            );
        }
    }
}
