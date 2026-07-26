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
//! Increment 1 of the pipeline redesign. These contracts are additive and are not yet
//! wired into compilation: the `Stage` chain in [`crate::stages`] remains the only live
//! path. See `tasks/specs/2026-07-25-midenc-compile-pipeline-design.md`.

mod artifact;
pub mod backend;
mod checkpoint;
mod flow;
mod frontend;
mod goal;
mod observer;
mod outcome;
mod registry;
pub mod testing;

pub use self::{
    artifact::{Artifact, ArtifactId},
    checkpoint::CheckpointId,
    flow::{Flow, Stopped},
    frontend::{CaptureSlot, Frontend, TargetContext, TargetKey},
    goal::{Goal, OutputRequest, artifact_id_for_output, resolve_goal},
    observer::{Observer, RecordingObserver, TargetRole},
    outcome::Outcome,
    registry::{FrontendId, FrontendRegistration, FrontendRegistry},
};
