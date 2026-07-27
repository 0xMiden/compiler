//! The frontend for targets rooted at WebAssembly.

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
use midenc_frontend_wasm::{FrontendOutput, WasmTranslationConfig, WatEmit, wasm_to_wat};
use midenc_session::{
    FileType, InputType, OutputMode, OutputType, Session,
    diagnostics::{IntoDiagnostic, Report, WrapErr},
};

use crate::{
    CodegenOutput, CompilerResult, MidenComponent,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, TargetContext, TargetKey,
        backend::{self, LoweredTarget},
    },
};

/// Declares the frontend that handles targets rooted at a `.wasm` or `.wat` file.
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
pub const WASM_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("wasm"),
    &["wasm", "wat"],
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
        ("analyze", CheckpointId::HIR_ANALYZED),
        ("transform", CheckpointId::HIR_TRANSFORMED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        // `--emit=wat` is written by [`WasmFrontend::emit_wat`], which runs as this checkpoint
        // is reached. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::WASM_PARSED,
            id: ArtifactId::WASM,
            render: unrendered,
        },
        // The *pre*-rewrite `--emit=hir` document is written by
        // [`WasmFrontend::translate`], which calls `crate::emit_hir_if_requested` on the
        // component it just translated. See [`unrendered`].
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
    make_wasm,
);

/// Build the frontend this registration declares.
fn make_wasm(_session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(WasmFrontend::default())
}

/// Lower the WebAssembly `input` to Miden Assembly, without an assembler.
///
/// This runs exactly the phases [`WasmFrontend::compile_wasm`] runs — decode, `--emit=wat`,
/// translate, analyze, rewrite, lower — and writes the same inline `--emit` documents, by
/// calling the same functions. What it does *not* have is a [`TargetContext`]: no checkpoint is
/// published, nothing can stop early, and the result is handed straight back as a
/// [`CodegenOutput`] instead of being stashed for
/// [`Frontend::post_process`](crate::pipeline::Frontend::post_process).
///
/// # Why that shape, rather than reusing the frontend
///
/// The one caller is
/// [`compile_manifest`](super::rust::compile_manifest), which builds a *source dependency* of
/// some other project by running `cargo` in this process. There is no assembler to build a
/// [`TargetContext`] from — the dependency is not a target of the project being assembled — and
/// what the caller needs back is the lowered component, its rodata and its account-component
/// metadata, which is precisely a [`CodegenOutput`] and precisely what `compile_wasm` keeps to
/// itself.
///
/// This is where the legacy `stages::wasm_codegen_pipeline` chain ended up. It is not a second
/// copy of the WebAssembly route: every phase below is the frontend's own or the shared
/// backend's, so a change to either reaches both. What the chain had and this does not is the
/// four `-C*-only` stops, which are stop policy and belong to the request's goal — and a nested
/// dependency build never sets them, because `cargo::cargo_build` constructs its nested
/// `Options` from scratch rather than inheriting the parent's.
pub(crate) fn lower_wasm_input(
    input: &midenc_session::InputFile,
    context: Rc<midenc_hir::Context>,
) -> CompilerResult<CodegenOutput> {
    let hir = translate_wasm_input(input, context.clone())?;
    let hir = backend::analyze(hir, context.clone())?;
    backend::apply_rewrites(hir.world.as_operation_ref(), context.clone())?;
    backend::codegen(hir, context)
}

