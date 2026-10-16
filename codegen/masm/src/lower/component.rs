#[cfg(test)]
mod tests;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};

use miden_assembly::{PathBuf as LibraryPath, ast::InvocationTarget};
use miden_assembly_syntax::ast::Attribute;
use midenc_hir::{
    FunctionIdent, Op, OpExt, SourceSpan, Span, Symbol, SymbolPath, SymbolRef,
    SymbolResolutionError, TraceTarget, Type,
    diagnostics::IntoDiagnostic,
    dialects::{builtin, debuginfo::attributes::SubprogramAttr},
    pass::AnalysisManager,
    reserved_names::{COMPONENT_INIT_PROCEDURE, FUNCTION_TABLE_INIT_PROC},
};
use midenc_session::diagnostics::{Report, Spanned, WrapErr};
use smallvec::SmallVec;

use crate::{
    artifact::MasmComponent,
    linker::{LinkInfo, Linker, LinkerError},
    lower::lowering::{qualified_invocation_target, symbol_module_path, world_root_module},
    masm,
};

/// Namespace selection made before executable source is selected.
#[derive(Clone, Copy)]
pub enum EmissionNamespace<'a> {
    /// Preserve the source identity for an explicitly target-independent consumer.
    Source,
    /// Apply the target's existing namespace eligibility policy before lowering.
    Target(&'a midenc_session::miden_project::Target),
}

/// Closed, immutable correspondence between actual emitted source owners and final names.
/// Construction derives every destination from source ownership and one namespace policy;
/// callers cannot provide arbitrary same-signature replacement targets.
pub(crate) struct EmissionOwners {
    modules: midenc_hir::FxHashMap<midenc_hir::OperationRef, masm::PathBuf>,
    targets: BTreeMap<masm::PathBuf, masm::PathBuf>,
    root: midenc_hir::OperationRef,
    source_root: masm::PathBuf,
    destination: Option<masm::PathBuf>,
    source_modules: Vec<midenc_hir::OperationRef>,
    source_symbols: Vec<SymbolRef>,
}

impl EmissionOwners {
    fn new(
        root: midenc_hir::OperationRef,
        supporting: &[builtin::ModuleRef],
        source_root: &masm::Path,
        namespace: EmissionNamespace<'_>,
        authored: bool,
        link: &LinkInfo,
    ) -> Result<Self, Report> {
        let destination = match namespace {
            EmissionNamespace::Target(target)
                if !target.is_executable()
                    && (!authored
                        || source_root
                            .strip_prefix(target.namespace.inner())
                            .is_some_and(|r| r.is_empty())) =>
            {
                Some(target.namespace.inner().as_ref())
            }
            _ => None,
        };
        let relocate = |path: &masm::Path| {
            let Some((to, rest)) =
                destination.and_then(|to| path.strip_prefix(source_root).map(|rest| (to, rest)))
            else {
                return path.to_path_buf();
            };
            let mut result = to.to_path_buf();
            for part in rest.components() {
                match part.expect("valid source path") {
                    masm::LibraryPathComponent::Normal(spelling) => result.push_component(spelling),
                    _ => unreachable!("absolute prefix leaves only normal components"),
                }
            }
            result
        };
        let mut modules = midenc_hir::FxHashMap::default();
        let mut original = Vec::new();
        original.push((root, source_root.to_path_buf()));
        let mut symbols = Vec::new();
        let mut collect = |op: &midenc_hir::Operation| {
            if let Some(module) = op.downcast_ref::<builtin::Module>() {
                original.push((
                    op.as_operation_ref(),
                    module.path().to_library_path().to_absolute().unwrap().into_owned(),
                ));
            } else if let Some(interface) = op.downcast_ref::<builtin::Interface>() {
                original.push((
                    op.as_operation_ref(),
                    interface.path().to_library_path().to_absolute().unwrap().into_owned(),
                ));
            }
            if let Some(symbol) = op.as_symbol_ref()
                && (op.is::<builtin::Function>() || op.is::<builtin::FunctionAlias>())
            {
                symbols.push(symbol);
            }
        };
        root.borrow().prewalk_all(&mut collect);
        for module in supporting {
            module.borrow().as_operation().prewalk_all(&mut collect);
        }
        let mut destinations = BTreeMap::new();
        for (owner, path) in &original {
            let final_path = relocate(path);
            if let Some((previous_path, previous_owner)) =
                destinations.insert(final_path.clone(), (path.clone(), *owner))
                && !(previous_owner == root && previous_path == *path)
            {
                return Err(Report::msg(format!("duplicate final module owner '{final_path}'")));
            }
            modules.insert(*owner, final_path);
        }
        let mut targets = BTreeMap::new();
        let mut definitions = BTreeSet::new();
        for symbol in &symbols {
            let symbol = symbol.borrow();
            let op = symbol.as_symbol_operation();
            if symbol.is_declaration() {
                continue;
            }
            let owner = op
                .nearest_symbol_table()
                .ok_or_else(|| Report::msg("emitted procedure has no owner"))?;
            let module = modules
                .get(&owner)
                .ok_or_else(|| Report::msg("procedure owner is outside the closed emitted map"))?;
            let name = symbol.name();
            let source = symbol_module_path(&*symbol).join(name.as_str());
            let target = module.join(name.as_str());
            if !definitions.insert(target.clone()) {
                return Err(Report::msg(format!("duplicate final procedure owner '{target}'")));
            }
            targets.insert(source, target);
        }
        if link.requires_init() {
            let init = modules[&root].join(COMPONENT_INIT_PROCEDURE);
            if !definitions.insert(init.clone()) {
                return Err(Report::msg(format!("duplicate final procedure owner '{init}'")));
            }
        }
        // Declaration-only siblings keep their external identities, including overlapping prefixes.
        let mut world = root;
        loop {
            let parent = world.borrow().parent_op();
            match parent {
                Some(parent) => world = parent,
                None => break,
            }
        }
        let mut collision = None;
        world.borrow().prewalk_all(|op| {
            if let Some(module) = op.downcast_ref::<builtin::Module>()
                && !modules.contains_key(&op.as_operation_ref())
                && is_declaration_only(&op.as_operation_ref())
            {
                let external = module.path().to_library_path().to_absolute().unwrap().into_owned();
                if destinations.contains_key(&external) {
                    collision = Some(external);
                }
            }
            if let Some(function) = op.downcast_ref::<builtin::Function>()
                && function.is_declaration()
            {
                let external = symbol_module_path(function).join(function.name().as_str());
                if definitions.contains(&external) {
                    collision = Some(external);
                }
            }
        });
        if let Some(path) = collision {
            return Err(Report::msg(format!(
                "final procedure owner collides with external declaration '{path}'"
            )));
        }
        let result = Self {
            modules,
            targets,
            root,
            source_root: source_root.to_path_buf(),
            destination: destination.map(masm::Path::to_path_buf),
            source_modules: original.iter().map(|(owner, _)| *owner).collect(),
            source_symbols: symbols,
        };
        result.validate()?;
        Ok(result)
    }

    /// Reconstruct destinations from the actual source owners before checking selected evidence.
    /// An immutable map is a packaging decision, never authority to substitute another procedure.
    pub(crate) fn validate(&self) -> Result<(), Report> {
        let relocate = |path: &masm::Path| {
            let Some((to, rest)) = self
                .destination
                .as_ref()
                .and_then(|to| path.strip_prefix(&self.source_root).map(|rest| (to, rest)))
            else {
                return path.to_path_buf();
            };
            let mut mapped = to.clone();
            for part in rest.components() {
                match part.expect("valid source path") {
                    masm::LibraryPathComponent::Normal(spelling) => mapped.push_component(spelling),
                    _ => unreachable!(),
                }
            }
            mapped
        };
        if self.modules.len() != self.source_modules.len() {
            return Err(Report::msg("final owner map has missing or extra source modules"));
        }
        for owner in &self.source_modules {
            let op = owner.borrow();
            let source = if *owner == self.root {
                if let Some(world) = op.downcast_ref::<builtin::World>() {
                    world_root_module(world)
                } else {
                    op.downcast_ref::<builtin::Component>()
                        .unwrap()
                        .namespace_path()
                        .to_library_path()
                        .to_absolute()
                        .unwrap()
                        .into_owned()
                }
            } else if let Some(module) = op.downcast_ref::<builtin::Module>() {
                module.path().to_library_path().to_absolute().unwrap().into_owned()
            } else {
                op.downcast_ref::<builtin::Interface>()
                    .unwrap()
                    .path()
                    .to_library_path()
                    .to_absolute()
                    .unwrap()
                    .into_owned()
            };
            if self.modules.get(owner) != Some(&relocate(&source)) {
                return Err(Report::msg("final module map disagrees with source ownership"));
            }
        }
        let mut count = 0;
        for symbol in &self.source_symbols {
            let symbol = symbol.borrow();
            if symbol.is_declaration() {
                continue;
            }
            count += 1;
            let source = symbol_module_path(&*symbol).join(symbol.name().as_str());
            if self.targets.get(&source) != Some(&relocate(&source)) {
                return Err(Report::msg("final procedure map disagrees with source identity"));
            }
        }
        if count != self.targets.len() {
            return Err(Report::msg("final owner map has missing or extra source procedures"));
        }
        Ok(())
    }

