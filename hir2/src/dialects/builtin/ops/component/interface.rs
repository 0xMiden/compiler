use core::fmt;

use super::Component;
use crate::{
    dialects::builtin::{Function, Module},
    version::Version,
    FxHashMap, Signature, Symbol, SymbolName, SymbolTable, Type, Visibility,
};

/// The fully-qualfied identifier of a component
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentId {
    /// The namespace in which the component is defined
    pub namespace: SymbolName,
    /// The name of this component
    pub name: SymbolName,
    /// The semantic version number of this component
    pub version: Version,
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@{}", &self.namespace, &self.name, &self.version)
    }
}

impl ComponentId {
    /// Returns true if `self` and `other` are equal according to semantic versioning rules:
    ///
    /// * Namespace and name are identical
    /// * Version numbers are considered equal according to semantic versioning (i.e. if the version
    ///   strings differ only in build metadata, then they are considered equal).
    pub fn is_match(&self, other: &Self) -> bool {
        self.namespace == other.namespace
            && self.name == other.name
            && self.version.cmp_precedence(&other.version).is_eq()
    }

    /// Get the Miden Assembly [LibraryPath] that uniquely identifies this interface.
    pub fn to_library_path(&self) -> midenc_session::LibraryPath {
        use midenc_session::{LibraryNamespace, LibraryPath};

        let ns = format!("{}:{}", &self.namespace, &self.name);
        let namespace = LibraryNamespace::User(ns.into_boxed_str().into());
        // TODO(pauls): Need to add the version component to LibraryPath
        LibraryPath::new_from_components(namespace, [])
    }
}

#[derive(Debug)]
pub enum InvalidComponentIdError {
    MissingNamespace,
    MissingName,
}

impl core::str::FromStr for ComponentId {
    type Err = InvalidComponentIdError;

    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        todo!()
    }
}

impl From<&Component> for ComponentId {
    fn from(value: &Component) -> Self {
        let namespace = value.namespace().as_symbol();
        let name = value.name().as_symbol();
        let version = value.version().clone();

        Self {
            namespace,
            name,
            version,
        }
    }
}

/// A [ComponentInterface] is a description of the "skeleton" of a component, i.e.:
///
/// * Basic metadata about the component itself, e.g. name
/// * The set of interfaces it requires to be provided in order to instantiate the component
/// * The set of exported items provided by the interface, which can be used to fulfill imports
///   of other components.
///
/// This type is derived from a [Component] operation, but does not represent an operation itself,
/// instead, this is used by the compiler to reason about what components are available, what is
/// required, and whether or not all requirements can be met.
pub struct ComponentInterface {
    id: ComponentId,
    /// The visibility of this component in the interface (public or internal)
    visibility: Visibility,
    /// This flag is set to `true` if the interface is completely abstract (no definitions)
    is_externally_defined: bool,
    /// The set of imports required by this component.
    ///
    /// An import can be satisfied by any [Component] whose interface matches the one specified.
    /// In specific terms, this refers to the signatures/types of all symbols in the interface,
    /// rather than the names. The compiler will handle rebinding uses of symbols in the interface
    /// to the concrete symbols of the provided implementation - the important part is that the
    /// implementation is explicitly provided as an implementation of that interface, i.e. we do
    /// not try to simply find a match amongst all components.
    ///
    /// In the Wasm Component Model, such explicit instantiations are provided for us, so wiring
    /// up the component hierarchy derived from a Wasm component should be straightforward. It
    /// remains to be seen if there are non-Wasm sources where this is more problematic.
    imports: FxHashMap<ComponentId, ComponentInterface>,
    /// The set of items which form the interface of this component, and can be referenced from
    /// other components.
    ///
    /// All "exports" from a component interface are named, but can represent a variety of IR
    /// entities.
    exports: FxHashMap<SymbolName, ComponentExport>,
}

