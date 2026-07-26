//! The frontend-neutral compilation pipeline.
//!
//! # Model
//!
//! A [`CheckpointId`] names a point at which an artifact exists. Frontends run
//! imperatively and call [`TargetContext::checkpoint`], which notifies observers and
//! returns [`Flow::Continue`] with the artifact, or [`Flow::Stop`] once the requested goal
//! is reached. Checkpoints carry no global ordering: each [`FrontendRegistration`]
//! declares an ordered route, and every comparison happens within one route.
//!
//! [`resolve_goal`] decides how far compilation runs: the `--stop-after` checkpoint when
//! one is given, otherwise the route's terminal checkpoint. `--emit` requests *additional*
//! artifacts and never shortens the build; it is only validated against the resulting
//! [`Goal`]. [`backend::hir_to_masm`] is the shared HIR backend, called by frontends that
//! produce HIR.
//!
//! # Status
//!
//! Increment 3 of the pipeline redesign. **Every project input is compiled here.** A
//! manifest input — `miden-project.toml`, or the `Cargo.toml` standing in for one — is
//! prepared, dispatched to a frontend by its selected target's root extension, and assembled
//! through [`Pipeline::compile`]. Both shipped frontends are registered for every such
//! build, so a Miden Assembly dependency of a Rust project is compiled by
//! [`frontends::masm::MasmProjectFrontend`] while the root is compiled by
//! [`frontends::rust::RustProjectFrontend`], each with the role [`RootTarget`] derives for it.
//!
//! [`frontends::masm::MasmProjectFrontend`] publishes checkpoints, so `--stop-after` is
//! meaningful on that route. [`Pipeline::compile`] attaches an observer of its own that
//! renders the *selected* target's artifacts as it reaches them, through the route's own
//! [`ArtifactDecl::render`] — so `--emit` now reaches a project build, decided by the session
//! rather than by the request. The entry point still does not forward `--emit` or
//! `--stop-after` into the [`OutputRequest`] it builds, so goal-based validation of an
//! explicit `--emit` is not yet in force for a project build; that belongs with the CLI, which
//! still holds the raw specs `Options` folds away.
//!
//! Direct file inputs are unchanged: the standalone `Stage` chains in [`crate::stages`]
//! still own parse and codegen for them, and reach [`RustProjectFrontend`](frontends::rust::RustProjectFrontend) only through a
//! [`FrontendProvider`] registered for `rs`.
//! See `tasks/specs/2026-07-25-midenc-compile-pipeline-design.md`.

mod artifact;
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
pub mod testing;

pub use self::{
    artifact::{Artifact, ArtifactId},
    checkpoint::CheckpointId,
    driver::{CompilationRequest, Pipeline},
    flow::{Flow, Stopped},
    frontend::{CaptureSlot, Frontend, TargetContext, TargetKey},
    goal::{Goal, OutputRequest, artifact_id_for_output, resolve_goal},
    observer::{Observer, RecordingObserver, TargetRole},
    outcome::Outcome,
    prepare::{PreparedProject, prepare_project},
    provider::{FrontendProvider, RootTarget},
    registry::{ArtifactDecl, FrontendId, FrontendRegistration, FrontendRegistry},
    request::RequestState,
};
