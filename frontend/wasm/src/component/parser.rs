//! Parsing of a Wasm component from binary into an intermediate form
//! suitable for further translation and IR component generation

// Based on wasmtime v16.0 Wasm component translation

use std::mem;

use cranelift_entity::PrimaryMap;
use indexmap::IndexMap;
use midenc_hir::{FxBuildHasher, FxHashMap};
use midenc_session::{Session, diagnostics::IntoDiagnostic};
use wasmparser::{
    Chunk, ComponentImportName, Encoding, Parser, Payload, Validator,
    component_types::{
        AliasableResourceId, ComponentEntityType, ComponentFuncTypeId, ComponentInstanceTypeId,
    },
    types::Types,
};

use crate::{
    WasmTranslationConfig,
    component::*,
    error::WasmResult,
    module::{
        module_env::{ModuleEnvironment, ParsedModule},
        types::{
            EntityIndex, FuncIndex, GlobalIndex, MemoryIndex, TableIndex, WasmType,
            convert_func_type, convert_valtype,
        },
    },
    unsupported_diag,
};

/// Structure used to parse a Wasm component
pub struct ComponentParser<'a, 'data> {
    /// Configuration options for the translation.
    config: &'a WasmTranslationConfig,

    /// The current compilation session
    session: &'a Session,

    /// The current component being parsed.
    ///
    /// This will get swapped out as parsing traverses the body of a
    /// component and a sub-component is entered or left.
    pub result: ParsedComponent<'data>,

    /// Current state of parsing a binary component. Note that like `result`
    /// this will change as the component is traversed.
    parser: Parser,

    /// Stack of lexical scopes that are in-progress but not finished yet.
    ///
    /// This is pushed to whenever a component is entered and popped from
    /// whenever a component is left. Each lexical scope also contains
    /// information about the variables that it is currently required to close
    /// over which is threaded into the current in-progress parsing of
    /// the sub-component which pushed a scope here.
    lexical_scopes: Vec<LexicalScope<'data>>,

    /// The validator in use to verify that the raw input binary is a valid
    /// component.
    validator: &'a mut Validator,

    /// Type information shared for the entire component.
    pub types: &'a mut ComponentTypesBuilder,

    /// Completely parsed core wasm modules that have been found so far.
    ///
    /// Note that this translation only involves learning about type
    /// information and functions are not actually translated here.
    pub static_modules: PrimaryMap<StaticModuleIndex, ParsedModule<'data>>,

    /// Completely parsed components that have been found so far.
    ///
    /// As frames are popped from `lexical_scopes` their completed component
    /// will be pushed onto this list.
    pub static_components: PrimaryMap<StaticComponentIndex, ParsedComponent<'data>>,
}

pub struct ParsedRootComponent<'data> {
    /// The root component
    pub root_component: ParsedComponent<'data>,

    /// Completely parsed core wasm modules that have been found so far.
    ///
    /// Note that this translation only involves learning about type
    /// information and functions are not actually translated here.
    pub static_modules: PrimaryMap<StaticModuleIndex, ParsedModule<'data>>,

    /// Completely parsed components that have been found so far.
    ///
    /// As frames are popped from `lexical_scopes` their completed component
    /// will be pushed onto this list.
    pub static_components: PrimaryMap<StaticComponentIndex, ParsedComponent<'data>>,
}

/// Representation of the syntactic scope of a component meaning where it is
/// and what its state is at in the binary format.
///
/// These scopes are pushed and popped when a sub-component starts being
/// parsed and finishes being parsed. The main purpose of this frame is to
/// have a `ClosedOverVars` field which encapsulates data that is inherited
/// from the scope specified into the component being parsed just beneath
/// it.
///
/// This structure exists to implement outer aliases to components and modules.
/// When a component or module is closed over then that means it needs to be
/// inherited in a sense to the component which actually had the alias. This is
/// achieved with a deceptively simple scheme where each parent of the
/// component with the alias will inherit the component from the desired
/// location.
///
/// For example with a component structure that looks like:
///
/// ```wasm
/// (component $A
///     (core module $M)
///     (component $B
///         (component $C
///             (alias outer $A $M (core module))
///         )
///     )
/// )
/// ```
///
/// here the `C` component is closing over `M` located in the root component
/// `A`. When `C` is being parsed the `lexical_scopes` field will look like
/// `[A, B]`. When the alias is encountered (for module index 0) this will
/// place a `ClosedOverModule::Local(0)` entry into the `closure_args` field of
/// `A`'s frame. This will in turn give a `ModuleUpvarIndex` which is then
/// inserted into `closure_args` in `B`'s frame. This produces yet another
/// `ModuleUpvarIndex` which is finally inserted into `C`'s module index space
/// via `LocalInitializer::AliasModuleUpvar` with the last index.
///
/// All of these upvar indices and such are interpreted in the "inline" phase of
/// translation. This means that when `A` is being instantiated one of its
/// initializers will be `LocalInitializer::ComponentStatic`. This starts to
/// create `B` and the variables captured for `B` are listed as local module 0,
/// or `M`. This list is then preserved in the definition of the component `B`
/// and later reused by `C` again to finally get access to the closed over
/// component.
///
/// Effectively the scopes are managed hierarchically where a reference to an
/// outer variable automatically injects references into all parents up to
/// where the reference is. This variable scopes are then processed during
/// inlining where a component definition is a reference to the static
/// component information (`ParsedComponent`) plus closed over variables
/// (`ComponentClosure` during inlining).
struct LexicalScope<'data> {
    /// Underlying wasmparser for the component being parsed
    parser: Parser,
    /// Current state of the component's parsing as found so far.
    parsed_component: ParsedComponent<'data>,
    /// List of captures that `parsed_component` will need to process to create the
    /// sub-component which is directly beneath this lexical scope.
    closure_args: ClosedOverVars,
}