/// Translate the WebAssembly `input` into HIR built in `context`, without an assembler.
///
/// The front half of [`lower_wasm_input`], and everything
/// [`WasmFrontend::compile_wasm`] does before it hands off to the shared backend: read the
/// bytes, write `--emit=wat`, record the provenance, translate, write the pre-rewrite
/// `--emit=hir`. Separated from [`lower_wasm_input`] so that the translation is reachable
/// without the backend, which is what a caller holding WebAssembly and no assembler needs.
pub(crate) fn translate_wasm_input(
    input: &midenc_session::InputFile,
    context: Rc<midenc_hir::Context>,
) -> CompilerResult<MidenComponent> {
    let source = WasmFrontend::read_input(input)?;
    WasmFrontend::emit_wat(&source.wasm, &context.session_rc())?;

    // Computed rather than memoized: there is no target to key a cache by, and a caller with no
    // assembler asks for this once.
    let provenance = WasmProvenance {
        path: source.path.clone(),
        wat: WasmFrontend::to_wat(&source.wasm)?.into_boxed_str(),
    }
    .to_inputs();

    WasmFrontend::translate(context, &source, provenance)
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

/// Compiles a target whose root is a WebAssembly module or component.
///
/// This is `ParseWasmStage` re-homed — decode, translate, publish — with the phases past
/// translation delegated to [`backend::hir_to_masm`], which every HIR-producing frontend shares.
///
/// # No stop policy lives here
///
/// The legacy stage consulted `parse_only` and `link_only` and raised
/// [`CompilerStopped`](crate::CompilerStopped) for each. Neither check came along: how far a
/// build runs is the request's goal, which [`TargetContext::checkpoint`] answers at every one of
/// the checkpoints below. Both flags now resolve to a goal at `hir.initial` — see
/// [`StopFlag`](crate::pipeline::StopFlag) — so re-expressing them here would give a build two
/// places to stop that could disagree.
///
/// # Per-target state, and why there are two maps
///
/// [`Frontend::post_process`] is called with a *fresh* [`TargetContext`], so what
/// [`backend::hir_to_masm`] produced has to survive in the frontend, keyed by
/// [`TargetContext::target_key`] — the same arrangement
/// [`RustProjectFrontend`](super::rust::RustProjectFrontend) uses for its cargo builds.
///
/// Provenance is memoized separately because it is reached separately: the assembler asks for it
/// repeatedly while hashing the dependency closure, and may ask *before* `compile` has run at
/// all. Answering costs a read of the target's sources and a full disassembly back to
/// WebAssembly text, which is precisely the cost [`Frontend::provenance`] says an implementation
/// must not pay twice. Folding it into the other map would mean either compiling to answer a
/// provenance question, or storing a half-populated entry.
#[derive(Default)]
pub struct WasmFrontend {
    /// The provenance of each target's WebAssembly.
    provenance: RefCell<BTreeMap<TargetKey, WasmProvenance>>,
    /// What codegen produced for each target, kept for [`Frontend::post_process`].
    lowered: RefCell<BTreeMap<TargetKey, CodegenOutput>>,
}

/// One target's build provenance, in the form this frontend memoizes it.
///
/// The two fields rather than a [`ProjectSourceProvenanceInputs`], which is neither `Clone` nor
/// copyable field by field without restating its shape at every call site.
struct WasmProvenance {
    /// The path the WebAssembly was read from.
    path: Box<Path>,
    /// The WebAssembly text the bytes disassemble to.
    wat: Box<str>,
}

impl WasmProvenance {
    /// This provenance, in the shape the assembler asks for.
    fn to_inputs(&self) -> ProjectSourceProvenanceInputs {
        ProjectSourceProvenanceInputs {
            root: SourceFileProvenance {
                path: self.path.clone(),
                content: self.wat.clone(),
            },
            support: Vec::new(),
        }
    }
}

/// One target's WebAssembly, as this frontend reads it.
///
/// Visible to the sibling frontends because
/// [`RustStandaloneFrontend`](super::rust::RustStandaloneFrontend) *produces* its WebAssembly
/// rather than reading it, and then hands it to [`WasmFrontend::compile_wasm`] to finish.
pub(super) struct WasmSource {
    /// The path the bytes are attributed to.
    ///
    /// For a stdin-backed input this names no file on disk; it is still what the module is
    /// named after and what its provenance records, exactly as the legacy stage did.
    path: Box<Path>,
    /// The WebAssembly binary, decoded from text if the source was `.wat`.
    wasm: Vec<u8>,
}

impl WasmSource {
    /// The WebAssembly `wasm`, attributed to `path`.
    ///
    /// The path is not merely provenance: it is what the translated module is *named* after,
    /// so a caller that compiled its WebAssembly from some other language passes the path of
    /// the module it produced, which is what the legacy `.rs → .wasm → HIR` chain named it
    /// after too.
    pub(super) fn new(path: Box<Path>, wasm: Vec<u8>) -> Self {
        Self { path, wasm }
    }
}

impl WasmFrontend {
    /// Read this target's WebAssembly, decoding WebAssembly text on the way if need be.
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
    /// bytes. That combination is expected to be unreachable (a synthesized project has one
    /// target and a registry-only dependency; see
    /// [`FrontendProvider::with_input`](crate::pipeline::FrontendProvider::with_input)), and
    /// reading each target's own root means it does not have to be.
    fn read(cx: &TargetContext<'_>) -> CompilerResult<WasmSource> {
        match cx.input().map(|input| (input.file_type(), &input.file)) {
            Some((file_type, InputType::Stdin { name, input })) => Ok(WasmSource {
                path: name.as_path().to_path_buf().into_boxed_path(),
                wasm: Self::decode_bytes(input, file_type, name.as_path())?,
            }),
            _ => {
                let path = cx.assembly().resolved_target_root.clone();
                let file_type = FileType::try_from(path.as_ref()).map_err(|err| {
                    Report::msg(format!("invalid target root '{}': {err}", path.display()))
                })?;
                Ok(WasmSource {
                    wasm: Self::read_file(&path, file_type)?,
                    path,
                })
            }
        }
    }

    /// Read the WebAssembly `input` names, decoding WebAssembly text on the way if need be.
    ///
    /// The counterpart of [`WasmFrontend::read`] for a caller that holds an [`InputFile`] and no
    /// target: it dispatches on the input's own shape rather than on a resolved target root, and
    /// so has no root to prefer. See [`lower_wasm_input`], its only caller.
    fn read_input(input: &midenc_session::InputFile) -> CompilerResult<WasmSource> {
        let file_type = input.file_type();
        match &input.file {
            InputType::Real(path) => Ok(WasmSource {
                wasm: Self::read_file(path, file_type)?,
                path: path.clone().into_boxed_path(),
            }),
            InputType::Stdin { name, input } => Ok(WasmSource {
                path: name.as_path().to_path_buf().into_boxed_path(),
                wasm: Self::decode_bytes(input, file_type, name.as_path())?,
            }),
        }
    }

    /// The WebAssembly binary at `path`, decoded from text when `file_type` says it is `.wat`.
    fn read_file(path: &Path, file_type: FileType) -> CompilerResult<Vec<u8>> {
        match file_type {
            FileType::Wat => wat::parse_file(path)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to parse wat from '{}'", path.display())),
            FileType::Wasm => std::fs::read(path)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to read wasm input from '{}'", path.display())),
            file_type => Err(Self::unsupported(file_type, path)),
        }
    }

    /// The same, for bytes that are already in hand.
    fn decode_bytes(bytes: &[u8], file_type: FileType, name: &Path) -> CompilerResult<Vec<u8>> {
        match file_type {
            FileType::Wat => wat::parse_bytes(bytes)
                .map(|wasm| wasm.into_owned())
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to parse wat from '{}'", name.display())),
            FileType::Wasm => Ok(bytes.to_vec()),
            file_type => Err(Self::unsupported(file_type, name)),
        }
    }

    /// The report for a target this frontend was asked to compile but cannot.
    ///
    /// Reachable only through a registry that dispatched some other extension here, or a target
    /// root whose extension and content disagree, so it names both what it got and what it
    /// handles.
    fn unsupported(file_type: FileType, path: &Path) -> Report {
        Report::msg(format!(
            "invalid input file '{}': expected '.wasm' or '.wat', got {file_type}",
            path.display()
        ))
    }

    /// Write the `--emit=wat` document for `wasm`, if one was asked for.
    ///
    /// This is `ParseWasmStage::emit_wat_for_wasm_input` re-homed, and it is the writer
    /// `wasm.parsed` declares. It runs for a `.wasm` input as well as a `.wat` one — the point
    /// of the output is to show what the compiler is about to translate, which for a binary
    /// input is the only way to read it at all.
    fn emit_wat(wasm: &[u8], session: &Session) -> CompilerResult<()> {
        // A cost guard, not the gate: [`Session::emit`] consults `should_emit` itself, so this
        // decides nothing about *whether* the document is written — it decides whether the
        // module is disassembled to produce one nobody asked for. The legacy stage checked here
        // for the same reason.
        if !session.should_emit(OutputType::Wat) {
            return Ok(());
        }
        let wat = Self::to_wat(wasm)?;
        session
            .emit(OutputMode::Text, &WatEmit(&wat))
            .into_diagnostic()
            .wrap_err("failed to emit wat output")
    }

    /// Disassemble `wasm` back to WebAssembly text.
    fn to_wat(wasm: &[u8]) -> CompilerResult<String> {
        wasm_to_wat(wasm).into_diagnostic().wrap_err("failed to convert wasm to wat")
    }

    /// The provenance of `source`, computing it only if this target has none yet.
    fn provenance_of(
        &self,
        cx: &TargetContext<'_>,
        source: &WasmSource,
    ) -> CompilerResult<ProjectSourceProvenanceInputs> {
        let key = cx.target_key();
        if let Some(found) = self.provenance.borrow().get(&key) {
            return Ok(found.to_inputs());
        }

        let provenance = WasmProvenance {
            path: source.path.clone(),
            wat: Self::to_wat(&source.wasm)?.into_boxed_str(),
        };
        let inputs = provenance.to_inputs();
        self.provenance.borrow_mut().insert(key, provenance);
        Ok(inputs)
    }

    /// Translate `source` to HIR, writing the pre-rewrite `--emit=hir` document on the way.
    ///
    /// The module is named after the source's file stem, which is what the legacy stage passed
    /// and therefore what every fixture's HIR is named after today. Note that it names the
    /// *module*: the enclosing component's id is the frontend's own, so this does not decide
    /// the namespace the target is lowered into.
    ///
    /// `noname` is the fallback for a root with no file stem, or one that is not valid UTF-8 —
    /// which the legacy stage handled by unwrapping, i.e. by panicking. It is not invented here:
    /// it is what [`WasmTranslationConfig::default`] already uses for a source it cannot name,
    /// so an unnameable root is translated exactly as an unnamed one is.
    fn translate(
        context: Rc<midenc_hir::Context>,
        source: &WasmSource,
        source_provenance: ProjectSourceProvenanceInputs,
    ) -> CompilerResult<MidenComponent> {
        use midenc_hir::{BuilderExt, Op, OpBuilder, SourceSpan, dialects::builtin};

        let session = context.session_rc();
        let world = {
            let mut builder = OpBuilder::new(context.clone());
            let world = builder.create::<builtin::World, ()>(SourceSpan::default());
            world()?
        };
        let config = WasmTranslationConfig {
            source_name: source
                .path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("noname")
                .to_string()
                .into(),
            remap_path_prefixes: session.options.remap_path_prefixes.clone(),
            world: Some(world),
            generate_native_debuginfo: session.options.emit_source_locations(),
            ..Default::default()
        };

        let FrontendOutput {
            component,
            account_component_metadata_bytes,
        } = midenc_frontend_wasm::translate(&source.wasm, &config, context.clone())?;
        log::debug!(
            "parsed hir component from wasm bytes with first module name: {}",
            component.borrow().id()
        );

        crate::emit_hir_if_requested(component.borrow().as_operation(), context)?;

        Ok(MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes,
            source_provenance,
        })
    }

    /// Everything a WebAssembly-rooted build does once its WebAssembly is in hand.
    ///
    /// This is the whole of [`Frontend::compile`] bar the read, and it is separated out
    /// because a second route reaches this same point by another way: a standalone `.rs`
    /// target is compiled to WebAssembly by
    /// [`RustStandaloneFrontend`](super::rust::RustStandaloneFrontend) and then continues from
    /// here. Sharing it rather than copying it is what keeps the two routes' checkpoints,
    /// their inline `--emit` documents, and their `post_process` stash in step — three things
    /// a copy would have to be kept identical by review alone.
    ///
    /// The `--emit=wat` document is written *before* `wasm.parsed` is published, so it is
    /// written for a run that stops there; that is where the legacy stage wrote it, and the
    /// point of the output is to show what the compiler is about to translate.
    pub(super) fn compile_wasm(
        &self,
        cx: &TargetContext<'_>,
        source: WasmSource,
    ) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let WasmSource { path, wasm } = source;
        Self::emit_wat(&wasm, &cx.session())?;

        let wasm = match cx.checkpoint(CheckpointId::WASM_PARSED, ArtifactId::WASM, wasm)? {
            Flow::Continue(wasm) => wasm,
            Flow::Stop(stopped) => return Ok(Flow::Stop(stopped)),
        };
        let source = WasmSource { path, wasm };

        let provenance = self.provenance_of(cx, &source)?;
        let hir = Self::translate(cx.context(), &source, provenance)?;
        let hir = match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, hir)? {
            Flow::Continue(hir) => hir,
            Flow::Stop(stopped) => return Ok(Flow::Stop(stopped)),
        };

        // Destructured rather than moved whole, so that anything assembly needs and this
        // frontend fails to keep is a compile error here rather than a missing section in the
        // package — or, for the component's rodata, a silent miscompile. See [`LoweredTarget`].
        let LoweredTarget {
            sources,
            component,
            account_component_metadata_bytes,
            source_provenance,
        } = match backend::hir_to_masm(cx, hir)? {
            Flow::Continue(lowered) => lowered,
            Flow::Stop(stopped) => return Ok(Flow::Stop(stopped)),
        };
        self.lowered.borrow_mut().insert(
            cx.target_key(),
            CodegenOutput {
                component,
                account_component_metadata_bytes,
                source_provenance,
            },
        );

        Ok(Flow::Continue(sources))
    }
}

