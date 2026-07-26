//! Preparation: turning a project input into what a compilation request needs.
//!
//! A project input names a manifest — either `miden-project.toml`, or a `Cargo.toml` standing
//! in for the sibling `miden-project.toml` that `cargo miden` generates next to it.
//! Preparation normalizes that locator, loads the project from it, selects the target to
//! build, and selects the frontend that handles that target's root.
//!
//! # Build profiles
//!
//! Preparation decides the profile *name* a project request carries — the profile itself is
//! resolved from that name by the assembler, once per target it builds — and the rule differs
//! by where the project's profiles come from:
//!
//! - **Synthesized (virtual) projects** — preparation builds the profiles itself, so
//!   profile-affecting flags fold in at synthesis: `--debug none` yields `debug = false`, and
//!   any positive `--debug` yields `debug = true`.
//! - **User-controlled manifests** — the requested profile name passes through untouched, and
//!   `--debug` does not alter the build profile at all. Users select or define a profile with
//!   the configuration they want.
//!
//! The asymmetry is forced: [`ProjectPackage::resolve_profile`] reads the profile out of the
//! package being assembled, and `Package` is neither `Clone` nor mutable through an `Arc`, so
//! the profiles of a loaded manifest cannot be adjusted on the way past. `--debug` continues
//! to govern compiler-side debug behavior in both cases; only its effect on the *build
//! profile* is confined to synthesized projects.
//!
//! [`prepare_project`] handles manifest inputs only, so the synthesized half of the rule has
//! no implementation here yet — it belongs with virtual project synthesis, which lands next.
//!
//! This is also only one arm of the compiler's profile behavior. Only project inputs — a
//! `miden-project.toml`, or the `Cargo.toml` standing in for one — are prepared here and run
//! through [`Pipeline::compile`](super::Pipeline::compile). Standalone inputs still run the
//! legacy stage chains, which pass a hardcoded `"dev"` to the assembler (see
//! `stages/assemble.rs`), so `cargo miden build --release` builds `release` while
//! `midenc --release foo.wasm` builds `dev`. The two converge when standalone inputs move onto
//! the pipeline.

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use std::path::{Path, PathBuf};

use miden_assembly::ProjectTargetSelector;
use midenc_session::{
    InputFile, Options,
    diagnostics::{Report, SourceManager},
    miden_project::{Package as ProjectPackage, Project, Target},
};

use super::{FrontendRegistration, FrontendRegistry};
use crate::CompilerResult;

/// Everything a compilation request needs about the project it was asked to build.
///
/// The [`FrontendRegistration`] is held by value: it is `Copy`, and
/// [`FrontendRegistry::for_extension`] hands back a borrow of the registry rather than a
/// `&'static`, so copying it out keeps this type free of a lifetime parameter it would
/// otherwise carry for nothing.
#[derive(Debug)]
pub struct PreparedProject {
    /// The project's package, as loaded from [`manifest_path`](Self::manifest_path).
    pub package: Arc<ProjectPackage>,
    /// The normalized manifest locator: always a `miden-project.toml`, never the `Cargo.toml`
    /// that may have named it.
    pub manifest_path: PathBuf,
    /// The target selected for this request.
    pub target: Target,
    /// The name of the build profile to build under, as requested in [`Options::profile`].
    pub profile_name: String,
    /// The frontend registered for the selected target root's extension.
    pub frontend: FrontendRegistration,
}

