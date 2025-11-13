//! Component level types for the Wasm component model.

// Based on wasmtime v16.0 Wasm component translation

#![allow(dead_code)]

pub mod resources;

use alloc::sync::Arc;
use core::{hash::Hash, ops::Index};

use anyhow::{bail, Result};
use cranelift_entity::{EntityRef, PrimaryMap};
use indexmap::IndexMap;
use midenc_hir::{FxHashMap, SmallVec};
use wasmparser::{
    collections::IndexSet,
    component_types,
    names::KebabString,
    types::{self, TypesRef},
};

use self::resources::ResourcesBuilder;
use crate::{
    indices,
    module::types::{
        convert_func_type, convert_global_type, convert_table_type, EntityType, ModuleTypes,
        ModuleTypesBuilder,
    },
    translation_utils::{DiscriminantSize, FlagsSize},
};

/// Maximum nesting depth of a type allowed
///
/// This constant isn't chosen via any scientific means and its main purpose is
/// to handle types via recursion without worrying about stack overflow.
const MAX_TYPE_DEPTH: u32 = 100;

/// Canonical ABI-defined constant for the maximum number of "flat" parameters
/// to a wasm function, or the maximum number of parameters a core wasm function
/// will take for just the parameters used. Over this number the heap is used
/// for transferring parameters.
pub const MAX_FLAT_PARAMS: usize = 16;

/// Canonical ABI-defined constant for the maximum number of "flat" results.
/// This number of results are returned directly from wasm and otherwise results
/// are transferred through memory.
pub const MAX_FLAT_RESULTS: usize = 1;

indices! {
    // ========================================================================
    // Like Core WebAssembly, the Component Model places each definition into
    // one of a fixed set of index spaces, allowing the definition to be
    // referred to by subsequent definitions (in the text and binary format) via
    // a nonnegative integral index. When defining, validating and executing a
    // component, there are 5 component-level index spaces:

    // (component) functions
    // (component) values
    // (component) types
    // component instances
    // components

    // and 2 additional core index spaces that contain core definition
    // introduced by the Component Model that are not in WebAssembly 1.0 (yet:
    // the module-linking proposal would add them):

    // module instances
    // modules

    // for a total of 12 index spaces that need to be maintained by an implementation when, e.g., validating a component.

    // These indices are used during translation time only when we're translating a
    // component at this time.

    /// Index within a component's component type index space.
    pub struct ComponentTypeIndex(u32);

    /// Index within a component's module index space.
    pub struct ModuleIndex(u32);

    /// Index within a component's component index space.
    pub struct ComponentIndex(u32);

    /// Index within a component's module instance index space.
    pub struct ModuleInstanceIndex(u32);

    /// Index within a component's component instance index space.
    pub struct ComponentInstanceIndex(u32);

    /// Index within a component's component function index space.
    pub struct ComponentFuncIndex(u32);


    /// Index into the global list of modules found within an entire component.
    ///
    /// Parsed modulu are saved on the side to get fully translated after
    /// the original component has finished being translated.
    pub struct StaticModuleIndex(u32);

    // ========================================================================
    // These indices are used to lookup type information within a `TypeTables`
    // structure. These represent generally deduplicated type information across
    // an entire component and are a form of RTTI in a sense.

    /// Index pointing to a component's type (exports/imports with
    /// component-model types)
    pub struct TypeComponentIndex(u32);

    /// Index pointing to a component instance's type (exports with
    /// component-model types, no imports)
    pub struct TypeComponentInstanceIndex(u32);

    /// Index pointing to a core wasm module's type (exports/imports with
    /// core wasm types)
    pub struct TypeModuleIndex(u32);

    /// Index pointing to a component model function type with arguments/result
    /// as interface types.
    pub struct TypeFuncIndex(u32);

    /// Index pointing to a record type in the component model (aka a struct).
    pub struct TypeRecordIndex(u32);
    /// Index pointing to a variant type in the component model (aka an enum).
    pub struct TypeVariantIndex(u32);
    /// Index pointing to a tuple type in the component model.
    pub struct TypeTupleIndex(u32);
    /// Index pointing to a flags type in the component model.
    pub struct TypeFlagsIndex(u32);
    /// Index pointing to an enum type in the component model.
    pub struct TypeEnumIndex(u32);
    /// Index pointing to an option type in the component model (aka a
    /// `Option<T, E>`)
    pub struct TypeOptionIndex(u32);
    /// Index pointing to an result type in the component model (aka a
    /// `Result<T, E>`)
    pub struct TypeResultIndex(u32);
    /// Index pointing to a list type in the component model.
    pub struct TypeListIndex(u32);

    /// Index pointing to a resource table within a component.
    ///
    /// This is a type index which isn't part of the component
    /// model per-se (or at least not the binary format). This index represents
    /// a pointer to a table of runtime information tracking state for resources
    /// within a component. Tables are generated per-resource-per-component
    /// meaning that if the exact same resource is imported into 4 subcomponents
    /// then that's 5 tables: one for the defining component and one for each
    /// subcomponent.
    ///
    /// All resource-related intrinsics operate on table-local indices which
    /// indicate which table the intrinsic is modifying. Each resource table has
    /// an origin resource type (defined by `ResourceIndex`) along with a
    /// component instance that it's recorded for.
    pub struct TypeResourceTableIndex(u32);

    /// Index pointing to a resource within a component.
    ///
    /// This index space covers all unique resource type definitions. For
    /// example all unique imports come first and then all locally-defined
    /// resources come next. Note that this does not count the number of runtime
    /// tables required to track resources (that's `TypeResourceTableIndex`
    /// instead). Instead this is a count of the number of unique
    /// `(type (resource (rep ..)))` declarations within a component, plus
    /// imports.
    ///
    /// This is then used for correlating various information such as
    /// destructors, origin information, etc.
    pub struct ResourceIndex(u32);

    /// Index pointing to a local resource defined within a component.
    ///
    /// This is similar to `FooIndex` and `DefinedFooIndex` for core wasm and
    /// the idea here is that this is guaranteed to be a wasm-defined resource
    /// which is connected to a component instance for example.
    pub struct DefinedResourceIndex(u32);

    // ========================================================================
    // Index types used to identify modules and components during translation.

    /// Index into a "closed over variables" list for components used to
    /// implement outer aliases. For more information on this see the
    /// documentation for the `LexicalScope` structure.
    pub struct ModuleUpvarIndex(u32);

    /// Same as `ModuleUpvarIndex` but for components.
    pub struct ComponentUpvarIndex(u32);

    /// Same as `StaticModuleIndex` but for components.
    pub struct StaticComponentIndex(u32);

    // ========================================================================
    // These indices are actually used at runtime when managing a component at
    // this time.

    /// Index that represents a core wasm instance created at runtime.
    ///
    /// This is used to keep track of when instances are created and is able to
    /// refer back to previously created instances for exports and such.
    pub struct RuntimeInstanceIndex(u32);

    /// Same as `RuntimeInstanceIndex` but tracks component instances instead.
    pub struct RuntimeComponentInstanceIndex(u32);

    /// Used to index imports into a `LinearComponent`
    ///
    /// This does not correspond to anything in the binary format for the
    /// component model.
    pub struct ImportIndex(u32);

    /// Index that represents a leaf item imported into a component where a
    /// "leaf" means "not an instance".
    ///
    /// This does not correspond to anything in the binary format for the
    /// component model.
    pub struct RuntimeImportIndex(u32);

    /// Index that represents a lowered host function and is used to represent
    /// host function lowerings with options and such.
    ///
    /// This does not correspond to anything in the binary format for the
    /// component model.
    pub struct LoweredIndex(u32);

    /// Index representing a linear memory extracted from a wasm instance.
    ///
    /// This does not correspond to anything in the binary format for the
    /// component model.
    pub struct RuntimeMemoryIndex(u32);

    /// Same as `RuntimeMemoryIndex` except for the `realloc` function.
    pub struct RuntimeReallocIndex(u32);

    /// Same as `RuntimeMemoryIndex` except for the `post-return` function.
    pub struct RuntimePostReturnIndex(u32);

    /// Index for all trampolines that are defined for a
    /// component.
    pub struct TrampolineIndex(u32);

    /// Index type of a signature (imported or defined) inside all of the core modules of the flattened root component.
    pub struct SignatureIndex(u32);

}