/// A "local" parsing of a component.
///
/// This structure is used as a sort of in-progress parsing of a component.
#[derive(Default)]
pub struct ParsedComponent<'data> {
    /// Instructions which form this component.
    ///
    /// There is one initializer for all members of each index space, and all
    /// index spaces are incrementally built here as the initializer list is
    /// processed.
    pub initializers: Vec<LocalInitializer<'data>>,

    /// The list of exports from this component, as pairs of names and an
    /// index into an index space of what's being exported.
    pub exports: IndexMap<&'data str, ComponentItem>,

    /// Type information produced by `wasmparser` for this component.
    ///
    /// This type information is available after the parsing of the entire
    /// component has finished, e.g. for the `inline` pass, but beforehand this
    /// is set to `None`.
    pub types: Option<Types>,
}

#[derive(Debug, Clone)]
pub struct CanonLower {
    pub func: ComponentFuncIndex,
    pub lower_ty: ComponentFuncTypeId,
    pub canonical_abi: SignatureIndex,
    pub options: LocalCanonicalOptions,
}

#[derive(Debug, Clone)]
pub struct CanonLift {
    pub ty: ComponentFuncTypeId,
    pub func: FuncIndex,
    pub options: LocalCanonicalOptions,
}

#[derive(Debug, Clone)]
// component instances
pub struct ComponentInstantiation<'data> {
    pub component: ComponentIndex,
    pub args: FxHashMap<&'data str, ComponentItem>,
    pub ty: ComponentInstanceTypeId,
}