    pub(crate) fn target(
        &self,
        target: masm::InvocationTarget,
    ) -> Result<masm::InvocationTarget, Report> {
        if let masm::InvocationTarget::Path(path) = &target
            && let Some(mapped) = self.targets.get(path.inner().as_ref())
        {
            let source: &masm::Path = path.inner().as_ref();
            let mut expected = source.to_path_buf();
            if let Some((to, rest)) = self
                .destination
                .as_ref()
                .and_then(|to| source.strip_prefix(&self.source_root).map(|rest| (to, rest)))
            {
                expected = to.clone();
                for part in rest.components() {
                    match part.expect("valid source path") {
                        masm::LibraryPathComponent::Normal(spelling) => {
                            expected.push_component(spelling)
                        }
                        _ => unreachable!(),
                    }
                }
            }
            if *mapped != expected {
                return Err(Report::msg("final invocation map disagrees with source identity"));
            }
            return Ok(masm::InvocationTarget::Path(Span::new(
                path.span(),
                mapped.clone().into_boxed_path().into(),
            )));
        }
        Ok(target)
    }

    fn module(&self, owner: midenc_hir::OperationRef) -> &masm::Path {
        &self.modules[&owner]
    }
}

/// This trait represents a conversion pass from some HIR entity to a Miden Assembly component.
pub trait ToMasmComponent {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
        namespace: EmissionNamespace<'_>,
    ) -> Result<MasmComponent, Report>;
}

/// Derivation of a MASM component from an HIR world
///
/// A world is not a component, and the difference is what this impl exists to handle: a
/// component's body holds modules, interfaces and functions, while a world's body holds
/// *components* as well. Handing a world's own operation to `MasmComponentBuilder`, which walks a
/// component body, therefore panics the moment it meets the first `builtin.component`.
///
/// So the shape of the world decides how it is lowered, counting only the components that define
/// something (a declaration-only component is an external dependency, see below):
///
/// - A world holding **no** component is treated as one logical component whose body is the
///   world's, which is what it has always meant here. This is the shape `frontend/masm`'s
///   disassembler produces — it defines modules directly on the world — so it is a live path.
///   A world holding nothing but declaration-only components has no definition to lower, and is
///   reported.
/// - A world holding **one** component is lowered by lowering that component, because a
///   component is what a Miden package is rooted at and carries the identity it is rooted at.
///   Delegating rather than reimplementing is deliberate: the result is then the same
///   [`MasmComponent`] the equivalent standalone `builtin.component` produces, by construction
///   rather than by two implementations agreeing.
/// - A world holding **more than one** component is reported, and that limitation is external to
///   this crate — see `too_many_components`.
///
/// # Top-level items beside the component are normal, and are not an error
///
/// A world is not "a component, optionally". It may hold a component — the current codegen unit —
/// **plus any number of sibling interfaces, modules and declaration-only components**, which are
/// either
///
/// - *external dependencies represented in the IR*, which hold declarations only and contribute
///   nothing to the generated Miden Assembly, or
/// - *supporting modules*, which are translated 1:1 to Miden Assembly modules and linked into the
///   final assembly as ad-hoc modules.
///
/// A world holding a single component is only the *happy path*, and only for the Rust frontend,
/// which compiles to one Wasm component and translates it to one HIR component. Other frontends,
/// the MASM one included, legitimately produce several top-level items. **Neither kind of sibling
/// may fail a build.**
///
/// The first kind is recognized by `is_declaration_only` (`Component::is_declaration_only` for a
/// component, e.g. an import's stub) and ignored, silently, because that is exactly what it is
/// worth. The second is translated beside the component, by handing it to the same
/// `MasmComponentBuilder` the component is lowered by — which is what makes it share the
/// component's `LinkInfo` rather than lay a layout of its own over it.
///
/// The one shape left out is a top-level module that **owns memory**, i.e. declares a global
/// variable or a data segment; see `report_siblings_that_own_memory` for the rule and why it is
/// where the line falls.
///
/// Every producer hands this impl a *top-level* world: the world a whole-`builtin.world` `.hir`
/// file parses to, the world `midenc_hir::parse` anchors any other top-level operation at, the
/// world the Wasm frontend builds, and the one `frontend/masm`'s disassembler builds.
impl ToMasmComponent for builtin::World {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
        namespace: EmissionNamespace<'_>,
    ) -> Result<MasmComponent, Report> {
        let mut components = Vec::new();
        let mut siblings = Vec::new();
        let mut holds_stubs = false;
        for op in self.body().entry().body().iter() {
            match op.as_operation_ref().try_downcast_op::<builtin::Component>() {
                // An external dependency, e.g. an import's stub, contributes nothing
                Ok(component) if component.borrow().is_declaration_only() => holds_stubs = true,
                Ok(component) => components.push(component),
                Err(op) => siblings.push(op),
            }
        }

        match components.len() {
            // Nothing but external dependencies: there is no definition to lower
            0 if holds_stubs && siblings.is_empty() => Err(Report::msg(LinkerError::Undefined)),
            0 => world_body_to_masm_component(self, analysis_manager, namespace),
            1 => {
                let (supporting, owns_memory) = classify_siblings(&siblings);
                // The analysis manager is rooted at the world, and `AnalysisManager::nest`
                // accepts any proper descendant, so the component impl can nest at its own
                // modules — and at these siblings, which are children of the world — exactly as
                // it does when codegen anchors it at the component itself.
                let lowered = component_to_masm_component(
                    &components[0].borrow(),
                    analysis_manager,
                    &supporting,
                    namespace,
                )?;
                // Reported after lowering succeeded, so that a build which failed for an
                // unrelated reason is not also told about a limitation it never reached.
                report_siblings_that_own_memory(self, &owns_memory);
                Ok(lowered)
            }
            _ => Err(too_many_components(self, &components)),
        }
    }
}

/// Whether `op`, a top-level item of a world, contributes nothing to the generated Miden Assembly.
///
/// This is how an *external dependency represented in the IR* is told apart from a *supporting
/// module*: the former holds declarations only, by the rule `Component::is_declaration_only`
/// applies to components (`Module::is_declaration_only`, `Interface::is_declaration_only`).
///
/// Deliberately conservative: anything unrecognized counts as carrying definitions. Guessing wrong
/// in that direction produces a warning about something that did not need one, while guessing
/// wrong in the other direction silently omits code.
fn is_declaration_only(op: &midenc_hir::OperationRef) -> bool {
    if let Ok(module) = op.try_downcast_op::<builtin::Module>() {
        module.borrow().is_declaration_only()
    } else if let Ok(interface) = op.try_downcast_op::<builtin::Interface>() {
        interface.borrow().is_declaration_only()
    } else if let Ok(function) = op.try_downcast_op::<builtin::Function>() {
        let function = function.borrow();
        function.is_declaration()
    } else {
        false
    }
}

/// Whether `module` **owns memory**, i.e. declares a `builtin::GlobalVariable` or a
/// `builtin::Segment`.
///
/// Those two are exactly the items [`Linker::link`] scans a component's modules for in order to
/// compute its data layout, and that correspondence is the whole reason a module which owns none
/// of them is safe to translate beside a component: it contributes nothing to the layout, so there
/// is nothing of the component's for it to overlay. A module which *does* own one is not safe,
/// because `Linker::link` walks only the direct module children of the component it is given and
/// so cannot see a sibling of the *world* — see `report_siblings_that_own_memory`.
///
/// Deliberately conservative in two places, both so that the failure mode stays *diagnosed* rather
/// than *silently mistranslated*:
///
/// - **Anything unrecognized counts as owning memory.** A module body is only ever legal here if
///   it holds functions, function aliases, global variables, segments, function tables and nested
///   modules — `MasmModuleBuilder::build` panics on anything else — so an item this does not
///   recognize is one that cannot be translated anyway. Treating it as owning memory is therefore
///   also what makes the answer right *at any depth*: the only container that could hold a global
///   or a segment deeper down is a nested `builtin::Module`, which is not recognized here and so
///   does not get past this. That is deliberately stricter than lowering now requires — a nested
///   module can be lowered, but a top-level sibling holding one still has no parent component to
///   own whatever memory that nesting hides, and this predicate cannot see that far.
/// - **A declared global counts, not just a defined one.** [`Linker::link`] skips declarations
///   when building the layout, so this is strictly stricter than the overlay hazard requires. It
///   is the right side to err on: a declaration whose definition is elsewhere is absent from the
///   component's layout, so lowering a use of it would panic in
///   `GlobalVariableLayout::get_computed_addr` — and it is a global variable declared by a module
///   with no parent component either way, which is the rule being enforced.
fn module_owns_memory(module: &builtin::Module) -> bool {
    module.body().entry().body().iter().any(|item| {
        // The two items a component owns, which are the two `Linker::link` scans for.
        if item.is::<builtin::GlobalVariable>() || item.is::<builtin::Segment>() {
            return true;
        }
        // Plus anything this cannot place, which includes a nested `builtin::Module` — the only
        // item that could hide a global or a segment deeper down — so this arm is the conservative
        // one, not a third case.
        // Function aliases own no memory, like functions.
        !(item.is::<builtin::Function>() || item.is::<builtin::FunctionAlias>())
    })
}