/// Equivalent of `EntityIndex` but for the component model instead of core
/// wasm.
#[derive(Debug, Clone, Copy)]
pub enum ComponentItem {
    Func(ComponentFuncIndex),
    Module(ModuleIndex),
    Component(ComponentIndex),
    ComponentInstance(ComponentInstanceIndex),
    Type(component_types::ComponentAnyTypeId),
}
impl ComponentItem {
    pub(crate) fn unwrap_instance(&self) -> ComponentInstanceIndex {
        match self {
            ComponentItem::ComponentInstance(i) => *i,
            _ => panic!("expected a component instance, got {self:?}"),
        }
    }
}

/// Runtime information about the type information contained within a component.
///
/// One of these is created per top-level component which describes all of the
/// types contained within the top-level component itself. Each sub-component
/// will have a pointer to this value as well.
#[derive(Default)]
pub struct ComponentTypes {
    modules: PrimaryMap<TypeModuleIndex, TypeModule>,
    components: PrimaryMap<TypeComponentIndex, TypeComponent>,
    component_instances: PrimaryMap<TypeComponentInstanceIndex, TypeComponentInstance>,
    functions: PrimaryMap<TypeFuncIndex, TypeFunc>,
    lists: PrimaryMap<TypeListIndex, TypeList>,
    records: PrimaryMap<TypeRecordIndex, TypeRecord>,
    variants: PrimaryMap<TypeVariantIndex, TypeVariant>,
    tuples: PrimaryMap<TypeTupleIndex, TypeTuple>,
    enums: PrimaryMap<TypeEnumIndex, TypeEnum>,
    flags: PrimaryMap<TypeFlagsIndex, TypeFlags>,
    options: PrimaryMap<TypeOptionIndex, TypeOption>,
    results: PrimaryMap<TypeResultIndex, TypeResult>,
    resource_tables: PrimaryMap<TypeResourceTableIndex, TypeResourceTable>,

    module_types: ModuleTypes,
}

impl ComponentTypes {
    /// Returns the core wasm module types known within this component.
    pub fn module_types(&self) -> &ModuleTypes {
        &self.module_types
    }

    /// Returns the canonical ABI information about the specified type.
    pub fn canonical_abi(&self, ty: &InterfaceType) -> &CanonicalAbiInfo {
        match ty {
            InterfaceType::U8 | InterfaceType::S8 | InterfaceType::Bool => {
                &CanonicalAbiInfo::SCALAR1
            }

            InterfaceType::U16 | InterfaceType::S16 => &CanonicalAbiInfo::SCALAR2,

            InterfaceType::U32
            | InterfaceType::S32
            | InterfaceType::Float32
            | InterfaceType::Char
            | InterfaceType::Own(_)
            | InterfaceType::Borrow(_) => &CanonicalAbiInfo::SCALAR4,

            InterfaceType::U64 | InterfaceType::S64 | InterfaceType::Float64 => {
                &CanonicalAbiInfo::SCALAR8
            }

            InterfaceType::String | InterfaceType::List(_) | InterfaceType::ErrorContext => {
                &CanonicalAbiInfo::POINTER_PAIR
            }

            InterfaceType::Record(i) => &self[*i].abi,
            InterfaceType::Variant(i) => &self[*i].abi,
            InterfaceType::Tuple(i) => &self[*i].abi,
            InterfaceType::Flags(i) => &self[*i].abi,
            InterfaceType::Enum(i) => &self[*i].abi,
            InterfaceType::Option(i) => &self[*i].abi,
            InterfaceType::Result(i) => &self[*i].abi,
        }
    }
}

macro_rules! impl_index {
    ($(impl Index<$ty:ident> for ComponentTypes { $output:ident => $field:ident })*) => ($(
        impl std::ops::Index<$ty> for ComponentTypes {
            type Output = $output;
            #[inline]
            fn index(&self, idx: $ty) -> &$output {
                &self.$field[idx]
            }
        }

        impl std::ops::Index<$ty> for ComponentTypesBuilder {
            type Output = $output;
            #[inline]
            fn index(&self, idx: $ty) -> &$output {
                &self.component_types[idx]
            }
        }
    )*)
}

impl_index! {
    impl Index<TypeModuleIndex> for ComponentTypes { TypeModule => modules }
    impl Index<TypeComponentIndex> for ComponentTypes { TypeComponent => components }
    impl Index<TypeComponentInstanceIndex> for ComponentTypes { TypeComponentInstance => component_instances }
    impl Index<TypeFuncIndex> for ComponentTypes { TypeFunc => functions }
    impl Index<TypeRecordIndex> for ComponentTypes { TypeRecord => records }
    impl Index<TypeVariantIndex> for ComponentTypes { TypeVariant => variants }
    impl Index<TypeTupleIndex> for ComponentTypes { TypeTuple => tuples }
    impl Index<TypeEnumIndex> for ComponentTypes { TypeEnum => enums }
    impl Index<TypeFlagsIndex> for ComponentTypes { TypeFlags => flags }
    impl Index<TypeOptionIndex> for ComponentTypes { TypeOption => options }
    impl Index<TypeResultIndex> for ComponentTypes { TypeResult => results }
    impl Index<TypeListIndex> for ComponentTypes { TypeList => lists }
    impl Index<TypeResourceTableIndex> for ComponentTypes { TypeResourceTable => resource_tables }
}

// Additionally forward anything that can index `ModuleTypes` to `ModuleTypes`
// (aka `SignatureIndex`)
impl<T> Index<T> for ComponentTypes
where
    ModuleTypes: Index<T>,
{
    type Output = <ModuleTypes as Index<T>>::Output;

    fn index(&self, idx: T) -> &Self::Output {
        self.module_types.index(idx)
    }
}

impl<T> Index<T> for ComponentTypesBuilder
where
    ModuleTypes: Index<T>,
{
    type Output = <ModuleTypes as Index<T>>::Output;

    fn index(&self, idx: T) -> &Self::Output {
        self.module_types.index(idx)
    }
}

/// Structured used to build a [`ComponentTypes`] during translation.
///
/// This contains tables to intern any component types found as well as
/// managing building up core wasm [`ModuleTypes`] as well.
#[derive(Default)]
pub struct ComponentTypesBuilder {
    functions: FxHashMap<TypeFunc, TypeFuncIndex>,
    lists: FxHashMap<TypeList, TypeListIndex>,
    records: FxHashMap<TypeRecord, TypeRecordIndex>,
    variants: FxHashMap<TypeVariant, TypeVariantIndex>,
    tuples: FxHashMap<TypeTuple, TypeTupleIndex>,
    enums: FxHashMap<TypeEnum, TypeEnumIndex>,
    flags: FxHashMap<TypeFlags, TypeFlagsIndex>,
    options: FxHashMap<TypeOption, TypeOptionIndex>,
    results: FxHashMap<TypeResult, TypeResultIndex>,

    component_types: ComponentTypes,
    module_types: ModuleTypesBuilder,

    // Cache of what the "flat" representation of all types are which is only
    // used at translation.
    type_info: TypeInformationCache,

    resources: ResourcesBuilder,
}

macro_rules! intern_and_fill_flat_types {
    ($me:ident, $name:ident, $val:ident) => {{
        if let Some(idx) = $me.$name.get(&$val) {
            return *idx;
        }
        let idx = $me.component_types.$name.push($val.clone());
        let mut info = TypeInformation::new();
        info.$name($me, &$val);
        let idx2 = $me.type_info.$name.push(info);
        assert_eq!(idx, idx2);
        $me.$name.insert($val, idx);
        return idx;
    }};
}

