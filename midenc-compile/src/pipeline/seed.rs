//! Continuation: resuming a compilation from an artifact the caller already holds.
//!
//! A caller that has already produced part of a build — HIR it translated itself, or HIR it
//! rewrote — can ask for the rest of it rather than starting over. That is [`Start::At`], and
//! everything it needs is one provider installed for this request alone.
//!
//! # Why a provider, and not a branch in the driver
//!
//! Which frontend compiles a target is decided *inside* the assembler, from the target root's
//! extension, one target at a time. A seed is therefore not a phase the driver can skip: it is
//! a different answer to the question "who compiles the root target?", and the only place that
//! question is asked is the provider registry. So a seeded request installs a provider for the
//! selected target's extension through the request-scoped override
//! [`Pipeline::providers`](super::Pipeline::providers) already takes — the same mechanism the
//! standalone Rust entry point uses, appended last so it wins for that extension. There is no
//! second mechanism.
//!
//! # The seed replaces one target's front half, not the build
//!
//! Everything downstream of the seeded checkpoint is the same code an unseeded run takes, and
//! *which* code that is falls out of the checkpoint: a `hir.initial` seed still runs analysis,
//! the rewrites and codegen, while a `hir.transformed` seed has had the first two done to it and
//! runs codegen alone. Either way the phases are the shared backend's, they publish the same
//! checkpoints, and they hand assembly the same [`LoweredTarget`] — so a seeded run can itself
//! stop early, emit the same documents, and post-process its package the same way. None of that
//! is a special case here, and all of it is worth a test rather than an assumption.
//!
//! Everything *sideways* of it is unchanged too. The override displaces the registry's provider
//! for one extension, and that provider serves every target with that extension — not only the
//! root. [`SeedFrontend`] therefore wraps the frontend the registry would have used and
//! delegates every callback for a target the seed is not for, so a dependency that happens to
//! share the root's extension is compiled exactly as it would have been.

use alloc::{boxed::Box, collections::BTreeMap, format, rc::Rc};
use core::cell::RefCell;

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs, ProjectSourceProvider};
use miden_mast_package::Package as MastPackage;
use midenc_hir::{Context, Op};
use midenc_session::{Session, diagnostics::Report};

use super::{
    Artifact, CheckpointId, Flow, Frontend, FrontendProvider, FrontendRegistration,
    PreparedProject, RequestState, RootTarget, TargetContext, TargetKey,
    backend::{self, LoweredTarget},
};
use crate::{CodegenOutput, CompilerResult, MidenComponent};

/// Where a compilation request begins.
///
/// # Which `Artifact` this is
///
/// [`Start::At`] carries [`pipeline::Artifact`](super::Artifact): the type-erased envelope a
/// checkpoint publishes, which holds any value at all. It is **not**
/// [`CompiledArtifact`](crate::CompiledArtifact), the two-variant `Lowered | Assembled` enum a
/// whole compilation returns — a `hir.initial` seed carries a [`MidenComponent`], which that
/// enum cannot hold.
#[derive(Debug)]
pub enum Start {
    /// Compile from the request's input, as an ordinary build does.
    Input,
    /// Resume from an artifact already in hand, as of `checkpoint`.
    ///
    /// The checkpoint is *not* republished: it was reached by whatever run produced the
    /// artifact, and a seeded run continues from the checkpoint after it.
    ///
    /// # Which checkpoint you seed at decides what still runs
    ///
    /// Both seedable checkpoints carry a [`MidenComponent`], and nothing about the component
    /// itself says how far it has been taken — so the checkpoint is the only thing that can say:
    ///
    /// - **`hir.initial`** — HIR as parsed. Analysis, the rewrites and codegen all still run.
    /// - **`hir.transformed`** — HIR already analyzed and rewritten. Codegen alone still runs.
    ///
    /// Choosing the wrong one is not a performance question. Seeding already-rewritten HIR at
    /// `hir.initial` runs the greedy rewrite pipeline over it a second time, and that pipeline is
    /// not idempotent: doing so produced `NoSolution` operand-scheduling failures in five 128-bit
    /// arithmetic fixtures and two differential proptests. Seeding never-rewritten HIR at
    /// `hir.transformed` is the opposite mistake, and lowers IR the rewrites were meant to
    /// legalize.
    ///
    /// # The seed's HIR context must be live when the request is made
    ///
    /// A seed carries a [`MidenComponent`], and HIR operations hold only a
    /// **raw pointer** to the [`Context`] they were allocated in — see `BlockRef::context_rc`,
    /// whose safety argument is that the context outlives the allocation. `RawEntityRef` offers
    /// no protection here by design, so nothing in the type system says this.
    ///
    /// What the pipeline can do, and does, is stop the window from being *open*: the seed
    /// provider takes an owning `Rc<Context>` off the component the moment it is built, exactly
    /// as [`backend::hir_to_masm`] and the `compile_link_output_to_masm*` entry points do. So the
    /// requirement on a caller is only that the context be alive **at the moment
    /// [`Pipeline::compile`](super::Pipeline::compile) is called** — not for the duration of the
    /// request — and dropping the caller's own handle immediately afterwards is safe.
    ///
    /// The shape that gets this wrong is worth naming, because it is the natural one: a helper
    /// that *consumes* the `Rc<Context>` it is passed and hands back a `MidenComponent` leaves
    /// nothing holding the arena, so
    ///
    /// ```ignore
    /// // Wrong: the only `Rc` died at the end of the argument list.
    /// let seed = Start::At {
    ///     checkpoint: CheckpointId::HIR_TRANSFORMED,
    ///     artifact: Artifact::new(ArtifactId::HIR, build_hir(context)?),
    /// };
    /// ```
    ///
    /// dangles before the request is ever made. Pass a *clone* of the context and keep the
    /// original until the seed is handed over.
    At {
        /// The checkpoint `artifact` was produced at.
        checkpoint: CheckpointId,
        /// The artifact to resume from.
        artifact: Artifact,
    },
}