/// Resolve `input` into the project, target and frontend a compilation request runs with.
///
/// The locator is normalized first, then the project is loaded from it, then the requested
/// target is selected, and finally the frontend is chosen from that target's root.
pub fn prepare_project(
    input: &InputFile,
    options: &Options,
    registry: &FrontendRegistry,
    source_manager: &dyn SourceManager,
) -> CompilerResult<PreparedProject> {
    let manifest_path = normalize_locator(input)?;

    // The project is loaded here rather than taken from the session, and that is
    // load-bearing. `Session::new` loads this same manifest, but for a `Cargo.toml` input
    // whose target type is executable it then replaces the package with the one
    // `fixup_cargo_target` rebuilds — which rewrites library targets' namespaces, and, being
    // built by `Package::new`, has no manifest path. That fixed-up package is not what a
    // project build has ever compiled: the stage this preparation replaced assembled through
    // `for_project_at_path_with_providers`, which loads the manifest itself, so the package it
    // built is this one — manifest-backed and un-fixed-up. Substituting
    // `session.project.package()` here would look like a simplification and would change two
    // things at once: the required library would be assembled under the rewritten namespace,
    // and `DependencyGraph::from_project` branches on the package's manifest path, so a missing
    // one takes the virtual path and yields a different dependency graph altogether.
    //
    // What that costs is a second load of the same manifest in every project build: `Session`
    // loaded it once already (`midenc-session/src/lib.rs`, `Session::new`). That is not new —
    // the legacy path also loaded twice, because `for_project_at_path_with_providers` re-loaded
    // the manifest internally — and the way to converge on one load is to remove `Session`'s own
    // Toml branch, not to drop this one for a package it does not build.
    let project = Project::load(&manifest_path, source_manager).map_err(|err| {
        err.wrap_err(format!("failed to load Miden project from {}", manifest_path.display()))
    })?;
    let package = project.package();

    // `Session` derives the artifact name from `--name` if given, and otherwise from the
    // loaded package's name (see `Session::new`). Preparation takes only `Options`, so that
    // rule is restated here rather than read off a session — and
    // `the_selected_executable_is_the_one_the_session_names` runs both, so the two cannot
    // diverge in silence.
    let name = options.name.clone().unwrap_or_else(|| package.name().inner().to_string());
    let selector = if options.target_type.unwrap_or_default().is_executable() {
        ProjectTargetSelector::Executable(name.as_str())
    } else {
        ProjectTargetSelector::Library
    };
    let target = selector.select_target(&package)?;
    let frontend = select_frontend(&target, registry)?;

    // The requested profile is carried by name, not by value, because the assembler resolves
    // it again from the package for each target it builds. Resolving it once here anyway, and
    // discarding the result, is what turns an unknown name into a diagnostic before any work
    // is done — and the diagnostic is the assembler's own, so both paths report it alike. The
    // resolved profile is not worth keeping: it borrows `package`, which would put a lifetime
    // on `PreparedProject` for a value the assembler does not accept.
    let profile_name = options.profile.clone();
    package.resolve_profile(&profile_name)?;

    Ok(PreparedProject {
        package,
        manifest_path,
        target,
        profile_name,
        frontend,
    })
}

/// Resolve the project locator `input` names to the `miden-project.toml` it stands for.
///
/// A `Cargo.toml` locates the `miden-project.toml` beside it, which is where `cargo miden`
/// writes the Miden manifest for a crate. This is the same normalization `Session::new`
/// performs, and the two must agree: they load the same project.
fn normalize_locator(input: &InputFile) -> CompilerResult<PathBuf> {
    let file_name = input.file_name();
    match file_name.file_name() {
        Some(name) if name.eq_ignore_ascii_case("Cargo.toml") => {
            let cargo_manifest_path = file_name.as_path();
            reject_unselected_workspace_root(cargo_manifest_path)?;
            Ok(cargo_manifest_path.with_file_name("miden-project.toml"))
        }
        Some(name) if name.eq_ignore_ascii_case("miden-project.toml") => {
            Ok(file_name.as_path().to_path_buf())
        }
        _ => Err(Report::msg(
            "unsupported toml input: expected either `miden-project.toml` or `Cargo.toml`",
        )),
    }
}

/// Reject `manifest_path` if it is a Cargo workspace root that selects no package.
///
/// A workspace root names members but no package of its own, so there is nothing to build;
/// which member was meant has to come from the caller.
///
/// `manifest_path` is a Cargo manifest by construction: [`normalize_locator`] is the only
/// caller, and it calls this from the arm that has just matched the file name. Nothing is
/// re-checked here. The version carried over from the deleted `stages/project.rs` did
/// re-check, with a case-*sensitive* comparison — so on a case-insensitive filesystem a
/// `cargo.toml` workspace root skipped the rejection entirely and failed later as a missing
/// Miden project.
fn reject_unselected_workspace_root(manifest_path: &Path) -> CompilerResult<()> {
    use toml_edit::DocumentMut;

    let manifest = std::fs::read_to_string(manifest_path).map_err(|err| {
        Report::msg(format!("failed to read Cargo manifest '{}': {err}", manifest_path.display()))
    })?;
    let manifest = manifest.parse::<DocumentMut>().map_err(|err| {
        Report::msg(format!("failed to parse Cargo manifest '{}': {err}", manifest_path.display()))
    })?;
    if manifest.get("workspace").is_some() && manifest.get("package").is_none() {
        Err(Report::msg(
            "unable to determine package from Cargo workspace root; run `miden build` from a \
             workspace member or select a member package explicitly with --manifest-path",
        ))
    } else {
        Ok(())
    }
}