/// Split a world's top-level items, beside its component, into the ones to translate and the ones
/// to report.
///
/// Three outcomes, and everything a world can hold falls into one of them:
///
/// - an item holding only declarations is an *external dependency represented in the IR*, and is
///   dropped here — silently, because that is exactly what it is worth. This is asked *first*, so
///   a stub module whose declarations happen to include a global variable is ignored rather than
///   reported: it contributes nothing at all, which is a stronger statement than owning no memory;
/// - a `builtin::Module` owning no memory is a *supporting module*, and is returned to be
///   translated 1:1 beside the component;
/// - everything else is returned to be reported. That includes a module which owns memory, and
///   also every non-module item, which is not an oversight: `MasmComponentBuilder::define_interface`
///   places an interface *under the component's path* when the component has an id, so a world
///   sibling translated through it would be silently relocated into a namespace it does not belong
///   to. Reporting is the conservative answer until that seam is taught the difference.
fn classify_siblings(
    siblings: &[midenc_hir::OperationRef],
) -> (SmallVec<[builtin::ModuleRef; 4]>, SmallVec<[midenc_hir::OperationRef; 4]>) {
    let mut supporting = SmallVec::<[builtin::ModuleRef; 4]>::new();
    let mut owns_memory = SmallVec::<[midenc_hir::OperationRef; 4]>::new();
    for op in siblings.iter().copied() {
        if is_declaration_only(&op) {
            continue;
        }
        match op.try_downcast_op::<builtin::Module>() {
            Ok(module) if !module_owns_memory(&module.borrow()) => supporting.push(module),
            _ => owns_memory.push(op),
        }
    }
    (supporting, owns_memory)
}

/// Warn about top-level items beside a component that own memory — plus, per `classify_siblings`,
/// the ones this crate treats as if they did — and are therefore left out.
///
/// The rule, which is what the message and help exist to teach:
///
/// > Global variables and data segments are owned by a *component*. It is the component that lays
/// > out memory for them, assigns each one its address, and emits the `init` that writes them
/// > there. A module declared at the top level of a world has no parent component, so there is
/// > nothing to own its memory — which makes declaring either of them there meaningless rather
/// > than merely unsupported.
///
/// The mechanism agrees with the rule, which is why the line falls here rather than anywhere else.
/// [`Linker::link`] walks only the direct `builtin::Module` children of the operation it is handed,
/// so the [`LinkInfo`] computed for the component — `link(Some(id), <the component>)` — cannot see
/// a sibling of the *world*. `LinkInfo` is what assigns every global its address and every segment
/// its offset, so a sibling that owned memory and was lowered anyway would have to be given a
/// layout of its own, laid straight over the component's. A sibling that owns none is invisible to
/// that computation in the only sense that matters: it contributes nothing to it, so sharing the
/// component's `LinkInfo` is not an approximation but the exact answer.
///
/// Declaration-only siblings are *not* reported. They are ignored by design, and warning about
/// them would make the normal case noisy.
fn report_siblings_that_own_memory(world: &builtin::World, siblings: &[midenc_hir::OperationRef]) {
    if siblings.is_empty() {
        return;
    }

    let mut diagnostic = world
        .as_operation()
        .context()
        .diagnostics()
        .diagnostic(miden_assembly::diagnostics::Severity::Warning)
        .with_message(
            "a top-level module beside a component cannot own global variables or data segments",
        );
    // The first label has to be the primary one; the builder asserts on that ordering.
    for (index, op) in siblings.iter().enumerate() {
        let op = op.borrow();
        let label = format!("this '{}' is omitted from the generated package", op.name());
        diagnostic = if index == 0 {
            diagnostic.with_primary_label(op.span(), label)
        } else {
            diagnostic.with_secondary_label(op.span(), label)
        };
    }
    diagnostic
        .with_help(
            "global variables and data segments belong to a component: the component is what lays \
             out memory for them and emits the code that initializes it. A module declared at the \
             top level of a world has no parent component to own them, so this build omits the \
             module, and code that calls into it will fail to resolve. A top-level module that \
             declares neither is a supporting module, and is translated 1:1 to a Miden Assembly \
             module and linked into the final assembly as an ad-hoc module. Top-level items that \
             only declare symbols — external dependencies represented in the IR — contribute no \
             Miden Assembly and are ignored by design; they are not reported here. Any other \
             top-level item is reported here as well, rather than translated on a guess.",
        )
        .emit();
}

/// The report for a world declaring more than one component.
///
/// The **one** shape this impl rejects, and the blocker is external to this crate rather than a
/// gap in it: a Miden package's metadata can currently describe a single component, so a build
/// emits one component per package. Two components in a world would have to become two packages.
/// Work on multi-component packages is happening elsewhere; until it lands there is nothing this
/// crate could do with the second component but invent merge semantics, which would be worse than
/// saying so.
///
/// Note what this is *not*: a claim that worlds are single-component by nature. They are not, and
/// sibling interfaces and modules are ordinary — see the docs on `ToMasmComponent for
/// builtin::World`. Only a second *component* stops a build.
///
/// The wording matters as much as the rejection, so the message says what is unimplemented and the
/// help says who is unblocking it, rather than implying the input is wrong.
fn too_many_components(world: &builtin::World, components: &[builtin::ComponentRef]) -> Report {
    // The limitation belongs in the *message*, not only in the help: a `Report` built from a
    // diagnostic renders its message alone under `Display`, which is all a caller that only
    // formats the error ever sees.
    let mut diagnostic = world
        .as_operation()
        .context()
        .diagnostics()
        .diagnostic(miden_assembly::diagnostics::Severity::Error)
        .with_message(format!(
            "lowering a world containing {} components is not yet implemented",
            components.len()
        ))
        .with_primary_label(world.span(), "in this world");
    for component in components {
        let component = component.borrow();
        diagnostic = diagnostic.with_secondary_label(component.span(), "this component");
    }
    diagnostic
        .with_help(
            "this is a known limitation of the compiler rather than a problem with this input: a \
             Miden package's metadata can currently describe only one component, so a build emits \
             one component per package. Support for multiple components in a package is being \
             worked on; until it lands, compile each component separately.",
        )
        .into_report()
}

/// Report a function that reached codegen with no body.
///
/// Unlike [`too_many_components`], this says the **input is invalid**, not that the compiler is
/// incomplete — and it is worth being precise about why, because the three facts below are what a
/// future reader needs and none of them is obvious from the code.
///
/// **Nothing can ever provide the definition.** A body-less function is a declaration: it names a
/// procedure whose implementation is expected to come from somewhere else. Miden Assembly has no
/// such somewhere else at this point — there is no later link step that could supply it — so a
/// declaration surviving into codegen names a procedure that will never exist.
///
/// **Why a surviving declaration is assumed to be referenced.** Dead symbol elimination would
/// strip a declaration nothing refers to. There is no such pass today, but when there is, an
/// unreferenced declaration will not reach here — so a declaration that *does* reach here is one
/// something referred to, which is exactly the case that cannot be satisfied. That is why this is
/// an error rather than an item to skip: skipping it would emit a module whose callers reference a
/// procedure it does not define, and the failure would surface at link time with nothing to point
/// at.
///
/// **Why this is not checked before codegen.** It is an invariant of Miden Assembly, not of the
/// IR. A body-less `builtin::Function` is a perfectly well-formed operation — verification cannot
/// reject it without rejecting every legitimate declaration — so the check can only live where the
/// IR is being turned into something that has to be complete. Moving it into verification will not
/// work; this is the note that saves the next person the attempt.
fn function_without_a_body(function: &builtin::Function) -> Report {
    // The reason belongs in the *message*, not only the help: a `Report` built from a diagnostic
    // renders its message alone under `Display`, which is all a caller that merely formats the
    // error ever sees. See the same note on `too_many_components`.
    function
        .as_operation()
        .context()
        .diagnostics()
        .diagnostic(miden_assembly::diagnostics::Severity::Error)
        .with_message(
            "cannot emit masm for a function with no body: nothing can provide its definition",
        )
        .with_primary_label(function.span(), "this function is declared but never defined")
        .with_help(
            "a declaration names a procedure whose implementation comes from elsewhere, and Miden \
             Assembly has no later step that could supply one. Either give this function a body, \
             or remove it along with whatever refers to it.",
        )
        .into_report()
}

