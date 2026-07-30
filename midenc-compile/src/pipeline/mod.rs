//! The frontend-neutral compilation pipeline.
//!
//! # Model
//!
//! A [`CheckpointId`] names a point at which an artifact exists. Frontends run
//! imperatively and call [`TargetContext::checkpoint`], which notifies observers and
//! returns [`Flow::Continue`] with the artifact, or [`Flow::Break`] once the requested goal
//! is reached. Checkpoints carry no global ordering: each [`FrontendRegistration`]
//! declares an ordered route, and every comparison happens within one route.
//!
//! [`resolve_goal`] decides how far compilation runs: the `--stop-after` checkpoint when
//! one is given, otherwise the route's terminal checkpoint. The `-C` stop flags are one more
//! way of naming that checkpoint — [`apply_stop_flags`] maps each onto the checkpoint its
//! route reaches, so they and `--stop-after` share one resolution path and one set of
//! diagnostics; see [`StopFlag`]. `--emit` requests *additional* artifacts and never shortens
//! the build; it is only validated against the resulting [`Goal`]. [`backend::hir_to_masm`] is
//! the shared HIR backend, called by frontends that produce HIR.
//!
//! # Status
//!
//! Increment 4 of the pipeline redesign. **Every input is compiled here** — there is no second
//! compilation path. [`prepare`](Pipeline::compile) asks one question of the input: a `.toml`
//! names a project to load, and everything else is a source file a project is synthesized
//! around. Both produce a [`PreparedProject`], and everything past that point — frontend
//! dispatch by the selected target's root extension, goal resolution, provider construction,
//! assembly — is common to the two.
//!
//! All four shipped frontends are registered for every build, so a Miden Assembly dependency of
//! a Rust project is compiled by [`frontends::masm::MasmProjectFrontend`] while the root is
//! compiled by [`frontends::rust::RustProjectFrontend`], each with the role [`RootTarget`]
//! derives for it. A standalone `.rs` root is the one target whose frontend is *not* the
//! registry's: it is compiled in this process rather than by `cargo`, and
//! [`frontends::rust::RUST_STANDALONE_FRONTEND`] is substituted for the root of that request
//! alone.
//!
//! [`Pipeline::compile`] attaches an observer of its own that renders the *selected* target's
//! artifacts as it reaches them, through the route's own [`ArtifactDecl::render`] — so `--emit`
//! is decided by the session rather than by the request. The crate's entry points still do not
//! forward `--emit` or `--stop-after` into the [`OutputRequest`] they build, so goal-based
//! validation of an explicit `--emit` is not yet in force; that belongs with the CLI, which
//! still holds the raw specs `Options` folds away. The `-C` stop flags *are* in force, because
//! [`Pipeline::compile`] reads them off the session rather than off the request.
//!
//! The one thing that does **not** come out of a pipeline run is HIR handed back to a caller:
//! each target's HIR is built in a [`Context`](midenc_hir::Context) the provider creates per
//! assembler callback, so it does not outlive the run. A caller that wants HIR *renders* it from
//! an [`Observer`], inside the callback, while that context is still alive — which is what
//! `tests/support` does for `--emit=hir`-shaped assertions. Handing back a live component instead
//! would mean retaining the root target's context, which is approved but has no consumer yet.
//!
//! See `tasks/specs/2026-07-25-midenc-compile-pipeline-design.md`.

mod artifact;
pub(crate) mod artifacts;
pub(crate) mod assembly;
pub mod backend;
mod checkpoint;
mod driver;
mod flow;
mod frontend;
pub mod frontends;
mod goal;
mod observer;
mod outcome;
mod prepare;
mod provider;
mod registry;
mod request;
mod seed;
#[cfg(test)]
pub mod testing;

/// The failure reported when an enabled lint raised error diagnostics.
///
/// Two places reach this conclusion — [`backend::analyze`] for targets compiled from HIR, and
/// [`frontends::masm::MasmProjectFrontend`] for targets that are already Miden Assembly — and
/// they must stay in step, so the wording lives here rather than in either of them.
///
/// It is a summary line over diagnostics the user has already been shown with spans and
/// labels, in the shape `rustc` uses. It is deliberately an ordinary
/// [`Report`](midenc_session::diagnostics::Report) rather than a
/// [`CompilerStopped`](crate::CompilerStopped): the latter means "the user asked to stop
/// early", which `midenc-driver` turns into a *successful* exit.
///
/// # Why the flag is a parameter and not part of the sentence
///
/// The line used to name `-Zlint` unconditionally. That is right for [`backend::analyze`],
/// which runs its lints only under that flag — but not for
/// [`frontends::masm::MasmProjectFrontend`], whose analysis also runs under `-Canalyze-only`
/// alone, because on that route the analysis *is* the lint. Naming `-Zlint` there would point
/// the user at a flag they did not pass, on a run that failed for something they did.
///
/// `-Zlint` wins when both are set: it is the flag that asks for the lints, and it is the one
/// to drop to get past this.
pub(crate) fn lint_errors_reported(options: &midenc_session::Options) -> alloc::string::String {
    let asked_by = if options.lint {
        "-Zlint"
    } else {
        "-Canalyze-only"
    };
    alloc::format!(
        "aborting due to errors reported by lint analysis ({asked_by}); see the diagnostics above"
    )
}

// Crate-internal, because their only callers are inside `pipeline`: the seed's input check is
// [`Pipeline::compile`]'s alone, and the target-root extension is the shared derivation
// `seed.rs` and preparation must not each own a copy of.
pub(crate) use self::prepare::{require_input_path_for_seed, selected_provider_extension};
pub use self::{
    artifact::{Artifact, ArtifactId},
    checkpoint::CheckpointId,
    driver::{CompilationRequest, Pipeline},
    flow::{Flow, Stopped},
    frontend::{CaptureSlot, Frontend, TargetContext, TargetKey},
    goal::{
        Goal, OutputRequest, StopFlag, apply_stop_flags, artifact_id_for_output, resolve_goal,
        stop_flag,
    },
    observer::{Observer, RecordingObserver, TargetRole},
    outcome::Outcome,
    prepare::{PreparedProject, prepare_project, prepare_standalone},
    provider::{FrontendProvider, RootTarget},
    registry::{ArtifactDecl, FrontendId, FrontendRegistration, FrontendRegistry},
    request::{PreAssemblyHook, RequestState},
    seed::Start,
};