/// Select the frontend that handles `target`'s root.
///
/// Dispatch is on the extension of the target root, never on the manifest that declared it: a
/// `.toml` is a project locator, and no frontend compiles one.
fn select_frontend(
    target: &Target,
    registry: &FrontendRegistry,
) -> CompilerResult<FrontendRegistration> {
    let root = target.path.inner();
    let root_path = root.to_path();
    let extension = root_path.as_deref().and_then(Path::extension).and_then(|ext| ext.to_str());
    let Some(extension) = extension else {
        return Err(Report::msg(format!(
            "cannot select a frontend for target '{}': its root '{root}' has no file extension; \
             registered extensions: [{}]",
            target.name.inner(),
            registered_extensions(registry)
        )));
    };
    registry.for_extension(extension).copied().ok_or_else(|| {
        Report::msg(format!(
            "cannot select a frontend for target '{}': no frontend is registered for the \
             '{extension}' extension of its root '{root}'; registered extensions: [{}]",
            target.name.inner(),
            registered_extensions(registry)
        ))
    })
}

/// The registry's extensions, in sorted order, for use in diagnostics.
fn registered_extensions(registry: &FrontendRegistry) -> String {
    registry.extensions().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, string::ToString, sync::Arc};
    use std::path::Path;

    use midenc_session::{
        DebugInfo, Session,
        diagnostics::{DefaultSourceManager, SourceManager},
    };

    use super::*;
    use crate::pipeline::{
        FrontendId,
        // `WASM` is a registration for `.wasm` and `.wat` target roots whose frontend is
        // never run: preparation selects a frontend, and never instantiates one.
        registry::tests::WASM,
        testing::fixture_source,
    };

    /// A library project whose target root is a `.wat` file, which [`registry`] handles.
    const LIBRARY_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[lib]
namespace = "prepare_fixture"
path = "lib.wat"
"#;

    /// The `Cargo.toml` that sits beside [`LIBRARY_MANIFEST`] in a `cargo miden` project.
    ///
    /// Deliberately not a valid Miden manifest: if preparation loaded the locator it was
    /// given instead of normalizing it, it would not quietly succeed.
    const CARGO_MANIFEST: &str = r#"
[package]
name = "prepare-fixture-crate"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
"#;

    /// A Cargo workspace root: it declares members, but names no package of its own.
    const CARGO_WORKSPACE_ROOT: &str = r#"
[workspace]
members = ["member"]
"#;

    /// The profile a project defines for itself, over and above the two every package is
    /// seeded with.
    const CUSTOM_PROFILE: &str = "checked";

    /// [`LIBRARY_MANIFEST`] plus a build profile of the project's own.
    ///
    /// [`CUSTOM_PROFILE`] is deliberately not one of the two profiles every package is seeded
    /// with, and it inherits `release` while re-enabling debug info — so a profile resolved
    /// under that name can only have come from the manifest.
    const CUSTOM_PROFILE_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[lib]
namespace = "prepare_fixture"
path = "lib.wat"

[profile.checked]
inherits = "release"
debug = true
"#;

    /// A project with two executable targets, one of them named after the package.
    ///
    /// Which of the two is selected is decided by the name, which is what
    /// [`the_selected_executable_is_the_one_the_session_names`] pins.
    const EXECUTABLE_MANIFEST: &str = r#"
[package]
name = "prepare_fixture"
version = "0.1.0"

[[bin]]
name = "prepare_fixture"
path = "main.wat"

