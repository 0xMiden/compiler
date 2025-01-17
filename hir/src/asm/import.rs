use core::{
    fmt::Write,
    hash::{Hash, Hasher},
    str::FromStr,
};

use anyhow::bail;

use crate::{
    diagnostics::{SourceSpan, Spanned},
    FunctionIdent, FxHashMap, FxHashSet, Ident, Symbol,
};

#[derive(Default, Debug, Clone)]
pub struct ModuleImportInfo {
    /// This maps original, fully-qualified module names to their corresponding import
    modules: FxHashMap<Ident, MasmImport>,
    /// This maps known aliases to their fully-qualified identifiers
    aliases: FxHashMap<Ident, Ident>,
    /// This maps short-form/aliased module names to the functions imported from that module
    functions: FxHashMap<Ident, FxHashSet<FunctionIdent>>,
}
impl ModuleImportInfo {
    /// Inserts a new import in the table
    pub fn insert(&mut self, import: MasmImport) {
        let name = Ident::new(import.name, import.span);
        assert!(self.modules.insert(name, import).is_none());
    }

    /// Adds an import of the given function to the import table
    ///
    /// NOTE: It is assumed that the caller is adding imports using fully-qualified names.
    pub fn add(&mut self, id: FunctionIdent) {
        use hashbrown::hash_map::Entry;

        let module_id = id.module;
        match self.modules.entry(module_id) {
            Entry::Vacant(entry) => {
                let alias = module_id_alias(module_id);
                let span = module_id.span();
                let alias_id = if self.aliases.contains_key(&alias) {
                    // The alias is already used by another module, we must
                    // produce a new, unique alias to avoid conflicts. We
                    // use the hash of the fully-qualified name for this
                    // purpose, hex-encoded
                    let mut hasher = rustc_hash::FxHasher::default();
                    alias.as_str().hash(&mut hasher);
                    let mut buf = String::with_capacity(16);
                    write!(&mut buf, "{:x}", hasher.finish()).expect("failed to write string");
                    let alias = Symbol::intern(buf.as_str());
                    let alias_id = Ident::new(alias, span);
                    assert_eq!(
                        self.aliases.insert(alias_id, module_id),
                        None,
                        "unexpected aliasing conflict"
                    );
                    alias_id
                } else {
                    Ident::new(alias, span)
                };
                entry.insert(MasmImport {
                    span,
                    name: module_id.as_symbol(),
                    alias: alias_id.name,
                });
                self.aliases.insert(alias_id, module_id);
                self.functions.entry(alias_id).or_default().insert(id);
            }
            Entry::Occupied(_) => {
                let module_id_alias = module_id_alias(module_id);
                let alias = self.aliases[&module_id_alias];
                let functions = self.functions.entry(alias).or_default();
                functions.insert(id);
            }
        }
    }

    /// Returns true if there are no imports recorded
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Given a fully-qualified module name, look up the corresponding import metadata
    pub fn get<Q>(&self, module: &Q) -> Option<&MasmImport>
    where
        Ident: core::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.modules.get(module)
    }

    /// Given a fully-qualified module name, get the aliased identifier
    pub fn alias<Q>(&self, module: &Q) -> Option<Ident>
    where
        Ident: core::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.modules.get(module).map(|i| Ident::new(i.alias, i.span))
    }

    /// Given an aliased module name, get the fully-qualified identifier
    pub fn unalias<Q>(&self, alias: &Q) -> Option<Ident>
    where
        Ident: core::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.aliases.get(alias).copied()
    }

    /// Returns true if `module` is an imported module
    pub fn is_import<Q>(&self, module: &Q) -> bool
    where
        Ident: core::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.modules.contains_key(module)
    }

    /// Given a module alias, get the set of functions imported from that module
    pub fn imported<Q>(&self, alias: &Q) -> Option<&FxHashSet<FunctionIdent>>
    where
        Ident: core::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.functions.get(alias)
    }

    /// Get an iterator over the [MasmImport] records in this table
    pub fn iter(&self) -> impl Iterator<Item = &MasmImport> {
        self.modules.values()
    }

    /// Get an iterator over the aliased module names in this table
    pub fn iter_module_names(&self) -> impl Iterator<Item = &Ident> {
        self.modules.keys()
    }
}

