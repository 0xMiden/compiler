//! The frontend for targets rooted at HIR text.

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use core::cell::RefCell;
use std::path::Path;

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs, SourceFileProvenance};
use miden_mast_package::Package as MastPackage;
use midenc_hir::{Context, OperationRef, dialects::builtin};
use midenc_session::{
    FileName, FileType, InputType, Session,
    diagnostics::{Report, Uri},
};

use crate::{
    CodegenOutput, CompilerResult, MidenComponent,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, TargetContext, TargetKey,
        backend::{self, LoweredTarget},
    },
};

/// Declares the frontend that handles targets rooted at a `.hir` file.
///
/// # Nothing on this route is rendered
///
/// Every checkpoint here declares its artifact unrendered, and each declaration below names
/// the code that writes it instead. That is deliberate, and the reason is that these documents
/// are written *inline* by the phases that produce them — `--emit=hir` in particular is written
/// twice per build, once before the rewrites and once after, and only the code running each
/// phase knows which document it is holding. Declaring a renderer here as well would make two
/// writers for one output type, and since [`Session::emit`] resolves an output type's
/// destination once, the second writer either races for the same file or prints the document
/// twice when that destination is stdout. Converting these emissions into renderers is the
/// consumer increment's work, not this one's.
pub const HIR_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("hir"),
    &["hir"],
    &[
        CheckpointId::HIR_INITIAL,
        CheckpointId::HIR_ANALYZED,
        CheckpointId::HIR_TRANSFORMED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    &[
        ("parse", CheckpointId::HIR_INITIAL),
        ("analyze", CheckpointId::HIR_ANALYZED),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        // The *pre*-rewrite `--emit=hir` document is written by `HirFrontend::parse`, which
        // calls `crate::emit_hir_if_requested` on the operation it just parsed — before the
        // world wrapping [`extract_miden_component_or_bail`] does, exactly where `ParseHirStage`
        // wrote it. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_INITIAL,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // Nothing writes here, and nothing may: [`backend::analyze`] emits no document of its
        // own, and the HIR it publishes is the very same world `hir.initial` already wrote.
        // See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_ANALYZED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // The *post*-rewrite `--emit=hir` document is written by [`backend::apply_rewrites`],
        // which is the one function both this route and the legacy stage chain reach the
        // rewrites through. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_TRANSFORMED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // `--emit=masm` is written by [`backend::codegen`], the only point at which the lowered
        // component exists in a form the session can write. Note that
        // [`MASM_FRONTEND`](super::masm::MASM_FRONTEND) instead renders its Miden Assembly from
        // `masm.parsed`: two routes, two writers, because a MASM target's sources are a module
        // tree read from disk while these are lowered from HIR. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::MASM_LOWERED,
            id: ArtifactId::MASM,
            render: unrendered,
        },
        // The assembled package is written by [`crate::compile`], which emits it in both `mast`
        // and `masp` form once the pipeline hands it back. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
            id: ArtifactId::PACKAGE,
            render: unrendered,
        },
    ],
    make_hir,
);

/// Build the frontend this registration declares.
fn make_hir(_session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(HirFrontend::default())
}

/// A renderer for the checkpoints on this route, every one of which something else writes.
///
/// Every use of it names its writer at the declaration site. A second emission would not be a
/// harmless duplicate: the destination for an output type is resolved once from the session, so
/// two writers of one artifact either race for the same file or, when that destination is
/// stdout, print the artifact twice.
fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
    Ok(())
}

/// Compiles a target whose root is a file of HIR text.
///
/// This is `ParseHirStage` re-homed — parse, extract, publish — with the phases past extraction
/// delegated to [`backend::hir_to_masm`], which every HIR-producing frontend shares.
///
/// # No stop policy lives here
///
/// The legacy `.hir` chain stopped in three places, and none of them came along.
/// `ParseHirStage` raised [`CompilerStopped`](crate::CompilerStopped) for `-Cparse-only`
/// (`stages/parse/hir.rs:44-47`), `ApplyRewritesStage` for the derived `rewrite_only()`
/// predicate (`stages/rewrite.rs:29-31`), and `ComponentAnalysisStage` for `-Canalyze-only`.
/// All three are stop policy, which is the goal's job: each resolves to a checkpoint on this
/// route — `hir.initial`, `hir.transformed` and `hir.analyzed` respectively, see
/// [`StopFlag`](crate::pipeline::StopFlag) — and [`TargetContext::checkpoint`] takes the stop
/// there. Re-expressing any of them here would give one build two places to stop that could
/// disagree.
///
/// Routing through [`backend::hir_to_masm`] does make the advice-taint analysis run for `.hir`
/// inputs, which the legacy `hir_pipeline` never did — so `-Zlint` findings, and the failure
/// they cause when any of them is an error, are newly live on this route.
///
/// # Per-target state
///
/// [`Frontend::post_process`] is called with a *fresh* [`TargetContext`], so what
/// [`backend::hir_to_masm`] produced has to survive in the frontend, keyed by
/// [`TargetContext::target_key`] — the same arrangement
/// [`RustProjectFrontend`](super::rust::RustProjectFrontend) uses for its cargo builds.
///
/// Provenance is *not* memoized alongside it, unlike
/// [`WasmFrontend`](super::wasm::WasmFrontend)'s. Answering costs one read of a text file and
/// no decoding at all — the HIR text is its own provenance — which is the case
/// [`Frontend::provenance`] explicitly exempts, and which
/// [`MasmProjectFrontend`](super::masm::MasmProjectFrontend) is also built on. A cache would
/// buy a file read and cost a second source of truth for the answer.
#[derive(Default)]
pub struct HirFrontend {
    /// What codegen produced for each target, kept for [`Frontend::post_process`].
    lowered: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
}

/// One target's HIR source, as this frontend reaches it.
///
/// The two cases are kept apart rather than reduced to a path plus an optional body, because
/// they differ in more than where the bytes come from: a file is handed to the parser as a
/// *path*, so the session's source manager loads it and diagnostics point at the file on disk,
/// while piped-in text has to be registered under a [`Uri`] made from the input's name.
enum HirSource {
    /// HIR read from a file on disk.
    File {
        /// The path the HIR is read from, and which its provenance records.
        path: Box<Path>,
    },
    /// HIR piped in, which exists only in memory and has no path to read it back from.
    Stdin {
        /// The name the input was given, which the module's diagnostics are attributed to.
        name: FileName,
        /// The HIR text itself.
        text: String,
    },
}