[[bin]]
name = "other"
path = "other.wat"
"#;

    /// A registry that handles `.wasm` and `.wat` target roots, and nothing else.
    fn registry() -> FrontendRegistry {
        let mut registry = FrontendRegistry::new();
        registry.register(WASM).expect("wasm should register");
        registry
    }

    /// The project input naming `manifest`, as the driver builds it from the command line.
    fn input(manifest: &Path) -> InputFile {
        InputFile::from_path(manifest).expect("a manifest is a valid compiler input")
    }

    /// The package name of a prepared project, for comparing two preparations.
    fn package_name(prepared: &PreparedProject) -> String {
        prepared.package.name().inner().to_string()
    }

    /// The options a request that asked for build profile `profile` arrives with.
    fn requesting_profile(profile: &str) -> Options {
        Options {
            profile: profile.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_cargo_locator_prepares_the_project_its_sibling_manifest_names() {
        let dir = "prepare_locator";
        let miden_manifest = fixture_source(dir, "miden-project.toml", LIBRARY_MANIFEST);
        let cargo_manifest = fixture_source(dir, "Cargo.toml", CARGO_MANIFEST);
        let registry = registry();
        let options = Options::default();
        let source_manager = DefaultSourceManager::default();

        let from_cargo =
            prepare_project(&input(&cargo_manifest), &options, &registry, &source_manager)
                .expect("a Cargo locator should prepare the sibling Miden project");
        let from_miden =
            prepare_project(&input(&miden_manifest), &options, &registry, &source_manager)
                .expect("a Miden manifest locator should prepare the project it names");

        assert_eq!(
            from_cargo.manifest_path, miden_manifest,
            "a Cargo locator must be normalized to its sibling miden-project.toml"
        );
        assert_eq!(from_miden.manifest_path, miden_manifest, "a Miden manifest is used as given");
        assert_eq!(
            package_name(&from_cargo),
            package_name(&from_miden),
            "both locators name one project, so both must load one package"
        );
        assert_eq!(package_name(&from_cargo), "prepare_fixture");
        assert_eq!(
            from_cargo.target, from_miden.target,
            "both locators must select the same target"
        );
        assert_eq!(
            from_cargo.target.path.inner().as_str(),
            "lib.wat",
            "the library target of the Miden manifest, not anything derived from the Cargo one"
        );
        assert_eq!(
            from_cargo.frontend.id(),
            FrontendId::new("wasm"),
            "the frontend follows the target root's extension; a `.toml` locator is not a \
             frontend format"
        );
        assert_eq!(from_miden.frontend.id(), from_cargo.frontend.id());
        assert!(
            from_cargo.package.manifest_path().is_some(),
            "the prepared package must be the manifest-backed one: a package rebuilt in memory \
             has no manifest path, and the dependency graph takes its virtual path when the \
             manifest path is missing"
        );
    }

    #[test]
    fn an_unselected_cargo_workspace_root_is_rejected() {
        // Nothing else in this directory: were the workspace root accepted, preparation would
        // fail on the missing miden-project.toml instead, which the message check separates.
        let cargo_manifest =
            fixture_source("prepare_workspace_root", "Cargo.toml", CARGO_WORKSPACE_ROOT);

        let err = prepare_project(
            &input(&cargo_manifest),
            &Options::default(),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("a Cargo workspace root selects no package to build");

        let rendered = format!("{err}");
        assert!(
            rendered.contains("unable to determine package from Cargo workspace root"),
            "the workspace root must be rejected on its own terms, not as a missing manifest: \
             {rendered}"
        );
    }

    #[test]
    fn a_target_root_with_an_unregistered_extension_is_reported() {
        let manifest = fixture_source(
            "prepare_unregistered_extension",
            "miden-project.toml",
            &LIBRARY_MANIFEST.replace("lib.wat", "lib.masm"),
        );

        let err = prepare_project(
            &input(&manifest),
            &Options::default(),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("no frontend handles `.masm` target roots in this registry");

        let rendered = format!("{err}");
        assert!(
            rendered.contains("'masm'"),
            "the diagnostic must name the extension it could not dispatch on: {rendered}"
        );
        assert!(
            rendered.contains("wasm, wat"),
            "the diagnostic must list the registered extensions, in sorted order: {rendered}"
        );
    }

    #[test]
    fn the_requested_profile_name_reaches_the_prepared_project_unchanged() {
        let manifest = fixture_source(
            "prepare_profile_passthrough",
            "miden-project.toml",
            CUSTOM_PROFILE_MANIFEST,
        );

        // The two seeded profiles and one the manifest defines itself: preparation must not
        // substitute a default for any of them, which is what hardcoding `"dev"` did.
        for requested in ["dev", "release", CUSTOM_PROFILE] {
            let prepared = prepare_project(
                &input(&manifest),
                &requesting_profile(requested),
                &registry(),
                &DefaultSourceManager::default(),
            )
            .unwrap_or_else(|err| panic!("the manifest defines a '{requested}' profile: {err}"));

            assert_eq!(
                prepared.profile_name, requested,
                "the requested profile name is what the assembler resolves per target, so \
                 preparation must carry it through untouched"
            );
        }
    }

    #[test]
    fn a_manifest_projects_profile_is_not_rewritten_by_the_debug_level() {
        let manifest =
            fixture_source("prepare_profile_debug", "miden-project.toml", CUSTOM_PROFILE_MANIFEST);

        // `release` emits no debug info; `checked` inherits it and turns debug info back on.
        // Neither answer may move with `--debug`: a user-controlled manifest owns its profiles.
        for (requested, emits_debug_info) in [("release", false), (CUSTOM_PROFILE, true)] {
            for debug in [DebugInfo::None, DebugInfo::Line, DebugInfo::Full] {
                let options = Options {
                    debug,
                    ..requesting_profile(requested)
                };
                let prepared = prepare_project(
                    &input(&manifest),
                    &options,
                    &registry(),
                    &DefaultSourceManager::default(),
                )
                .unwrap_or_else(|err| {
                    panic!("'{requested}' should prepare under {debug:?}: {err}")
                });

                assert_eq!(prepared.profile_name, requested, "--debug must not select a profile");
                let profile = prepared
                    .package
                    .resolve_profile(&prepared.profile_name)
                    .expect("a prepared profile name resolves against its own package");
                assert_eq!(
                    profile.should_emit_debug_info(),
                    emits_debug_info,
                    "--debug {debug:?} must not fold into the '{requested}' profile of a \
                     manifest-backed project"
                );
            }
        }
    }

    #[test]
    fn a_profile_the_manifest_does_not_define_is_rejected() {
        let manifest =
            fixture_source("prepare_profile_unknown", "miden-project.toml", LIBRARY_MANIFEST);

        let err = prepare_project(
            &input(&manifest),
            &requesting_profile("nonexistent"),
            &registry(),
            &DefaultSourceManager::default(),
        )
        .expect_err("the manifest defines no 'nonexistent' build profile");

        assert_eq!(
            format!("{err}"),
            "project 'prepare_fixture' does not define a 'nonexistent' build profile",
            "the profile is resolved against the package so that an unknown name fails here with \
             the assembler's own diagnostic, rather than deep inside assembly"
        );
    }

    #[test]
    fn the_selected_executable_is_the_one_the_session_names() {
        let manifest =
            fixture_source("prepare_executable_name", "miden-project.toml", EXECUTABLE_MANIFEST);

        // Preparation restates `Session`'s naming rule from `Options` alone, so this runs both
        // and holds them to the same answer: were either side to change how the name is
        // derived, the two would pick different executable targets.
        for requested_name in [None, Some("other")] {
            let mut options = Box::new(Options::default());
            options.name = requested_name.map(ToString::to_string);
            let source_manager: Arc<dyn SourceManager + Send + Sync> =
                Arc::new(DefaultSourceManager::default());
            let session = Session::new(input(&manifest), options, None, source_manager.clone())
                .expect("a Miden manifest with executable targets should open a compiler session");

            let prepared = prepare_project(
                &input(&manifest),
                &session.options,
                &registry(),
                source_manager.as_ref(),
            )
            .expect("an executable project should prepare");

            assert_eq!(
                prepared.target.name.inner().as_ref(),
                session.name.as_str(),
                "the selected executable is the one named by the session"
            );
            assert_eq!(
                prepared.target.name.inner().as_ref(),
                requested_name.unwrap_or("prepare_fixture"),
                "an explicit --name selects that executable; without one, the package's own name \
                 does"
            );
        }
    }
}