fn module_id_alias(module_id: Ident) -> Symbol {
    match module_id.as_str().rsplit_once("::") {
        None => module_id.as_symbol(),
        Some((_, alias)) => Symbol::intern(alias),
    }
}

/// This represents an import statement in Miden Assembly
#[derive(Debug, Copy, Clone, Spanned)]
pub struct MasmImport {
    /// The source span corresponding to this import statement, if applicable
    #[span]
    pub span: SourceSpan,
    /// The fully-qualified name of the imported module, e.g. `std::math::u64`
    pub name: Symbol,
    /// The name to which the imported module is aliased locally, e.g. `u64`
    /// is the alias for `use std::math::u64`, which is the default behavior.
    ///
    /// However, custom aliases are permitted, and we may use this to disambiguate
    /// imported modules, e.g. `use std::math::u64->my_u64` will result in the
    /// alias for this import being `my_u64`.
    pub alias: Symbol,
}
impl MasmImport {
    /// Returns true if this import has a custom alias, or if it uses the
    /// default aliasing behavior for imports
    pub fn is_aliased(&self) -> bool {
        !self.name.as_str().ends_with(self.alias.as_str())
    }

    /// Returns true if this import conflicts with `other`
    ///
    /// A conflict arises when the same name is used to reference two different
    /// imports locally within a module, i.e. the aliases conflict
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.alias == other.alias && self.name != other.name
    }
}
impl Eq for MasmImport {}
impl PartialEq for MasmImport {
    fn eq(&self, other: &Self) -> bool {
        // If the names are different, the imports can't be equivalent
        if self.name != other.name {
            return false;
        }
        // Otherwise, equivalence depends on the aliasing of the import
        match (self.is_aliased(), other.is_aliased()) {
            (true, true) => {
                // Two imports that are custom aliased are equivalent only if
                // both the fully-qualified name and the alias are identical
                self.alias == other.alias
            }
            (true, false) | (false, true) => {
                // If one import is aliased and the other is not, the imports
                // are never equivalent, because they can't possibly refer to
                // the same module by the same name
                false
            }
            (false, false) => {
                // Two unaliased imports are the same if their names are the same
                true
            }
        }
    }
}
impl PartialOrd for MasmImport {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for MasmImport {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.name.cmp(&other.name).then_with(|| self.alias.cmp(&other.alias))
    }
}
impl Hash for MasmImport {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.alias.hash(state);
    }
}
impl TryFrom<Ident> for MasmImport {
    type Error = anyhow::Error;

    fn try_from(module: Ident) -> Result<Self, Self::Error> {
        let name = module.as_str();
        if name.contains(char::is_whitespace) {
            bail!("invalid module identifier '{name}': cannot contain whitespace",);
        }
        match name.rsplit_once("::") {
            None => {
                let name = module.as_symbol();
                Ok(Self {
                    span: module.span(),
                    name,
                    alias: name,
                })
            }
            Some((_, "")) => {
                bail!("invalid module identifier '{name}': trailing '::' is invalid");
            }
            Some((_, alias)) => {
                let name = module.as_symbol();
                let alias = Symbol::intern(alias);
                Ok(Self {
                    span: module.span(),
                    name,
                    alias,
                })
            }
        }
    }
}
impl FromStr for MasmImport {
    type Err = anyhow::Error;

    /// Parse an import statement as seen in Miden Assembly, e.g. `use std::math::u64->bigint`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let s = s.strip_prefix("use ").unwrap_or(s);
        match s.rsplit_once("->") {
            None => {
                let name = Ident::with_empty_span(Symbol::intern(s));
                name.try_into()
            }
            Some((_, "")) => {
                bail!("invalid import '{s}': alias cannot be empty")
            }
            Some((fqn, alias)) => {
                let name = Symbol::intern(fqn);
                let alias = Symbol::intern(alias);
                Ok(Self {
                    span: SourceSpan::UNKNOWN,
                    name,
                    alias,
                })
            }
        }
    }
}