impl ComponentTypesBuilder {
    /// Finishes this list of component types and returns the finished
    /// structure.
    pub fn finish(mut self) -> ComponentTypes {
        self.component_types.module_types = self.module_types.finish();
        self.component_types
    }

    /// Returns the underlying builder used to build up core wasm module types.
    ///
    /// Note that this is shared across all modules found within a component to
    /// improve the wins from deduplicating function signatures.
    pub fn module_types_builder(&self) -> &ModuleTypesBuilder {
        &self.module_types
    }

    /// Same as `module_types_builder`, but `mut`.
    pub fn module_types_builder_mut(&mut self) -> &mut ModuleTypesBuilder {
        &mut self.module_types
    }

    /// Returns the number of resource tables allocated so far, or the maximum
    /// `TypeResourceTableIndex`.
    pub fn num_resource_tables(&self) -> usize {
        self.component_types.resource_tables.len()
    }

    /// Returns a mutable reference to the underlying `ResourcesBuilder`.
    pub fn resources_mut(&mut self) -> &mut ResourcesBuilder {
        &mut self.resources
    }

    /// Work around the borrow checker to borrow two sub-fields simultaneously
    /// externally.
    pub fn resources_mut_and_types(&mut self) -> (&mut ResourcesBuilder, &ComponentTypes) {
        (&mut self.resources, &self.component_types)
    }

    /// Converts a wasmparser `ComponentFuncType`
    pub fn convert_component_func_type(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentFuncTypeId,
    ) -> Result<TypeFuncIndex> {
        let ty = &types[id];
        let params = ty
            .params
            .iter()
            .map(|(_name, ty)| self.valtype(types, ty))
            .collect::<Result<_>>()?;
        let results = match ty.result.as_ref() {
            Some(ty) => SmallVec::from_buf([self.valtype(types, ty)?]).into_boxed_slice(),
            None => Box::<[_]>::default(),
        };
        let ty = TypeFunc {
            params: self.new_tuple_type(params),
            results: self.new_tuple_type(results),
        };
        Ok(self.add_func_type(ty))
    }

    /// Converts a wasmparser `ComponentEntityType`
    pub fn convert_component_entity_type(
        &mut self,
        types: TypesRef<'_>,
        ty: component_types::ComponentEntityType,
    ) -> Result<TypeDef> {
        Ok(match ty {
            component_types::ComponentEntityType::Module(id) => {
                TypeDef::Module(self.convert_module(types, id)?)
            }
            component_types::ComponentEntityType::Component(id) => {
                TypeDef::Component(self.convert_component(types, id)?)
            }
            component_types::ComponentEntityType::Instance(id) => {
                TypeDef::ComponentInstance(self.convert_instance(types, id)?)
            }
            component_types::ComponentEntityType::Func(id) => {
                TypeDef::ComponentFunc(self.convert_component_func_type(types, id)?)
            }
            component_types::ComponentEntityType::Type { created, .. } => match created {
                component_types::ComponentAnyTypeId::Defined(id) => {
                    TypeDef::Interface(self.defined_type(types, id)?)
                }
                component_types::ComponentAnyTypeId::Resource(id) => {
                    TypeDef::Resource(self.resource_id(id.resource()))
                }
                _ => bail!("unsupported type export"),
            },
            component_types::ComponentEntityType::Value(_) => bail!("values not supported"),
        })
    }

    /// Converts a wasmparser `Type`
    pub fn convert_type(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentAnyTypeId,
    ) -> Result<TypeDef> {
        Ok(match id {
            component_types::ComponentAnyTypeId::Defined(id) => {
                TypeDef::Interface(self.defined_type(types, id)?)
            }
            component_types::ComponentAnyTypeId::Component(id) => {
                TypeDef::Component(self.convert_component(types, id)?)
            }
            component_types::ComponentAnyTypeId::Instance(id) => {
                TypeDef::ComponentInstance(self.convert_instance(types, id)?)
            }
            component_types::ComponentAnyTypeId::Func(id) => {
                TypeDef::ComponentFunc(self.convert_component_func_type(types, id)?)
            }
            component_types::ComponentAnyTypeId::Resource(id) => {
                TypeDef::Resource(self.resource_id(id.resource()))
            }
        })
    }

    fn convert_component(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentTypeId,
    ) -> Result<TypeComponentIndex> {
        let ty = &types[id];
        let mut result = TypeComponent::default();
        for (name, ty) in ty.imports.iter() {
            result
                .imports
                .insert(name.clone(), self.convert_component_entity_type(types, *ty)?);
        }
        for (name, ty) in ty.exports.iter() {
            result
                .exports
                .insert(name.clone(), self.convert_component_entity_type(types, *ty)?);
        }
        Ok(self.component_types.components.push(result))
    }

    fn convert_instance(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentInstanceTypeId,
    ) -> Result<TypeComponentInstanceIndex> {
        let ty = &types[id];
        let mut result = TypeComponentInstance::default();
        for (name, ty) in ty.exports.iter() {
            result
                .exports
                .insert(name.clone(), self.convert_component_entity_type(types, *ty)?);
        }
        Ok(self.component_types.component_instances.push(result))
    }

    fn convert_module(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentCoreModuleTypeId,
    ) -> Result<TypeModuleIndex> {
        let ty = &types[id];
        let mut result = TypeModule::default();
        for ((module, field), ty) in ty.imports.iter() {
            result
                .imports
                .insert((module.clone(), field.clone()), self.entity_type(types, ty)?);
        }
        for (name, ty) in ty.exports.iter() {
            result.exports.insert(name.clone(), self.entity_type(types, ty)?);
        }
        Ok(self.component_types.modules.push(result))
    }

    fn entity_type(&mut self, types: TypesRef<'_>, ty: &types::EntityType) -> Result<EntityType> {
        Ok(match ty {
            types::EntityType::Func(idx) => {
                let ty = types[*idx].unwrap_func();
                let ty = convert_func_type(ty);
                EntityType::Function(self.module_types_builder_mut().wasm_func_type(*idx, ty))
            }
            types::EntityType::Table(ty) => EntityType::Table(convert_table_type(ty)),
            types::EntityType::Memory(ty) => EntityType::Memory((*ty).into()),
            types::EntityType::Global(ty) => EntityType::Global(convert_global_type(ty)),
            types::EntityType::Tag(_) => bail!("exceptions proposal not implemented"),
        })
    }

    fn defined_type(
        &mut self,
        types: TypesRef<'_>,
        id: component_types::ComponentDefinedTypeId,
    ) -> Result<InterfaceType> {
        let ret = match &types[id] {
            component_types::ComponentDefinedType::Primitive(ty) => ty.into(),
            component_types::ComponentDefinedType::Record(e) => {
                InterfaceType::Record(self.record_type(types, e)?)
            }
            component_types::ComponentDefinedType::Variant(e) => {
                InterfaceType::Variant(self.variant_type(types, e)?)
            }
            component_types::ComponentDefinedType::List(e) => {
                InterfaceType::List(self.list_type(types, e)?)
            }
            component_types::ComponentDefinedType::Tuple(e) => {
                InterfaceType::Tuple(self.tuple_type(types, e)?)
            }
            component_types::ComponentDefinedType::Flags(e) => {
                InterfaceType::Flags(self.flags_type(e))
            }
            component_types::ComponentDefinedType::Enum(e) => {
                InterfaceType::Enum(self.enum_type(e))
            }
            component_types::ComponentDefinedType::Option(e) => {
                InterfaceType::Option(self.option_type(types, e)?)
            }
            component_types::ComponentDefinedType::Result { ok, err } => {
                InterfaceType::Result(self.result_type(types, ok, err)?)
            }
            component_types::ComponentDefinedType::Own(r) => {
                InterfaceType::Own(self.resource_id(r.resource()))
            }
            component_types::ComponentDefinedType::Borrow(r) => {
                InterfaceType::Borrow(self.resource_id(r.resource()))
            }
            component_types::ComponentDefinedType::Stream(_)
            | component_types::ComponentDefinedType::Future(_) => {
                unimplemented!("support for the async proposal is not implemented")
            }
        };
        let info = self.type_information(&ret);
        if info.depth > MAX_TYPE_DEPTH {
            bail!("type nesting is too deep");
        }
        Ok(ret)
    }