impl Frontend for WasmFrontend {
    /// Decode this target's WebAssembly, translate it to HIR, and lower it for assembly.
    ///
    /// The checkpoints are published as they are reached: `wasm.parsed` once the binary is in
    /// hand, `hir.initial` once it has been translated, and the three the shared backend owns
    /// thereafter. What [`backend::hir_to_masm`] returns beyond the sources is kept for
    /// [`Frontend::post_process`], which the assembler calls against a context of its own.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        self.compile_wasm(cx, Self::read(cx)?)
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
            cx.assembly().target,
            cx.assembly().package_registry,
        )
    }

    /// This target's build provenance: its WebAssembly, as text.
    ///
    /// The text rather than the binary because provenance is what a human reads out of a
    /// package to see what it was built from, and it is what the legacy stage recorded.
    /// Memoized, because the assembler asks repeatedly while hashing the dependency closure and
    /// each answer costs a read and a disassembly; `compile` fills the same cache, so whichever
    /// runs first pays.
    ///
    /// # A standalone build never reaches this, so `Self::read`'s stdin arm is unexercised here
    ///
    /// A synthesized project has no manifest path, which makes its dependency-graph node a
    /// `ProjectSource::Virtual` — and `build_source_provenance` returns `None` for those without
    /// consulting any provider (`miden-assembly/src/project/dependency_graph.rs`, the
    /// `ProjectSource::Virtual` arm). A stdin-backed input is only ever a *standalone* one, so
    /// the bytes branch of `Self::read` can only be reached from [`Frontend::compile`], never
    /// from here.
    ///
    /// Nothing is removed on that account: the unreachability is upstream's to keep, and a
    /// project whose package gained a manifest path would reach here at once. But the tests
    /// covering provenance for a stdin-backed target construct the call themselves, and are not
    /// end-to-end evidence that any command line produces it.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        // The lookup is here as well as in `provenance_of`, because this is the caller that has
        // not read the sources yet: `provenance_of` takes them already in hand, so consulting
        // the cache only there would mean paying for the read to discover it was unnecessary.
        if let Some(found) = self.provenance.borrow().get(&cx.target_key()) {
            return Ok(found.to_inputs());
        }
        let source = Self::read(cx)?;
        self.provenance_of(cx, &source)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        format,
        rc::Rc,
        string::{String, ToString},
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;
    use std::path::PathBuf;

    use midenc_hir::Context;
    use midenc_session::{
        InputFile, OutputFile, OutputType, OutputTypeSpec, OutputTypes, miden_project::TargetType,
    };

    use super::*;
    use crate::{
        MidenComponent,
        pipeline::{
            FrontendRegistry, Goal, Observer, Outcome, RecordingObserver, RequestState, TargetRole,
            testing::{self, VirtualProject},
        },
    };

    // ---------------------------------------------------------------------------------------
    // Fixtures.
    // ---------------------------------------------------------------------------------------

    /// The WebAssembly text every fixture below is built from.
    ///
    /// One exported function, whose body survives the rewrites and code generation the shared
    /// backend runs — an empty `(module)` would publish the same checkpoints without proving
    /// that anything was translated.
    const WAT: &str = r#"