impl ComponentInterface {
    /// Derive a [ComponentInterface] from the given [Component]
    pub fn new(component: &Component) -> Self {
        let mut imports = FxHashMap::default();
        let mut exports = FxHashMap::default();
        let mut is_externally_defined = true;

        let id = ComponentId::from(component);

        let symbol_manager = component.symbol_manager();
        for symbol_ref in symbol_manager.symbols().symbols() {
            let symbol = symbol_ref.borrow();
            let symbol_op = symbol.as_symbol_operation();
            let name = symbol.name();
            if let Some(module) = symbol_op.downcast_ref::<Module>() {
                let interface = ModuleInterface::new(module);
                let visibility = interface.visibility;
                let is_abstract = interface.is_abstract;
                let item = ComponentExport::Module(interface);
                // Modules at the top level of a component are always exports, however we care about
                // whether the module is abstract or not. Abstract module interfaces are only
                // permitted in abstract component interfaces, otherwise all modules in the component
                // must be definitions. We assert that this is the case, in order to catch any
                // instances where the compiler produces invalid component IR.
                if is_abstract {
                    // This represents an abstract module interface provided by this component
                    assert!(
                        is_externally_defined,
                        "invalid component: abstract module '{name}' is not permitted in a \
                         non-abstract component"
                    );
                    assert!(visibility.is_public(), "abstract modules must have public visibility");
                    exports.insert(name, item);
                } else {
                    // This represents a concrete module definition
                    assert!(
                        !is_externally_defined || exports.is_empty(),
                        "invalid component: concrete module '{name}' is not permitted in an \
                         abstract component interface"
                    );
                    // We only export public or internal modules
                    if !visibility.is_private() {
                        exports.insert(name, item);
                    }
                    is_externally_defined = false;
                }
            } else if let Some(child_component) = symbol_op.downcast_ref::<Component>() {
                let interface = ComponentInterface::new(child_component);
                let visibility = interface.visibility;
                if interface.is_externally_defined {
                    // This is an import of an externally-defined component
                    let import_id = interface.id.clone();
                    imports.insert(import_id, interface);
                } else {
                    if !visibility.is_private() {
                        // This is an exported component definition (either internally or globally)
                        exports.insert(name, ComponentExport::Component(interface));
                    }
                    is_externally_defined = false;
                }
            } else {
                // If this happens we should assert - something is definitely wrong
                unimplemented!(
                    "unsupported symbol type `{}` in component: '{}'",
                    symbol_op.name(),
                    symbol.name()
                );
            }
        }

        Self {
            id,
            is_externally_defined,
            visibility: *component.visibility(),
            imports,
            exports,
        }
    }

    pub fn id(&self) -> &ComponentId {
        &self.id
    }

    /// Returns true if this interface describes a component for which we do not have a definition.
    pub fn is_externally_defined(&self) -> bool {
        self.is_externally_defined
    }

    /// Returns the visibility of this component in its current context.
    ///
    /// This is primarily used to determine whether or not this component is exportable from its
    /// parent component, and whether or not symbols defined within it are visible to siblings.
    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn exports(&self) -> &FxHashMap<SymbolName, ComponentExport> {
        &self.exports
    }

    /// Returns true if this component exports `name`
    ///
    /// The given symbol name is expected to be found at the top level of the component, i.e. this
    /// function does not attempt to resolve nested symbol references.
    pub fn exports_symbol(&self, name: SymbolName) -> bool {
        self.exports.contains_key(&name)
    }

    /// Returns true if this component provides the given [ModuleInterface].
    ///
    /// A component "provides" a module if it defines a module with the same name, and which
    /// contains all of the symbols of the given interface with matching types/signatures, i.e. it
    /// is a superset of the provided interface.
    ///
    /// NOTE: This does not return false if the component or module are externally-defined, it only
    /// answers the question of whether or not, if we had an instance of this component, would it
    /// satisfy the provided interface's requirements.
    pub fn provides_module(&self, interface: &ModuleInterface) -> bool {
        self.exports
            // Do we export a symbol with the given name
            .get(&interface.name)
            // The symbol must be a module
            .and_then(|export| match export {
                ComponentExport::Module(definition) => Some(definition),
                _ => None,
            })
            // The module must provide exports for all of `interface`'s imports
            .map(|definition| {
                interface.imports.iter().all(|(imported_symbol, import)| {
                    definition.exports.get(imported_symbol).is_some_and(|export| export == import)
                })
            })
            // If we didn't find the symbol, or it wasn't a module, return false
            .unwrap_or(false)
    }