    fn valtype(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::ComponentValType,
    ) -> Result<InterfaceType> {
        match ty {
            component_types::ComponentValType::Primitive(p) => Ok(p.into()),
            component_types::ComponentValType::Type(id) => self.defined_type(types, *id),
        }
    }

    fn record_type(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::RecordType,
    ) -> Result<TypeRecordIndex> {
        let fields = ty
            .fields
            .iter()
            .map(|(name, ty)| {
                Ok(RecordField {
                    name: name.to_string(),
                    ty: self.valtype(types, ty)?,
                })
            })
            .collect::<Result<Box<[_]>>>()?;
        let abi = CanonicalAbiInfo::record(
            fields.iter().map(|field| self.component_types.canonical_abi(&field.ty)),
        );
        Ok(self.add_record_type(TypeRecord { fields, abi }))
    }

    fn variant_type(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::VariantType,
    ) -> Result<TypeVariantIndex> {
        let cases = ty
            .cases
            .iter()
            .map(|(name, case)| {
                if case.refines.is_some() {
                    bail!("refines is not supported at this time");
                }
                Ok(VariantCase {
                    name: name.to_string(),
                    ty: match &case.ty.as_ref() {
                        Some(ty) => Some(self.valtype(types, ty)?),
                        None => None,
                    },
                })
            })
            .collect::<Result<Box<[_]>>>()?;
        let (info, abi) = VariantInfo::new(
            cases
                .iter()
                .map(|c| c.ty.as_ref().map(|ty| self.component_types.canonical_abi(ty))),
        );
        Ok(self.add_variant_type(TypeVariant { cases, abi, info }))
    }

    fn tuple_type(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::TupleType,
    ) -> Result<TypeTupleIndex> {
        let types = ty
            .types
            .iter()
            .map(|ty| self.valtype(types, ty))
            .collect::<Result<Box<[_]>>>()?;
        Ok(self.new_tuple_type(types))
    }

    fn new_tuple_type(&mut self, types: Box<[InterfaceType]>) -> TypeTupleIndex {
        let abi =
            CanonicalAbiInfo::record(types.iter().map(|ty| self.component_types.canonical_abi(ty)));
        self.add_tuple_type(TypeTuple { types, abi })
    }

    fn flags_type(&mut self, flags: &IndexSet<KebabString>) -> TypeFlagsIndex {
        let flags = TypeFlags {
            names: flags.iter().map(|s| s.to_string()).collect(),
            abi: CanonicalAbiInfo::flags(flags.len()),
        };
        self.add_flags_type(flags)
    }

    fn enum_type(&mut self, variants: &IndexSet<KebabString>) -> TypeEnumIndex {
        let names = variants.iter().map(|s| s.to_string()).collect::<Box<[_]>>();
        let (info, abi) = VariantInfo::new(names.iter().map(|_| None));
        self.add_enum_type(TypeEnum { names, abi, info })
    }

    fn option_type(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::ComponentValType,
    ) -> Result<TypeOptionIndex> {
        let ty = self.valtype(types, ty)?;
        let (info, abi) = VariantInfo::new([None, Some(self.component_types.canonical_abi(&ty))]);
        Ok(self.add_option_type(TypeOption { ty, abi, info }))
    }

    fn result_type(
        &mut self,
        types: TypesRef<'_>,
        ok: &Option<component_types::ComponentValType>,
        err: &Option<component_types::ComponentValType>,
    ) -> Result<TypeResultIndex> {
        let ok = match ok {
            Some(ty) => Some(self.valtype(types, ty)?),
            None => None,
        };
        let err = match err {
            Some(ty) => Some(self.valtype(types, ty)?),
            None => None,
        };
        let (info, abi) = VariantInfo::new([
            ok.as_ref().map(|t| self.component_types.canonical_abi(t)),
            err.as_ref().map(|t| self.component_types.canonical_abi(t)),
        ]);
        Ok(self.add_result_type(TypeResult { ok, err, abi, info }))
    }

    fn list_type(
        &mut self,
        types: TypesRef<'_>,
        ty: &component_types::ComponentValType,
    ) -> Result<TypeListIndex> {
        let element = self.valtype(types, ty)?;
        Ok(self.add_list_type(TypeList { element }))
    }

    /// Converts a wasmparser `id`, which must point to a resource, to its
    /// corresponding `TypeResourceTableIndex`.
    pub fn resource_id(&mut self, id: component_types::ResourceId) -> TypeResourceTableIndex {
        self.resources.convert(id, &mut self.component_types)
    }

    /// Interns a new function type within this type information.
    pub fn add_func_type(&mut self, ty: TypeFunc) -> TypeFuncIndex {
        intern(&mut self.functions, &mut self.component_types.functions, ty)
    }

    /// Interns a new record type within this type information.
    pub fn add_record_type(&mut self, ty: TypeRecord) -> TypeRecordIndex {
        intern_and_fill_flat_types!(self, records, ty)
    }

    /// Interns a new flags type within this type information.
    pub fn add_flags_type(&mut self, ty: TypeFlags) -> TypeFlagsIndex {
        intern_and_fill_flat_types!(self, flags, ty)
    }

    /// Interns a new tuple type within this type information.
    pub fn add_tuple_type(&mut self, ty: TypeTuple) -> TypeTupleIndex {
        intern_and_fill_flat_types!(self, tuples, ty)
    }

    /// Interns a new variant type within this type information.
    pub fn add_variant_type(&mut self, ty: TypeVariant) -> TypeVariantIndex {
        intern_and_fill_flat_types!(self, variants, ty)
    }

    /// Interns a new enum type within this type information.
    pub fn add_enum_type(&mut self, ty: TypeEnum) -> TypeEnumIndex {
        intern_and_fill_flat_types!(self, enums, ty)
    }

    /// Interns a new option type within this type information.
    pub fn add_option_type(&mut self, ty: TypeOption) -> TypeOptionIndex {
        intern_and_fill_flat_types!(self, options, ty)
    }

    /// Interns a new result type within this type information.
    pub fn add_result_type(&mut self, ty: TypeResult) -> TypeResultIndex {
        intern_and_fill_flat_types!(self, results, ty)
    }

    /// Interns a new type within this type information.
    pub fn add_list_type(&mut self, ty: TypeList) -> TypeListIndex {
        intern_and_fill_flat_types!(self, lists, ty)
    }

    /// Returns the canonical ABI information about the specified type.
    pub fn canonical_abi(&self, ty: &InterfaceType) -> &CanonicalAbiInfo {
        self.component_types.canonical_abi(ty)
    }

    /// Returns the "flat types" for the given interface type used in the
    /// canonical ABI.
    ///
    /// Returns `None` if the type is too large to be represented via flat types
    /// in the canonical abi.
    pub fn flat_types(&self, ty: &InterfaceType) -> Option<FlatTypes<'_>> {
        self.type_information(ty).flat.as_flat_types()
    }

    /// Returns whether the type specified contains any borrowed resources
    /// within it.
    pub fn ty_contains_borrow_resource(&self, ty: &InterfaceType) -> bool {
        self.type_information(ty).has_borrow
    }

    fn type_information(&self, ty: &InterfaceType) -> &TypeInformation {
        match ty {
            InterfaceType::U8
            | InterfaceType::S8
            | InterfaceType::Bool
            | InterfaceType::U16
            | InterfaceType::S16
            | InterfaceType::U32
            | InterfaceType::S32
            | InterfaceType::Char
            | InterfaceType::ErrorContext
            | InterfaceType::Own(_) => {
                static INFO: TypeInformation = TypeInformation::primitive(FlatType::I32);
                &INFO
            }
            InterfaceType::Borrow(_) => {
                static INFO: TypeInformation = {
                    let mut info = TypeInformation::primitive(FlatType::I32);
                    info.has_borrow = true;
                    info
                };
                &INFO
            }
            InterfaceType::U64 | InterfaceType::S64 => {
                static INFO: TypeInformation = TypeInformation::primitive(FlatType::I64);
                &INFO
            }
            InterfaceType::Float32 => {
                static INFO: TypeInformation = TypeInformation::primitive(FlatType::F32);
                &INFO
            }
            InterfaceType::Float64 => {
                static INFO: TypeInformation = TypeInformation::primitive(FlatType::F64);
                &INFO
            }
            InterfaceType::String => {
                static INFO: TypeInformation = TypeInformation::string();
                &INFO
            }

            InterfaceType::List(i) => &self.type_info.lists[*i],
            InterfaceType::Record(i) => &self.type_info.records[*i],
            InterfaceType::Variant(i) => &self.type_info.variants[*i],
            InterfaceType::Tuple(i) => &self.type_info.tuples[*i],
            InterfaceType::Flags(i) => &self.type_info.flags[*i],
            InterfaceType::Enum(i) => &self.type_info.enums[*i],
            InterfaceType::Option(i) => &self.type_info.options[*i],
            InterfaceType::Result(i) => &self.type_info.results[*i],
        }
    }
}