/// Derive a MASM component by treating `world`'s body as a component body.
///
/// The meaning a world has always had here, and correct only when the world declares no
/// component of its own beside declaration-only ones (see `World::has_defining_component`): every
/// definition-carrying module in it belongs to one logical component, which has no identity
/// beyond the namespace those modules sit in.
fn world_body_to_masm_component(
    world: &builtin::World,
    analysis_manager: AnalysisManager,
    namespace: EmissionNamespace<'_>,
) -> Result<MasmComponent, Report> {
    // Get the current compiler context
    let context = world.as_operation().context_rc();

    // Run the linker for this component in order to compute its data layout
    let link_info = Linker::default().link(None, world.as_operation()).map_err(Report::msg)?;

    // Get the entrypoint, if specified
    let entrypoint = match context.session().options.entrypoint.as_deref() {
        Some(entry) => {
            let entry_id = entry
                .parse::<FunctionIdent>()
                .map_err(|_| Report::msg(format!("invalid entrypoint identifier: '{entry}'")))?;
            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
                entry_id.function.span,
                entry_id.function.as_str().into(),
            )));

            let path = LibraryPath::new(entry_id.module.as_str())
                .into_diagnostic()?
                .to_absolute()
                .map_err(|error| Report::msg(error.to_string()))?
                .into_owned();
            let qualified = masm::QualifiedProcedureName::new(path.as_path(), name);
            Some(masm::InvocationTarget::Path(Span::new(
                entry_id.function.span,
                qualified.into_inner(),
            )))
        }
        None => None,
    };
    let entrypoint_arity =
        resolve_entrypoint_arity(world.as_operation_ref(), &[], entrypoint.as_ref(), namespace)?;
    let owners = EmissionOwners::new(
        world.as_operation_ref(),
        &[],
        &world_root_module(world),
        namespace,
        false,
        &link_info,
    )?;
    let entrypoint = entrypoint.map(|target| owners.target(target)).transpose()?;

    // If we have global variables, data segments, function tables, or a core Wasm start, we will
    // require a component initializer function, as well as a module to hold component-level
    // functions such as init
    let requires_init = link_info.requires_init();
    let root: Arc<masm::Path> =
        owners.module(world.as_operation_ref()).to_path_buf().into_boxed_path().into();
    let init = if requires_init {
        let name = masm::ProcedureName::new(COMPONENT_INIT_PROCEDURE).unwrap();
        let qualified = masm::QualifiedProcedureName::new(&root, name);
        Some(masm::InvocationTarget::Path(Span::new(
            SourceSpan::default(),
            qualified.into_inner(),
        )))
    } else {
        None
    };

    // Define the initial component modules set
    //
    // The top-level component module is always defined, but may be empty
    let init_module = Arc::new(masm::Module::new(masm::ModuleKind::Library, &root));
    let modules = vec![init_module];

    let rodata = data_segments_to_rodata(&link_info)?;

    let heap_base = link_info.heap_base();
    let stack_pointer = link_info.globals_layout().stack_pointer_offset();
    let mut masm_component = MasmComponent {
        id: None,
        // A world declaring no component is not the compiler's wrapper around one: it has no
        // component boundary at all.
        synthetic_wrapper: false,
        root,
        init,
        entrypoint,
        entrypoint_arity,
        prepared_target: match namespace {
            EmissionNamespace::Source => None,
            EmissionNamespace::Target(target) => {
                Some((target.namespace.inner().clone(), target.is_executable()))
            }
        },
        rodata,
        heap_base,
        stack_pointer,
        modules,
    };
    let builder = MasmComponentBuilder {
        analysis_manager,
        component: &mut masm_component,
        owners: &owners,
        link_info: &link_info,
        source_manager: context.session().source_manager.clone(),
        init_body: Default::default(),
    };

    // A world declaring no component has no siblings *beside* one: every top-level module in it
    // is part of the one logical component this treats its body as.
    builder
        .build(world.as_operation(), &[])
        .map_err(|report| super::report_planning_failure(report, &context.session().diagnostics))?;

    Ok(masm_component)
}

/// 1:1 conversion from HIR component to MASM component
impl ToMasmComponent for builtin::Component {
    fn to_masm_component(
        &self,
        analysis_manager: AnalysisManager,
        namespace: EmissionNamespace<'_>,
    ) -> Result<MasmComponent, Report> {
        component_to_masm_component(self, analysis_manager, &[], namespace)
    }
}

/// Derive a MASM component from `component`, and from `supporting` beside it.
///
/// `supporting` is empty for a standalone `builtin.component`, which has no siblings; it is
/// non-empty only when the component was reached through the world that declares it, and holds
/// that world's supporting modules — see `ToMasmComponent for builtin::World`.
///
/// They are lowered *here*, rather than by a second pass of their own, for one reason: the
/// [`LinkInfo`] computed below is the component's data layout, and a supporting module has to be
/// lowered against it. `classify_siblings` is what makes that exact rather than approximate — a
/// module that owns no memory contributes nothing to the layout, so the component's `LinkInfo` is
/// the same one a link over the whole world would have produced.
fn component_to_masm_component(
    component: &builtin::Component,
    analysis_manager: AnalysisManager,
    supporting: &[builtin::ModuleRef],
    namespace: EmissionNamespace<'_>,
) -> Result<MasmComponent, Report> {
    // Get the current compiler context
    let context = component.as_operation().context_rc();

    // Whether this component is one the compiler invented to wrap a bare core module, which it
    // says by carrying a marker the frontend set rather than by its name — see
    // `builtin::Component::SYNTHETIC_WRAPPER_ATTR`.
    let synthetic_wrapper = component.is_synthetic_wrapper();

    // Run the linker for this component in order to compute its data layout
    let id = component.namespace_path();
    let link_info = Linker::default()
        .link(Some(id.clone()), component.as_operation())
        .map_err(Report::msg)?;

    // Get the library path of the component
    let component_path = id
        .to_library_path()
        .to_absolute()
        .map_err(|err| {
            Report::msg(format!("unable to canonicalize '{}': {err}", id.to_library_path()))
        })?
        .into_owned();

    // Get the entrypoint, if specified
    let entrypoint = match context.session().options.entrypoint.as_deref() {
        Some(entry) => {
            let entry_id = entry
                .parse::<FunctionIdent>()
                .map_err(|_| Report::msg(format!("invalid entrypoint identifier: '{entry}'")))?;
            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
                entry_id.function.span,
                entry_id.function.as_str().into(),
            )));

            // Check if we're inside the synthetic "wrapper" component used for pure Rust
            // compilation. Since the user does not know about it, their entrypoint does not
            // include the synthetic component path. We append the user-provided path to the
            // root component path here if needed.
            //
            // TODO(pauls): Narrow this to only be true if the target env is not 'rollup', we
            // cannot currently do so because we do not have sufficient Cargo metadata yet in
            // 'cargo miden build' to detect the target env, and we default it to 'rollup'
            let path = if synthetic_wrapper {
                component_path.join(entry_id.module.as_str())
            } else {
                // We're compiling a Wasm component and the component's namespace path is
                // included in the entrypoint.
                LibraryPath::new(entry_id.module.as_str())
                    .into_diagnostic()?
                    .to_absolute()
                    .map_err(|error| Report::msg(error.to_string()))?
                    .into_owned()
            };
            let qualified = masm::QualifiedProcedureName::new(path.as_path(), name);
            Some(masm::InvocationTarget::Path(Span::new(
                entry_id.function.span,
                qualified.into_inner(),
            )))
        }
        None => None,
    };
    let entrypoint_arity = resolve_entrypoint_arity(
        component.as_operation_ref(),
        supporting,
        entrypoint.as_ref(),
        namespace,
    )?;
    let owners = EmissionOwners::new(
        component.as_operation_ref(),
        supporting,
        &component_path,
        namespace,
        true,
        &link_info,
    )?;
    let entrypoint = entrypoint.map(|target| owners.target(target)).transpose()?;

    // If we have global variables, data segments, function tables, or a core Wasm start, we will
    // require a component initializer function, as well as a module to hold component-level
    // functions such as init
    let requires_init = link_info.requires_init();
    let init = if requires_init {
        let name = masm::ProcedureName::new(COMPONENT_INIT_PROCEDURE).unwrap();
        let qualified =
            masm::QualifiedProcedureName::new(owners.module(component.as_operation_ref()), name);
        Some(masm::InvocationTarget::Path(Span::new(
            SourceSpan::default(),
            qualified.into_inner(),
        )))
    } else {
        None
    };

    // Define the initial component modules set
    //
    // The top-level component module is always defined, but may be empty
    let root: Arc<miden_assembly_syntax::Path> = owners
        .module(component.as_operation_ref())
        .to_path_buf()
        .into_boxed_path()
        .into();
    let root_module = Arc::new(masm::Module::new(masm::ModuleKind::Library, &root));
    let modules = vec![root_module];

    let rodata = data_segments_to_rodata(&link_info)?;

    let heap_base = link_info.heap_base();
    let stack_pointer = link_info.globals_layout().stack_pointer_offset();
    let mut masm_component = MasmComponent {
        id: Some(id),
        synthetic_wrapper,
        root,
        init,
        entrypoint,
        entrypoint_arity,
        prepared_target: match namespace {
            EmissionNamespace::Source => None,
            EmissionNamespace::Target(target) => {
                Some((target.namespace.inner().clone(), target.is_executable()))
            }
        },
        rodata,
        heap_base,
        stack_pointer,
        modules,
    };
    let builder = MasmComponentBuilder {
        analysis_manager,
        component: &mut masm_component,
        owners: &owners,
        link_info: &link_info,
        source_manager: context.session().source_manager.clone(),
        init_body: Default::default(),
    };

    builder
        .build(component.as_operation(), supporting)
        .map_err(|report| super::report_planning_failure(report, &context.session().diagnostics))?;

    Ok(masm_component)
}

fn data_segments_to_rodata(link_info: &LinkInfo) -> Result<Vec<crate::Rodata>, Report> {
    use midenc_hir::constants::ConstantData;

    use crate::data_segments::{ResolvedDataSegment, merge_data_segments};
    let mut resolved = SmallVec::<[ResolvedDataSegment; 2]>::new();
    for sref in link_info.segment_layout().iter() {
        let s = sref.borrow();
        resolved.push(ResolvedDataSegment {
            offset: *s.get_offset(),
            data: s.initializer().as_slice().to_vec(),
            readonly: *s.get_readonly(),
        });
    }
    Ok(match merge_data_segments(resolved).map_err(Report::msg)? {
        None => alloc::vec::Vec::new(),
        Some(merged) => {
            let data = alloc::sync::Arc::new(ConstantData::from(merged.data));
            let felts = crate::Rodata::bytes_to_elements(data.as_slice());
            let digest = miden_core::crypto::hash::Poseidon2::hash_elements(&felts);
            alloc::vec![crate::Rodata {
                component: link_info
                    .component()
                    .cloned()
                    // A world without a component has no namespace; the segment only needs an
                    // owner label, which nothing resolves as a path.
                    .unwrap_or_else(|| SymbolPath::from_masm_module_id("root")),
                digest,
                start: super::NativePtr::from_ptr(merged.offset),
                data,
            }]
        }
    })
}