    /// Returns true if this component provides the given [ComponentInterface].
    ///
    /// A component "provides" a component if either of the following are true:
    ///
    /// * The component itself has the same name as the given interface, and defines all of the
    ///   items imported by the interface, with matching types/signatures where appropriate.
    /// * The component exports a component that matches the given interface as described above.
    ///
    /// NOTE: This does not return false if the component or a matching child component are
    /// externally-defined, it only answers the question of whether or not, if we had an instance of
    /// the matching component, would it satisfy the provided interface's requirements.
    pub fn provides_component(&self, interface: &ComponentInterface) -> bool {
        if self.matches(interface) {
            return true;
        }

        self.exports
            // Do we export a symbol with the given name
            .get(&interface.id.name)
            // The symbol must be a component
            .and_then(|export| match export {
                ComponentExport::Component(definition) => Some(definition),
                _ => None,
            })
            // The component must provide exports for all of `interface`'s imports
            .map(|definition| definition.matches(interface))
            // If we didn't find the symbol, or it wasn't a module, return false
            .unwrap_or(false)
    }

    /// Returns true if `self` provides a superset of the imports required by `other`, or put
    /// another way - `self` matches the component import described by `other`.
    pub fn matches(&self, other: &Self) -> bool {
        if !self.id.is_match(&other.id) {
            return false;
        }

        other.imports.iter().all(|(imported_id, import)| {
            self.exports
                .get(&imported_id.name)
                .is_some_and(|export| export.matches_component(import))
        })
    }
}

pub enum ComponentExport {
    /// A nested component which has public visibility and is thus exported from its parent component
    Component(ComponentInterface),
    /// A module which has public visibility and is thus exported from its parent component
    Module(ModuleInterface),
}
impl ComponentExport {
    /// Returns true if this export describes a component that provides a superset of the imports
    /// required by `other`, i.e. `self` is a component which matches the component import described
    /// by `other`.
    pub fn matches_component(&self, other: &ComponentInterface) -> bool {
        let Self::Component(definition) = self else {
            return false;
        };

        definition.matches(other)
    }
}

pub struct ModuleInterface {
    name: SymbolName,
    /// The visibility of this module in the interface (public vs internal)
    visibility: Visibility,
    /// This flag is set to `true` if the interface is completely abstract (no definitions)
    is_abstract: bool,
    imports: FxHashMap<SymbolName, ModuleExport>,
    exports: FxHashMap<SymbolName, ModuleExport>,
}

impl ModuleInterface {
    /// Derive a [ModuleInterface] from the given [Module]
    pub fn new(module: &Module) -> Self {
        let mut imports = FxHashMap::default();
        let mut exports = FxHashMap::default();
        let mut is_abstract = true;

        let symbol_manager = module.symbol_manager();
        for symbol_ref in symbol_manager.symbols().symbols() {
            let symbol = symbol_ref.borrow();
            let name = symbol.name();
            if let Some(func) = symbol.as_symbol_operation().downcast_ref::<Function>() {
                let signature = func.signature().clone();
                let visibility = signature.visibility;
                let item = ModuleExport::Function { name, signature };
                if func.is_declaration() {
                    // This is an import of an externally-defined function
                    imports.insert(name, item);
                } else {
                    if !visibility.is_private() {
                        // This is an exported function definition (either internally or globally)
                        exports.insert(name, item);
                    }
                    is_abstract = false;
                }
            }

            // TODO: GlobalVariable
        }

        Self {
            name: module.name().as_symbol(),
            visibility: *module.visibility(),
            is_abstract,
            imports,
            exports,
        }
    }

    pub fn name(&self) -> SymbolName {
        self.name
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn is_externally_defined(&self) -> bool {
        self.is_abstract
    }

    pub fn imports(&self) -> &FxHashMap<SymbolName, ModuleExport> {
        &self.imports
    }

    pub fn exports(&self) -> &FxHashMap<SymbolName, ModuleExport> {
        &self.exports
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ModuleExport {
    /// A global symbol with the given type (if known/specified)
    ///
    /// NOTE: Global variables are _not_ exportable across component boundaries
    #[allow(unused)]
    Global { name: SymbolName, ty: Option<Type> },
    /// A function symbol with the given signature
    Function {
        name: SymbolName,
        signature: Signature,
    },
}