fn intern<T, U>(map: &mut FxHashMap<T, U>, list: &mut PrimaryMap<U, T>, item: T) -> U
where
    T: Hash + Clone + Eq,
    U: Copy + EntityRef,
{
    if let Some(idx) = map.get(&item) {
        return *idx;
    }
    let idx = list.push(item.clone());
    map.insert(item, idx);
    idx
}

/// Types of imports and exports in the component model.
///
/// These types are what's available for import and export in components. Note
/// that all indirect indices contained here are intended to be looked up
/// through a sibling `ComponentTypes` structure.
#[derive(Copy, Clone, Debug)]
pub enum TypeDef {
    /// A component and its type.
    Component(TypeComponentIndex),
    /// An instance of a component.
    ComponentInstance(TypeComponentInstanceIndex),
    /// A component function, not to be confused with a core wasm function.
    ComponentFunc(TypeFuncIndex),
    /// An interface type.
    Interface(InterfaceType),
    /// A core wasm module and its type.
    Module(TypeModuleIndex),
    /// A resource type which operates on the specified resource table.
    ///
    /// Note that different resource tables may point to the same underlying
    /// actual resource type, but that's a private detail.
    Resource(TypeResourceTableIndex),
}

/// The type of a module in the component model.
///
/// Note that this is not to be confused with `TypeComponent` below. This is
/// intended only for core wasm modules, not for components.
#[derive(Default)]
pub struct TypeModule {
    /// The values that this module imports.
    ///
    /// Note that the value of this map is a core wasm `EntityType`, not a
    /// component model `TypeRef`. Additionally note that this reflects the
    /// two-level namespace of core WebAssembly, but unlike core wasm all import
    /// names are required to be unique to describe a module in the component
    /// model.
    pub imports: FxHashMap<(String, String), EntityType>,

    /// The values that this module exports.
    ///
    /// Note that the value of this map is the core wasm `EntityType` to
    /// represent that core wasm items are being exported.
    pub exports: FxHashMap<String, EntityType>,
}

/// The type of a component in the component model.
#[derive(Default)]
pub struct TypeComponent {
    /// The named values that this component imports.
    pub imports: FxHashMap<String, TypeDef>,
    /// The named values that this component exports.
    pub exports: FxHashMap<String, TypeDef>,
}

/// The type of a component instance in the component model, or an instantiated
/// component.
///
/// Component instances only have exports of types in the component model.
#[derive(Default)]
pub struct TypeComponentInstance {
    /// The list of exports that this component has along with their types.
    pub exports: IndexMap<String, TypeDef>,
}

/// A component function type in the component model.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeFunc {
    /// Parameters to the function represented as a tuple.
    pub params: TypeTupleIndex,
    /// Results of the function represented as a tuple.
    pub results: TypeTupleIndex,
}

/// All possible interface types that values can have.
///
/// This list represents an exhaustive listing of interface types and the
/// shapes that they can take. Note that this enum is considered an "index" of
/// forms where for non-primitive types a `ComponentTypes` structure is used to
/// lookup further information based on the index found here.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
#[allow(missing_docs)]
pub enum InterfaceType {
    Bool,
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    Float32,
    Float64,
    Char,
    String,
    ErrorContext,
    Record(TypeRecordIndex),
    Variant(TypeVariantIndex),
    List(TypeListIndex),
    Tuple(TypeTupleIndex),
    Flags(TypeFlagsIndex),
    Enum(TypeEnumIndex),
    Option(TypeOptionIndex),
    Result(TypeResultIndex),
    Own(TypeResourceTableIndex),
    Borrow(TypeResourceTableIndex),
}

impl From<&wasmparser::PrimitiveValType> for InterfaceType {
    fn from(ty: &wasmparser::PrimitiveValType) -> InterfaceType {
        match ty {
            wasmparser::PrimitiveValType::Bool => InterfaceType::Bool,
            wasmparser::PrimitiveValType::S8 => InterfaceType::S8,
            wasmparser::PrimitiveValType::U8 => InterfaceType::U8,
            wasmparser::PrimitiveValType::S16 => InterfaceType::S16,
            wasmparser::PrimitiveValType::U16 => InterfaceType::U16,
            wasmparser::PrimitiveValType::S32 => InterfaceType::S32,
            wasmparser::PrimitiveValType::U32 => InterfaceType::U32,
            wasmparser::PrimitiveValType::S64 => InterfaceType::S64,
            wasmparser::PrimitiveValType::U64 => InterfaceType::U64,
            wasmparser::PrimitiveValType::F32 => InterfaceType::Float32,
            wasmparser::PrimitiveValType::F64 => InterfaceType::Float64,
            wasmparser::PrimitiveValType::Char => InterfaceType::Char,
            wasmparser::PrimitiveValType::String => InterfaceType::String,
            wasmparser::PrimitiveValType::ErrorContext => InterfaceType::ErrorContext,
        }
    }
}

/// Bye information about a type in the canonical ABI
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct CanonicalAbiInfo {
    /// The byte-size of this type in a 32-bit memory.
    pub size32: u32,
    /// The byte-alignment of this type in a 32-bit memory.
    pub align32: u32,
    /// The number of types it takes to represents this type in the "flat"
    /// representation of the canonical abi where everything is passed as
    /// immediate arguments or results.
    ///
    /// If this is `None` then this type is not representable in the flat ABI
    /// because it is too large.
    pub flat_count: Option<u8>,
}

impl Default for CanonicalAbiInfo {
    fn default() -> CanonicalAbiInfo {
        CanonicalAbiInfo {
            size32: 0,
            align32: 1,
            flat_count: Some(0),
        }
    }
}

const fn align_to(a: u32, b: u32) -> u32 {
    assert!(b.is_power_of_two());
    (a + (b - 1)) & !(b - 1)
}

const fn max(a: u32, b: u32) -> u32 {
    if a > b {
        a
    } else {
        b
    }
}

impl CanonicalAbiInfo {
    /// ABI information for lists/strings which are "pointer pairs"
    pub const POINTER_PAIR: CanonicalAbiInfo = CanonicalAbiInfo {
        size32: 8,
        align32: 4,
        flat_count: Some(2),
    };
    /// ABI information for one-byte scalars.
    pub const SCALAR1: CanonicalAbiInfo = CanonicalAbiInfo::scalar(1);
    /// ABI information for two-byte scalars.
    pub const SCALAR2: CanonicalAbiInfo = CanonicalAbiInfo::scalar(2);
    /// ABI information for four-byte scalars.
    pub const SCALAR4: CanonicalAbiInfo = CanonicalAbiInfo::scalar(4);
    /// ABI information for eight-byte scalars.
    pub const SCALAR8: CanonicalAbiInfo = CanonicalAbiInfo::scalar(8);
    /// ABI information for zero-sized types.
    const ZERO: CanonicalAbiInfo = CanonicalAbiInfo {
        size32: 0,
        align32: 1,
        flat_count: Some(0),
    };