/// Build the provider that resumes this request from `artifact`, produced at `checkpoint`.
///
/// Everything that can be checked before any work is done is checked here, so that a seed the
/// route cannot use is a diagnostic rather than a failure deep inside assembly:
///
/// - the checkpoint must be one the selected frontend's route reaches, or the seed names a
///   point in some other route;
/// - it must come *before* this request's goal, or the request asks for nothing and the driver
///   would report the internal error it raises for a build that ran past its goal;
/// - it must be one this module knows how to resume from, and the artifact must hold what that
///   checkpoint produces.
///
/// The extension the provider is installed under is the `&'static str` the selected
/// registration declares, matched against the target root's own extension — never the root's
/// runtime `Cow<str>`, which is not what
/// [`SourceProviderRegistry`](miden_assembly::SourceProviderRegistry) is keyed by.
pub(crate) fn provider(
    checkpoint: CheckpointId,
    artifact: Artifact,
    prepared: &PreparedProject,
    session: &Rc<Session>,
    state: &Rc<RequestState>,
) -> CompilerResult<Box<dyn ProjectSourceProvider>> {
    check_route(checkpoint, &prepared.frontend, state.goal().checkpoint())?;
    let hir = resume_from(checkpoint, artifact)?;
    let extension = seed_extension(prepared)?;
    Ok(Box::new(FrontendProvider::new(
        extension,
        Rc::new(SeedFrontend::new(
            checkpoint,
            hir,
            prepared.frontend.instantiate(session.clone()),
        )) as Rc<dyn Frontend>,
        session.clone(),
        state.clone(),
        RootTarget::new(prepared.package.clone(), &prepared.target),
    )))
}

/// Reject a seed the selected route does not reach, or that this request has no room for.
///
/// Both comparisons are made against the *selected registration's* route, because that is the
/// only place checkpoints are ordered at all — see
/// [`CheckpointId`](super::CheckpointId)'s note on why its `Ord` is not route order.
///
/// A goal at or before the seed is a caller error rather than a degenerate build: nothing
/// downstream of the seed would run, so nothing would be captured, and the request would end in
/// the driver's "ran to completion for a request that asked to stop" internal error — a
/// compiler-bug report for a mistake the caller made and can fix.
fn check_route(
    checkpoint: CheckpointId,
    frontend: &FrontendRegistration,
    goal: CheckpointId,
) -> CompilerResult<()> {
    let Some(seeded_at) = frontend.position(checkpoint) else {
        return Err(Report::msg(format!(
            "cannot resume compilation at '{checkpoint}': frontend '{}' does not reach it; its \
             route is [{}]",
            frontend.id(),
            route_of(frontend),
        )));
    };
    // The goal was resolved against this very route, so a missing position is an internal
    // inconsistency rather than anything a caller can produce.
    let goal_at = frontend.position(goal).ok_or_else(|| {
        Report::msg(format!(
            "internal error: this request stops at '{goal}', which is not on frontend '{}'s route",
            frontend.id(),
        ))
    })?;
    if seeded_at >= goal_at {
        return Err(Report::msg(format!(
            "cannot resume compilation at '{checkpoint}': this request stops at '{goal}', which \
             frontend '{}' reaches no later than the seed, so there is nothing left to compile",
            frontend.id(),
        )));
    }
    Ok(())
}

/// `frontend`'s route, in order, for use in diagnostics.
fn route_of(frontend: &FrontendRegistration) -> alloc::string::String {
    frontend
        .route()
        .iter()
        .map(CheckpointId::as_str)
        .collect::<alloc::vec::Vec<_>>()
        .join(", ")
}

/// Recover the HIR a seed at `checkpoint` must carry.
///
/// # Two checkpoints seed a compilation, and they differ in what still has to run
///
/// Both carry a [`MidenComponent`]; what distinguishes them is how much of the backend the
/// resumed run performs, which [`SeedFrontend::compile`] reads off the checkpoint:
///
/// - **`hir.initial`** — HIR as parsed. Analysis, the rewrites and codegen all still have to
///   run, so the seeded target goes through [`backend::hir_to_masm`] unchanged.
/// - **`hir.transformed`** — HIR that has already been analyzed and rewritten. Only codegen
///   remains, so it goes through [`backend::masm_from_transformed_hir`]. Running the rewrites
///   again here is not a wasted pass but a miscompile; see [`Start::At`].
///
/// # `masm.lowered` is the one worth explaining
///
/// The plan left open what a `masm.lowered` seed would carry, and the answer is that it cannot
/// carry what the checkpoint *publishes*. `masm.lowered` publishes [`ProjectSourceInputs`],
/// while post-processing needs the [`LoweredTarget`] the frontend kept beside them — above all
/// the lowered component, whose rodata becomes the package's advice map. A seed carrying the
/// sources alone would assemble a package whose code pushes commitments the advice provider has
/// never heard of: a silent miscompile, not a missing section, and precisely what
/// [`LoweredTarget`] exists to prevent.
///
/// So such a seed would have to carry a `LoweredTarget`. No caller holds one — every caller of
/// the continuation seeds *before* codegen — so the arm is refused rather than written blind,
/// and the refusal carries the answer to whoever tries it next.
fn resume_from(checkpoint: CheckpointId, artifact: Artifact) -> CompilerResult<MidenComponent> {
    if !matches!(checkpoint, CheckpointId::HIR_INITIAL | CheckpointId::HIR_TRANSFORMED) {
        return Err(Report::msg(format!(
            "cannot resume compilation at '{checkpoint}': the checkpoints a request can be seeded \
             at are '{}' and '{}'{}",
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_TRANSFORMED,
            if checkpoint == CheckpointId::MASM_LOWERED {
                MASM_LOWERED_SEED
            } else {
                ""
            },
        )));
    }
    artifact.downcast::<MidenComponent>().map_err(|artifact| {
        Report::msg(format!(
            "cannot resume compilation at '{checkpoint}': the seed is tagged '{}' but does not \
             hold the HIR component that checkpoint produces",
            artifact.id(),
        ))
    })
}

/// Why a `masm.lowered` seed is refused, appended to the rejection that names it.
///
/// The reasoning lives in [`resume_from`]'s documentation; this is the half of it the person
/// who just tried it needs, where they will see it.
const MASM_LOWERED_SEED: &str =
    ". A 'masm.lowered' seed would have to carry a `LoweredTarget`, not the `ProjectSourceInputs` \
     that checkpoint publishes: post-processing needs the lowered component to build the \
     package's rodata advice map, and a seed that dropped it would assemble a package whose code \
     asks the advice provider for data that was never attached. No caller holds a \
     `LoweredTarget`, so the arm is left unwritten rather than written blind";

/// The extension the seed provider must be installed under.
///
/// [`SourceProviderRegistry`](miden_assembly::SourceProviderRegistry) is keyed by
/// [`ProjectSourceProvider::file_type`], a `&'static str`, and the override wins by being
/// written last under the very key the registry's provider used. So the extension is taken from
/// the selected registration's own
/// [`extensions`](FrontendRegistration::extensions) — matched against the target root's, since
/// a registration may claim several — and never from the resolved root itself, which is a
/// runtime `Cow<str>` that could not be a key at all.
///
/// The root's own extension is read through
/// [`target_root_extension`](super::target_root_extension), the same derivation preparation
/// dispatched on. That is deliberate: a second copy of those six lines here would be the kind of
/// duplicate that only diverges once — and the divergence would present as this function's
/// internal error on a project nothing is wrong with.
fn seed_extension(prepared: &PreparedProject) -> CompilerResult<&'static str> {
    super::selected_provider_extension(prepared)
}