impl HirSource {
    /// This source's build provenance.
    ///
    /// For a file this re-reads it, which is what makes the read the whole cost of answering a
    /// provenance question; for piped-in text the answer is already in hand.
    fn provenance(&self) -> CompilerResult<ProjectSourceProvenanceInputs> {
        let root = match self {
            Self::File { path } => SourceFileProvenance::from_path(path.to_path_buf())?,
            Self::Stdin { name, text } => SourceFileProvenance {
                path: name.as_path().to_path_buf().into_boxed_path(),
                content: text.clone().into_boxed_str(),
            },
        };
        Ok(ProjectSourceProvenanceInputs {
            root,
            support: Vec::new(),
        })
    }

    /// Parse this source into whatever operation it holds.
    ///
    ///
    /// `verify: true` matches `ParseHirStage`: HIR that reaches the compiler as text has not
    /// been through any of the builders that maintain the IR's invariants, so it is the one
    /// input shape that has to be checked before anything else looks at it.
    fn parse_operation(&self, context: Rc<Context>) -> CompilerResult<OperationRef> {
        let config = midenc_hir::parse::ParserConfig {
            context,
            verify: true,
        };
        match self {
            Self::File { path } => midenc_hir::parse::parse_file_any(config, path),
            Self::Stdin { name, text } => {
                midenc_hir::parse::parse_any(config, Uri::new(name.as_str()), text.as_str())
            }
        }
    }
}

impl HirFrontend {
    /// Locate this target's HIR source.
    ///
    /// # Which of the two sources wins
    ///
    /// A stdin-backed input is the one case a path cannot serve: it exists only in memory, and
    /// the target root synthesized for it names a file that was never written. So it is read
    /// from [`TargetContext::input`], and everything else from
    /// `assembly().resolved_target_root`.
    ///
    /// Preferring the path for a *file*-backed input is not merely equivalent — it is the safer
    /// of the two. The input and the resolved target root are the same file for the target the
    /// input backs, but a provider is handed one input and serves every callback for its
    /// extension, so a same-extension dependency would otherwise be compiled from the root's
    /// bytes.
    fn read(cx: &TargetContext<'_>) -> CompilerResult<HirSource> {
        match cx.input().map(|input| (input.file_type(), &input.file)) {
            Some((file_type, InputType::Stdin { name, input })) => {
                Self::expect_hir(file_type, name.as_path())?;
                // Strictly, not lossily: the parser needs `&str` regardless, so replacing an
                // invalid sequence here would only move the failure to a worse place to
                // report it from.
                let text = core::str::from_utf8(input)
                    .map_err(|err| Report::msg(format!("failed to parse {name}: {err}")))?;
                Ok(HirSource::Stdin {
                    name: name.clone(),
                    text: text.to_string(),
                })
            }
            _ => {
                let path = cx.assembly().resolved_target_root.clone();
                let file_type = FileType::try_from(path.as_ref()).map_err(|err| {
                    Report::msg(format!("invalid target root '{}': {err}", path.display()))
                })?;
                Self::expect_hir(file_type, &path)?;
                Ok(HirSource::File { path })
            }
        }
    }

    /// The report for a target this frontend was asked to compile but cannot.
    ///
    /// Reachable only through a registry that dispatched some other extension here, or a target
    /// root whose extension and content disagree, so it names both what it got and what it
    /// handles.
    fn expect_hir(file_type: FileType, path: &Path) -> CompilerResult<()> {
        match file_type {
            FileType::Hir => Ok(()),
            file_type => Err(Report::msg(format!(
                "invalid input file '{}': expected '.hir', got {file_type}",
                path.display()
            ))),
        }
    }

    /// Parse `source` into a component, writing the pre-rewrite `--emit=hir` document on the way.
    ///
    /// The emission is `ParseHirStage`'s, at the point it made it: the *parsed* operation,
    /// before [`extract_miden_component_or_bail`] anchors it. It is the only inline emission
    /// this route owns — the post-rewrite document and `--emit=masm` belong to
    /// [`backend::apply_rewrites`] and [`backend::codegen`], and calling
    /// [`backend::hir_to_masm`] inherits both.
    ///
    /// # Why the placeholder provenance is overwritten
    ///
    /// [`extract_miden_component_or_bail`] fills in a `mod.hir` placeholder, because it is
    /// reached from callers that have no source file to point at. This route does have one, so
    /// it overwrites the placeholder with the target's own — which is what `ParseComponentStage`
    /// already did for `.hir` inputs (`stages/parse.rs:45-55`).
    ///
    /// Be clear about what that buys, because it is less than it looks: on *this* route the
    /// field is write-only. It travels into [`backend::codegen`], back out on [`LoweredTarget`],
    /// and into the [`CodegenOutput`] stashed for [`Frontend::post_process`], which never reads
    /// it — the provenance the assembler hashes is what [`Frontend::provenance`] returns, not
    /// this field. So the overwrite is a consistency measure, not a correctness one: it keeps
    /// the artifact published at `hir.initial` honest about where its HIR came from, and keeps
    /// the one value two code paths could disagree about in agreement.
    /// [`RustProjectFrontend`](super::rust::RustProjectFrontend) *does* read the same field back
    /// off its `CodegenOutput` (`frontends/rust.rs:191,196`), so a future reader here is a
    /// plausible thing to leave correct rather than a hypothetical one.
    fn parse(context: Rc<Context>, source: &HirSource) -> CompilerResult<MidenComponent> {
        let op = source.parse_operation(context.clone())?;
        {
            let op = op.borrow();
            crate::emit_hir_if_requested(&op, context.clone())?;
        }

        let mut hir = extract_miden_component_or_bail(op, context)?;
        hir.source_provenance = source.provenance()?;
        Ok(hir)
    }
}