    const fn scalar(size: u32) -> CanonicalAbiInfo {
        CanonicalAbiInfo {
            size32: size,
            align32: size,
            flat_count: Some(1),
        }
    }

    /// Returns the abi for a record represented by the specified fields.
    pub fn record<'a>(fields: impl Iterator<Item = &'a CanonicalAbiInfo>) -> CanonicalAbiInfo {
        // NB: this is basically a duplicate copy of
        // `CanonicalAbiInfo::record_static` and the two should be kept in sync.

        let mut ret = CanonicalAbiInfo::default();
        for field in fields {
            ret.size32 = align_to(ret.size32, field.align32) + field.size32;
            ret.align32 = ret.align32.max(field.align32);
            ret.flat_count = add_flat(ret.flat_count, field.flat_count);
        }
        ret.size32 = align_to(ret.size32, ret.align32);
        ret
    }

    /// Same as `CanonicalAbiInfo::record` but in a `const`-friendly context.
    pub const fn record_static(fields: &[CanonicalAbiInfo]) -> CanonicalAbiInfo {
        // NB: this is basically a duplicate copy of `CanonicalAbiInfo::record`
        // and the two should be kept in sync.

        let mut ret = CanonicalAbiInfo::ZERO;
        let mut i = 0;
        while i < fields.len() {
            let field = &fields[i];
            ret.size32 = align_to(ret.size32, field.align32) + field.size32;
            ret.align32 = max(ret.align32, field.align32);
            ret.flat_count = add_flat(ret.flat_count, field.flat_count);
            i += 1;
        }
        ret.size32 = align_to(ret.size32, ret.align32);
        ret
    }

    /// Returns the delta from the current value of `offset` to align properly
    /// and read the next record field of type `abi` for 32-bit memories.
    pub fn next_field32(&self, offset: &mut u32) -> u32 {
        *offset = align_to(*offset, self.align32) + self.size32;
        *offset - self.size32
    }

    /// Same as `next_field32`, but bumps a usize pointer
    pub fn next_field32_size(&self, offset: &mut usize) -> usize {
        let cur = u32::try_from(*offset).unwrap();
        let cur = align_to(cur, self.align32) + self.size32;
        *offset = usize::try_from(cur).unwrap();
        usize::try_from(cur - self.size32).unwrap()
    }

    /// Returns ABI information for a structure which contains `count` flags.
    pub const fn flags(count: usize) -> CanonicalAbiInfo {
        let (size, align, flat_count) = match FlagsSize::from_count(count) {
            FlagsSize::Size0 => (0, 1, 0),
            FlagsSize::Size1 => (1, 1, 1),
            FlagsSize::Size2 => (2, 2, 1),
            FlagsSize::Size4Plus(n) => ((n as u32) * 4, 4, n),
        };
        CanonicalAbiInfo {
            size32: size,
            align32: align,
            flat_count: Some(flat_count),
        }
    }

    fn variant<'a, I>(cases: I) -> CanonicalAbiInfo
    where
        I: IntoIterator<Item = Option<&'a CanonicalAbiInfo>>,
        I::IntoIter: ExactSizeIterator,
    {
        // NB: this is basically a duplicate definition of
        // `CanonicalAbiInfo::variant_static`, these should be kept in sync.

        let cases = cases.into_iter();
        let discrim_size = u32::from(DiscriminantSize::from_count(cases.len()).unwrap());
        let mut max_size32 = 0;
        let mut max_align32 = discrim_size;
        let mut max_case_count = Some(0);
        for case in cases.flatten() {
            max_size32 = max_size32.max(case.size32);
            max_align32 = max_align32.max(case.align32);
            max_case_count = max_flat(max_case_count, case.flat_count);
        }
        CanonicalAbiInfo {
            size32: align_to(align_to(discrim_size, max_align32) + max_size32, max_align32),
            align32: max_align32,
            flat_count: add_flat(max_case_count, Some(1)),
        }
    }

    /// Same as `CanonicalAbiInfo::variant` but `const`-safe
    pub const fn variant_static(cases: &[Option<CanonicalAbiInfo>]) -> CanonicalAbiInfo {
        // NB: this is basically a duplicate definition of
        // `CanonicalAbiInfo::variant`, these should be kept in sync.

        let discrim_size = match DiscriminantSize::from_count(cases.len()) {
            Some(size) => size.byte_size(),
            None => unreachable!(),
        };
        let mut max_size32 = 0;
        let mut max_align32 = discrim_size;
        let mut max_case_count = Some(0);
        let mut i = 0;
        while i < cases.len() {
            let case = &cases[i];
            if let Some(case) = case {
                max_size32 = max(max_size32, case.size32);
                max_align32 = max(max_align32, case.align32);
                max_case_count = max_flat(max_case_count, case.flat_count);
            }
            i += 1;
        }
        CanonicalAbiInfo {
            size32: align_to(align_to(discrim_size, max_align32) + max_size32, max_align32),
            align32: max_align32,
            flat_count: add_flat(max_case_count, Some(1)),
        }
    }

    /// Returns the flat count of this ABI information so long as the count
    /// doesn't exceed the `max` specified.
    pub fn flat_count(&self, max: usize) -> Option<usize> {
        let flat = usize::from(self.flat_count?);
        if flat > max {
            None
        } else {
            Some(flat)
        }
    }
}

/// ABI information about the representation of a variant.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct VariantInfo {
    /// The size of the discriminant used.
    pub size: DiscriminantSize,
    /// The offset of the payload from the start of the variant in 32-bit
    /// memories.
    pub payload_offset32: u32,
}

impl VariantInfo {
    /// Returns the abi information for a variant represented by the specified
    /// cases.
    pub fn new<'a, I>(cases: I) -> (VariantInfo, CanonicalAbiInfo)
    where
        I: IntoIterator<Item = Option<&'a CanonicalAbiInfo>>,
        I::IntoIter: ExactSizeIterator,
    {
        let cases = cases.into_iter();
        let size = DiscriminantSize::from_count(cases.len()).unwrap();
        let abi = CanonicalAbiInfo::variant(cases);
        (
            VariantInfo {
                size,
                payload_offset32: align_to(u32::from(size), abi.align32),
            },
            abi,
        )
    }

    pub const fn new_static(cases: &[Option<CanonicalAbiInfo>]) -> VariantInfo {
        let size = match DiscriminantSize::from_count(cases.len()) {
            Some(size) => size,
            None => unreachable!(),
        };
        let abi = CanonicalAbiInfo::variant_static(cases);
        VariantInfo {
            size,
            payload_offset32: align_to(size.byte_size(), abi.align32),
        }
    }
}

/// Shape of a "record" type in interface types.
///
/// This is equivalent to a `struct` in Rust.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeRecord {
    /// The fields that are contained within this struct type.
    pub fields: Box<[RecordField]>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
}

/// One field within a record.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct RecordField {
    /// The name of the field, unique amongst all fields in a record.
    pub name: String,
    /// The type that this field contains.
    pub ty: InterfaceType,
}

/// Shape of a "variant" type in interface types.
///
/// Variants are close to Rust `enum` declarations where a value is one of many
/// cases and each case has a unique name and an optional payload associated
/// with it.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeVariant {
    /// The list of cases that this variant can take.
    pub cases: Box<[VariantCase]>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
    /// Byte information about this variant type.
    pub info: VariantInfo,
}

/// One case of a `variant` type which contains the name of the variant as well
/// as the payload.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct VariantCase {
    /// Name of the variant, unique amongst all cases in a variant.
    pub name: String,
    /// Optional type associated with this payload.
    pub ty: Option<InterfaceType>,
}

/// Shape of a "tuple" type in interface types.
///
/// This is largely the same as a tuple in Rust, basically a record with
/// unnamed fields.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeTuple {
    /// The types that are contained within this tuple.
    pub types: Box<[InterfaceType]>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
}

/// Shape of a "flags" type in interface types.
///
/// This can be thought of as a record-of-bools, although the representation is
/// more efficient as bitflags.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeFlags {
    /// The names of all flags, all of which are unique.
    pub names: Box<[String]>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
}