/// Resolve the ABI used by a generated program's root invocation.
///
/// Library targets may select an export as their entrypoint without generating a program main.
/// Their exports retain their own calling conventions and need no root-invocation arity. A
/// target-independent consumer may still package an executable, so Source mode retains validation.
fn resolve_entrypoint_arity(
    root: midenc_hir::OperationRef,
    supporting: &[builtin::ModuleRef],
    target: Option<&masm::InvocationTarget>,
    namespace: EmissionNamespace<'_>,
) -> Result<Option<(u32, u32)>, Report> {
    if matches!(namespace, EmissionNamespace::Target(target) if !target.is_executable()) {
        return Ok(None);
    }
    let Some(target) = target else {
        return Ok(None);
    };
    let mut found = None;
    let mut find = |op: &midenc_hir::Operation| {
        let function = if let Some(alias) = op.downcast_ref::<builtin::FunctionAlias>() {
            if qualified_invocation_target(alias, alias.span()) != *target {
                return;
            }
            alias.resolve_target().and_then(builtin::FunctionAlias::canonicalize_function)
        } else if let Some(function) = op.downcast_ref::<builtin::Function>() {
            (qualified_invocation_target(function, function.span()) == *target)
                .then(|| function.as_function_ref())
        } else {
            None
        };
        if let Some(function) = function {
            found = Some(function);
        }
    };
    root.borrow().prewalk_all(&mut find);
    for module in supporting {
        module.borrow().as_operation().prewalk_all(&mut find);
    }
    let function = found.ok_or_else(|| {
        Report::msg(format!("unable to resolve executable entrypoint '{target}'"))
    })?;
    let function = function.borrow();
    let signature = function.signature();
    if !matches!(
        signature.cc,
        midenc_hir::CallConv::C | midenc_hir::CallConv::Fast | midenc_hir::CallConv::Wasm
    ) || crate::lower::units::calls::is_kernel(&*function)
    {
        return Err(Report::msg(
            "executable entrypoint requires a same-context calling convention",
        ));
    }
    let lanes =
        |types: &[midenc_hir::dialects::builtin::attributes::AbiParam]| -> Result<u32, Report> {
            types.iter().try_fold(0u32, |n, p| {
                n.checked_add(
                    u32::try_from(p.ty.size_in_felts())
                        .map_err(|_| Report::msg("entrypoint layout exceeds target"))?,
                )
                .ok_or_else(|| Report::msg("entrypoint layout exceeds target"))
            })
        };
    Ok(Some((lanes(signature.params())?, lanes(signature.results())?)))
}

struct MasmComponentBuilder<'a> {
    owners: &'a EmissionOwners,
    component: &'a mut MasmComponent,
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
    source_manager: Arc<dyn midenc_session::SourceManager>,
    init_body: Vec<crate::select::generated::Initialization>,
}