impl Frontend for HirFrontend {
    /// Parse this target's HIR and lower it for assembly.
    ///
    /// `hir.initial` is published once the text has been parsed and anchored, and the three the
    /// shared backend owns follow. What [`backend::hir_to_masm`] returns beyond the sources is
    /// kept for [`Frontend::post_process`], which the assembler calls against a context of its
    /// own.
    ///
    /// # The rewrites run after extraction, which changes what they are anchored at
    ///
    /// The legacy `hir_pipeline` ran `ApplyRewritesStage` on the *parsed* operation and extracted
    /// a component afterwards. Here extraction comes first, so [`backend::apply_rewrites`] is
    /// anchored at `component.world` instead.
    ///
    /// That is a fix rather than a reordering. The rewrite pipeline is built with
    /// `PassManager::on::<builtin::World>`, so anchoring it anywhere else fails outright with
    /// "failed to construct pass manager" — which is what the legacy chain did for every `.hir`
    /// file whose top-level operation was a bare module or a bare component, since the parser
    /// hands back the operation it parsed rather than the world it anchors it at. Only a file
    /// written as a whole `builtin.world` compiled. Extracting first makes the anchor a world in
    /// all three cases.
    ///
    /// It is also a real difference for a top-level operation that is *not* compilable: the
    /// legacy chain rewrote it and then rejected it, while this rejects it first.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let source = Self::read(cx)?;
        let hir = Self::parse(cx.context(), &source)?;
        let hir = match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, hir)? {
            Flow::Continue(hir) => hir,
            Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
        };

        // Destructured rather than moved whole, so that anything assembly needs and this
        // frontend fails to keep is a compile error here rather than a missing section in the
        // package — or, for the component's rodata, a silent miscompile. See [`LoweredTarget`].
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

    /// Attach the account-component metadata and the rodata advice map this target produced.
    ///
    /// A cache miss means the assembler asked to post-process a target this frontend never
    /// compiled, which is a compiler bug: report it rather than indexing into the map and
    /// panicking.
    fn post_process(
        &self,
        package: &mut MastPackage,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        let key = cx.target_key();
        let lowered = self.lowered.borrow();
        let found = lowered.get(&key).ok_or_else(|| {
            Report::msg(format!(
                "internal error: cannot post-process target '{}' of package '{}': nothing was \
                 lowered for it, so `compile` never ran for this target",
                key.name(),
                key.package()
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

    /// This target's build provenance: its HIR, as the text it was written in.
    ///
    /// This is the answer that *matters*, because this is what the assembler asks for: the
    /// frontend provider forwards straight to here (`pipeline/provider.rs:205-210`), and what
    /// comes back is hashed to decide whether a cached build of this target is still current.
    ///
    /// So it is deliberately **not** the `mod.hir` placeholder
    /// [`extract_miden_component_or_bail`] fills in, which is a *constant*: answering with it
    /// would make every `.hir` target in every project hash alike, and a cached build of one
    /// could then be served for another. The legacy standalone chain never had to face this,
    /// because `AssembleStage` handed the assembler its sources directly and bypassed
    /// `assemble_source_package` — and therefore this callback — entirely.
    ///
    /// Computed from the same `HirSource` `compile` compiles, so the two cannot disagree.
    ///
    /// Not memoized: the whole cost is one read of one text file, which is the case
    /// [`Frontend::provenance`] exempts.
    ///
    /// # A standalone build never reaches this, so `HirSource::Stdin`'s arm is unexercised here
    ///
    /// A synthesized project has no manifest path, which makes its dependency-graph node a
    /// `ProjectSource::Virtual` — and `build_source_provenance` returns `None` for those without
    /// consulting any provider (`miden-assembly/src/project/dependency_graph.rs`, the
    /// `ProjectSource::Virtual` arm). A stdin-backed input is only ever a *standalone* one, so
    /// the `Stdin` arm of `HirSource::provenance` can only be reached from [`Frontend::compile`],
    /// never from here.
    ///
    /// Nothing is removed on that account: the unreachability is upstream's to keep, and a
    /// project whose package gained a manifest path would reach here at once. But the tests
    /// covering provenance for a stdin-backed target construct the call themselves, and are not
    /// end-to-end evidence that any command line produces it. What the paragraphs above say
    /// about *manifest-backed* `.hir` targets is unaffected: those are `Real` nodes, and this is
    /// reached for them.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        Self::read(cx)?.provenance()
    }
}

/// Interpret the operation `op` anchors as a [`MidenComponent`], or say why it cannot be one.
///
/// A `.hir` file may hold a whole world, a component, or a bare module, and each has to end up
/// as the same thing: a world for the backend to rewrite, and the component within it when
/// there is one. Anything else — a function on its own, a module under an operation that is
/// neither a world nor a component — is rejected here rather than failing further down, where
/// the report would name a pass instead of the input.
///
/// This lives with the `.hir` frontend because that is the only route that reaches it, whether
/// through [`HirFrontend::compile`].
pub fn extract_miden_component_or_bail(
    op: OperationRef,
    context: Rc<Context>,
) -> CompilerResult<MidenComponent> {
    // A placeholder, and the only honest one available here: this function is handed an
    // operation, not a source file, so it cannot know what the HIR was read from. Every caller
    // that *does* know overwrites it — `HirFrontend::parse` does, with the target's own
    // source — which is what keeps the assembler from hashing every
    // `.hir` target to the same provenance. A caller that leaves it as it is gets a constant.
    let source_provenance = ProjectSourceProvenanceInputs {
        root: SourceFileProvenance {
            path: std::path::PathBuf::from("mod.hir").into_boxed_path(),
            content: String::new().into_boxed_str(),
        },
        support: Default::default(),
    };

    if let Ok(world) = op.try_downcast_op::<builtin::World>() {
        Ok(MidenComponent {
            world,
            component: None,
            account_component_metadata_bytes: None,
            component_wit_bytes: None,
            source_provenance,
        })
    } else if let Ok(component) = op.try_downcast_op::<builtin::Component>() {
        let world = ensure_world_for_operation(op, context.clone())?;
        Ok(MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes: None,
            component_wit_bytes: None,
            source_provenance,
        })
    } else if let Ok(module) = op.try_downcast_op::<builtin::Module>() {
        if let Some(parent) = op.parent_op() {
            if let Ok(component) = parent.try_downcast_op::<builtin::Component>() {
                let world = ensure_world_for_operation(parent, context.clone())?;
                Ok(MidenComponent {
                    world,
                    component: Some(component),
                    account_component_metadata_bytes: None,
            component_wit_bytes: None,
                    source_provenance,
                })
            } else if let Ok(world) = parent.try_downcast_op::<builtin::World>() {
                Ok(MidenComponent {
                    world,
                    component: None,
                    account_component_metadata_bytes: None,
            component_wit_bytes: None,
                    source_provenance,
                })
            } else {
                Err(Report::msg(format!(
                    "cannot compile a module nested under '{}'",
                    parent.borrow().name(),
                )))
            }
        } else {
            let world = ensure_world_for_operation(module.as_operation_ref(), context.clone())?;
            Ok(MidenComponent {
                world,
                component: None,
                account_component_metadata_bytes: None,
            component_wit_bytes: None,
                source_provenance,
            })
        }
    } else {
        Err(Report::msg(format!("cannot compile a '{}' alone", op.borrow().name())))
    }
}

/// The world `op` hangs from, creating one and moving `op` into it if it hangs from nothing.
///
/// An operation that already has a parent must have a world for a parent: the alternative is a
/// component or module nested somewhere the compiler cannot lower it from, and there is nothing
/// to be done about that but say so.
fn ensure_world_for_operation(
    op: OperationRef,
    context: Rc<Context>,
) -> CompilerResult<builtin::WorldRef> {
    use midenc_hir::{
        BuilderExt, Op, Rewriter, SourceSpan,
        patterns::{NoopRewriterListener, RewriterImpl},
    };
    if let Some(parent) = op.parent_op() {
        parent.try_downcast_op::<builtin::World>().map_err(|op| {
            Report::msg(
                format!("cannot compile a component nested under '{}'", op.borrow().name(),),
            )
        })
    } else {
        let mut builder = RewriterImpl::<NoopRewriterListener>::new(context);
        let world: builtin::WorldRef = {
            let world_builder = builder.create::<builtin::World, ()>(SourceSpan::default());
            world_builder()?
        };
        let body = world.borrow().as_operation().region(0).entry().as_block_ref();
        builder.move_op_to_end(op, body);
        Ok(world)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use alloc::{
        format,
        rc::Rc,
        string::{String, ToString},
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;

    use midenc_hir::Context;
    use midenc_session::{
        InputFile, OutputFile, OutputType, OutputTypeSpec, OutputTypes, miden_project::TargetType,
    };

    use super::*;
    use crate::pipeline::{
        FrontendRegistry, Goal, Observer, Outcome, RecordingObserver, RequestState, TargetRole,
        testing::{self, VirtualProject},
    };

    // ---------------------------------------------------------------------------------------
    // Fixtures.
    // ---------------------------------------------------------------------------------------

    /// A bare `builtin.module`, the shape `hir-opt` fixtures are written in.
    ///
    /// Shared with the namespace pre-scan's tests in
    /// [`prepare`](crate::pipeline::prepare), which must agree with this frontend about what a
    /// `.hir` root declares — see `hir_declared_namespace`. One set of fixtures, so that
    /// the scan cannot be tested against text this frontend has never compiled.
    pub(crate) const MODULE: &str = r#"
builtin.module public @lib {
    builtin.function public extern("C") @main() {
        builtin.ret;
    };
};
"#;

    /// A second module, distinguishable from [`MODULE`] by the name of its function.
    const OTHER_MODULE: &str = r#"
builtin.module public @lib {
    builtin.function public extern("C") @other() {
        builtin.ret;
    };
};
"#;

    /// A whole world, component and all — the *shape* `--emit=hir` writes, though not its
    /// literal text; see below.
    ///
    /// The component's id is quoted because it is one symbol-path component, not three: the
    /// `:` and the `@` are part of the name, and `ComponentId::try_from` splits them back out
    /// itself.
    ///
    /// **And that quoting is why this is not verbatim `--emit=hir` output.** `OpPrinter for
    /// builtin::Component` emits the id *bare* — `@hir_ns:test@1.0.0` — which the parser then
    /// rejects with "invalid component id: missing namespace identifier". So a printed world
    /// holding a component does not re-parse today, and this fixture is hand-quoted to work
    /// around that. A world holding only modules, like [`MODULE`] wrapped in a world, does
    /// round-trip. The defect is a printer/parser disagreement in `builtin::Component`, not in
    /// the anchoring parser, and it is recorded as a `TODO(hir)` on that printer.
    ///
    /// Shared with the namespace pre-scan's tests, as [`MODULE`] is.
    pub(crate) const WORLD: &str = r#"
builtin.world {
    builtin.component private @"hir_ns:test@1.0.0" {
        builtin.module private @test {
            builtin.function public extern("C") @main() {
                builtin.ret;
            };
        };
    };
};
"#;

    /// A component on its own, without the enclosing world.
    ///
    /// Shared with the namespace pre-scan's tests, as [`MODULE`] is.
    pub(crate) const COMPONENT: &str = r#"
builtin.component private @"hir_ns:test@1.0.0" {
    builtin.module private @test {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
"#;

    /// A top-level operation that is neither a world, a component, nor a module.
    const BARE_FUNCTION: &str = r#"
builtin.function public extern("C") @main() {
    builtin.ret;
};
"#;

    /// A single-target library project named after `dir`, rooted at a `.hir` file.
    fn hir_project(dir: &str, contents: &str) -> VirtualProject {
        let root = testing::fixture_source(dir, "lib.hir", contents);
        VirtualProject::new(dir, &root, TargetType::Library).expect("should build project")
    }

    /// A default HIR context, which is also the source of a target's session.
    fn context() -> Rc<Context> {
        Rc::new(Context::default())
    }

    /// A context whose session emits `output_type` into `out_dir`.
    fn emitting_context(output_type: OutputType, out_dir: &std::path::Path) -> Rc<Context> {
        let output_types = OutputTypes::new([OutputTypeSpec::Typed {
            output_type,
            path: Some(OutputFile::Directory(out_dir.to_path_buf())),
        }])
        .expect("the output type is valid");
        testing::context_emitting(output_types, |_| {})
    }

    /// Every document with the `extension` extension in `dir`, sorted by file stem.
    fn documents(dir: &std::path::Path, extension: &str) -> Vec<(String, String)> {
        let mut documents = std::fs::read_dir(dir)
            .expect("the output directory should exist")
            .map(|entry| entry.expect("should read a directory entry").path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
            .map(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("a written document has a name")
                    .to_string();
                (stem, std::fs::read_to_string(&path).expect("should read the document"))
            })
            .collect::<Vec<_>>();
        documents.sort();
        documents
    }

    /// What one run of the frontend did.
    struct Run {
        stopped: bool,
        trace: Vec<CheckpointId>,
        captured: Option<Outcome>,
    }

    /// Run the HIR frontend over `project`'s selected target to `goal`.
    fn run(
        project: &VirtualProject,
        context: Rc<Context>,
        goal: Goal,
        input: Option<&InputFile>,
    ) -> Run {
        try_run(project, context, goal, input).expect("the hir frontend should compile")
    }

    /// [`run`], but handing back the frontend's own failure.
    fn try_run(
        project: &VirtualProject,
        context: Rc<Context>,
        goal: Goal,
        input: Option<&InputFile>,
    ) -> CompilerResult<Run> {
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(goal, vec![observer.clone() as Rc<RefCell<dyn Observer>>]);
        let cx = TargetContext::new(&assembly, context, input, TargetRole::Root, &state);

        let frontend = HIR_FRONTEND.instantiate(cx.session());
        let flow = frontend.compile(&cx)?;
        Ok(Run {
            stopped: flow.is_break(),
            trace: observer.borrow().records().iter().map(|(checkpoint, _)| *checkpoint).collect(),
            captured: state.take_outcome(),
        })
    }

    /// The checkpoints a run that is not asked to stop publishes, in order.
    fn full_trace() -> Vec<CheckpointId> {
        vec![
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_LOWERED,
        ]
    }

    /// HIR text as a stdin-backed compiler input.
    ///
    /// Deliberately not [`InputFile::from_bytes`]: `FileType::detect` recognizes HIR only by
    /// the legacy `(module #` prefix, so it cannot classify text written in the syntax the
    /// parser accepts. Detecting the modern syntax belongs to `midenc-session`, not here; what
    /// this frontend has to handle is an input that *is* HIR and has no path.
    fn piped(text: &str) -> InputFile {
        InputFile::new(
            FileType::Hir,
            InputType::Stdin {
                name: "stdin.hir".into(),
                input: text.as_bytes().to_vec(),
            },
        )
    }

    /// The component published at `hir.initial` for `project`.
    ///
    /// `context` is borrowed rather than taken, because a [`MidenComponent`] is a handle into
    /// the [`Context`]'s arena: dropping the last reference to it while the component is still
    /// in hand frees the IR the caller is about to read.
    fn parsed_component(
        project: &VirtualProject,
        context: &Rc<Context>,
        input: Option<&InputFile>,
    ) -> MidenComponent {
        run(project, context.clone(), Goal::at(CheckpointId::HIR_INITIAL), input)
            .captured
            .expect("stopping at hir.initial must capture the parsed HIR")
            .downcast::<MidenComponent>()
            .expect("the initial artifact must be the parsed Miden component")
    }

    /// The HIR `component` holds, as text.
    fn rendered(component: &MidenComponent) -> String {
        use midenc_hir::Op;

        format!("{}", component.world.borrow().as_operation())
    }

    // ---------------------------------------------------------------------------------------
    // The route.
    // ---------------------------------------------------------------------------------------

    /// A `.hir` target reaches every checkpoint on the route, in order.
    #[test]
    fn a_hir_target_reaches_every_checkpoint_in_order() {
        let project = hir_project("hir_frontend_route", MODULE);
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None);

        assert!(!run.stopped, "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            run.trace,
            full_trace(),
            "the frontend publishes the parsed HIR, and the shared backend the three that follow"
        );
        assert!(run.captured.is_none(), "nothing may be captured before the goal is reached");
    }

    /// And so does a `.hir` target written as a whole `builtin.world`.
    ///
    /// The world-shaped input is what makes the `.hir` route useful for exercising rewrites and
    /// lowering, because a whole `builtin.world` is what `--emit=hir` writes. It used to panic
    /// before it reached any of that: the parser anchored every top-level operation at a
    /// `builtin.world` it created, *including* one that was already a world, and `Symbol::path`
    /// asserts that an anonymous symbol table — which a world is — has no parent, so the first
    /// module path taken inside the inner world panicked with "anonymous symbol tables cannot
    /// have parents" somewhere in codegen. Reaching `masm.lowered` at all is the assertion; the
    /// trace pins that it got there the same way a module does.
    ///
    /// # This is not yet a full `--emit=hir` round trip, and the fixture is not verbatim output
    ///
    /// Be precise about what is and is not pinned here. A world of **modules** does round-trip:
    /// `--emit=hir` output for one re-parses and recompiles unchanged. A world holding a
    /// **component** — which is what [`WORLD`] is — does not, for a *second*, unrelated defect
    /// that this test does not cover and that is not in the parser: a `builtin.component`'s id
    /// **prints unquoted** but must be **written quoted**, so `--emit=hir` emits
    /// `@hir_ns:test@1.0.0` where the parser demands `@"hir_ns:test@1.0.0"` and rejects the
    /// former with "invalid component id: missing namespace identifier". [`WORLD`] is hand-quoted
    /// for that reason and is therefore *not* the literal text `--emit=hir` produces for the
    /// component it describes. Until that is fixed, this test pins "a world-shaped `.hir` file
    /// compiles", not "`midenc`'s own output for a component recompiles".
    #[test]
    fn a_whole_world_hir_target_reaches_every_checkpoint_in_order() {
        let project = hir_project("hir_frontend_world_route", WORLD);
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None);

        assert!(!run.stopped, "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            run.trace,
            full_trace(),
            "a world-shaped .hir file takes the same route a module-shaped one does"
        );
    }

    /// Stopping at `hir.initial` captures the parsed component, and nothing past it runs.
    #[test]
    fn stopping_at_hir_initial_captures_the_parsed_component() {
        let project = hir_project("hir_frontend_initial", MODULE);
        let run = run(&project, context(), Goal::at(CheckpointId::HIR_INITIAL), None);

        assert!(run.stopped, "the goal is on this frontend's route, so it must stop there");
        assert_eq!(
            run.trace,
            vec![CheckpointId::HIR_INITIAL],
            "nothing past the goal may run: the backend's checkpoints must be absent"
        );

        let captured = run.captured.expect("stopping at the goal must capture an artifact");
        assert_eq!(captured.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(captured.artifact().id(), ArtifactId::HIR);
        captured
            .downcast::<MidenComponent>()
            .expect("the initial artifact must be the parsed Miden component");
    }

    // ---------------------------------------------------------------------------------------
    // What the HIR is extracted into.
    // ---------------------------------------------------------------------------------------

    /// A bare module arrives wrapped in a world, with the module's own contents inside it.
    #[test]
    fn a_bare_module_is_wrapped_in_a_world() {
        let context = context();
        let project = hir_project("hir_frontend_bare_module", MODULE);
        let component = parsed_component(&project, &context, None);

        assert!(
            component.component.is_none(),
            "a bare module declares no component, so none may be invented for it"
        );
        let rendered = rendered(&component);
        assert!(
            rendered.starts_with("builtin.world"),
            "the module must be anchored at a world: {rendered}"
        );
        assert!(
            rendered.contains("builtin.module public @lib"),
            "and the world must hold the module that was parsed, not an empty one: {rendered}"
        );
    }

    /// A whole world is taken as the anchor, and its component stays where it was written.
    ///
    /// `component` is `None` here even though the world declares one — the extraction names a
    /// component only when the *top-level* operation is one, and code generation lowers the
    /// world in that case. This pins the shape rather than endorsing it; it is what the legacy
    /// `.hir` chain produced too.
    #[test]
    fn a_whole_world_is_taken_as_the_anchor() {
        let context = context();
        let project = hir_project("hir_frontend_world", WORLD);
        let component = parsed_component(&project, &context, None);

        assert!(component.component.is_none());
        let rendered = rendered(&component);
        assert!(
            rendered.starts_with("builtin.world"),
            "the parsed world is the anchor, not a world wrapped around it: {rendered}"
        );
        assert!(
            rendered.contains("builtin.component"),
            "and its component must survive: {rendered}"
        );
    }

    /// A component written on its own is named, and anchored at the world the parser built.
    #[test]
    fn a_bare_component_is_named_and_anchored() {
        let context = context();
        let project = hir_project("hir_frontend_component", COMPONENT);
        let component = parsed_component(&project, &context, None);

        assert!(
            component.component.is_some(),
            "a top-level component must be carried through, not merely left inside the world"
        );
        let rendered = rendered(&component);
        assert!(
            rendered.starts_with("builtin.world"),
            "and it must still be anchored at a world for the backend to rewrite: {rendered}"
        );
    }

    /// A module under an operation that can hold neither is rejected, by name.
    ///
    /// Reached by handing the *inner* module to the extraction directly, because no `.hir` file
    /// can reach it through [`HirFrontend::compile`]: the parser anchors whatever it parses at a
    /// world it creates, so the top-level operation's parent is always a world. That is also why
    /// this asserts on the extraction rather than on a compilation.
    #[test]
    fn a_module_nested_under_an_unsupported_operation_is_rejected() {
        const NESTED: &str = r#"
builtin.module public @outer {
    builtin.module public @inner {
        builtin.function public extern("C") @main() {
            builtin.ret;
        };
    };
};
"#;
        let context = context();
        let outer = HirSource::Stdin {
            name: "nested.hir".into(),
            text: NESTED.to_string(),
        }
        .parse_operation(context.clone())
        .expect("a module holding a module must parse, or this test proves nothing");

        let inner = outer
            .borrow()
            .region(0)
            .entry()
            .body()
            .front()
            .as_pointer()
            .expect("the outer module must hold the inner one");

        let err = extract_miden_component_or_bail(inner, context)
            .err()
            .expect("a module under a module is not something the compiler can lower");
        assert_eq!(
            format!("{err}"),
            "cannot compile a module nested under 'builtin.module'",
            "the report must name the operation the module was found under"
        );
    }

    /// An operation that is neither a world, a component, nor a module is rejected.
    #[test]
    fn a_top_level_operation_that_is_not_compilable_is_reported() {
        let project = hir_project("hir_frontend_bare_function", BARE_FUNCTION);
        let err = try_run(&project, context(), Goal::at(CheckpointId::HIR_INITIAL), None)
            .err()
            .expect("a bare function is not something this frontend can compile");

        let msg = format!("{err}");
        assert!(
            msg.contains("cannot compile a 'builtin.function' alone"),
            "the report must name the operation it was handed: {msg}"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Where the source comes from.
    // ---------------------------------------------------------------------------------------

    /// A stdin-backed input is compiled from its bytes, not from the target root's path.
    ///
    /// The fixture on disk holds a *different* module, so a frontend that read the resolved
    /// target root would compile the wrong program rather than merely fail to find a file.
    #[test]
    fn a_stdin_backed_input_is_compiled_from_its_own_bytes() {
        let context = context();
        let project = hir_project("hir_frontend_stdin", OTHER_MODULE);
        let input = piped(MODULE);

        let from_stdin = rendered(&parsed_component(&project, &context, Some(&input)));
        assert!(
            from_stdin.contains("@main") && !from_stdin.contains("@other"),
            "the text piped in is the one compiled; a stdin input has no path to read back: \
             {from_stdin}"
        );

        let from_disk = rendered(&parsed_component(&project, &context, None));
        assert!(
            from_disk.contains("@other"),
            "and the fixture on disk really is a different module, or this proves nothing: \
             {from_disk}"
        );
    }

    /// A target root that is not HIR is reported.
    #[test]
    fn a_target_root_that_is_not_hir_is_reported() {
        let root = testing::fixture_source("hir_frontend_bad_root", "lib.masm", "begin end");
        let project = VirtualProject::new("hir_frontend_bad_root", &root, TargetType::Library)
            .expect("should build project");

        let err = try_run(&project, context(), Goal::at(CheckpointId::HIR_INITIAL), None)
            .err()
            .expect("a `.masm` root is not this frontend's to compile");

        let msg = format!("{err}");
        assert!(
            msg.contains("expected '.hir'"),
            "the report must say which file type this frontend handles: {msg}"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Provenance.
    // ---------------------------------------------------------------------------------------

    /// Provenance reports the target's own HIR file, not the `mod.hir` placeholder.
    #[test]
    fn provenance_reports_the_targets_own_source() {
        let project = hir_project("hir_frontend_provenance", MODULE);
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::HIR_INITIAL),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = HIR_FRONTEND.instantiate(cx.session());
        let provenance = frontend.provenance(&cx).expect("provenance should succeed");

        assert_eq!(&*provenance.root.content, MODULE, "the content is the target's own HIR text");
        assert_eq!(
            provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("lib.hir"),
            "and it is attributed to the file the target is rooted at, not to 'mod.hir'"
        );
        assert!(provenance.support.is_empty(), "a HIR target has a single source");
        assert!(
            observer.borrow().records().is_empty(),
            "provenance must not publish checkpoints, even at the goal"
        );
    }

    /// And what `compile` publishes carries that same provenance.
    #[test]
    fn the_compiled_component_carries_the_same_provenance() {
        let context = context();
        let project = hir_project("hir_frontend_provenance_agrees", MODULE);
        let component = parsed_component(&project, &context, None);

        assert_eq!(
            &*component.source_provenance.root.content, MODULE,
            "a build must not fall back to the empty placeholder"
        );
        assert_eq!(
            component.source_provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("lib.hir"),
        );
    }

    /// The assembler's provenance callback answers from the piped-in text too.
    ///
    /// [`Frontend::provenance`] is the entry point the assembler actually calls, and the
    /// stdin case is the one the [`HirSource`] enum exists for — a target root that names a
    /// file nobody ever wrote. The fixture on disk holds a *different* module, and the
    /// assertion is that none of it comes back.
    #[test]
    fn provenance_of_a_stdin_backed_input_is_the_piped_text() {
        let project = hir_project("hir_frontend_stdin_provenance_callback", OTHER_MODULE);
        let assembly = project.assembly_context().expect("assembly context");
        let input = piped(MODULE);
        let state = RequestState::new(Goal::at(CheckpointId::HIR_INITIAL), Vec::new());
        let cx = TargetContext::new(&assembly, context(), Some(&input), TargetRole::Root, &state);

        let frontend = HIR_FRONTEND.instantiate(cx.session());
        let provenance = frontend.provenance(&cx).expect("provenance should succeed");

        assert_eq!(
            &*provenance.root.content, MODULE,
            "a stdin input has no path to read back, so its provenance is its own bytes"
        );
        assert_eq!(
            provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("stdin.hir"),
            "and it is attributed to the name the input was given, not to the target root"
        );
        assert!(provenance.support.is_empty());
    }

    /// A stdin-backed input's provenance is the text that was piped in.
    #[test]
    fn a_stdin_backed_inputs_provenance_is_its_own_text() {
        let context = context();
        let project = hir_project("hir_frontend_stdin_provenance", OTHER_MODULE);
        let input = piped(MODULE);
        let component = parsed_component(&project, &context, Some(&input));

        assert_eq!(
            &*component.source_provenance.root.content, MODULE,
            "the provenance of a stdin build is the text it was built from"
        );
        assert_eq!(
            component.source_provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("stdin.hir"),
        );
    }

    // ---------------------------------------------------------------------------------------
    // Post-processing.
    // ---------------------------------------------------------------------------------------

    /// A target this frontend compiled is served from the stash, and only that target is.
    ///
    /// The *hit* path, which nothing else pins. `post_process` runs against a [`TargetContext`]
    /// the assembler builds afresh, so the map keyed by [`TargetContext::target_key`] is the
    /// only route from `compile` to here; a frontend that stashed nothing, or keyed it wrongly,
    /// reports the miss instead. The second half is what makes the first discriminate: one
    /// frontend instance, two targets, and only the compiled one is found — so this cannot pass
    /// by `post_process` succeeding unconditionally.
    ///
    /// # What this deliberately does not assert
    ///
    /// That the `Arc<MasmComponent>`'s rodata reaches the package's advice map — the silent
    /// miscompile [`LoweredTarget`] exists to prevent. It is not observable *on this route*:
    /// rodata cannot be written in HIR text at all, because the parser has no attribute parser
    /// for `builtin.bytes` (`hir/src/ir/parse/parser.rs:1534` panics with "not yet
    /// implemented"), and the `.hir` route never produces account-component metadata either
    /// (`extract_miden_component_or_bail` sets it to `None` on every arm). The link from a
    /// stashed component to the assembled package is pinned instead by
    /// `backend::tests::a_frontend_that_lowers_through_the_backend_assembles_a_complete_package`,
    /// over a frontend with the same destructure-and-stash shape as this one. What is left for
    /// this test is the lookup, and that is what it asserts.
    #[test]
    fn post_processing_a_compiled_target_is_served_from_the_stash() {
        let context = context();
        let project = hir_project("hir_frontend_post_process_hit", MODULE);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let cx = TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &state);

        let frontend = HIR_FRONTEND.instantiate(cx.session());
        let _ = frontend.compile(&cx).expect("the fixture should lower");

        // A *second* context, as the assembler builds for this callback: nothing carries over
        // from the run above except the frontend itself.
        let fresh_state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let fresh =
            TargetContext::for_testing(&assembly, context.clone(), TargetRole::Root, &fresh_state);
        frontend
            .post_process(&mut any_package(), &fresh)
            .expect("a target this frontend compiled must be found, not reported missing");

        // And a target the same frontend did *not* compile still misses, so the success above
        // is the stash answering rather than `post_process` succeeding for anything.
        let other = hir_project("hir_frontend_post_process_hit_other", OTHER_MODULE);
        let other_assembly = other.assembly_context().expect("assembly context");
        let other_state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let other_cx =
            TargetContext::for_testing(&other_assembly, context, TargetRole::Root, &other_state);
        assert_ne!(
            fresh.target_key(),
            other_cx.target_key(),
            "the two targets must be distinguishable, or the assertion below is vacuous"
        );
        let err = frontend
            .post_process(&mut any_package(), &other_cx)
            .expect_err("nothing was compiled for the second target");
        assert!(format!("{err}").contains("internal error"));
    }

    /// A well-formed package to hand to `post_process`.
    ///
    /// A package must export at least one procedure to be constructible, so rather than
    /// hand-building a MAST forest this borrows the compiler's own intrinsics package.
    fn any_package() -> MastPackage {
        (*midenc_codegen_masm::intrinsics::load()).clone()
    }

    /// Post-processing a target this frontend never compiled is an invariant violation.
    #[test]
    fn post_processing_a_target_that_was_never_compiled_is_an_invariant_error() {
        let project = hir_project("hir_frontend_post_process", MODULE);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = HIR_FRONTEND.instantiate(cx.session());
        let mut package = (*midenc_codegen_masm::intrinsics::load()).clone();
        let err = frontend
            .post_process(&mut package, &cx)
            .expect_err("nothing was compiled for this target");

        let msg = format!("{err}");
        assert!(msg.contains("internal error"), "a miss here is a compiler bug: {msg}");
        assert!(
            msg.contains(&**cx.target_key().name()),
            "and the report must name the target it looked for: {msg}"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Stop policy.
    // ---------------------------------------------------------------------------------------

    /// `-Canalyze-only` does not stop a `.hir` build.
    #[test]
    fn analyze_only_does_not_stop_this_route() {
        let project = hir_project("hir_frontend_analyze_only", MODULE);
        let context = testing::context_emitting(Default::default(), |options| {
            options.analyze_only = true;
        });

        let run = run(&project, context, Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None);

        assert_eq!(
            run.trace,
            full_trace(),
            "how far a build runs is the request's goal; the frontend holds no stop policy"
        );
    }

    /// `-Zlint` errors do fail a `.hir` build, which is new on this route.
    #[test]
    fn lint_errors_fail_this_route() {
        use midenc_session::diagnostics::Severity;

        let project = hir_project("hir_frontend_lint", MODULE);
        let context = testing::context_emitting(Default::default(), |options| options.lint = true);
        context
            .session()
            .diagnostics
            .diagnostic(Severity::Error)
            .with_message("planted lint finding")
            .emit();

        let err = try_run(&project, context, Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None)
            .err()
            .expect("a lint that raised errors must fail the compilation");
        let msg = format!("{err}");
        assert!(msg.contains("lint"), "the message must name the check that failed: {msg}");
    }

    // ---------------------------------------------------------------------------------------
    // Emission.
    // ---------------------------------------------------------------------------------------

    /// `--emit=hir` is written inline by the parse step, without a renderer.
    #[test]
    fn the_parsed_hir_document_is_written_without_a_renderer() {
        let project = hir_project("hir_frontend_emit_hir", MODULE);
        let out_dir = testing::fixture_dir("hir_frontend_emit_hir_out");
        let context = emitting_context(OutputType::Hir, &out_dir);

        run(&project, context, Goal::at(CheckpointId::HIR_INITIAL), None);

        assert_eq!(
            documents(&out_dir, "hir").len(),
            1,
            "the parse step writes the pre-rewrite hir document itself"
        );
    }

    /// This route names the HIR emission exactly once.
    ///
    /// The half a document count cannot see. `Emit` for a `String` reports no name, so both HIR
    /// emissions in a session resolve to one destination and `write_to_file` truncates it — a
    /// document written twice is byte for byte a document written once. The doubling is only
    /// *visible* when the destination is stdout, which `--emit=hir=-` selects and which no
    /// in-process test can capture, and it is what the spec's Risks section names as the way
    /// this increment could break `--emit=hir`.
    ///
    /// So the invariant is asserted where it is decidable. The post-rewrite document belongs to
    /// [`backend::apply_rewrites`], which [`backend::hir_to_masm`] reaches on this route's
    /// behalf; a copy here would print it twice.
    #[test]
    fn this_route_names_the_hir_emission_once() {
        let source = include_str!("hir.rs");
        // Spelled in two pieces so that this needle is not itself an occurrence: the file being
        // searched is the one this test lives in.
        let calls = source.matches(concat!("emit_hir_if_requested", "(")).count();
        assert_eq!(
            calls, 1,
            "the only HIR emission this frontend owns is the parsed operation; the post-rewrite \
             document is `pipeline::backend::apply_rewrites`'s"
        );
    }

    /// And none of the route's declared renderers writes anything of its own.
    #[test]
    fn every_declaration_on_this_route_is_unrendered() {
        let out_dir = testing::fixture_dir("hir_frontend_unrendered_out");
        for (checkpoint, output_type, extension) in [
            (CheckpointId::HIR_INITIAL, OutputType::Hir, "hir"),
            (CheckpointId::HIR_ANALYZED, OutputType::Hir, "hir"),
            (CheckpointId::HIR_TRANSFORMED, OutputType::Hir, "hir"),
            (CheckpointId::MASM_LOWERED, OutputType::Masm, "masm"),
            (CheckpointId::PACKAGE_ASSEMBLED, OutputType::Masp, "masp"),
        ] {
            let context = emitting_context(output_type, &out_dir);
            let decl = HIR_FRONTEND
                .decl_at(checkpoint)
                .expect("every route checkpoint declares an artifact");
            let artifact = Artifact::new(decl.id, String::from("unused"));

            (decl.render)(&artifact, context.session())
                .unwrap_or_else(|err| panic!("rendering {checkpoint} must succeed: {err}"));

            assert!(
                documents(&out_dir, extension).is_empty(),
                "{checkpoint} must write nothing: its artifact is written inline"
            );
        }
    }

    // ---------------------------------------------------------------------------------------
    // The registration.
    // ---------------------------------------------------------------------------------------

    /// The registration is well-formed, and dispatches from its extension.
    #[test]
    fn the_registration_is_accepted_by_the_registry() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(HIR_FRONTEND)
            .expect("the hir registration must be well-formed");

        let found = registry.for_extension("hir").expect("dispatch is by target-root extension");
        assert_eq!(found.id(), FrontendId::new("hir"));
        assert_eq!(found.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_eq!(found.resolve_alias("parse"), Some(CheckpointId::HIR_INITIAL));
        assert_eq!(found.resolve_alias("analyze"), Some(CheckpointId::HIR_ANALYZED));
        assert_eq!(found.resolve_alias("transform"), Some(CheckpointId::HIR_TRANSFORMED));
        assert_eq!(found.resolve_alias("lower"), Some(CheckpointId::MASM_LOWERED));
        assert_eq!(found.resolve_alias("assemble"), Some(CheckpointId::PACKAGE_ASSEMBLED));
        assert_eq!(found.artifact_at(CheckpointId::HIR_INITIAL), Some(ArtifactId::HIR));
        assert_eq!(found.artifact_at(CheckpointId::MASM_LOWERED), Some(ArtifactId::MASM));
    }
}