/// Shape of an "enum" type in interface types, not to be confused with a Rust
/// `enum` type.
///
/// In interface types enums are simply a bag of names, and can be seen as a
/// variant where all payloads are `Unit`.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeEnum {
    /// The names of this enum, all of which are unique.
    pub names: Box<[String]>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
    /// Byte information about this variant type.
    pub info: VariantInfo,
}

/// Shape of an "option" interface type.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeOption {
    /// The `T` in `Option<T>`
    pub ty: InterfaceType,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
    /// Byte information about this variant type.
    pub info: VariantInfo,
}

/// Shape of a "result" interface type.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeResult {
    /// The `T` in `Result<T, E>`
    pub ok: Option<InterfaceType>,
    /// The `E` in `Result<T, E>`
    pub err: Option<InterfaceType>,
    /// Byte information about this type in the canonical ABI.
    pub abi: CanonicalAbiInfo,
    /// Byte information about this variant type.
    pub info: VariantInfo,
}

/// Metadata about a resource table added to a component.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeResourceTable {
    /// The original resource that this table contains.
    ///
    /// This is used when destroying resources within this table since this
    /// original definition will know how to execute destructors.
    pub ty: ResourceIndex,

    /// The component instance that contains this resource table.
    pub instance: RuntimeComponentInstanceIndex,
}

/// Shape of a "list" interface type.
#[derive(Clone, Hash, Eq, PartialEq, Debug)]
pub struct TypeList {
    /// The element type of the list.
    pub element: InterfaceType,
}

const MAX_FLAT_TYPES: usize = if MAX_FLAT_PARAMS > MAX_FLAT_RESULTS {
    MAX_FLAT_PARAMS
} else {
    MAX_FLAT_RESULTS
};

const fn add_flat(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    const MAX: u8 = MAX_FLAT_TYPES as u8;
    let sum = match (a, b) {
        (Some(a), Some(b)) => match a.checked_add(b) {
            Some(c) => c,
            None => return None,
        },
        _ => return None,
    };
    if sum > MAX {
        None
    } else {
        Some(sum)
    }
}

const fn max_flat(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    match (a, b) {
        (Some(a), Some(b)) => {
            if a > b {
                Some(a)
            } else {
                Some(b)
            }
        }
        _ => None,
    }
}

/// Flat representation of a type in just core wasm types.
pub struct FlatTypes<'a> {
    /// The flat representation of this type in 32-bit memories.
    pub memory32: &'a [FlatType],
}

impl FlatTypes<'_> {
    /// Returns the number of flat types used to represent this type.
    ///
    /// Note that this length is the same regardless to the size of memory.
    pub fn len(&self) -> usize {
        self.memory32.len()
    }
}

// Note that this is intentionally duplicated here to keep the size to 1 byte
// irregardless to changes in the core wasm type system since this will only
// ever use integers/floats for the forseeable future.
#[derive(PartialEq, Eq, Copy, Clone)]
#[allow(missing_docs)]
pub enum FlatType {
    I32,
    I64,
    F32,
    F64,
}

struct FlatTypesStorage {
    // This could be represented as `Vec<FlatType>` but on 64-bit architectures
    // that's 24 bytes. Otherwise `FlatType` is 1 byte large and
    // `MAX_FLAT_TYPES` is 16, so it should ideally be more space-efficient to
    // use a flat array instead of a heap-based vector.
    memory32: [FlatType; MAX_FLAT_TYPES],

    // Tracks the number of flat types pushed into this storage. If this is
    // `MAX_FLAT_TYPES + 1` then this storage represents an un-reprsentable
    // type in flat types.
    len: u8,
}

impl FlatTypesStorage {
    const fn new() -> FlatTypesStorage {
        FlatTypesStorage {
            memory32: [FlatType::I32; MAX_FLAT_TYPES],
            len: 0,
        }
    }

    fn as_flat_types(&self) -> Option<FlatTypes<'_>> {
        let len = usize::from(self.len);
        if len > MAX_FLAT_TYPES {
            assert_eq!(len, MAX_FLAT_TYPES + 1);
            None
        } else {
            Some(FlatTypes {
                memory32: &self.memory32[..len],
            })
        }
    }

    /// Pushes a new flat type into this list using `t32` for 32-bit memories
    ///
    /// Returns whether the type was actually pushed or whether this list of
    /// flat types just exceeded the maximum meaning that it is now
    /// unrepresentable with a flat list of types.
    fn push(&mut self, t32: FlatType) -> bool {
        let len = usize::from(self.len);
        if len < MAX_FLAT_TYPES {
            self.memory32[len] = t32;
            self.len += 1;
            true
        } else {
            // If this was the first one to go over then flag the length as
            // being incompatible with a flat representation.
            if len == MAX_FLAT_TYPES {
                self.len += 1;
            }
            false
        }
    }
}

impl FlatType {
    fn join(&mut self, other: FlatType) {
        if *self == other {
            return;
        }
        *self = match (*self, other) {
            (FlatType::I32, FlatType::F32) | (FlatType::F32, FlatType::I32) => FlatType::I32,
            _ => FlatType::I64,
        };
    }
}

#[derive(Default)]
struct TypeInformationCache {
    records: PrimaryMap<TypeRecordIndex, TypeInformation>,
    variants: PrimaryMap<TypeVariantIndex, TypeInformation>,
    tuples: PrimaryMap<TypeTupleIndex, TypeInformation>,
    enums: PrimaryMap<TypeEnumIndex, TypeInformation>,
    flags: PrimaryMap<TypeFlagsIndex, TypeInformation>,
    options: PrimaryMap<TypeOptionIndex, TypeInformation>,
    results: PrimaryMap<TypeResultIndex, TypeInformation>,
    lists: PrimaryMap<TypeListIndex, TypeInformation>,
}

struct TypeInformation {
    depth: u32,
    flat: FlatTypesStorage,
    has_borrow: bool,
}

impl TypeInformation {
    const fn new() -> TypeInformation {
        TypeInformation {
            depth: 0,
            flat: FlatTypesStorage::new(),
            has_borrow: false,
        }
    }

    const fn primitive(flat: FlatType) -> TypeInformation {
        let mut info = TypeInformation::new();
        info.depth = 1;
        info.flat.memory32[0] = flat;
        info.flat.len = 1;
        info
    }

    const fn string() -> TypeInformation {
        let mut info = TypeInformation::new();
        info.depth = 1;
        info.flat.memory32[0] = FlatType::I32;
        info.flat.memory32[1] = FlatType::I32;
        info.flat.len = 2;
        info
    }