impl MasmComponentBuilder<'_> {
    /// Convert the component body to Miden Assembly, along with any `supporting` modules that sit
    /// beside the component in the world that declares it.
    pub fn build(
        mut self,
        component: &midenc_hir::Operation,
        supporting: &[builtin::ModuleRef],
    ) -> Result<(), Report> {
        // Validate exactly the operations this builder will emit. In particular, a world may
        // contain declaration-only or memory-owning siblings which codegen deliberately omits;
        // invalid roots in those items must not mask the established omission diagnostic.
        crate::legalization::validate_procedure_roots(component)?;
        for module in supporting {
            crate::legalization::validate_procedure_roots(module.borrow().as_operation())?;
        }

        if self.component.init.is_some() {
            self.init_body.push(crate::select::generated::Initialization::Heap {
                base: self.component.heap_base,
            });
            self.emit_data_segment_initialization();
        }

        // Translate component body
        let region = component.region(0);
        let block = region.entry();
        for op in block.body() {
            if let Some(module) = op.downcast_ref::<builtin::Module>() {
                self.define_module(module)?;
            } else if let Some(interface) = op.downcast_ref::<builtin::Interface>() {
                self.define_interface(interface)?;
            } else if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else if let Some(alias) = op.downcast_ref::<builtin::FunctionAlias>() {
                self.define_function_alias(alias)?;
            } else if op
                .downcast_ref::<builtin::Component>()
                .is_some_and(builtin::Component::is_declaration_only)
            {
                // An external dependency beside the modules of a world lowered as one component
            } else {
                panic!(
                    "invalid component-level operation: '{}' is not supported in a component body",
                    op.name()
                )
            }
        }

        // Translate the supporting modules beside the component, into the same set of modules.
        //
        // `define_module` roots a module whose path does not begin with the component's at the top
        // level, which is exactly where these belong, so they end up as siblings of the component
        // root rather than children of it — and therefore in `support` when
        // `MasmComponent::source_inputs` splits the set. None of them can contribute to `init`:
        // `MasmModuleBuilder` only ever appends to it for a global variable, and a module that
        // declared one would not be here.
        for module in supporting {
            self.define_module(&module.borrow())?;
        }

        // Finalize the component-level init, if required
        if self.component.init.is_some() {
            // Function tables are initialized from the modules that define their callees, so this
            // has to run after those modules exist — which is why it is here rather than beside
            // the heap and data segment initialization above. Each fragment invokes the fragments
            // of the modules nested within it; `init` only reaches the outermost ones.
            let fragments = self.build_function_table_fragments()?;
            let owners = fragments.keys().cloned().collect::<Vec<_>>();
            let mut child_calls: BTreeMap<masm::PathBuf, Vec<masm::PathBuf>> = BTreeMap::new();
            let mut roots: Vec<masm::PathBuf> = Vec::new();
            for owner in owners.iter() {
                // The nearest fragment-bearing ancestor, if any: a module's fragment is reached
                // from its closest enclosing one, and only the modules with no such enclosing
                // fragment are reached from `init`. `Path::starts_with_exactly` matches
                // component-wise, so `::a::bc` is not an ancestor's descendant merely because
                // the text of `::a::b` is a prefix of it.
                //
                // A root is only reachable if `init`, which lives in the component root, may
                // `exec` into it — so a *private* nested module whose parent defines no table
                // callee is a root `init` cannot reach: MASM visibility lets a module reach its
                // own children and its siblings, not a private grandchild. Nothing produces that
                // shape today (the Wasm frontend puts every core callee in one module, and a
                // private nested module reached from the component root would be unreachable for
                // ordinary calls too), and it fails loudly at assembly time rather than silently
                // leaving the slots zeroed, so it is recorded here rather than worked around. The
                // fix, if it ever arises, is to give the parent an empty fragment to relay
                // through rather than to promote anyone's visibility.
                match owners
                    .iter()
                    .filter(|candidate| {
                        *candidate != owner
                            && owner.as_path().starts_with_exactly(candidate.as_path())
                    })
                    .max_by_key(|candidate| candidate.as_path().components().count())
                {
                    Some(parent) => {
                        child_calls.entry(parent.clone()).or_default().push(owner.clone())
                    }
                    None => roots.push(owner.clone()),
                }
            }

            let proc_name = masm::ProcedureName::new(FUNCTION_TABLE_INIT_PROC).unwrap();
            let span = component.span();
            for (owner, mut fragment) in fragments {
                for child in child_calls.remove(&owner).unwrap_or_default() {
                    let qualified =
                        masm::QualifiedProcedureName::new(child.as_path(), proc_name.clone());
                    fragment
                        .children
                        .push(InvocationTarget::Path(Span::new(span, qualified.into_inner())));
                }
                let Some(index) = self
                    .component
                    .modules
                    .iter()
                    .position(|module| module.path() == owner.as_path())
                else {
                    return Err(Report::msg(format!(
                        "invalid function table entry: callee owner '{owner}' was not lowered \
                         because it holds only declarations"
                    )));
                };
                let env = crate::select::SelectEnv {
                    link_info: Some(self.link_info),
                    owners: Some(self.owners),
                };
                let source = crate::select::generated::GeneratedInitializer {
                    stages: vec![crate::select::generated::Initialization::Table(fragment)],
                    env: &env,
                };
                let mut procedure = finalize_evidence(
                    source.evidence()?,
                    crate::verify::SourceContext::Generated(&source),
                    proc_name.clone(),
                    span,
                )?;
                procedure.set_visibility(masm::Visibility::Public);
                procedure.set_signature(masm::FunctionType::new(
                    midenc_hir::CallConv::Fast,
                    vec![],
                    vec![],
                ));
                Arc::get_mut(&mut self.component.modules[index])
                    .expect("expected unique reference")
                    .define_procedure(procedure, self.source_manager.clone())
                    .into_diagnostic()
                    .wrap_err("failed to define a function table initializer")?;
            }
            for root in roots {
                let qualified =
                    masm::QualifiedProcedureName::new(root.as_path(), proc_name.clone());
                self.init_body.push(crate::select::generated::Initialization::Invoke(
                    InvocationTarget::Path(Span::new(span, qualified.into_inner())),
                ));
            }
            if let Some(start) = self.link_info.component_start() {
                let start = start.borrow();
                self.init_body.push(crate::select::generated::Initialization::Invoke(
                    self.owners.target(qualified_invocation_target(&*start, start.span()))?,
                ));
            }
            let env = crate::select::SelectEnv {
                link_info: Some(self.link_info),
                owners: Some(self.owners),
            };
            let source = crate::select::generated::GeneratedInitializer {
                stages: core::mem::take(&mut self.init_body),
                env: &env,
            };
            let mut init = finalize_evidence(
                source.evidence()?,
                crate::verify::SourceContext::Generated(&source),
                masm::ProcedureName::new(COMPONENT_INIT_PROCEDURE).unwrap(),
                span,
            )?;
            init.set_visibility(masm::Visibility::Public);
            init.set_signature(masm::FunctionType::new(midenc_hir::CallConv::Fast, vec![], vec![]));
            Arc::get_mut(&mut self.component.modules[0])
                .expect("expected unique reference")
                .define_procedure(init, self.source_manager.clone())
                .into_diagnostic()
                .wrap_err("failed to define component `init` procedure")?;
        } else {
            assert!(
                self.init_body.is_empty(),
                "the need for an 'init' function was not expected, but code was generated for one"
            );
        }

        Ok(())
    }

    fn define_interface(&mut self, interface: &builtin::Interface) -> Result<(), Report> {
        let interface_path = self.owners.module(interface.as_operation_ref());
        // The assembler exposes interface functions and re-exports only through public
        // submodule declarations reachable from the package root.
        let module_index = if let Some(rest) = interface_path.strip_prefix(&self.component.root) {
            self.define_module_tree(rest, Some(0), masm::Visibility::Public)?
        } else {
            self.define_module_tree(interface_path, None, masm::Visibility::Public)?
        };
        let masm_module = Arc::get_mut(&mut self.component.modules[module_index])
            .expect("expected unique reference");
        let builder = MasmModuleBuilder {
            owners: self.owners,
            module: masm_module,
            analysis_manager: self.analysis_manager.clone(),
            link_info: self.link_info,
            source_manager: self.source_manager.clone(),
            init_body: &mut self.init_body,
        };
        builder.build_from_interface(interface)
    }

    fn define_module(&mut self, module: &builtin::Module) -> Result<(), Report> {
        let module_path = self.owners.module(module.as_operation_ref());
        let trace_target = TraceTarget::category("codegen");
        log::debug!(target: &trace_target, "defining module '{module_path}'");
        // The submodule declaration's visibility decides whether the module's public procedures
        // belong to the public surface of the assembled package: the assembler derives that
        // surface from the modules reachable from the root through *public* submodule
        // declarations. Core modules are private in HIR, so their public procedures stay
        // resolvable package-internally (a private submodule is visible to its parent and siblings)
        // without becoming part of the package's interface.
        //
        // Two of the shapes reaching here have no component boundary to speak of, and in both
        // the modules *are* the artifact's interface, so they keep public submodules. A world
        // lowered without a component namespace is one. The other is the wrapper the compiler
        // invents around a bare core module, which is not a real boundary either: the wrapped
        // module is the artifact's own interface (the entrypoint of an executable, or the exports
        // of a bare library), and the generated executable `main` module lives outside the
        // wrapper's module tree. That the wrapper is the compiler's is something it *says* — the
        // frontend marks it (`builtin::Component::SYNTHETIC_WRAPPER_ATTR`) — rather than
        // something read off its id, which is a name an author may write. An authored component
        // is the complement, and keeps the visibility its author declared.
        let is_artifact_interface = self.component.id.is_none() || self.component.synthetic_wrapper;
        let visibility = if is_artifact_interface {
            masm::Visibility::Public
        } else {
            lower_visibility(*module.get_visibility())
        };
        let module_index = if let Some(rest) = module_path.strip_prefix(&self.component.root) {
            self.define_module_tree(rest, Some(0), visibility)?
        } else {
            self.define_module_tree(module_path, None, visibility)?
        };

        let masm_module = Arc::get_mut(&mut self.component.modules[module_index])
            .expect("expected unique reference");
        let builder = MasmModuleBuilder {
            owners: self.owners,
            module: masm_module,
            analysis_manager: self.analysis_manager.clone(),
            link_info: self.link_info,
            source_manager: self.source_manager.clone(),
            init_body: &mut self.init_body,
        };
        let nested = builder.build(module)?;
        for nested_module in nested {
            self.define_module(&nested_module.borrow())?;
        }

        Ok(())
    }

    fn define_module_tree(
        &mut self,
        module_path: &masm::Path,
        mut parent: Option<usize>,
        visibility: masm::Visibility,
    ) -> Result<usize, Report> {
        let trace_target = TraceTarget::category("codegen");
        let mut path = masm::PathBuf::with_capacity(256);
        if let Some(parent) = parent {
            path = self.component.modules[parent].path().to_path_buf();
        }
        let mut components = module_path.components().peekable();
        while let Some(component) = components.next() {
            let name = component.unwrap().as_str();
            // Ignore the root component
            if name == "::" {
                continue;
            }
            path.push_component(name);
            if !path.is_absolute() {
                path = path.to_absolute().unwrap().into_owned();
            }
            // Use the input visibility for the last module we crate, for parent modules, we must
            // specify public visibility so that references to this module are valid.
            let visibility = if components.peek().is_none() {
                visibility
            } else {
                masm::Visibility::Public
            };
            let module_path = &path;
            if let Some(parent_index) = parent {
                let parent_module = Arc::get_mut(&mut self.component.modules[parent_index])
                    .expect("expected unique reference");
                if parent_module.submodules().iter().any(|sm| sm.name.as_str() == name) {
                    // Already defined, look up the submodule as the new `parent`
                    parent = Some(
                        self.component
                            .modules
                            .iter()
                            .position(|m| m.path() == module_path.as_path())
                            .expect(
                                "submodule was already defined, but not registered with component",
                            ),
                    );
                } else {
                    // Create the submodule
                    let submodule =
                        Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
                    let name = masm::Ident::new(submodule.name()).map_err(|err| {
                        Report::msg(format!(
                            "invalid module name `{}` in `{module_path}`: {err}",
                            submodule.name()
                        ))
                    })?;
                    log::debug!(target: &trace_target, "declaring submodule '{name}' of '{}'", parent_module.path());
                    parent_module.declare_submodule(name, visibility)?;
                    parent = Some(self.component.modules.len());
                    self.component.modules.push(Arc::from(submodule));
                }
            } else {
                log::debug!(target: &trace_target, "declaring module '{module_path}'");
                let module = Box::new(masm::Module::new(masm::ModuleKind::Library, module_path));
                parent = Some(self.component.modules.len());
                self.component.modules.push(Arc::from(module));
            }
        }

        Ok(parent.unwrap())
    }

    fn define_function(&mut self, function: &builtin::Function) -> Result<(), Report> {
        let builder = MasmFunctionBuilder::new(function)?;
        let procedure = builder.build(
            function,
            self.analysis_manager.nest(function.as_operation_ref()),
            self.link_info,
            self.owners,
        )?;

        let module =
            Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");
        assert_eq!(
            module.path(),
            &*self.component.root,
            "expected top-level namespace module, but one has not been defined (in '{}' of '{}')",
            module.path(),
            function.path()
        );
        module
            .define_procedure(procedure, self.source_manager.clone())
            .into_diagnostic()
            .wrap_err("failed to define MASM procedure")?;

        Ok(())
    }

    fn define_function_alias(&mut self, alias: &builtin::FunctionAlias) -> Result<(), Report> {
        let import = lower_function_alias(alias, self.owners)?;
        let module =
            Arc::get_mut(&mut self.component.modules[0]).expect("expected unique reference");
        module
            .define_import(import)
            .into_diagnostic()
            .wrap_err("failed to define MASM import for alias")
    }

    /// Emit the sequence of instructions necessary to consume rodata from the advice stack and
    /// populate the global heap with the data segments of this component, verifying that the
    /// commitments match.
    fn emit_data_segment_initialization(&mut self) {
        for rodata in &self.component.rodata {
            self.init_body.push(crate::select::generated::Initialization::Data {
                commitment: rodata.digest.as_elements().try_into().expect("digest word"),
                destination: rodata.start.addr,
                words: u32::try_from(rodata.size_in_words()).expect("linked data size"),
            });
        }
    }

    /// Build the slot-initialization code for every function table in the component, grouped by
    /// the module that *defines the callee* whose MAST root fills each slot.
    ///
    /// Grouping by callee rather than by table is what keeps every `procref` intra-module: a
    /// table in one module may name a callee in another, and it is the `procref` — not the
    /// store — that the assembler resolves against visibility.
    ///
    /// Only the entries [`builtin::FunctionTable::live_entries`] considers live are written: a
    /// later entry at the same index overwrites an earlier one, so the earlier one is dead. That
    /// is a soundness requirement rather than a saving. The `hir.exec_indirect` verifier compares
    /// signatures only for the entry that wins a slot, so a dead entry's callee has never been
    /// checked against any call site's stack contract — and grouping by owning module means store
    /// order no longer follows the entries' textual order, so "the last store wins" would not even
    /// pick the entry the verifier looked at.
    ///
    /// Uninitialized (null) slots are left as the zero word, since VM memory is
    /// zero-initialized; `dynexec` on such a slot fails at runtime.
    ///
    /// Each fragment carries the `procref`s its stores consume, which its procedure declares as
    /// invocations: that set is what the assembler's linker reads to build its call graph. It is
    /// a declaration of the dependency, not what creates it — the linker also resolves an
    /// invocation target from the instruction itself, so an omission here is a call graph missing
    /// an edge rather than an unresolved symbol.
    fn build_function_table_fragments(
        &self,
    ) -> Result<BTreeMap<masm::PathBuf, crate::select::generated::TableFragment>, Report> {
        let mut fragments = BTreeMap::new();
        let layout = self.link_info.function_tables();
        for (table_ref, _) in layout.iter() {
            let base = layout.element_addr_of(table_ref).expect("linked function table");
            let table = table_ref.borrow();
            for (slot, entry) in table.live_entries().map_err(|op| {
                Report::msg(format!("invalid function table entry: '{op}' is unsupported"))
            })? {
                let entry = entry.borrow();
                if slot >= *table.get_num_slots() || *entry.get_type_tag() == 0 {
                    return Err(Report::msg("invalid function table slot or null signature tag"));
                }
                let callee = entry
                    .resolve_callee()
                    .ok_or_else(|| Report::msg("unresolved function table callee"))?;
                let callee = callee.borrow();
                let owner = callee
                    .as_symbol_operation()
                    .nearest_symbol_table()
                    .ok_or_else(|| Report::msg("table callee has no owner"))?;
                let emitted_owner = self.owners.modules.get(&owner).ok_or_else(|| {
                    Report::msg(format!(
                        "invalid function table entry: callee '{}' is defined in module '{}', \
                         which was not lowered because it holds only declarations",
                        callee.path(),
                        symbol_module_path(&*callee)
                    ))
                })?;
                let fragment = fragments.entry(emitted_owner.clone()).or_insert_with(|| {
                    crate::select::generated::TableFragment {
                        owner,
                        tables: vec![],
                        children: vec![],
                    }
                });
                if !fragment.tables.iter().any(|(table, _)| *table == table_ref) {
                    fragment.tables.push((table_ref, base));
                }
            }
        }
        Ok(fragments)
    }
}