/// Serves the seeded target from the artifact in hand, and every other target from the
/// frontend the registry would have used.
///
/// # The seed is taken, not borrowed
///
/// A [`Frontend`] compiles through `&self`, and the seed is an owned artifact that
/// [`backend::hir_to_masm`] consumes — so it lives in a cell and the first root callback takes
/// it. A second one is an internal error rather than a re-run: the assembler compiles each
/// target once, and silently recompiling from an input the caller told us not to read would be
/// worse than saying so.
struct SeedFrontend {
    /// The context the seed's HIR was allocated in, held so it cannot be freed under us.
    ///
    /// Never read: **owning it is the point.** HIR operations reach their context through a raw
    /// pointer (`BlockRef::context_rc`), so a `MidenComponent` handed over by value does not keep
    /// the arena it points into alive. Taking an owning handle here is what turns [`Start::At`]'s
    /// requirement from "hold your `Rc` until the request returns" into "have one when you make
    /// the request", which is the weaker and more honest contract — and it is what
    /// [`backend::hir_to_masm`] and the `compile_link_output_to_masm*` entry points already do
    /// with the very same expression.
    #[expect(
        dead_code,
        reason = "held solely to keep the seed's HIR context alive; see this field's doc"
    )]
    context: Rc<Context>,
    /// The checkpoint the seed was produced at, which decides how much of the backend still runs.
    ///
    /// Kept rather than re-derived: [`resume_from`] has already validated it, and it is the only
    /// thing distinguishing the two seeds — both carry a [`MidenComponent`], and nothing about
    /// the component itself says whether the rewrites have been applied to it.
    resumed_at: CheckpointId,
    /// The HIR this request resumes from, until the seeded target takes it.
    seed: RefCell<Option<MidenComponent>>,
    /// The frontend that serves every target this seed is not for.
    inner: Rc<dyn Frontend>,
    /// What codegen produced for the seeded target, kept for [`Frontend::post_process`].
    lowered: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
}

impl SeedFrontend {
    /// Resume from `seed`, produced at `resumed_at`, delegating every other target to `inner`.
    ///
    /// The seed's own context is taken here rather than asked of the caller; see the
    /// [`context`](SeedFrontend::context) field.
    fn new(resumed_at: CheckpointId, seed: MidenComponent, inner: Rc<dyn Frontend>) -> Self {
        let context = seed.world.borrow().as_operation().context_rc();
        Self {
            context,
            resumed_at,
            seed: RefCell::new(Some(seed)),
            inner,
            lowered: RefCell::new(BTreeMap::new()),
        }
    }

    /// Run the part of the backend the seeded checkpoint has *not* already had done to it.
    ///
    /// The match is exhaustive over what [`resume_from`] accepts, and the fallthrough is an
    /// internal error rather than a default: a checkpoint reaching here that `resume_from` did
    /// not validate would silently pick one of the two amounts of work, and picking the wrong one
    /// is either a miscompile (rewrites run twice) or unlowered IR (rewrites never run).
    fn resume(
        &self,
        cx: &TargetContext<'_>,
        hir: MidenComponent,
    ) -> CompilerResult<Flow<LoweredTarget>> {
        match self.resumed_at {
            CheckpointId::HIR_INITIAL => backend::hir_to_masm(cx, hir),
            CheckpointId::HIR_TRANSFORMED => backend::masm_from_transformed_hir(cx, hir),
            checkpoint => Err(Report::msg(format!(
                "internal error: a request was seeded at '{checkpoint}', which is not a \
                 checkpoint compilation can resume from; `resume_from` should have rejected it"
            ))),
        }
    }
}

impl Frontend for SeedFrontend {
    /// Resume the seeded target from the artifact in hand; compile every other one as usual.
    ///
    /// The seeded checkpoint is **not** republished: it was reached by whatever run produced
    /// the seed, and observers of *this* run see the route from the checkpoint after it.
    ///
    /// **Which phases still run is decided by that checkpoint**, and this is the only place it
    /// is read. A `hir.initial` seed reaches [`backend::hir_to_masm`], so analysis, the rewrites
    /// and codegen all run and the route's remaining checkpoints, `--emit` documents and goal
    /// behave exactly as an unseeded run's. A `hir.transformed` seed has had the first two done
    /// to it already, so it reaches [`backend::masm_from_transformed_hir`] instead — running the
    /// rewrites over already-rewritten IR is a miscompile, not a wasted pass; see [`Start::At`].
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        if !cx.role().is_root() {
            return self.inner.compile(cx);
        }
        let hir = self.seed.borrow_mut().take().ok_or_else(|| {
            Report::msg(format!(
                "internal error: the seed for target '{}' of package '{}' was already consumed; a \
                 seeded target is compiled once, and recompiling it would read an input the \
                 caller asked to resume past",
                cx.assembly().target.name.inner(),
                cx.assembly().package.name().inner(),
            ))
        })?;