    /// Builds up all flat types internally using the specified representation
    /// for all of the component fields of the record.
    fn build_record<'a>(&mut self, types: impl Iterator<Item = &'a TypeInformation>) {
        self.depth = 1;
        for info in types {
            self.depth = self.depth.max(1 + info.depth);
            self.has_borrow = self.has_borrow || info.has_borrow;
            match info.flat.as_flat_types() {
                Some(types) => {
                    for t32 in types.memory32.iter() {
                        if !self.flat.push(*t32) {
                            break;
                        }
                    }
                }
                None => {
                    self.flat.len = u8::try_from(MAX_FLAT_TYPES + 1).unwrap();
                }
            }
        }
    }

    /// Builds up the flat types used to represent a `variant` which notably
    /// handles "join"ing types together so each case is representable as a
    /// single flat list of types.
    ///
    /// The iterator item is:
    ///
    /// * `None` - no payload for this case
    /// * `Some(None)` - this case has a payload but can't be represented with flat types
    /// * `Some(Some(types))` - this case has a payload and is represented with the types specified
    ///   in the flat representation.
    fn build_variant<'a, I>(&mut self, cases: I)
    where
        I: IntoIterator<Item = Option<&'a TypeInformation>>,
    {
        let cases = cases.into_iter();
        self.flat.push(FlatType::I32);
        self.depth = 1;

        for info in cases {
            let info = match info {
                Some(info) => info,
                // If this case doesn't have a payload then it doesn't change
                // the depth/flat representation
                None => continue,
            };
            self.depth = self.depth.max(1 + info.depth);
            self.has_borrow = self.has_borrow || info.has_borrow;

            // If this variant is already unrepresentable in a flat
            // representation then this can be skipped.
            if usize::from(self.flat.len) > MAX_FLAT_TYPES {
                continue;
            }

            let types = match info.flat.as_flat_types() {
                Some(types) => types,
                // If this case isn't representable with a flat list of types
                // then this variant also isn't representable.
                None => {
                    self.flat.len = u8::try_from(MAX_FLAT_TYPES + 1).unwrap();
                    continue;
                }
            };
            // If the case used all of the flat types then the discriminant
            // added for this variant means that this variant is no longer
            // representable.
            if types.memory32.len() >= MAX_FLAT_TYPES {
                self.flat.len = u8::try_from(MAX_FLAT_TYPES + 1).unwrap();
                continue;
            }
            let dst = self.flat.memory32.iter_mut().skip(1);
            for (i, (t32, dst32)) in types.memory32.iter().zip(dst).enumerate() {
                if i + 1 < usize::from(self.flat.len) {
                    // If this index hs already been set by some previous case
                    // then the types are joined together.
                    dst32.join(*t32);
                } else {
                    // Otherwise if this is the first time that the
                    // representation has gotten this large then the destination
                    // is simply whatever the type is. The length is also
                    // increased here to indicate this.
                    self.flat.len += 1;
                    *dst32 = *t32;
                }
            }
        }
    }

    fn records(&mut self, types: &ComponentTypesBuilder, ty: &TypeRecord) {
        self.build_record(ty.fields.iter().map(|f| types.type_information(&f.ty)));
    }

    fn tuples(&mut self, types: &ComponentTypesBuilder, ty: &TypeTuple) {
        self.build_record(ty.types.iter().map(|t| types.type_information(t)));
    }

    fn enums(&mut self, _types: &ComponentTypesBuilder, _ty: &TypeEnum) {
        self.depth = 1;
        self.flat.push(FlatType::I32);
    }

    fn flags(&mut self, _types: &ComponentTypesBuilder, ty: &TypeFlags) {
        self.depth = 1;
        match FlagsSize::from_count(ty.names.len()) {
            FlagsSize::Size0 => {}
            FlagsSize::Size1 | FlagsSize::Size2 => {
                self.flat.push(FlatType::I32);
            }
            FlagsSize::Size4Plus(n) => {
                for _ in 0..n {
                    self.flat.push(FlatType::I32);
                }
            }
        }
    }

    fn variants(&mut self, types: &ComponentTypesBuilder, ty: &TypeVariant) {
        self.build_variant(
            ty.cases.iter().map(|c| c.ty.as_ref().map(|ty| types.type_information(ty))),
        )
    }

    fn results(&mut self, types: &ComponentTypesBuilder, ty: &TypeResult) {
        self.build_variant([
            ty.ok.as_ref().map(|ty| types.type_information(ty)),
            ty.err.as_ref().map(|ty| types.type_information(ty)),
        ])
    }

    fn options(&mut self, types: &ComponentTypesBuilder, ty: &TypeOption) {
        self.build_variant([None, Some(types.type_information(&ty.ty))]);
    }

    fn lists(&mut self, types: &ComponentTypesBuilder, ty: &TypeList) {
        *self = TypeInformation::string();
        let info = types.type_information(&ty.element);
        self.depth += info.depth;
        self.has_borrow = info.has_borrow;
    }
}

pub fn interface_type_to_ir(
    ty: &InterfaceType,
    component_types: &ComponentTypes,
) -> midenc_hir::Type {
    match ty {
        InterfaceType::Bool => midenc_hir::Type::I1,
        InterfaceType::S8 => midenc_hir::Type::I8,
        InterfaceType::U8 => midenc_hir::Type::U8,
        InterfaceType::S16 => midenc_hir::Type::I16,
        InterfaceType::U16 => midenc_hir::Type::U16,
        InterfaceType::S32 => midenc_hir::Type::I32,
        InterfaceType::U32 => midenc_hir::Type::U32,
        InterfaceType::S64 => midenc_hir::Type::I64,
        InterfaceType::U64 => midenc_hir::Type::U64,
        InterfaceType::Float32 => midenc_hir::Type::Felt,
        InterfaceType::Float64 => todo!(),
        InterfaceType::Char => todo!(),
        InterfaceType::String => todo!(),
        InterfaceType::ErrorContext => todo!("the async proposal is not currently supported"),
        InterfaceType::Record(idx) => {
            let tys = component_types.records[*idx]
                .fields
                .iter()
                .map(|f| interface_type_to_ir(&f.ty, component_types));
            midenc_hir::Type::from(midenc_hir::StructType::new(tys))
        }
        InterfaceType::List(idx) => {
            let element_ty =
                interface_type_to_ir(&component_types.lists[*idx].element, component_types);
            midenc_hir::Type::List(Arc::new(element_ty))
        }
        InterfaceType::Tuple(tuple_idx) => {
            let tys = component_types.tuples[*tuple_idx]
                .types
                .iter()
                .map(|t| interface_type_to_ir(t, component_types));
            midenc_hir::Type::from(midenc_hir::StructType::new(tys))
        }
        InterfaceType::Flags(_) => midenc_hir::Type::I32,

        // XXX: THESE ARE ALL DISABLED UNTIL WE HAVE `midenc_hir::Type::Enum` sorted out.
        //
        // Enum, Option and Result are all specialisations of Variant.
        // InterfaceType::Enum(enum_idx) => {
        //     let enum_info = &component_types.enums[*enum_idx];
        //
        //     midenc_hir::Type::from(midenc_hir::EnumType::new(
        //         std::iter::repeat_n(None, enum_info.names.len()),
        //         enum_info.info.size.into(),
        //         enum_info.info.payload_offset32,
        //     ))
        // }
        // InterfaceType::Option(option_idx) => {
        //     let option_info = &component_types.options[*option_idx];
        //
        //     let some_ty = interface_type_to_ir(&option_info.ty, component_types);
        //
        //     midenc_hir::Type::from(midenc_hir::EnumType::new(
        //         [None, Some(some_ty)],
        //         option_info.info.size.into(),
        //         option_info.info.payload_offset32,
        //     ))
        // }
        // InterfaceType::Result(result_idx) => {
        //     let result_info = &component_types.results[*result_idx];
        //
        //     let variant_tys = [result_info.ok, result_info.err]
        //         .map(|opt_ty| opt_ty.as_ref().map(|ty| interface_type_to_ir(ty, component_types)));
        //
        //     midenc_hir::Type::from(midenc_hir::EnumType::new(
        //         variant_tys,
        //         result_info.info.size.into(),
        //         result_info.info.payload_offset32,
        //     ))
        // }
        // InterfaceType::Variant(variant_idx) => {
        //     let variant_info = &component_types.variants[*variant_idx];
        //
        //     let variant_tys = variant_info
        //         .cases
        //         .iter()
        //         .map(|case| case.ty.as_ref().map(|ty| interface_type_to_ir(ty, component_types)));
        //
        //     midenc_hir::Type::from(midenc_hir::EnumType::new(
        //         variant_tys,
        //         variant_info.info.size.into(),
        //         variant_info.info.payload_offset32,
        //     ))
        // }
        InterfaceType::Enum(_enum_idx) => todo!("MIDENC_HIR::TYPE::ENUM TBD"),
        InterfaceType::Option(_option_idx) => todo!("MIDENC_HIR::TYPE::ENUM TBD"),
        InterfaceType::Result(_result_idx) => todo!("MIDENC_HIR::TYPE::ENUM TBD"),
        InterfaceType::Variant(_variant_idx) => todo!("MIDENC_HIR::TYPE::ENUM TBD"),

        InterfaceType::Own(_) => todo!(),
        InterfaceType::Borrow(_) => todo!(),
    }
}