/// Resolve an alias to the symbol its MASM import should reference.
///
/// Stop at the first symbol in another module so the import targets a symbol available through
/// that module.
fn resolve_alias_import_target(alias: &builtin::FunctionAlias) -> Result<SymbolRef, Report> {
    alias
        .as_operation()
        .as_symbol_ref()
        .expect("function aliases are symbols")
        .resolve_function()
        .into_diagnostic()?;

    let owner = alias
        .as_operation()
        .nearest_symbol_table()
        .ok_or(SymbolResolutionError::NoSymbolTable)
        .into_diagnostic()?;
    let mut target = alias.resolve_target().expect("canonical resolution validated the target");
    loop {
        let symbol = target.borrow();
        let target_owner = symbol
            .as_symbol_operation()
            .nearest_symbol_table()
            .ok_or(SymbolResolutionError::NoSymbolTable)
            .into_diagnostic()?;
        if !crate::legalization::share_masm_module(owner, target_owner) {
            return Ok(target);
        }
        let Some(local_alias) =
            symbol.as_symbol_operation().downcast_ref::<builtin::FunctionAlias>()
        else {
            return Ok(target);
        };
        target = local_alias
            .resolve_target()
            .expect("canonical resolution validated the local alias chain");
    }
}

fn lower_visibility(visibility: midenc_hir::Visibility) -> masm::Visibility {
    use midenc_hir::Visibility;

    match visibility {
        Visibility::Public => masm::Visibility::Public,
        Visibility::Internal => masm::Visibility::Internal,
        Visibility::Private => masm::Visibility::Private,
    }
}

fn lower_function_alias(
    alias: &builtin::FunctionAlias,
    owners: &EmissionOwners,
) -> Result<masm::Import, Report> {
    let target = resolve_alias_import_target(alias)?;
    let target = target.borrow();
    let owner = alias.as_operation().nearest_symbol_table().unwrap();
    let target_owner = target.as_symbol_operation().nearest_symbol_table().unwrap();
    let module_path = if crate::legalization::share_masm_module(owner, target_owner) {
        Arc::from(masm::Path::new("self"))
    } else {
        Arc::from(
            owners
                .modules
                .get(&target_owner)
                .cloned()
                .unwrap_or_else(|| symbol_module_path(&*target))
                .into_boxed_path(),
        )
    };
    let visibility = lower_visibility(alias.visibility());
    let span = alias.span();
    let source_name = masm::Ident::from_raw_parts(Span::new(span, target.name().as_str().into()));
    let local_name = masm::Ident::from_raw_parts(Span::new(span, alias.name().as_str().into()));
    Ok(masm::Import::Item(masm::ItemImport::new(
        span,
        visibility,
        Span::new(span, module_path),
        source_name,
        local_name,
    )))
}

struct MasmModuleBuilder<'a> {
    owners: &'a EmissionOwners,
    module: &'a mut masm::Module,
    /// Shared world/component analysis root, covering cross-module alias targets.
    analysis_manager: AnalysisManager,
    link_info: &'a LinkInfo,
    source_manager: Arc<dyn midenc_session::SourceManager>,
    init_body: &'a mut Vec<crate::select::generated::Initialization>,
}

impl MasmModuleBuilder<'_> {
    /// Lower `module`'s body, returning any modules nested within it.
    ///
    /// A nested module is not lowered here: MASM's module set is flat and keyed by path, and
    /// [`MasmComponentBuilder::define_module`] is what turns a fully-qualified HIR module path
    /// into that set's entry. Returning them lets the component builder recurse without this
    /// builder having to know how modules are rooted.
    pub fn build(mut self, module: &builtin::Module) -> Result<Vec<builtin::ModuleRef>, Report> {
        let mut nested = Vec::new();
        let region = module.body();
        let block = region.entry();
        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else if let Some(alias) = op.downcast_ref::<builtin::FunctionAlias>() {
                self.define_function_alias(alias)?;
            } else if let Some(gv) = op.downcast_ref::<builtin::GlobalVariable>() {
                self.emit_global_variable_initializer(gv)?;
            } else if let Some(nested_module) = op.downcast_ref::<builtin::Module>() {
                nested.push(nested_module.as_module_ref());
            } else if op.is::<builtin::Segment>() {
                continue;
            } else if op.is::<builtin::FunctionTable>() {
                // Laid out by the linker; slots are filled by the `__init_function_table`
                // procedures `MasmComponentBuilder::build` attaches to the modules defining the
                // callees, from fragments `build_function_table_fragments` produces
                continue;
            } else {
                panic!(
                    "invalid module-level operation: '{}' is not legal in a MASM module body",
                    op.name()
                )
            }
        }

        Ok(nested)
    }

    pub fn build_from_interface(mut self, interface: &builtin::Interface) -> Result<(), Report> {
        let region = interface.body();
        let block = region.entry();
        for op in block.body() {
            if let Some(function) = op.downcast_ref::<builtin::Function>() {
                self.define_function(function)?;
            } else if let Some(alias) = op.downcast_ref::<builtin::FunctionAlias>() {
                self.define_function_alias(alias)?;
            } else {
                panic!(
                    "invalid interface-level operation: '{}' is not legal in a MASM module body",
                    op.name()
                )
            }
        }

        Ok(())
    }

    fn define_function(&mut self, function: &builtin::Function) -> Result<(), Report> {
        let builder = MasmFunctionBuilder::new(function)?;

        let procedure = builder.build(
            function,
            self.analysis_manager.nest(function.as_operation_ref()),
            self.link_info,
            self.owners,
        )?;

        self.module
            .define_procedure(procedure, self.source_manager.clone())
            .map_err(|e| Report::msg(e.to_string()))?;

        Ok(())
    }

    fn define_function_alias(&mut self, alias: &builtin::FunctionAlias) -> Result<(), Report> {
        self.module
            .define_import(lower_function_alias(alias, self.owners)?)
            .into_diagnostic()
            .wrap_err("failed to define MASM import for alias")
    }

    fn emit_global_variable_initializer(
        &mut self,
        gv: &builtin::GlobalVariable,
    ) -> Result<(), Report> {
        // We don't emit anything for declarations
        if gv.is_declaration() {
            return Ok(());
        }

        self.init_body
            .push(crate::select::generated::Initialization::Global(gv.as_global_var_ref()));

        Ok(())
    }
}

struct MasmFunctionBuilder {
    span: midenc_hir::SourceSpan,
    name: masm::ProcedureName,
    signature: masm::FunctionType,
    visibility: masm::Visibility,
}

impl MasmFunctionBuilder {
    /// Prepare to translate `function`, or report why it cannot be translated.
    ///
    /// This is the single point every function reaches, whichever kind of item declares it —
    /// `MasmComponentBuilder::define_function` for a component-level function,
    /// `MasmModuleBuilder::define_function` for one in a module or an interface — which is why the
    /// check below lives here rather than at any one of them.
    pub fn new(function: &builtin::Function) -> Result<Self, Report> {
        if function.is_declaration() {
            return Err(function_without_a_body(function));
        }

        let name = *function.get_name();
        let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(Span::new(
            name.span,
            name.as_ref().into(),
        )));
        let visibility = lower_visibility(function.visibility());
        let signature =
            semantic_debug_signature(function).unwrap_or_else(|| lowered_signature(function));