        // Destructured rather than moved whole, so that anything assembly needs and this
        // frontend fails to keep is a compile error here rather than a missing section in the
        // package — or, for the component's rodata, a silent miscompile. See [`LoweredTarget`].
        let LoweredTarget {
            sources,
            component,
            account_component_metadata_bytes,
            component_wit_bytes,
            source_provenance,
        } = match self.resume(cx, hir)? {
            Flow::Continue(lowered) => lowered,
            Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
        };
        self.lowered.borrow_mut().insert(
            cx.target_key(),
            CodegenOutput {
                component,
                account_component_metadata_bytes,
                component_wit_bytes,
                source_provenance,
            },
        );
        Ok(Flow::Continue(sources))
    }

    /// Attach what the seeded target's codegen produced; delegate every other target.
    ///
    /// The branch is on the *role*, not on whether the map holds an entry: a missing entry for
    /// the seeded target means the assembler post-processed a target this frontend never
    /// compiled, which is a compiler bug and must be reported as one rather than quietly
    /// answered by the frontend the seed displaced.
    fn post_process(
        &self,
        package: &mut MastPackage,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        if !cx.role().is_root() {
            return self.inner.post_process(package, cx);
        }
        let key = cx.target_key();
        let lowered = self.lowered.borrow();
        let found = lowered.get(&key).ok_or_else(|| {
            Report::msg(format!(
                "internal error: cannot post-process the seeded target '{}' of package '{}': \
                 nothing was lowered for it, so `compile` never ran",
                key.name(),
                key.package(),
            ))
        })?;
        crate::pipeline::assembly::post_process_package(
            package,
            &found.component,
            found.account_component_metadata_bytes.as_deref(),
            found.component_wit_bytes.as_deref(),
            cx.assembly().target,
            cx.assembly().package_registry,
        )
    }

    /// The seeded target's provenance is still the frontend's own.
    ///
    /// Provenance describes what a target was *built from* on disk, which is what the assembler
    /// hashes to decide whether a cached build is current — and it may be asked for before
    /// `compile` runs, so it cannot be answered out of a seed that `compile` consumes. The
    /// answer is therefore the one an unseeded run would give, which is also what makes a
    /// seeded build's provenance comparable with an unseeded one's.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        self.inner.provenance(cx)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        boxed::Box,
        string::{String, ToString},
        sync::Arc,
        vec,
        vec::Vec,
    };

    use miden_assembly::SourceFileProvenance;
    use miden_package_registry::NoPackageStore;
    use midenc_hir::Context;
    use midenc_session::{
        InputFile, Options,
        diagnostics::{DefaultSourceManager, SourceManager},
        miden_project::TargetType,
    };

    use super::*;
    use crate::pipeline::{
        ArtifactDecl, ArtifactId, CompilationRequest, FrontendId, FrontendRegistry, Goal, Observer,
        Outcome, OutputRequest, Pipeline, RecordingObserver, TargetRole,
        testing::{self, VirtualProject},
    };

    // -------------------------------------------------------------------------------------
    // A frontend that reaches `hir.initial` by translating, so that a seed can replace it.
    //
    // Deliberately not one of the shipped frontends: what a seed does is independent of the
    // language the input is written in, and the shipped route that would exercise it —
    // WebAssembly — would fail here for its own reasons. What the fixture must reproduce is
    // the *shape* of that route: a parse checkpoint the seed skips, `hir.initial`, and the
    // three the shared backend publishes.
    // -------------------------------------------------------------------------------------

    /// The extension the fixture frontend claims.
    const SEEDED: &str = "seedfixture";

    /// A second extension the fixture registration claims, and that no fixture project uses.
    ///
    /// Declared **first**, so that a seed installed under "the registration's first extension"
    /// rather than under the one the selected root actually uses is a test failure rather than a
    /// coincidence. A registration claiming several extensions is the normal case —
    /// [`WASM_FRONTEND`](crate::pipeline::frontends::WASM_FRONTEND) claims `wasm` and `wat`.
    const SEEDED_UNUSED: &str = "seedfixture-unused";

    /// The name of the fixture project, and the namespace its component is built in.
    ///
    /// Not the namespace its *target* declares; see
    /// [`component_target_namespace`](crate::pipeline::testing::component_target_namespace).
    const TARGET_NAME: &str = "seed_fixture";

    /// The read-only data the fixture component carries.
    ///
    /// It is what makes `post_process` observable from the assembled package: the rodata
    /// becomes the package's advice map, and a seeded run that never post-processed would
    /// produce a package without one.
    const RODATA: &[u8] = b"seeded rodata fixture bytes!!!!!";

    /// Stands in for the serialized account-component metadata a real account component
    /// carries; nothing validates it.
    const METADATA: &[u8] = b"seeded account component metadata";

    /// What the fixture frontend publishes at `wasm.parsed`: the checkpoint a seed skips.
    #[derive(Debug, PartialEq, Eq)]
    struct Parsed(String);

    /// A frontend that translates, then delegates to the shared backend.
    #[derive(Default)]
    struct SeedFixtureFrontend {
        lowered: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
    }

    impl SeedFixtureFrontend {
        /// The HIR this frontend pretends to have translated from its target root.
        ///
        /// Built in [`TARGET_NAME`]'s namespace, so that the root module codegen produces lands
        /// on the namespace [`seed_manifest`] declares. This is also what a *seed* must carry,
        /// which is the point: the two runs differ in who built the component, not in what it
        /// is.
        fn hir(context: &Rc<Context>) -> MidenComponent {
            testing::component_in_namespace(
                context,
                TARGET_NAME,
                Some(RODATA),
                Some(METADATA.to_vec()),
            )
        }

        /// Everything the frontend does once it holds HIR, which is what a seed replaces the
        /// front of.
        fn lower(
            &self,
            cx: &TargetContext<'_>,
            hir: MidenComponent,
        ) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let LoweredTarget {
                sources,
                component,
                account_component_metadata_bytes,
                component_wit_bytes,
                source_provenance,
            } = match backend::hir_to_masm(cx, hir)? {
                Flow::Continue(lowered) => lowered,
                Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
            };
            self.lowered.borrow_mut().insert(
                cx.target_key(),
                CodegenOutput {
                    component,
                    account_component_metadata_bytes,
                    component_wit_bytes,
                    source_provenance,
                },
            );
            Ok(Flow::Continue(sources))
        }
    }

    impl Frontend for SeedFixtureFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let name = cx.assembly().target.name.inner().to_string();
            if let Flow::Break(stopped) =
                cx.checkpoint(CheckpointId::WASM_PARSED, ArtifactId::WASM, Parsed(name))?
            {
                return Ok(Flow::Break(stopped));
            }
            let hir = Self::hir(&cx.context());
            let hir = match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, hir)? {
                Flow::Continue(hir) => hir,
                Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
            };
            self.lower(cx, hir)
        }

        fn post_process(
            &self,
            package: &mut MastPackage,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<()> {
            let lowered = self.lowered.borrow();
            let found = lowered.get(&cx.target_key()).ok_or_else(|| {
                Report::msg(format!(
                    "internal error: nothing was lowered for target '{}'",
                    cx.target_key().name()
                ))
            })?;
            crate::pipeline::assembly::post_process_package(
                package,
                &found.component,
                found.account_component_metadata_bytes.as_deref(),
                found.component_wit_bytes.as_deref(),
                cx.assembly().target,
                cx.assembly().package_registry,
            )
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: String::from("fixture").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    fn make_seed_fixture(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(SeedFixtureFrontend::default())
    }

    /// Nothing on this route is rendered; these tests never emit.
    fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        Ok(())
    }

    /// Declare `id` at `checkpoint`, rendered by nothing; see [`unrendered`].
    const fn decl(checkpoint: CheckpointId, id: ArtifactId) -> ArtifactDecl {
        ArtifactDecl {
            checkpoint,
            id,
            render: unrendered,
        }
    }

    /// The fixture route, shaped like the WebAssembly one.
    ///
    /// `translate` is an alias the shipped routes do not have: it names `hir.initial`, which is
    /// what lets a test ask for a goal *at* the seed and see it rejected.
    const SEED_FIXTURE_FRONTEND: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("seed-fixture"),
        &[SEEDED_UNUSED, SEEDED],
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
            ("translate", CheckpointId::HIR_INITIAL),
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
        make_seed_fixture,
    );

    // -------------------------------------------------------------------------------------
    // Fixtures.
    // -------------------------------------------------------------------------------------

    /// Materialize a fixture project rooted at a `.seedfixture` file, returning its manifest.
    ///
    /// The root file's contents are never read: the fixture frontend builds its HIR itself, and
    /// a seeded run does not even do that. It only has to exist with the extension dispatch
    /// keys on — for a manifest-backed project it must, because the assembler resolves a
    /// target root with a fallible `canonicalize`.
    fn seed_manifest(dir: &str) -> std::path::PathBuf {
        let namespace = testing::component_target_namespace(TARGET_NAME);
        let root = format!("lib.{SEEDED}");
        testing::fixture_source(dir, &root, "fixture");
        testing::fixture_source(
            dir,
            "miden-project.toml",
            &format!(
                r#"
[package]
name = "{TARGET_NAME}"
version = "0.1.0"

[lib]
namespace = "{namespace}"
path = "{root}"
"#
            ),
        )
    }

    /// A session over the fixture project in `dir`, and the input that names it.
    fn session_over(dir: &str) -> (Rc<Session>, InputFile) {
        let manifest = seed_manifest(dir);
        let input = InputFile::from_path(&manifest).expect("a manifest is a valid input");
        (session_for(input.clone()), input)
    }

    /// A session compiling `input`.
    fn session_for(input: InputFile) -> Rc<Session> {
        let options = Box::new(Options::default()).with_output_types(Default::default(), None);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        Rc::new(
            Session::new(input, options, None, source_manager)
                .expect("the fixture project should open a session"),
        )
    }

    /// A pipeline that dispatches `.seedfixture` roots to the fixture frontend.
    fn pipeline() -> Pipeline {
        let mut registry = FrontendRegistry::new();
        registry
            .register(SEED_FIXTURE_FRONTEND)
            .expect("the fixture route is consistent");
        Pipeline::new(registry)
    }

    /// The HIR a caller seeds with: the very component the fixture frontend would translate.
    ///
    /// Built in a context the *caller* owns, which is the arrangement a real continuation has —
    /// and the reason `context` is a parameter rather than created here. HIR operations hold a
    /// raw pointer to their context, so a helper that built its own would hand back a component
    /// pointing into freed memory; see [`Start::At`].
    fn seed_hir(context: &Rc<Context>) -> MidenComponent {
        SeedFixtureFrontend::hir(context)
    }

    /// Seed a request at `checkpoint` with `artifact`.
    fn seeded(checkpoint: CheckpointId, artifact: Artifact) -> Start {
        Start::At {
            checkpoint,
            artifact,
        }
    }

    /// A recording observer, ready to attach to a request.
    fn recorder() -> Rc<RefCell<RecordingObserver>> {
        Rc::new(RefCell::new(RecordingObserver::default()))
    }

    /// The checkpoints `observer` saw, in order.
    fn trace(observer: &Rc<RefCell<RecordingObserver>>) -> Vec<CheckpointId> {
        observer.borrow().records().iter().map(|(checkpoint, _)| *checkpoint).collect()
    }

    /// Compile the fixture project in `dir`, starting at `start` and stopping at `stop_after`.
    ///
    /// The caller's HIR context is created here and dropped only once the request has returned,
    /// which is what a real continuation does with the context it translated into; see
    /// [`Start::At`].
    fn compile(
        dir: &str,
        start: impl FnOnce(&Rc<Context>) -> Start,
        stop_after: Option<&str>,
    ) -> (CompilerResult<Outcome>, Rc<RefCell<RecordingObserver>>) {
        let (session, input) = session_over(dir);
        let context = Rc::new(Context::new(session.clone()));
        let start = start(&context);
        let observer = recorder();
        let request = CompilationRequest::new(session, input)
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>])
            .with_outputs(OutputRequest::default().with_stop_after(stop_after.map(str::to_string)))
            .with_start(start);
        let outcome = pipeline().compile(request, &mut NoPackageStore);
        drop(context);
        (outcome, observer)
    }

    /// Compile the fixture project in `dir` from a seed at `hir.initial`.
    fn compile_seeded(
        dir: &str,
        stop_after: Option<&str>,
    ) -> (CompilerResult<Outcome>, Rc<RefCell<RecordingObserver>>) {
        compile(
            dir,
            |context| {
                seeded(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, seed_hir(context)))
            },
            stop_after,
        )
    }

    /// The message a failed compilation reports.
    fn error(result: CompilerResult<Outcome>) -> String {
        match result {
            Err(err) => format!("{err}"),
            Ok(outcome) => panic!("expected a rejected request, got {outcome:?}"),
        }
    }

    // -------------------------------------------------------------------------------------
    // The seeded build.
    // -------------------------------------------------------------------------------------

    #[test]
    fn a_seeded_run_builds_the_same_package_without_reaching_the_frontends_parse() {
        let (from_input, unseeded_trace) = compile("seed_from_input", |_| Start::Input, None);
        let from_input = from_input.expect("the fixture project should build from its input");
        let (from_seed, seeded_trace) = compile_seeded("seed_from_seed", None);
        let from_seed = from_seed.expect("a seeded request should build the same project");

        assert_eq!(
            trace(&unseeded_trace),
            vec![
                CheckpointId::WASM_PARSED,
                CheckpointId::HIR_INITIAL,
                CheckpointId::HIR_ANALYZED,
                CheckpointId::HIR_TRANSFORMED,
                CheckpointId::MASM_LOWERED,
                CheckpointId::PACKAGE_ASSEMBLED,
            ],
            "the unseeded run must reach the whole route, or the comparison below means nothing"
        );
        assert_eq!(
            trace(&seeded_trace),
            vec![
                CheckpointId::HIR_ANALYZED,
                CheckpointId::HIR_TRANSFORMED,
                CheckpointId::MASM_LOWERED,
                CheckpointId::PACKAGE_ASSEMBLED,
            ],
            "a seeded run must not reach the frontend's parse, and must not republish the \
             checkpoint the seed was taken at: that checkpoint was reached by whoever produced it"
        );

        let unseeded = from_input.into_package().expect("a full build yields a package");
        let seeded = from_seed.into_package().expect("a seeded full build yields a package");
        assert_eq!(
            seeded.digest(),
            unseeded.digest(),
            "everything downstream of the seed is the same code, so the package must be the same"
        );
        assert_eq!(seeded.name, unseeded.name);
    }

    /// A `hir.transformed` seed reaches codegen **without** passing through the rewrites again.
    ///
    /// This is the defect the arm exists to prevent, and it is asserted on the checkpoint trace
    /// rather than on the package, because the two seeds produce the *same* package for a
    /// fixture whose rewrites are idempotent — which every fixture small enough to unit-test is.
    /// The greedy pipeline is not idempotent in general: run twice over real code it produced
    /// `NoSolution` operand-scheduling failures in five 128-bit arithmetic fixtures and two
    /// differential proptests, which is what a trace showing `hir.transformed` on a run that was
    /// *seeded* there would mean.
    #[test]
    fn a_seed_at_hir_transformed_runs_codegen_and_nothing_before_it() {
        let (outcome, trace_of) = compile(
            "seed_from_transformed",
            |context| {
                seeded(
                    CheckpointId::HIR_TRANSFORMED,
                    Artifact::new(ArtifactId::HIR, seed_hir(context)),
                )
            },
            None,
        );
        outcome.expect("a request seeded after the rewrites should still build a package");

        assert_eq!(
            trace(&trace_of),
            vec![CheckpointId::MASM_LOWERED, CheckpointId::PACKAGE_ASSEMBLED],
            "a seed taken after the rewrites must reach codegen directly: neither `hir.analyzed` \
             nor `hir.transformed` may be published, because publishing either would mean the \
             phase that produces it ran a second time over IR it had already transformed"
        );
    }

    #[test]
    fn a_seeded_run_post_processes_the_package_it_assembled() {
        // The half a digest comparison cannot see. `post_process` is what attaches the rodata
        // advice map and the account-component metadata, and it runs against a context the
        // assembler builds afresh — so a seed that lowered the HIR but kept nothing would
        // assemble a package whose code asks the advice provider for data that is not there.
        use midenc_codegen_masm::Rodata;

        let (outcome, _) = compile_seeded("seed_post_process", None);
        let package = outcome
            .expect("a seeded request should build")
            .into_package()
            .expect("a full build yields a package");

        let advice_map = package.mast_forest().advice_map();
        assert_eq!(advice_map.len(), 1, "the seed's rodata must reach the package's advice map");
        let (_, value) = advice_map.iter().next().expect("the advice map has an entry");
        assert_eq!(
            value.as_ref(),
            Rodata::bytes_to_elements(RODATA).as_slice(),
            "and it must be the rodata the seeded component carried"
        );

        let section = package
            .sections
            .iter()
            .find(|section| section.id == miden_mast_package::SectionId::ACCOUNT_COMPONENT_METADATA)
            .expect("the account-component metadata must reach the package as a section");
        assert_eq!(section.data.as_ref(), METADATA);
    }

    #[test]
    fn a_seeded_run_can_itself_stop_early() {
        // Everything past the seed is the ordinary route, so the goal is answered by the same
        // `TargetContext::checkpoint` an unseeded run reaches — but that is a consequence, not
        // an assumption, and it is the property a caller resuming a build in order to inspect
        // the middle of it depends on.
        let (outcome, observer) = compile_seeded("seed_stop_after", Some("analyze"));
        let outcome = outcome.expect("stopping short of assembly is a success, not an error");

        assert_eq!(outcome.checkpoint(), CheckpointId::HIR_ANALYZED);
        assert_eq!(
            trace(&observer),
            vec![CheckpointId::HIR_ANALYZED],
            "no work may happen past the stop point of a seeded run either"
        );
        assert_eq!(
            outcome.artifact().id(),
            ArtifactId::HIR,
            "and what it captured is the route's own artifact, not the seed handed back"
        );
        assert!(
            outcome.downcast::<MidenComponent>().is_ok(),
            "the analyzed HIR must survive the capture intact"
        );
    }

    // -------------------------------------------------------------------------------------
    // What a seed is not allowed to be.
    // -------------------------------------------------------------------------------------

    #[test]
    fn a_seeded_request_with_no_input_path_is_rejected() {
        // A seed replaces *reading* the input, not the input: which project is built, which
        // target it holds, and therefore which route the seed resumes are all still derived
        // from the input's path. A stdin-backed input has none.
        let input = InputFile::from_bytes(b"(module)".to_vec(), "stdin".into())
            .expect("wasm text is a recognized input");
        let session = session_for(input.clone());
        let context = Rc::new(Context::new(session.clone()));
        let seed = Artifact::new(ArtifactId::HIR, seed_hir(&context));
        let request = CompilationRequest::new(session, input)
            .with_start(seeded(CheckpointId::HIR_INITIAL, seed));

        let rendered = error(pipeline().compile(request, &mut NoPackageStore));
        assert!(
            rendered.contains("seeded"),
            "the rejection must be the seed's own, not the one a stdin input would get for being \
             an unreadable project locator: {rendered}"
        );
        assert!(
            rendered.contains("path"),
            "and it must say what is missing, or a caller cannot act on it: {rendered}"
        );
    }

    #[test]
    fn a_seed_at_a_checkpoint_this_route_does_not_reach_is_rejected() {
        let (outcome, _) = compile(
            "seed_off_route",
            |context| {
                seeded(
                    CheckpointId::MASM_PARSED,
                    Artifact::new(ArtifactId::MASM, seed_hir(context)),
                )
            },
            None,
        );

        let rendered = error(outcome);
        assert!(
            rendered.contains("masm.parsed"),
            "the rejection must name the checkpoint the seed claimed: {rendered}"
        );
        assert!(
            rendered.contains("seed-fixture"),
            "and the frontend whose route does not reach it: {rendered}"
        );
    }

    #[test]
    fn a_masm_lowered_seed_is_rejected_and_says_what_it_would_have_to_carry() {
        // The plan left this open. It is settled by refusing it: the checkpoint publishes
        // `ProjectSourceInputs`, and a seed carrying those alone cannot post-process, which is
        // not a missing section but a silent miscompile. The rejection carries the answer.
        let (outcome, _) = compile(
            "seed_masm_lowered",
            |context| {
                seeded(
                    CheckpointId::MASM_LOWERED,
                    Artifact::new(ArtifactId::MASM, seed_hir(context)),
                )
            },
            None,
        );

        let rendered = error(outcome);
        assert!(rendered.contains("masm.lowered"), "{rendered}");
        assert!(
            rendered.contains("LoweredTarget"),
            "the rejection must name what such a seed would have to carry: {rendered}"
        );
        assert!(
            rendered.contains("advice map"),
            "and why the sources alone are not enough: {rendered}"
        );
    }

    #[test]
    fn a_seed_that_does_not_hold_what_its_checkpoint_produces_is_rejected() {
        // Tagged `wasm`, which is the plausible mistake on this route: the checkpoint before the
        // seed publishes the WebAssembly, and handing *that* over is what a caller off by one
        // does. Tagging it `hir` would leave the assertion below unable to tell a message that
        // reports the tag from one that merely mentions HIR.
        let (outcome, _) = compile(
            "seed_wrong_type",
            |_| {
                seeded(
                    CheckpointId::HIR_INITIAL,
                    Artifact::new(ArtifactId::WASM, String::from("not a component")),
                )
            },
            None,
        );

        let rendered = error(outcome);
        assert!(rendered.contains("hir.initial"), "{rendered}");
        assert!(
            rendered.contains(&format!("'{}'", ArtifactId::WASM)),
            "the rejection must report the tag the seed actually carried, not a fixed string: \
             {rendered}"
        );
        assert!(
            rendered.contains("HIR component"),
            "and say what that checkpoint produces instead: {rendered}"
        );
    }

    #[test]
    fn a_seed_this_request_has_no_room_for_is_rejected() {
        // Two shapes of the same mistake: a goal the route reaches *before* the seed, and one
        // it reaches *at* the seed. Both leave nothing to compile, and without this check the
        // run reaches the driver's "ran past its goal" internal error instead — a compiler bug
        // report for a caller error.
        for (dir, stop_after) in [("seed_goal_before", "parse"), ("seed_goal_at", "translate")] {
            let (outcome, _) = compile_seeded(dir, Some(stop_after));
            let rendered = error(outcome);
            assert!(
                rendered.contains("hir.initial"),
                "the rejection must name the seed: {rendered}"
            );
            assert!(
                rendered.contains("--stop-after") || rendered.contains("stops at"),
                "and the goal that leaves no room for it: {rendered}"
            );
        }
    }

    // -------------------------------------------------------------------------------------
    // What the seed must leave alone.
    // -------------------------------------------------------------------------------------

    #[test]
    fn the_seed_provider_serves_the_selected_targets_extension() {
        // The override wins by being installed under the same `&'static str` the registry's
        // provider is keyed by. Taken from the registration's own extensions, never from the
        // resolved target root, which is a runtime `Cow<str>` — but *which* of them, since this
        // registration claims two and the fixture project's root uses the second.
        let (session, input) = session_over("seed_extension");
        let prepared = crate::pipeline::prepare_project(
            &input,
            &session.options,
            pipeline_registry(),
            session.source_manager.as_ref(),
        )
        .expect("the fixture project should prepare");
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));

        let context = Rc::new(Context::new(session.clone()));
        let provider = provider(
            CheckpointId::HIR_INITIAL,
            Artifact::new(ArtifactId::HIR, seed_hir(&context)),
            &prepared,
            &session,
            &state,
        )
        .expect("a well-formed seed should install a provider");

        assert_eq!(
            provider.file_type(),
            SEEDED,
            "the seed must be installed under the extension the *selected root* uses, which is \
             what the registry's provider for that root is keyed by — not under whichever of the \
             registration's extensions happens to come first"
        );
        assert_ne!(SEEDED_UNUSED, SEEDED, "the two extensions must differ, or this proves nothing");
    }

    /// The registry the fixture pipeline dispatches with, for the tests that prepare by hand.
    fn pipeline_registry() -> &'static FrontendRegistry {
        use std::sync::OnceLock;
        static REGISTRY: OnceLock<FrontendRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            let mut registry = FrontendRegistry::new();
            registry
                .register(SEED_FIXTURE_FRONTEND)
                .expect("the fixture route is consistent");
            registry
        })
    }

    /// A frontend that records that it was asked, and does the least it can.
    #[derive(Default)]
    struct DelegateFrontend {
        calls: RefCell<Vec<(&'static str, TargetRole)>>,
    }

    impl DelegateFrontend {
        fn calls(&self) -> Vec<(&'static str, TargetRole)> {
            self.calls.borrow().clone()
        }
    }

    impl Frontend for DelegateFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            use miden_assembly::{ModuleParser, ast::ModuleKind};

            self.calls.borrow_mut().push(("compile", cx.role()));
            let namespace = cx.assembly().target.namespace.inner().clone();
            let root = ModuleParser::new(Some(ModuleKind::Library)).parse_str(
                Some(namespace.as_ref()),
                "pub proc main\n    push.1\n    drop\nend\n",
                cx.session().source_manager.clone(),
            )?;
            Ok(Flow::Continue(ProjectSourceInputs {
                root,
                support: Vec::new(),
            }))
        }

        fn post_process(
            &self,
            _package: &mut MastPackage,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<()> {
            self.calls.borrow_mut().push(("post_process", cx.role()));
            Ok(())
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            self.calls.borrow_mut().push(("provenance", cx.role()));
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: String::from("delegate").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    #[test]
    fn a_target_the_seed_is_not_for_is_served_by_the_frontend_it_displaced() {
        // The override displaces the registry's provider for a whole extension, and that
        // provider serves every target with it — not only the root. A seed that answered for
        // all of them would compile a dependency from the root's HIR, or fail for having no
        // seed left. Both are worse than the build the caller asked for.
        let exe_root = testing::wat_fixture("seed_delegate", "main.wat");
        let lib_root = testing::wat_fixture("seed_delegate", "lib.wat");
        let project = VirtualProject::executable_and_library("seed_delegate", &exe_root, &lib_root)
            .expect("should build a two-target virtual project");
        let library = project
            .targets()
            .iter()
            .find(|target| target.is_library())
            .expect("the fixture declares a library target");

        let session = Context::default().session_rc();
        let context = Rc::new(Context::new(session.clone()));
        let inner = Rc::new(DelegateFrontend::default());
        let seed = SeedFrontend::new(
            CheckpointId::HIR_INITIAL,
            testing::minimal_component(&context),
            inner.clone() as Rc<dyn Frontend>,
        );
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            Rc::new(seed) as Rc<dyn Frontend>,
            session,
            state,
            RootTarget::new(project.package(), project.target()),
        );

        let assembly = project.assembly_context_for(library).expect("library context");
        assert!(
            provider
                .provide_sources_interruptible(&assembly)
                .expect("the delegate must serve a target the seed is not for")
                .is_continue(),
            "a target the seed is not for may not stop the build"
        );
        provider
            .provide_source_provenance(&assembly)
            .expect("provenance is the displaced frontend's on every target");
        provider
            .post_process_package(&mut any_package(), &assembly)
            .expect("post-processing a delegated target is the displaced frontend's too");

        assert_eq!(
            inner.calls(),
            vec![
                ("compile", TargetRole::RequiredLibrary),
                ("provenance", TargetRole::RequiredLibrary),
                ("post_process", TargetRole::RequiredLibrary),
            ],
            "every callback for a target the seed is not for must reach the frontend the seed \
             displaced"
        );
    }

    /// A well-formed assembled package, for a delegated `post_process` to be handed.
    fn any_package() -> MastPackage {
        (*midenc_codegen_masm::intrinsics::load()).clone()
    }

    #[test]
    fn the_seeded_targets_provenance_is_the_displaced_frontends() {
        // Provenance describes what a target was built *from*, which the assembler hashes to
        // decide whether a cached build is current — and it can be asked before `compile`, so
        // it cannot come out of a seed that `compile` consumes.
        let root = testing::wat_fixture("seed_provenance", "lib.wat");
        let project = VirtualProject::new("seed_provenance", &root, TargetType::Library)
            .expect("should build a virtual project");
        let session = Context::default().session_rc();
        let context = Rc::new(Context::new(session.clone()));
        let inner = Rc::new(DelegateFrontend::default());
        let seed = SeedFrontend::new(
            CheckpointId::HIR_INITIAL,
            testing::minimal_component(&context),
            inner.clone() as Rc<dyn Frontend>,
        );
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            Rc::new(seed) as Rc<dyn Frontend>,
            session,
            state,
            RootTarget::new(project.package(), project.target()),
        );

        let assembly = project.assembly_context().expect("assembly context");
        provider
            .provide_source_provenance(&assembly)
            .expect("the root target's provenance must still be answerable");

        assert_eq!(
            inner.calls(),
            vec![("provenance", TargetRole::Root)],
            "even the seeded target's provenance is the displaced frontend's"
        );
    }

    #[test]
    fn a_seed_is_never_read_from_the_input_path_it_names() {
        // The property `tests/support` depends on: its session is built from a `dummy.wasm`
        // that was never written. A synthesized project resolves its target root with
        // `canonicalize().unwrap_or(..)`, so the path survives unresolved, and the seed means
        // nothing ever opens it.
        let root = std::env::temp_dir()
            .join("midenc-pipeline-fixtures")
            .join("seed_absent_root")
            .join("dummy.wat");
        assert!(!root.exists(), "the fixture must name a file that does not exist");
        let project = VirtualProject::new("seed_absent_root", &root, TargetType::Library)
            .expect("a virtual project tolerates a target root that does not exist");

        let session = Context::default().session_rc();
        let context = Rc::new(Context::new(session.clone()));
        let seed = SeedFrontend::new(
            CheckpointId::HIR_INITIAL,
            testing::minimal_component(&context),
            Rc::new(DelegateFrontend::default()) as Rc<dyn Frontend>,
        );
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            Rc::new(seed) as Rc<dyn Frontend>,
            session,
            state,
            RootTarget::new(project.package(), project.target()),
        );

        let assembly = project.assembly_context().expect("assembly context");
        assert!(
            provider
                .provide_sources_interruptible(&assembly)
                .expect("a seeded target must compile without its root ever being opened")
                .is_continue(),
            "the goal is the terminal checkpoint, so the seeded target must hand back sources"
        );
    }

    #[test]
    fn the_seed_provider_owns_the_context_the_seed_points_into() {
        // The contract of `Start::At` made executable. HIR operations reach their context
        // through a raw pointer, so a `MidenComponent` handed over by value does not keep the
        // arena it points into alive — and the caller's `Rc` is the natural thing to drop the
        // moment the seed has been handed over. `SeedFrontend` takes an owning handle of its
        // own at construction, which is what makes that safe.
        //
        // Without that handle this test does not fail an assertion: it takes SIGSEGV. That is
        // exactly why the guarantee belongs in the type rather than in a doc comment.
        let root = testing::wat_fixture("seed_owns_context", "lib.wat");
        let project = VirtualProject::new("seed_owns_context", &root, TargetType::Library)
            .expect("should build a virtual project");
        let session = Context::default().session_rc();
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));

        let provider = {
            let context = Rc::new(Context::new(session.clone()));
            let seed = SeedFrontend::new(
                CheckpointId::HIR_INITIAL,
                testing::minimal_component(&context),
                Rc::new(DelegateFrontend::default()) as Rc<dyn Frontend>,
            );
            let provider = FrontendProvider::new(
                "wat",
                Rc::new(seed) as Rc<dyn Frontend>,
                session,
                state,
                RootTarget::new(project.package(), project.target()),
            );
            // The caller lets go the instant the seed is in the pipeline's hands.
            drop(context);
            provider
        };

        let assembly = project.assembly_context().expect("assembly context");
        assert!(
            provider
                .provide_sources_interruptible(&assembly)
                .expect("the seed must still be sound after the caller dropped its context")
                .is_continue(),
        );
    }

    #[test]
    fn a_second_callback_for_the_seeded_target_is_an_invariant_error() {
        // The seed is owned and taken once. Recompiling the target from its input instead
        // would be a build the caller did not ask for, from bytes it told us not to read.
        let root = testing::wat_fixture("seed_twice", "lib.wat");
        let project = VirtualProject::new("seed_twice", &root, TargetType::Library)
            .expect("should build a virtual project");
        let session = Context::default().session_rc();
        let context = Rc::new(Context::new(session.clone()));
        let seed = SeedFrontend::new(
            CheckpointId::HIR_INITIAL,
            testing::minimal_component(&context),
            Rc::new(DelegateFrontend::default()) as Rc<dyn Frontend>,
        );
        let state =
            Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new()));
        let provider = FrontendProvider::new(
            "wat",
            Rc::new(seed) as Rc<dyn Frontend>,
            session,
            state,
            RootTarget::new(project.package(), project.target()),
        );

        let assembly = project.assembly_context().expect("assembly context");
        assert!(
            provider
                .provide_sources_interruptible(&assembly)
                .expect("the first callback consumes the seed")
                .is_continue(),
        );
        let Err(err) = provider.provide_sources_interruptible(&assembly) else {
            panic!("a second callback for the seeded target must be reported");
        };
        let rendered = format!("{err}");
        assert!(rendered.contains("internal error"), "{rendered}");
        assert!(rendered.contains("seed"), "the report must say what was consumed: {rendered}");
    }
}