// NB: the type information contained in `LocalInitializer` should always point
// to `wasmparser`'s type information, not ours. Component types cannot be
// fully determined due to resources until instantiations are known which is
// tracked during the inlining phase. This means that all type information below
// is straight from `wasmparser`'s passes.
#[derive(Debug)]
pub enum LocalInitializer<'data> {
    // imports
    Import(ComponentImportName<'data>, ComponentEntityType),

    // canonical function sections
    Lower(CanonLower),
    Lift(CanonLift),

    // resources
    Resource(AliasableResourceId, WasmType, Option<FuncIndex>),
    ResourceNew(AliasableResourceId, SignatureIndex),
    ResourceRep(AliasableResourceId, SignatureIndex),
    ResourceDrop(AliasableResourceId, SignatureIndex),
    ResourceDropAsync(AliasableResourceId, SignatureIndex),

    // core wasm modules
    ModuleStatic(StaticModuleIndex),

    // core wasm module instances
    ModuleInstantiate(ModuleIndex, FxHashMap<&'data str, ModuleInstanceIndex>),
    ModuleSynthetic(FxHashMap<&'data str, EntityIndex>),

    // components
    ComponentStatic(StaticComponentIndex, ClosedOverVars),

    ComponentInstantiate(ComponentInstantiation<'data>),
    ComponentSynthetic(FxHashMap<&'data str, ComponentItem>),

    // alias section
    AliasExportFunc(ModuleInstanceIndex, &'data str),
    AliasExportTable(ModuleInstanceIndex, &'data str),
    AliasExportGlobal(ModuleInstanceIndex, &'data str),
    AliasExportMemory(ModuleInstanceIndex, &'data str),
    AliasComponentExport(ComponentInstanceIndex, &'data str),
    AliasModule(ClosedOverModule),
    AliasComponent(ClosedOverComponent),

    // export section
    Export(&'data str, ComponentItem),
}

/// The "closure environment" of components themselves.
///
/// For more information see `LexicalScope`.
#[derive(Default, Debug)]
pub struct ClosedOverVars {
    pub components: PrimaryMap<ComponentUpvarIndex, ClosedOverComponent>,
    pub modules: PrimaryMap<ModuleUpvarIndex, ClosedOverModule>,
}

/// Description how a component is closed over when the closure variables for
/// a component are being created.
///
/// For more information see `LexicalScope`.
#[derive(Debug)]
pub enum ClosedOverComponent {
    /// A closed over component is coming from the local component's index
    /// space, meaning a previously defined component is being captured.
    Local(ComponentIndex),
    /// A closed over component is coming from our own component's list of
    /// upvars. This list was passed to us by our enclosing component, which
    /// will eventually have bottomed out in closing over a `Local` component
    /// index for some parent component.
    Upvar(ComponentUpvarIndex),
}

/// Same as `ClosedOverComponent`, but for modules.
#[derive(Debug)]
pub enum ClosedOverModule {
    Local(ModuleIndex),
    Upvar(ModuleUpvarIndex),
}

/// Representation of canonical ABI options.
#[derive(Debug, Clone)]
pub struct LocalCanonicalOptions {
    pub string_encoding: StringEncoding,
    pub memory: Option<MemoryIndex>,
    pub realloc: Option<FuncIndex>,
    pub post_return: Option<FuncIndex>,
    pub is_async: bool,
    pub async_callback: Option<FuncIndex>,
}

/// Action to take after parsing a payload.
enum Action {
    /// Keep going with parsing the input data.
    KeepGoing,
    /// Skip `n` bytes of input data (e.g. a module section which was parsed separately).
    Skip(usize),
    /// Parsing is done when there is no parent component in the scope stack.
    Done,
}

impl<'a, 'data> ComponentParser<'a, 'data> {
    /// Creates a new parsing state ready to parse a component.
    pub fn new(
        config: &'a WasmTranslationConfig,
        session: &'a Session,
        validator: &'a mut Validator,
        types: &'a mut ComponentTypesBuilder,
    ) -> Self {
        Self {
            config,
            session,
            result: ParsedComponent::default(),
            validator,
            types,
            parser: Parser::new(0),
            lexical_scopes: Vec::new(),
            static_components: Default::default(),
            static_modules: Default::default(),
        }
    }

    /// Parses the given the Wasm component
    pub fn parse(mut self, component: &'data [u8]) -> WasmResult<ParsedRootComponent<'data>> {
        let mut remaining = component;
        loop {
            let payload = match self.parser.parse(remaining, true).into_diagnostic()? {
                Chunk::Parsed { payload, consumed } => {
                    remaining = &remaining[consumed..];
                    payload
                }
                Chunk::NeedMoreData(_) => unreachable!(),
            };

            match self.parse_payload(payload, component)? {
                Action::KeepGoing => {}
                Action::Skip(n) => remaining = &remaining[n..],
                Action::Done => break,
            }
        }
        assert!(remaining.is_empty());
        assert!(self.lexical_scopes.is_empty());

        Ok(ParsedRootComponent {
            root_component: self.result,
            static_modules: self.static_modules,
            static_components: self.static_components,
        })
    }

    /// Parses a given payload from the Wasm component.
    fn parse_payload(
        &mut self,
        payload: Payload<'data>,
        component: &'data [u8],
    ) -> WasmResult<Action> {
        match payload {
            Payload::Version {
                num,
                encoding,
                range,
            } => {
                self.validator.version(num, encoding, &range).into_diagnostic()?;

                match encoding {
                    Encoding::Component => {}
                    Encoding::Module => {
                        panic!("attempted to parse a wasm module with a component parser");
                    }
                }
            }
            Payload::End(offset) => {
                assert!(self.result.types.is_none());
                self.result.types = Some(self.validator.end(offset).into_diagnostic()?);
                // Exit the current lexical scope. If there is no parent (no
                // frame currently on the stack) then parsing is finished.
                // Otherwise that means that a nested component has been
                // completed and is recorded as such.
                let LexicalScope {
                    parser,
                    parsed_component,
                    closure_args,
                } = match self.lexical_scopes.pop() {
                    Some(frame) => frame,
                    None => return Ok(Action::Done),
                };
                self.parser = parser;
                let component = mem::replace(&mut self.result, parsed_component);
                let static_idx = self.static_components.push(component);
                self.result
                    .initializers
                    .push(LocalInitializer::ComponentStatic(static_idx, closure_args));
            }
            Payload::ComponentTypeSection(s) => self.component_type_section(s)?,
            Payload::CoreTypeSection(s) => {
                self.validator.core_type_section(&s).into_diagnostic()?
            }
            Payload::ComponentImportSection(s) => self.component_import_section(s)?,
            Payload::ComponentCanonicalSection(s) => self.component_canonical_section(s)?,
            Payload::ModuleSection {
                parser,
                unchecked_range: range,
            } => {
                self.module_section(range.clone(), parser, component)?;
                return Ok(Action::Skip(range.end - range.start));
            }
            Payload::ComponentSection {
                parser,
                unchecked_range: range,
            } => self.component_section(range, parser)?,
            Payload::InstanceSection(s) => self.core_instance_section(s)?,
            Payload::ComponentInstanceSection(s) => self.component_instance_section(s)?,
            Payload::ComponentExportSection(s) => self.component_export_section(s)?,
            Payload::ComponentStartSection { start, range } => {
                self.validator.component_start_section(&start, &range).into_diagnostic()?;
                unsupported_diag!(
                    &self.session.diagnostics,
                    "component start section is not supported"
                );
            }
            Payload::ComponentAliasSection(s) => self.component_alias_section(s)?,
            // All custom sections are ignored at this time.
            // and parse a `name` section here.
            Payload::CustomSection { .. } => {}
            // Anything else is either not reachable since we never enable the
            // feature or we do enable it and it's a bug we don't
            // implement it, so let validation take care of most errors here and
            // if it gets past validation provide a helpful error message to
            // debug.
            other => {
                self.validator.payload(&other).into_diagnostic()?;
                unsupported_diag!(
                    &self.session.diagnostics,
                    "unsupported component section {other:?}"
                );
            }
        }

        Ok(Action::KeepGoing)
    }

    fn component_type_section(
        &mut self,
        s: wasmparser::ComponentTypeSectionReader<'data>,
    ) -> WasmResult<()> {
        // When we see a type section the types are validated and then parsed.
        // Each active type definition is recorded in the
        // `ComponentTypesBuilder` tables, or this component's active scope.
        //
        // Note that the push/pop of the component types scope happens above in
        // `Version` and `End` since multiple type sections can appear within a
        // component.
        let mut component_type_index = self.validator.types(0).unwrap().component_type_count();
        self.validator.component_type_section(&s).into_diagnostic()?;
        let types = self.validator.types(0).unwrap();
        for ty in s {
            match ty.into_diagnostic()? {
                wasmparser::ComponentType::Resource { rep, dtor } => {
                    let rep = convert_valtype(rep);
                    let id = types.component_any_type_at(component_type_index).unwrap_resource();
                    let dtor = dtor.map(FuncIndex::from_u32);
                    self.result.initializers.push(LocalInitializer::Resource(id, rep, dtor));
                }

                // no extra processing needed
                wasmparser::ComponentType::Defined(_)
                | wasmparser::ComponentType::Func(_)
                | wasmparser::ComponentType::Instance(_)
                | wasmparser::ComponentType::Component(_) => {}
            }

            component_type_index += 1;
        }
        Ok(())
    }

    fn component_import_section(
        &mut self,
        s: wasmparser::ComponentImportSectionReader<'data>,
    ) -> WasmResult<()> {
        // Processing the import section at this point is relatively simple
        // which is to simply record the name of the import and the type
        // information associated with it.
        self.validator.component_import_section(&s).into_diagnostic()?;
        for import in s {
            let import = import.into_diagnostic()?;
            let types = self.validator.types(0).unwrap();
            let ty = types.component_entity_type_of_import(import.name.0).unwrap();
            self.result.initializers.push(LocalInitializer::Import(import.name, ty));
        }
        Ok(())
    }

    fn component_canonical_section(
        &mut self,
        s: wasmparser::ComponentCanonicalSectionReader<'data>,
    ) -> WasmResult<()> {
        // Entries in the canonical section will get initializers recorded
        // with the listed options for lifting/lowering.
        let mut core_func_index = self.validator.types(0).unwrap().function_count();
        self.validator.component_canonical_section(&s).into_diagnostic()?;
        for func in s {
            let types = self.validator.types(0).unwrap();
            let canonical_func = func.into_diagnostic()?;
            log::debug!(target: "component-parser","Processing canonical function: {canonical_func:?}");
            let init = match canonical_func {
                wasmparser::CanonicalFunction::Lift {
                    type_index,
                    core_func_index,
                    options,
                } => {
                    let ty = types.component_any_type_at(type_index).unwrap_func();
                    let func = FuncIndex::from_u32(core_func_index);
                    let options = canonical_options(&options);
                    LocalInitializer::Lift(CanonLift { ty, func, options })
                }
                wasmparser::CanonicalFunction::Lower {
                    func_index,
                    options,
                } => {
                    let lower_ty = types.component_function_at(func_index);
                    let func = ComponentFuncIndex::from_u32(func_index);
                    let options = canonical_options(&options);
                    let canonical_abi = self.core_func_signature(core_func_index);

                    core_func_index += 1;
                    LocalInitializer::Lower(CanonLower {
                        func,
                        options,
                        canonical_abi,
                        lower_ty,
                    })
                }
                wasmparser::CanonicalFunction::ResourceNew { resource } => {
                    let resource = types.component_any_type_at(resource).unwrap_resource();
                    let ty = self.core_func_signature(core_func_index);
                    core_func_index += 1;
                    LocalInitializer::ResourceNew(resource, ty)
                }
                wasmparser::CanonicalFunction::ResourceDrop { resource } => {
                    let resource = types.component_any_type_at(resource).unwrap_resource();
                    let ty = self.core_func_signature(core_func_index);
                    core_func_index += 1;
                    LocalInitializer::ResourceDrop(resource, ty)
                }
                wasmparser::CanonicalFunction::ResourceDropAsync { resource } => {
                    let resource = types.component_any_type_at(resource).unwrap_resource();
                    let ty = self.core_func_signature(core_func_index);
                    core_func_index += 1;
                    LocalInitializer::ResourceDropAsync(resource, ty)
                }
                wasmparser::CanonicalFunction::ResourceRep { resource } => {
                    let resource = types.component_any_type_at(resource).unwrap_resource();
                    let ty = self.core_func_signature(core_func_index);
                    core_func_index += 1;
                    LocalInitializer::ResourceRep(resource, ty)
                }
                wasmparser::CanonicalFunction::ErrorContextNew { .. }
                | wasmparser::CanonicalFunction::ErrorContextDrop
                | wasmparser::CanonicalFunction::ErrorContextDebugMessage { .. }
                | wasmparser::CanonicalFunction::ThreadSpawn { .. }
                | wasmparser::CanonicalFunction::ThreadAvailableParallelism
                | wasmparser::CanonicalFunction::Yield { .. }
                | wasmparser::CanonicalFunction::BackpressureSet
                | wasmparser::CanonicalFunction::WaitableJoin
                | wasmparser::CanonicalFunction::WaitableSetNew
                | wasmparser::CanonicalFunction::WaitableSetDrop
                | wasmparser::CanonicalFunction::WaitableSetPoll { .. }
                | wasmparser::CanonicalFunction::WaitableSetWait { .. }
                | wasmparser::CanonicalFunction::FutureNew { .. }
                | wasmparser::CanonicalFunction::FutureRead { .. }
                | wasmparser::CanonicalFunction::FutureWrite { .. }
                | wasmparser::CanonicalFunction::FutureCancelRead { .. }
                | wasmparser::CanonicalFunction::FutureCancelWrite { .. }
                | wasmparser::CanonicalFunction::FutureCloseWritable { .. }
                | wasmparser::CanonicalFunction::FutureCloseReadable { .. }
                | wasmparser::CanonicalFunction::SubtaskDrop
                | wasmparser::CanonicalFunction::TaskReturn { .. }
                | wasmparser::CanonicalFunction::StreamNew { .. }
                | wasmparser::CanonicalFunction::StreamRead { .. }
                | wasmparser::CanonicalFunction::StreamWrite { .. }
                | wasmparser::CanonicalFunction::StreamCancelRead { .. }
                | wasmparser::CanonicalFunction::StreamCancelWrite { .. }
                | wasmparser::CanonicalFunction::StreamCloseWritable { .. }
                | wasmparser::CanonicalFunction::StreamCloseReadable { .. } => unimplemented!(),
            };
            log::debug!(target: "component-parser", "Adding canonical initializer: {init:?}");
            self.result.initializers.push(init);
        }
        Ok(())
    }

    fn module_section(
        &mut self,
        range: std::ops::Range<usize>,
        parser: Parser,
        component: &'data [u8],
    ) -> WasmResult<()> {
        // Core wasm modules are parsed inline directly here with the
        // `ModuleEnvironment` from core wasm compilation. This will return
        // to the caller the size of the module so it knows how many bytes
        // of the input are skipped.
        //
        // Note that this is just initial type parsing of the core wasm
        // module and actual function translation is deferred until this
        // entire process has completed.
        self.validator.module_section(&range).into_diagnostic()?;
        let module_environment = ModuleEnvironment::new(
            self.config,
            self.validator,
            self.types.module_types_builder_mut(),
        );
        let parsed_module = module_environment.parse(
            parser,
            &component[range.start..range.end],
            &self.session.diagnostics,
        )?;
        let static_idx = self.static_modules.push(parsed_module);
        self.result.initializers.push(LocalInitializer::ModuleStatic(static_idx));
        // Set a fallback name for the newly added parsed module to be used if
        // the name section does not define a name for the module.
        self.static_modules
            .get_mut(static_idx)
            .unwrap()
            .module
            .set_name_fallback(format!("module{}", static_idx.as_u32()).into());
        Ok(())
    }

    fn component_section(
        &mut self,
        range: std::ops::Range<usize>,
        parser: Parser,
    ) -> WasmResult<()> {
        // When a sub-component is found then the current parsing state
        // is pushed onto the `lexical_scopes` stack. This will subsequently
        // get popped as part of `Payload::End` processing above.
        //
        // Note that the set of closure args for this new lexical scope
        // starts empty since it will only get populated if parsing of
        // the nested component ends up aliasing some outer module or
        // component.
        self.validator.component_section(&range).into_diagnostic()?;
        self.lexical_scopes.push(LexicalScope {
            parser: mem::replace(&mut self.parser, parser),
            parsed_component: mem::take(&mut self.result),
            closure_args: ClosedOverVars::default(),
        });
        Ok(())
    }

    fn core_instance_section(
        &mut self,
        s: wasmparser::InstanceSectionReader<'data>,
    ) -> WasmResult<()> {
        // Both core wasm instances and component instances record
        // initializers of what form of instantiation is performed which
        // largely just records the arguments given from wasmparser into a
        // `HashMap` for processing later during inlining.
        self.validator.instance_section(&s).into_diagnostic()?;
        for instance in s {
            let init = match instance.into_diagnostic()? {
                wasmparser::Instance::Instantiate { module_index, args } => {
                    let index = ModuleIndex::from_u32(module_index);
                    instantiate_module(index, &args)
                }
                wasmparser::Instance::FromExports(exports) => {
                    instantiate_module_from_exports(&exports)
                }
            };
            self.result.initializers.push(init);
        }
        Ok(())
    }

    fn component_instance_section(
        &mut self,
        s: wasmparser::ComponentInstanceSectionReader<'data>,
    ) -> WasmResult<()> {
        let mut index = self.validator.types(0).unwrap().component_instance_count();
        self.validator.component_instance_section(&s).into_diagnostic()?;
        for instance in s {
            let init = match instance.into_diagnostic()? {
                wasmparser::ComponentInstance::Instantiate {
                    component_index,
                    args,
                } => {
                    let types = self.validator.types(0).unwrap();
                    let ty = types.component_instance_at(index);
                    let index = ComponentIndex::from_u32(component_index);
                    self.instantiate_component(index, &args, ty)?
                }
                wasmparser::ComponentInstance::FromExports(exports) => {
                    self.instantiate_component_from_exports(&exports)?
                }
            };
            self.result.initializers.push(init);
            index += 1;
        }
        Ok(())
    }

    fn component_export_section(
        &mut self,
        s: wasmparser::ComponentExportSectionReader<'data>,
    ) -> WasmResult<()> {
        // Exports don't actually fill out the `initializers` array but
        // instead fill out the one other field in a `ParsedComponent`, the
        // `exports` field (as one might imagine). This for now simply
        // records the index of what's exported and that's tracked further
        // later during inlining.
        self.validator.component_export_section(&s).into_diagnostic()?;
        for export in s {
            let export = export.into_diagnostic()?;
            let item = self.kind_to_item(export.kind, export.index)?;
            let prev = self.result.exports.insert(export.name.0, item);
            assert!(prev.is_none());
            self.result.initializers.push(LocalInitializer::Export(export.name.0, item));
        }
        Ok(())
    }

    fn component_alias_section(
        &mut self,
        s: wasmparser::ComponentAliasSectionReader<'data>,
    ) -> WasmResult<()> {
        // Aliases of instance exports (either core or component) will be
        // recorded as an initializer of the appropriate type with outer
        // aliases handled specially via upvars and type processing.
        self.validator.component_alias_section(&s).into_diagnostic()?;
        for alias in s {
            let init = match alias.into_diagnostic()? {
                wasmparser::ComponentAlias::InstanceExport {
                    kind: _,
                    instance_index,
                    name,
                } => {
                    let instance = ComponentInstanceIndex::from_u32(instance_index);
                    LocalInitializer::AliasComponentExport(instance, name)
                }
                wasmparser::ComponentAlias::Outer { kind, count, index } => {
                    self.alias_component_outer(kind, count, index);
                    continue;
                }
                wasmparser::ComponentAlias::CoreInstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    let instance = ModuleInstanceIndex::from_u32(instance_index);
                    alias_module_instance_export(kind, instance, name)
                }
            };
            self.result.initializers.push(init);
        }
        Ok(())
    }

    /// Parses a component instance
    fn instantiate_component(
        &mut self,
        component: ComponentIndex,
        raw_args: &[wasmparser::ComponentInstantiationArg<'data>],
        ty: ComponentInstanceTypeId,
    ) -> WasmResult<LocalInitializer<'data>> {
        let mut args = FxHashMap::with_capacity_and_hasher(raw_args.len(), FxBuildHasher);
        for arg in raw_args {
            let idx = self.kind_to_item(arg.kind, arg.index)?;
            args.insert(arg.name, idx);
        }

        Ok(LocalInitializer::ComponentInstantiate(ComponentInstantiation {
            component,
            args,
            ty,
        }))
    }

    /// Creates a synthetic module from the list of items currently in the
    /// module and their given names.
    fn instantiate_component_from_exports(
        &mut self,
        exports: &[wasmparser::ComponentExport<'data>],
    ) -> WasmResult<LocalInitializer<'data>> {
        let mut map = FxHashMap::with_capacity_and_hasher(exports.len(), FxBuildHasher);
        for export in exports {
            let idx = self.kind_to_item(export.kind, export.index)?;
            map.insert(export.name.0, idx);
        }

        Ok(LocalInitializer::ComponentSynthetic(map))
    }

    /// Converts wasmparser's `ComponentExternalKind` into our `ComponentItem`.
    fn kind_to_item(
        &mut self,
        kind: wasmparser::ComponentExternalKind,
        index: u32,
    ) -> WasmResult<ComponentItem> {
        Ok(match kind {
            wasmparser::ComponentExternalKind::Func => {
                let index = ComponentFuncIndex::from_u32(index);
                ComponentItem::Func(index)
            }
            wasmparser::ComponentExternalKind::Module => {
                let index = ModuleIndex::from_u32(index);
                ComponentItem::Module(index)
            }
            wasmparser::ComponentExternalKind::Instance => {
                let index = ComponentInstanceIndex::from_u32(index);
                ComponentItem::ComponentInstance(index)
            }
            wasmparser::ComponentExternalKind::Component => {
                let index = ComponentIndex::from_u32(index);
                ComponentItem::Component(index)
            }
            wasmparser::ComponentExternalKind::Value => {
                unsupported_diag!(&self.session.diagnostics, "component values are not supported");
            }
            wasmparser::ComponentExternalKind::Type => {
                let types = self.validator.types(0).unwrap();
                let ty = types.component_any_type_at(index);
                ComponentItem::Type(ty)
            }
        })
    }

    /// Parses an outer alias to a module or component.
    fn alias_component_outer(
        &mut self,
        kind: wasmparser::ComponentOuterAliasKind,
        count: u32,
        index: u32,
    ) {
        match kind {
            wasmparser::ComponentOuterAliasKind::CoreType
            | wasmparser::ComponentOuterAliasKind::Type => {}

            // For more information about the implementation of outer aliases
            // see the documentation of `LexicalScope`. Otherwise though the
            // main idea here is that the data to close over starts as `Local`
            // and then transitions to `Upvar` as its inserted into the parents
            // in order from target we're aliasing back to the current
            // component.
            wasmparser::ComponentOuterAliasKind::CoreModule => {
                let index = ModuleIndex::from_u32(index);
                let mut module = ClosedOverModule::Local(index);
                let depth = self.lexical_scopes.len() - (count as usize);
                for frame in self.lexical_scopes[depth..].iter_mut() {
                    module = ClosedOverModule::Upvar(frame.closure_args.modules.push(module));
                }

                // If the `module` is still `Local` then the `depth` was 0 and
                // it's an alias into our own space. Otherwise it's switched to
                // an upvar and will index into the upvar space. Either way
                // it's just plumbed directly into the initializer.
                self.result.initializers.push(LocalInitializer::AliasModule(module));
            }
            wasmparser::ComponentOuterAliasKind::Component => {
                let index = ComponentIndex::from_u32(index);
                let mut component = ClosedOverComponent::Local(index);
                let depth = self.lexical_scopes.len() - (count as usize);
                for frame in self.lexical_scopes[depth..].iter_mut() {
                    component =
                        ClosedOverComponent::Upvar(frame.closure_args.components.push(component));
                }

                self.result.initializers.push(LocalInitializer::AliasComponent(component));
            }
        }
    }

    /// Converts a core wasm function type into our `SignatureIndex`.
    fn core_func_signature(&mut self, idx: u32) -> SignatureIndex {
        let types = self.validator.types(0).unwrap();
        let id = types.core_function_at(idx);
        let ty = types[id].unwrap_func();
        let ty = convert_func_type(ty);
        self.types.module_types_builder_mut().wasm_func_type(id, ty)
    }
}

/// Parses core module instance
fn instantiate_module<'data>(
    module: ModuleIndex,
    raw_args: &[wasmparser::InstantiationArg<'data>],
) -> LocalInitializer<'data> {
    let mut args = FxHashMap::with_capacity_and_hasher(raw_args.len(), FxBuildHasher);
    for arg in raw_args {
        match arg.kind {
            wasmparser::InstantiationArgKind::Instance => {
                let idx = ModuleInstanceIndex::from_u32(arg.index);
                args.insert(arg.name, idx);
            }
        }
    }
    LocalInitializer::ModuleInstantiate(module, args)
}

/// Creates a synthetic module from the list of items currently in the
/// module and their given names.
fn instantiate_module_from_exports<'data>(
    exports: &[wasmparser::Export<'data>],
) -> LocalInitializer<'data> {
    let mut map = FxHashMap::with_capacity_and_hasher(exports.len(), FxBuildHasher);
    for export in exports {
        let idx = match export.kind {
            wasmparser::ExternalKind::Func => {
                let index = FuncIndex::from_u32(export.index);
                EntityIndex::Function(index)
            }
            wasmparser::ExternalKind::Table => {
                let index = TableIndex::from_u32(export.index);
                EntityIndex::Table(index)
            }
            wasmparser::ExternalKind::Memory => {
                let index = MemoryIndex::from_u32(export.index);
                EntityIndex::Memory(index)
            }
            wasmparser::ExternalKind::Global => {
                let index = GlobalIndex::from_u32(export.index);
                EntityIndex::Global(index)
            }

            // doesn't get past validation
            wasmparser::ExternalKind::Tag => unimplemented!("wasm exceptions"),
        };
        map.insert(export.name, idx);
    }
    LocalInitializer::ModuleSynthetic(map)
}

/// Converts wasmparser's `CanonicalOption` into our `LocalCanonicalOptions`.
fn canonical_options(opts: &[wasmparser::CanonicalOption]) -> LocalCanonicalOptions {
    let mut ret = LocalCanonicalOptions {
        string_encoding: StringEncoding::Utf8,
        memory: None,
        realloc: None,
        post_return: None,
        is_async: false,
        async_callback: None,
    };
    for opt in opts {
        match opt {
            wasmparser::CanonicalOption::UTF8 => {
                ret.string_encoding = StringEncoding::Utf8;
            }
            wasmparser::CanonicalOption::UTF16 => {
                ret.string_encoding = StringEncoding::Utf16;
            }
            wasmparser::CanonicalOption::CompactUTF16 => {
                ret.string_encoding = StringEncoding::CompactUtf16;
            }
            wasmparser::CanonicalOption::Memory(idx) => {
                let idx = MemoryIndex::from_u32(*idx);
                ret.memory = Some(idx);
            }
            wasmparser::CanonicalOption::Realloc(idx) => {
                let idx = FuncIndex::from_u32(*idx);
                ret.realloc = Some(idx);
            }
            wasmparser::CanonicalOption::PostReturn(idx) => {
                let idx = FuncIndex::from_u32(*idx);
                ret.post_return = Some(idx);
            }
            wasmparser::CanonicalOption::Async => {
                ret.is_async = true;
            }
            wasmparser::CanonicalOption::Callback(idx) => {
                ret.async_callback = Some(FuncIndex::from_u32(*idx));
            }
        }
    }
    ret
}

/// Converts wasmparser module instance alias information into `LocalInitializer`.
fn alias_module_instance_export(
    kind: wasmparser::ExternalKind,
    instance: ModuleInstanceIndex,
    name: &str,
) -> LocalInitializer<'_> {
    match kind {
        wasmparser::ExternalKind::Func => LocalInitializer::AliasExportFunc(instance, name),
        wasmparser::ExternalKind::Memory => LocalInitializer::AliasExportMemory(instance, name),
        wasmparser::ExternalKind::Table => LocalInitializer::AliasExportTable(instance, name),
        wasmparser::ExternalKind::Global => LocalInitializer::AliasExportGlobal(instance, name),
        wasmparser::ExternalKind::Tag => {
            unimplemented!("wasm exceptions");
        }
    }
}

impl ParsedComponent<'_> {
    pub fn types_ref(&self) -> wasmparser::types::TypesRef<'_> {
        self.types.as_ref().unwrap().as_ref()
    }
}

/// Possible encodings of strings within the component model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
#[repr(u8)]
pub enum StringEncoding {
    Utf8,
    Utf16,
    CompactUtf16,
}