        Ok(Self {
            span: function.span(),
            name,
            signature,
            visibility,
        })
    }

    pub fn build(
        self,
        function: &builtin::Function,
        _analysis_manager: AnalysisManager,
        link_info: &LinkInfo,
        owners: &EmissionOwners,
    ) -> Result<masm::Procedure, Report> {
        let env = crate::select::SelectEnv {
            link_info: Some(link_info),
            owners: Some(owners),
        };
        let mut procedure = if function.signature().cc.is_wasm_canonical_abi()
            && link_info.requires_init()
        {
            let source = crate::select::generated::GeneratedExport {
                function: function.as_function_ref(),
                initializer: InvocationTarget::Symbol(COMPONENT_INIT_PROCEDURE.parse().unwrap()),
                env: &env,
            };
            finalize_evidence(
                source.evidence()?,
                crate::verify::SourceContext::Generated(&source),
                self.name.clone(),
                self.span,
            )?
        } else {
            finalize_evidence(
                crate::select::evidence(function, &env)?,
                crate::verify::SourceContext::Hir {
                    function,
                    env: &env,
                },
                self.name.clone(),
                self.span,
            )?
        };
        let Self {
            signature,
            visibility,
            ..
        } = self;
        procedure.set_visibility(visibility);
        procedure.set_signature(signature);
        {
            for attribute in [
                midenc_dialect_hir::ACCOUNT_PROCEDURE_EXPORT_ATTR,
                midenc_dialect_hir::AUTH_SCRIPT_EXPORT_ATTR,
                midenc_dialect_hir::NOTE_SCRIPT_EXPORT_ATTR,
                midenc_dialect_hir::TRANSACTION_SCRIPT_EXPORT_ATTR,
            ] {
                if function.has_attribute(attribute) {
                    procedure
                        .attributes_mut()
                        .insert(Attribute::Marker(masm::Ident::new(attribute).unwrap()));
                }
            }
        }
        Ok(procedure)
    }
}

fn lowered_signature(function: &builtin::Function) -> masm::FunctionType {
    let sig = function.signature();
    let args = sig.params.iter().map(|param| masm::TypeExpr::from(param.ty.clone())).collect();
    let results = sig
        .results
        .iter()
        .map(|result| masm::TypeExpr::from(result.ty.clone()))
        .collect();
    masm::FunctionType::new(sig.cc.clone(), args, results)
}

fn semantic_debug_signature(function: &builtin::Function) -> Option<masm::FunctionType> {
    let subprogram = function
        .as_operation()
        .get_attribute("di.subprogram")?
        .try_downcast_attr::<SubprogramAttr>()
        .ok()?;
    let subprogram = subprogram.borrow();
    let Type::Function(ty) = subprogram.ty.as_ref()? else {
        return None;
    };

    let args = ty.params().iter().cloned().map(masm::TypeExpr::from).collect();
    let results = ty.results().iter().cloned().map(masm::TypeExpr::from).collect();
    Some(masm::FunctionType::new(ty.calling_convention(), args, results))
}

/// Finish one source-checked procedure through the only physical emission pipeline.
pub(crate) fn finalize_evidence(
    evidence: crate::select::HirEvidence,
    source: crate::verify::SourceContext<'_>,
    name: masm::ProcedureName,
    span: SourceSpan,
) -> Result<masm::Procedure, Report> {
    let locals = evidence.source.source_locals.clone();
    let checked = crate::verify::check_selection(evidence.source.program, &source)?;
    let mut registry = crate::lower::units::LoweringRegistry::default();
    for definition in &evidence.definitions {
        registry.register(definition)?;
    }
    finish_checked(checked, &locals, &registry, name, span)
}

/// Lower a registered generated definition through the same checked physical pipeline.
pub(crate) fn finalize_generated<D: crate::lower::units::LoweringDefinition>(
    source: &dyn crate::verify::GeneratedDefinition,
    definitions: &[D],
    name: masm::ProcedureName,
    span: SourceSpan,
) -> Result<masm::Procedure, Report> {
    let evidence = source.instantiate(&[])?;
    let checked = crate::verify::check_selection(
        evidence.program,
        &crate::verify::SourceContext::Generated(source),
    )?;
    let mut registry = crate::lower::units::LoweringRegistry::default();
    for definition in definitions {
        registry.register(definition)?;
    }
    finish_checked(checked, &evidence.source_locals, &registry, name, span)
}

fn finish_checked(
    checked: crate::verify::CheckedSelectedProgram,
    locals: &crate::plan::SourceLocalLayout,
    registry: &crate::lower::units::LoweringRegistry<'_>,
    name: masm::ProcedureName,
    span: SourceSpan,
) -> Result<masm::Procedure, Report> {
    use crate::{
        allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
        lower::expand::{ExpansionEnv, finalize, resolve_frame_bindings},
    };
    let trace_target = TraceTarget::category("codegen");
    let started =
        log::log_enabled!(target: &trace_target, log::Level::Trace).then(std::time::Instant::now);
    let allocation = allocate(checked, &AllocationOptions::default());
    if let Some(started) = started {
        log::trace!(target: &trace_target, "physical-stage procedure={name} stage=allocation elapsed_ns={}", started.elapsed().as_nanos());
    }
    let mut plan = allocation.map_err(|error| allocation_error(error, &name, span))?;
    let frame =
        assign_frame(&plan, locals).map_err(|error| allocation_error(error, &name, span))?;
    resolve_frame_bindings(&mut plan, &frame)?;
    let mut env = ExpansionEnv::default();
    env.resolve_debug(&plan, &frame);
    plan.trace(&frame);
    Ok(finalize(&plan, &frame, registry, &env, &name)?.into_masm_with_span(name, span)?)
}

/// Keep the planning error bare so the session can render its source labels.
pub(super) fn allocation_error(
    error: crate::allocate::state::AllocationError,
    name: &masm::ProcedureName,
    span: SourceSpan,
) -> Report {
    error.into_plan_error(name.as_str(), span).into()
}

#[cfg(test)]
mod owner_map_tests {
    use alloc::rc::Rc;

    use midenc_hir::{Context, diagnostics::Uri};

    use super::*;

    fn parse(context: &Rc<Context>, source: &str) -> midenc_hir::OperationRef {
        midenc_hir::parse::parse_any(
            midenc_hir::parse::ParserConfig {
                context: context.clone(),
                verify: true,
            },
            Uri::new("owners.hir"),
            source,
        )
        .unwrap()
    }

    fn target() -> midenc_session::miden_project::Target {
        midenc_session::miden_project::Target::library(
            masm::PathBuf::new("::renamed").unwrap(),
            Uri::new("owners.hir"),
        )
    }

    #[test]
    fn wrong_same_signature_owner_cannot_authorize_selected_calls() {
        let context = Rc::new(Context::default());
        let root = parse(
            &context,
            r#"builtin.world { builtin.module public @lib {
            builtin.function public extern("C") @a() { builtin.ret; };
            builtin.function public extern("C") @b() { builtin.ret; };
            builtin.function public extern("C") @caller() {
                hir.exec @a() : extern("C") () -> (); builtin.ret;
            };
        }; };"#,
        );
        let world = root.try_downcast_op::<builtin::World>().unwrap();
        let link = Linker::default().link(None, &root.borrow()).unwrap();
        let target = target();
        let mut owners = EmissionOwners::new(
            root,
            &[],
            &world_root_module(&world.borrow()),
            EmissionNamespace::Target(&target),
            false,
            &link,
        )
        .unwrap();
        let a = masm::PathBuf::new("::lib::a").unwrap();
        owners.targets.insert(a, masm::PathBuf::new("::renamed::b").unwrap());
        assert!(owners.validate().is_err());
        let mut caller = None;
        root.borrow().prewalk_all(|op| {
            if let Some(function) = op.downcast_ref::<builtin::Function>()
                && function.name().as_str() == "caller"
            {
                caller = Some(function.as_function_ref());
            }
        });
        let env = crate::select::SelectEnv {
            link_info: Some(&link),
            owners: Some(&owners),
        };
        assert!(crate::select::evidence(&caller.unwrap().borrow(), &env).is_err());
    }

    #[test]
    fn external_prefix_overlap_and_digest_targets_keep_their_identity() {
        let context = Rc::new(Context::default());
        let root = parse(
            &context,
            "builtin.world { builtin.module public @lib { builtin.function public extern(\"C\") \
             @a() { builtin.ret; }; }; };",
        );
        let world = root.try_downcast_op::<builtin::World>().unwrap();
        let link = Linker::default().link(None, &root.borrow()).unwrap();
        let target = target();
        let owners = EmissionOwners::new(
            root,
            &[],
            &world_root_module(&world.borrow()),
            EmissionNamespace::Target(&target),
            false,
            &link,
        )
        .unwrap();
        let external = crate::lower::raw::target("::lib::external::body");
        assert_eq!(owners.target(external.clone()).unwrap(), external);
        let local = InvocationTarget::Symbol("local".parse().unwrap());
        assert_eq!(owners.target(local.clone()).unwrap(), local);
        let digest = InvocationTarget::MastRoot(Span::unknown(miden_core::Word::default()));
        assert_eq!(owners.target(digest.clone()).unwrap(), digest);
    }

    #[test]
    fn distinct_emitted_module_owners_cannot_share_a_destination() {
        let context = Rc::new(Context::default());
        let root = parse(&context, "builtin.world { builtin.module public @lib {}; };");
        let duplicate = parse(&context, "builtin.module public @lib {};")
            .try_downcast_op::<builtin::Module>()
            .unwrap();
        let world = root.try_downcast_op::<builtin::World>().unwrap();
        let link = Linker::default().link(None, &root.borrow()).unwrap();
        let target = target();
        assert!(
            EmissionOwners::new(
                root,
                &[duplicate],
                &world_root_module(&world.borrow()),
                EmissionNamespace::Target(&target),
                false,
                &link
            )
            .is_err()
        );
    }
}