(module
  (func $add (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add)
)
"#;

    /// A second module, distinguishable from [`WAT`] by the name of its export.
    ///
    /// Used only where a test has to tell *which* source a run compiled: two runs of one
    /// module cannot.
    const OTHER_WAT: &str = r#"
(module
  (func $sub (export "sub") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.sub)
)
"#;

    /// [`WAT`] as the WebAssembly binary it decodes to.
    fn wasm_bytes(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("the fixture must be valid WebAssembly text")
    }

    /// Write `bytes` to `<fixtures>/<dir>/<file>` and return the path.
    ///
    /// [`testing::fixture_source`] writes text, and a `.wasm` fixture is a binary that must
    /// reach the frontend byte for byte — that being the whole difference between the two
    /// extensions this frontend claims.
    fn binary_fixture(dir: &str, file: &str, bytes: &[u8]) -> PathBuf {
        let path = testing::fixture_source(dir, file, "");
        std::fs::write(&path, bytes).expect("should write the binary fixture");
        path
    }

    /// A single-target project named after `dir`, rooted at a `.wat` file holding `contents`.
    fn wat_project(dir: &str, contents: &str, ty: TargetType) -> VirtualProject {
        let file = if ty.is_executable() {
            "main.wat"
        } else {
            "lib.wat"
        };
        let root = testing::fixture_source(dir, file, contents);
        VirtualProject::new(dir, &root, ty).expect("should build project")
    }

    /// The same, rooted at a `.wasm` file holding the binary [`WAT`] decodes to.
    fn wasm_project(dir: &str) -> VirtualProject {
        let root = binary_fixture(dir, "lib.wasm", &wasm_bytes(WAT));
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
    fn documents(dir: &std::path::Path, extension: &str) -> Vec<(String, Vec<u8>)> {
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
                (stem, std::fs::read(&path).expect("should read the document"))
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

    /// Run the wasm frontend over `project`'s selected target to `goal`, within `context`'s
    /// session, with `input` as the request's compiler input.
    ///
    /// The frontend comes from [`FrontendRegistration::instantiate`] rather than being
    /// constructed here, so what these tests exercise is what a caller holding only the
    /// registration would get.
    fn run(
        project: &VirtualProject,
        context: Rc<Context>,
        goal: Goal,
        input: Option<&InputFile>,
    ) -> Run {
        try_run(project, context, goal, input).expect("the wasm frontend should compile")
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

        let frontend = WASM_FRONTEND.instantiate(cx.session());
        let flow = frontend.compile(&cx)?;
        Ok(Run {
            stopped: flow.is_stop(),
            trace: observer.borrow().records().iter().map(|(checkpoint, _)| *checkpoint).collect(),
            captured: state.take_outcome(),
        })
    }

    /// The checkpoints a run that is not asked to stop publishes, in order.
    ///
    /// `package.assembled` is absent because the orchestrator publishes it, not the frontend.
    fn full_trace() -> Vec<CheckpointId> {
        vec![
            CheckpointId::WASM_PARSED,
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_LOWERED,
        ]
    }

    /// The WebAssembly published at `wasm.parsed` for `project`.
    fn parsed_wasm(project: &VirtualProject, input: Option<&InputFile>) -> Vec<u8> {
        run(project, context(), Goal::at(CheckpointId::WASM_PARSED), input)
            .captured
            .expect("stopping at wasm.parsed must capture the WebAssembly")
            .downcast::<Vec<u8>>()
            .expect("the parsed artifact must be the WebAssembly binary")
    }

    /// The Miden Assembly `project` lowers to.
    fn lowered_sources(project: &VirtualProject, context: Rc<Context>) -> ProjectSourceInputs {
        run(project, context, Goal::at(CheckpointId::MASM_LOWERED), None)
            .captured
            .expect("stopping at masm.lowered must capture the lowered Miden Assembly")
            .downcast::<ProjectSourceInputs>()
            .expect("the lowered artifact must be the assembler's own source inputs")
    }

    // ---------------------------------------------------------------------------------------
    // The route.
    // ---------------------------------------------------------------------------------------

    /// A `.wat` target reaches every checkpoint on the route, in order.
    #[test]
    fn a_wat_target_reaches_every_checkpoint_in_order() {
        let project = wat_project("wasm_frontend_route", WAT, TargetType::Library);
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None);

        assert!(!run.stopped, "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            run.trace,
            full_trace(),
            "the frontend publishes the WebAssembly and the initial HIR, and the shared backend \
             the three that follow"
        );
        assert!(run.captured.is_none(), "nothing may be captured before the goal is reached");
    }

    /// So does a `.wasm` target: only the decode differs, and a decode is not a checkpoint.
    #[test]
    fn a_wasm_target_reaches_the_same_checkpoints() {
        let project = wasm_project("wasm_frontend_route_binary");
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED), None);

        assert!(!run.stopped);
        assert_eq!(
            run.trace,
            full_trace(),
            "a binary input skips the text decode, which publishes nothing of its own"
        );
    }

    /// A `.wasm` target's bytes reach `wasm.parsed` verbatim; a `.wat` target's are decoded
    /// first.
    ///
    /// The two fixtures hold the same module, so the published bytes must be equal — which is
    /// what says the `.wat` route decoded rather than published its own text, and the `.wasm`
    /// route published rather than re-encoded.
    #[test]
    fn wat_is_decoded_and_wasm_is_published_unchanged() {
        let expected = wasm_bytes(WAT);
        assert_eq!(&expected[..4], b"\0asm", "the fixture must decode to a WebAssembly binary");

        let from_text =
            parsed_wasm(&wat_project("wasm_frontend_text", WAT, TargetType::Library), None);
        assert_eq!(
            from_text, expected,
            "a `.wat` target must publish the WebAssembly it decodes to"
        );

        let from_binary = parsed_wasm(&wasm_project("wasm_frontend_binary"), None);
        assert_eq!(from_binary, expected, "a `.wasm` target must publish its own bytes, unchanged");
    }

    /// The artifact at `hir.initial` is the translated component.
    #[test]
    fn stopping_at_hir_initial_captures_the_translated_component() {
        let project = wat_project("wasm_frontend_hir_initial", WAT, TargetType::Library);
        let run = run(&project, context(), Goal::at(CheckpointId::HIR_INITIAL), None);

        assert!(run.stopped, "the goal is on this frontend's route, so it must stop there");
        assert_eq!(
            run.trace,
            vec![CheckpointId::WASM_PARSED, CheckpointId::HIR_INITIAL],
            "nothing past the goal may run: the backend's checkpoints must be absent"
        );

        let captured = run.captured.expect("stopping at the goal must capture an artifact");
        assert_eq!(captured.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(captured.artifact().id(), ArtifactId::HIR);
        let component = captured
            .downcast::<MidenComponent>()
            .expect("the initial artifact must be the translated Miden component");
        assert!(
            component.component.is_some(),
            "the wasm frontend translates into a component, not a bare world"
        );
    }

    // ---------------------------------------------------------------------------------------
    // The root module's path.
    //
    // `load_target_sources` rejects a root module whose path is not exactly the target's
    // namespace (`crates/assembly/src/project.rs:839`), so this is what decides whether a
    // target assembles at all.
    // ---------------------------------------------------------------------------------------

    /// An executable target's root module is the `$exec` module code generation builds for it.
    #[test]
    fn an_executable_targets_root_module_is_the_targets_namespace() {
        let project = wat_project("wasm_frontend_executable", WAT, TargetType::Executable);
        let namespace = project.target().namespace.inner().clone();
        // `MasmComponent::source_inputs` only generates the `$exec` root when the component
        // has an entrypoint, which is what `--entrypoint` supplies — and `--entrypoint`
        // implies `--target-type executable` on the command line, so this is the shape a
        // standalone executable is always built in.
        let context = testing::context_emitting(Default::default(), |options| {
            options.entrypoint = Some("main::add".to_string());
        });

        let sources = lowered_sources(&project, context);

        assert_eq!(
            sources.root.path(),
            namespace.as_ref(),
            "the assembler rejects any root module whose path is not the target's namespace"
        );
        assert!(
            sources.root.kind().is_executable(),
            "and an executable target's root is a program"
        );
    }

    /// A library target's root module is the target's namespace too, and its whole module tree
    /// moves there with it.
    ///
    /// A core Wasm module is translated into a component whose id is always the synthetic
    /// wrapper `root_ns:root@1.0.0`, and code generation would otherwise root the target's Miden
    /// Assembly at that id. `assemble_source_package` rejects a root module whose path is not
    /// exactly `target.namespace`, and the two can never agree — the id renders as the single
    /// quoted component `::"root_ns:root@1.0.0"`, which no target is named — so
    /// `MasmComponent::source_inputs` re-roots the wrapper at the target's namespace instead.
    ///
    /// Asserted against `project.target()`'s namespace rather than a literal, so that it is the
    /// agreement between the two that is pinned, not one particular fixture name.
    #[test]
    fn a_library_targets_root_module_is_the_targets_namespace() {
        let project = wat_project("wasm_frontend_library_root", WAT, TargetType::Library);
        let namespace = project.target().namespace.inner().clone();

        let sources = lowered_sources(&project, context());

        assert_eq!(
            sources.root.path(),
            namespace.as_ref(),
            "the assembler rejects any root module whose path is not the target's namespace"
        );
        assert!(
            !sources.support.is_empty(),
            "the fixture's code lives in a submodule, or the next assertion is vacuous"
        );
        for module in sources.support.iter() {
            assert!(
                module.path().starts_with(namespace.as_ref()),
                "a support module left behind under the wrapper's id would not be reachable from \
                 the root that declares it: {}",
                module.path()
            );
        }
    }

    /// And the re-rooted sources assemble, calls between the component's own modules included.
    ///
    /// Re-rooting has to move more than the module paths: code generation emits intra-component
    /// calls as absolute paths and records each callee on the calling procedure, and the linker
    /// resolves *both*. A rewrite that missed either would still satisfy the assertion above
    /// while failing every real build with "undefined item", which is why this goes as far as
    /// assembling. The fixture's exported function calls a second, unexported one, so there is
    /// an intra-component call to resolve.
    #[test]
    fn the_re_rooted_sources_assemble() {
        const CALLS_WITHIN_THE_COMPONENT: &str = r#"
(module
  (func $helper (param i32) (result i32)
    local.get 0
    i32.const 1
    i32.add)
  (func $add (export "add") (param i32 i32) (result i32)
    local.get 0
    call $helper
    local.get 1
    i32.add)
)
"#;
        let project = wat_project(
            "wasm_frontend_library_assembles",
            CALLS_WITHIN_THE_COMPONENT,
            TargetType::Library,
        );
        let context = context();
        let source_manager = context.session().source_manager.clone();

        let sources = lowered_sources(&project, context);

        let mut assembler = miden_assembly::Assembler::new(source_manager);
        assembler
            .link_package(midenc_codegen_masm::intrinsics::load(), miden_assembly::Linkage::Static)
            .expect("the compiler intrinsics should link");
        assembler
            .assemble_library("wasm_frontend_library_assembles", sources.root, sources.support)
            .expect("a re-rooted library should assemble");
    }

    // ---------------------------------------------------------------------------------------
    // Where the bytes come from.
    // ---------------------------------------------------------------------------------------

    /// A stdin-backed input is compiled from its bytes, not from the target root's path.
    ///
    /// The fixture on disk holds a *different* module, so a frontend that read the resolved
    /// target root would compile the wrong program rather than merely fail to find a file.
    #[test]
    fn a_stdin_backed_input_is_compiled_from_its_own_bytes() {
        let project = wat_project("wasm_frontend_stdin", OTHER_WAT, TargetType::Library);
        let piped = wasm_bytes(WAT);
        let input = InputFile::from_bytes(piped.clone(), "stdin.wasm".into())
            .expect("WebAssembly bytes are a valid compiler input");

        assert_eq!(
            parsed_wasm(&project, Some(&input)),
            piped,
            "the bytes piped in are the ones compiled; a stdin input has no path to read back"
        );
        assert_ne!(
            parsed_wasm(&project, None),
            piped,
            "and the fixture on disk really is a different module, or this proves nothing"
        );
    }

    /// A stdin-backed `.wat` input is decoded from its bytes too.
    #[test]
    fn a_stdin_backed_wat_input_is_decoded() {
        let project = wat_project("wasm_frontend_stdin_wat", OTHER_WAT, TargetType::Library);
        let input = InputFile::from_bytes(WAT.as_bytes().to_vec(), "stdin.wat".into())
            .expect("WebAssembly text is a valid compiler input");

        assert_eq!(
            parsed_wasm(&project, Some(&input)),
            wasm_bytes(WAT),
            "wasm text on standard input is decoded in memory, as `wat::parse_bytes` does"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Provenance.
    // ---------------------------------------------------------------------------------------

    /// Provenance reports this target's WebAssembly, as text, without publishing anything.
    #[test]
    fn provenance_reports_the_targets_wasm_as_text_without_publishing() {
        let project = wat_project("wasm_frontend_provenance", WAT, TargetType::Library);
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::WASM_PARSED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = WASM_FRONTEND.instantiate(cx.session());
        let provenance = frontend.provenance(&cx).expect("provenance should succeed");

        assert!(
            provenance.root.content.contains("(func"),
            "the provenance content is the module's WebAssembly text: {}",
            provenance.root.content
        );
        assert_eq!(
            provenance.root.path.file_name().and_then(|name| name.to_str()),
            Some("lib.wat"),
            "and it is attributed to the file the target is rooted at"
        );
        assert!(provenance.support.is_empty(), "a WebAssembly target has a single source");
        assert!(
            observer.borrow().records().is_empty(),
            "provenance must not publish checkpoints, even at the goal"
        );
    }

    /// Provenance is answered from what the compilation already read, not by reading again.
    ///
    /// The assembler asks for provenance repeatedly while hashing the dependency closure, and
    /// each answer costs a read and a full disassembly back to WebAssembly text. Memoization is
    /// therefore a requirement of [`Frontend::provenance`] rather than an optimization — so it
    /// is asserted by *removing the sources* after compiling: an implementation that re-read
    /// them cannot answer, and the fresh frontend at the end of the test proves that it could
    /// not.
    #[test]
    fn provenance_is_answered_from_what_the_compilation_already_read() {
        let project = wat_project("wasm_frontend_provenance_memo", WAT, TargetType::Library);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = WASM_FRONTEND.instantiate(cx.session());
        frontend.compile(&cx).expect("the fixture should compile");
        std::fs::remove_file(&assembly.resolved_target_root)
            .expect("the fixture's own sources should be removable");

        let provenance = frontend
            .provenance(&cx)
            .expect("a compiled target's provenance is already in hand");
        assert!(
            provenance.root.content.contains("(func"),
            "and it is the same WebAssembly text, not an empty placeholder: {}",
            provenance.root.content
        );

        assert!(
            WASM_FRONTEND.instantiate(cx.session()).provenance(&cx).is_err(),
            "the sources really are gone, so the answer above can only have been remembered"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Post-processing.
    // ---------------------------------------------------------------------------------------

    /// Post-processing a target this frontend never compiled is an invariant violation.
    ///
    /// The assembler builds a *fresh* context for that callback, so what `compile` produced is
    /// recovered from the frontend's own state; a miss means the two disagree about the target,
    /// which is a compiler bug and must be reported rather than panicked on.
    #[test]
    fn post_processing_a_target_that_was_never_compiled_is_an_invariant_error() {
        let project = wat_project("wasm_frontend_post_process", WAT, TargetType::Library);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new());
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = WASM_FRONTEND.instantiate(cx.session());
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
    // Emission.
    //
    // Every declaration on this route is `unrendered`, because every `--emit` this route can
    // satisfy is written inline by the code that owns it. These tests are what keeps that from
    // becoming two writers for one document.
    // ---------------------------------------------------------------------------------------

    /// `--emit=wat` is written by the frontend itself, as the parse step reaches it.
    #[test]
    fn the_wat_document_is_written_by_the_frontend() {
        let project = wat_project("wasm_frontend_emit_wat", WAT, TargetType::Library);
        let out_dir = testing::fixture_dir("wasm_frontend_emit_wat_out");
        let context = emitting_context(OutputType::Wat, &out_dir);

        run(&project, context, Goal::at(CheckpointId::WASM_PARSED), None);

        let written = documents(&out_dir, "wat");
        assert_eq!(written.len(), 1, "the frontend must write the wat document: {written:?}");
        assert!(
            String::from_utf8_lossy(&written[0].1).contains("(func"),
            "and it must be the module's own text"
        );
    }

    /// A run that asked for no wat writes none, while its session is writing to that very
    /// directory.
    ///
    /// The session emits `hir` into the directory the assertions read, so the destination is
    /// demonstrably live: something *does* land there in this run. That is what makes the
    /// absence of a `.wat` document mean something, rather than merely that nothing was ever
    /// pointed here — which is all an empty directory and a session emitting nothing would show.
    ///
    /// What it pins is that the wat document goes through [`Session::emit`] and is therefore
    /// governed by the session's own output configuration: an implementation that resolved a
    /// destination of its own, or emitted under some other output type, would put a file in this
    /// directory. It does **not** pin the `should_emit` check in
    /// [`WasmFrontend::emit_wat`] — deleting that check changes nothing observable here, because
    /// `Session::emit` consults `should_emit` again. That check is a cost guard, and is
    /// documented as one.
    #[test]
    fn whether_the_wat_is_written_is_the_sessions_decision() {
        let project = wat_project("wasm_frontend_emit_wat_off", WAT, TargetType::Library);
        let out_dir = testing::fixture_dir("wasm_frontend_emit_wat_off_out");
        let context = emitting_context(OutputType::Hir, &out_dir);

        run(&project, context, Goal::at(CheckpointId::HIR_INITIAL), None);

        assert_eq!(
            documents(&out_dir, "hir").len(),
            1,
            "the session must really be writing here, or the assertion below is vacuous"
        );
        assert!(
            documents(&out_dir, "wat").is_empty(),
            "a session that asked for hir and not wat must come away with no wat"
        );
    }

    /// `--emit=hir` is written inline as well: one document per run reaches the destination.
    #[test]
    fn the_hir_document_is_written_without_a_renderer() {
        let project = wat_project("wasm_frontend_emit_hir", WAT, TargetType::Library);
        let out_dir = testing::fixture_dir("wasm_frontend_emit_hir_out");
        let context = emitting_context(OutputType::Hir, &out_dir);

        run(&project, context, Goal::at(CheckpointId::HIR_INITIAL), None);

        assert_eq!(
            documents(&out_dir, "hir").len(),
            1,
            "the parse step writes the pre-rewrite hir document itself"
        );
    }

    /// And none of the route's declared renderers writes anything of its own.
    ///
    /// Rendering any of them would be a *second* writer for a document already written inline:
    /// the destination for an output type is resolved once from the session, so two writers
    /// either race for one file or print it twice when that destination is stdout.
    #[test]
    fn every_declaration_on_this_route_is_unrendered() {
        let out_dir = testing::fixture_dir("wasm_frontend_unrendered_out");
        for (checkpoint, output_type, extension) in [
            (CheckpointId::WASM_PARSED, OutputType::Wat, "wat"),
            (CheckpointId::HIR_INITIAL, OutputType::Hir, "hir"),
            (CheckpointId::HIR_ANALYZED, OutputType::Hir, "hir"),
            (CheckpointId::HIR_TRANSFORMED, OutputType::Hir, "hir"),
            (CheckpointId::MASM_LOWERED, OutputType::Masm, "masm"),
            (CheckpointId::PACKAGE_ASSEMBLED, OutputType::Masp, "masp"),
        ] {
            let context = emitting_context(output_type, &out_dir);
            let decl = WASM_FRONTEND
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

    /// The registration is well-formed, and dispatches from both of its extensions.
    #[test]
    fn the_registration_is_accepted_by_the_registry() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(WASM_FRONTEND)
            .expect("the wasm registration must be well-formed");

        for extension in ["wasm", "wat"] {
            let found = registry
                .for_extension(extension)
                .unwrap_or_else(|| panic!("dispatch is by target-root extension: {extension}"));
            assert_eq!(found.id(), FrontendId::new("wasm"));
        }

        let found = registry.for_extension("wat").expect("registered");
        assert_eq!(found.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_eq!(found.resolve_alias("parse"), Some(CheckpointId::WASM_PARSED));
        assert_eq!(found.resolve_alias("analyze"), Some(CheckpointId::HIR_ANALYZED));
        assert_eq!(found.resolve_alias("transform"), Some(CheckpointId::HIR_TRANSFORMED));
        assert_eq!(found.resolve_alias("lower"), Some(CheckpointId::MASM_LOWERED));
        assert_eq!(found.resolve_alias("assemble"), Some(CheckpointId::PACKAGE_ASSEMBLED));
        assert_eq!(found.artifact_at(CheckpointId::WASM_PARSED), Some(ArtifactId::WASM));
        assert_eq!(found.artifact_at(CheckpointId::HIR_INITIAL), Some(ArtifactId::HIR));
        assert_eq!(found.artifact_at(CheckpointId::MASM_LOWERED), Some(ArtifactId::MASM));
    }

    /// An input that is neither WebAssembly nor WebAssembly text is reported.
    #[test]
    fn a_target_root_that_is_not_webassembly_is_reported() {
        let root = testing::fixture_source("wasm_frontend_bad_root", "lib.masm", "begin end");
        let project = VirtualProject::new("wasm_frontend_bad_root", &root, TargetType::Library)
            .expect("should build project");

        let err = try_run(&project, context(), Goal::at(CheckpointId::WASM_PARSED), None)
            .err()
            .expect("a `.masm` root is not this frontend's to compile");

        let msg = format!("{err}");
        // Not `contains("masm")`: the report names the offending *path*, `lib.masm`, so that
        // assertion would hold even if the report said nothing about what this frontend accepts.
        assert!(
            msg.contains("expected '.wasm' or '.wat'"),
            "the report must say which file types this frontend handles: {msg}"
        );
    }
}
