//! Miden types as Rust types.
//!
//! A Miden type in a signature or a type export becomes Rust type text: integers and `bool` as
//! the Rust primitives, `felt` and `word` as the support module's `Felt` and `Word`, pointers as
//! `ElementPtr` (element space) or `*mut` (byte space), and a struct or enum as the generated type
//! of the type export it is equal to — or, where no export the bindings can refer to is, as a type
//! the bindings declare themselves ([`LocalType`]).
//!
//! A type that has no Rust form ([`layout::check`]) is in neither: no export with it is generated
//! and none is declared, so whatever uses it has no Rust form either.

use std::sync::Arc;

use miden_assembly_syntax::ast::{
    Path, PathBuf,
    types::{AddressSpace, StructField, StructType, Type},
};
use midenc_package_interface::{PackageInterface, TypeItem, WasmScalar, abi};

use crate::{Error, External, Options, layout, names};

/// The support module's items. Every generated module imports them by name, except a name the
/// module defines itself; see [`TypeUniverse::defines`].
///
/// `FeltConstant` and `WordConstant` hold a felt or word constant as canonical `u64`s, since no
/// `const fn` constructs a `Felt` on the Miden target; `get()` makes the `Felt` or `Word`.
pub(crate) const SUPPORT_ITEMS: [&str; 6] =
    ["ElementPtr", "Felt", "FeltConstant", "Word", "WordAligned", "WordConstant"];

/// What a [`RustType`] is, for the backends that treat kinds differently: a struct is spread
/// into its fields, an enum converted from its discriminant, a pointer converted between address
/// spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RustTypeKind {
    /// `bool` or a Rust integer.
    Scalar,
    /// The support module's `Felt`.
    Felt,
    /// The support module's `Word`: a standalone `[felt; 4]`.
    Word,
    /// Any other fixed-size array.
    Array {
        /// The number of elements.
        len: usize,
    },
    /// A struct: the MASM path of the type export it resolved to or, for a [`LocalType`], the
    /// path of the module it is declared in followed by its name.
    Struct(String),
    /// An enum, with the path of what it resolved to as for [`Self::Struct`].
    Enum(String),
    /// A pointer, in either address space.
    Pointer {
        /// The Rust text of the pointee.
        pointee: String,
    },
}

/// A Miden type as Rust: the text to write, and what kind of type it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustType {
    /// The Rust type, as it is written at the place it was resolved for.
    pub(crate) text: String,
    /// What kind of type it is.
    pub(crate) kind: RustTypeKind,
}

/// A struct or enum the bindings declare themselves, because no type export they can refer to is
/// equal to it: an anonymous struct, or a type of another package that has no
/// [`Options::with`] entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalType {
    /// The MASM module it is declared in: the module of the item that uses it.
    pub(crate) module: PathBuf,
    /// Its Rust name.
    pub(crate) name: String,
    /// The type.
    pub(crate) ty: Type,
    /// Its doc line: what it is and where it is used.
    pub(crate) doc: String,
    /// The type export it is declared just before, because that export uses it; `None` for a
    /// type only a signature uses, which is declared after the module's type exports.
    pub(crate) before: Option<Arc<Path>>,
}

/// Every named type the generated bindings may refer to: the package's own type exports, those
/// of the [`Options::with`] packages, and the types the bindings declare themselves; each with a
/// Rust form.
pub(crate) struct TypeUniverse<'a> {
    /// The generated package's own type exports that have a Rust form, sorted by path.
    own: Vec<&'a TypeItem>,
    /// The type exports of the `with` packages that have a Rust form, each with where that
    /// package's bindings live, in the order the packages were given, each sorted by path.
    external: Vec<(&'a TypeItem, &'a External)>,
    /// The types the bindings declare themselves, in the order they are declared in: a struct
    /// after the anonymous structs of its fields.
    locals: Vec<LocalType>,
    /// The root the package's bindings are generated from.
    root: &'a str,
    /// The Rust path of the support module.
    support: &'a str,
}

/// The types whose declaration [`TypeUniverse::declare_local_types`] is in the middle of: the
/// module, name and type of each, outermost first.
type Declaring = Vec<(PathBuf, String, Type)>;

impl<'a> TypeUniverse<'a> {
    /// The types `package` may refer to, given `externals` and the `with` map of `options`.
    ///
    /// An external package with no `with` entry contributes nothing: there is no Rust path to
    /// refer to its types by. Nor does a type export with no Rust form, which produces no code.
    /// The universe holds no [`LocalType`] until [`Self::declare_local_types`].
    pub(crate) fn new(
        package: &'a PackageInterface,
        externals: &[&'a PackageInterface],
        options: &'a Options,
    ) -> Self {
        let bindable = |item: &&TypeItem| layout::check(&item.ty).is_ok();
        let external = externals
            .iter()
            .filter_map(|ext| Some((*ext, options.with.get(AsRef::<str>::as_ref(&ext.name))?)))
            .flat_map(|(ext, with)| ext.types.iter().filter(bindable).map(move |item| (item, with)))
            .collect();
        Self {
            own: package.types.iter().filter(bindable).collect(),
            external,
            locals: Vec::new(),
            root: &options.root,
            support: &options.support,
        }
    }

    /// Declare every struct and enum the bindings of `package` use that no export resolves to:
    /// those reached through the fields of a type export, through the type of an alias, and
    /// through the signature of a bindable procedure (inside arrays and behind pointers too).
    ///
    /// Each is declared in the module of the item that uses it, once per module however often it
    /// is used there (types are compared by structure). A named type keeps its name. An anonymous
    /// struct is named after where it appears: `<Type><Field>` in a field of a type export or of
    /// another declared struct, `<Type>Inner` in an alias, `<Proc>Param<i>` and `<Proc>Result<i>`
    /// in a signature. A name the module already gives a type of another structure is
    /// [`Error::ConflictingType`].
    ///
    /// A type with no Rust form is not declared, and a type export with none declares nothing.
    pub(crate) fn declare_local_types(&mut self, package: &PackageInterface) -> Result<(), Error> {
        let mut declaring = Declaring::new();
        for item in &package.types {
            let (Some(name), Some(module)) = (item.path.last(), item.path.parent()) else {
                continue;
            };
            if layout::check(&item.ty).is_err() {
                continue;
            }
            let name = names::type_ident(name);
            let path = format!("`{}`", item.path);
            let before = Some(&item.path);
            match &item.ty {
                Type::Struct(st) => {
                    self.declare_fields(&st.get(), module, &name, &path, before, &mut declaring)?
                }
                Type::Enum(_) => {}
                ty => {
                    let hint = format!("{name}Inner");
                    self.declare(ty, module, &hint, &path, before, &mut declaring)?
                }
            }
        }
        for (procedure, _) in package.bindable() {
            let Some(signature) = &procedure.signature else {
                continue;
            };
            let name = names::type_ident(procedure.name());
            let module = procedure.namespace();
            let path = &procedure.path;
            for (i, ty) in signature.params().iter().enumerate() {
                let (hint, context) =
                    (format!("{name}Param{i}"), format!("parameter {i} of `{path}`"));
                self.declare(ty, module, &hint, &context, None, &mut declaring)?;
            }
            for (i, ty) in signature.results().iter().enumerate() {
                let (hint, context) =
                    (format!("{name}Result{i}"), format!("result {i} of `{path}`"));
                self.declare(ty, module, &hint, &context, None, &mut declaring)?;
            }
        }
        Ok(())
    }

    /// Declare what the fields of `st`, a struct named `owner` in `module`, need.
    fn declare_fields(
        &mut self,
        st: &StructType,
        module: &Path,
        owner: &str,
        owner_doc: &str,
        before: Option<&Arc<Path>>,
        declaring: &mut Declaring,
    ) -> Result<(), Error> {
        for (index, field) in st.fields().iter().enumerate() {
            let field_name = field_name(field, index);
            let hint = format!("{owner}{}", names::type_ident(&field_name));
            let context = format!("field `{field_name}` of {owner_doc}");
            self.declare(&field.ty, module, &hint, &context, before, declaring)?;
        }
        Ok(())
    }

    /// Declare `ty` in `module` if it is a struct or enum nothing resolves to, as `hint` if it is
    /// anonymous; `context` says where it is used, for its doc line.
    fn declare(
        &mut self,
        ty: &Type,
        module: &Path,
        hint: &str,
        context: &str,
        before: Option<&Arc<Path>>,
        declaring: &mut Declaring,
    ) -> Result<(), Error> {
        let (name, doc) = match ty {
            Type::Array(array) => {
                return self.declare(&array.ty, module, hint, context, before, declaring);
            }
            Type::Ptr(ptr) => {
                return self.declare(ptr.pointee(), module, hint, context, before, declaring);
            }
            Type::Struct(st) => match st.name() {
                Some(name) => (names::type_ident(&name), not_exported("struct", &name)),
                None => {
                    (hint.to_string(), format!("An anonymous struct, in the type of {context}."))
                }
            },
            Type::Enum(en) => (names::type_ident(&en.name()), not_exported("enum", &en.name())),
            _ => return Ok(()),
        };
        if layout::check(ty).is_err() {
            return Ok(());
        }
        let resolved = self.find_export(ty, module)?.is_some()
            || self.local_in(module, ty).is_some()
            || declaring.iter().any(|(m, _, t)| same_module(m, module) && t == ty);
        if resolved {
            return Ok(());
        }
        let taken = self.exports_in(module).any(|item| export_name(item) == name)
            || self.locals.iter().any(|l| same_module(&l.module, module) && l.name == name)
            || declaring.iter().any(|(m, n, _)| same_module(m, module) && *n == name);
        if taken {
            return Err(Error::ConflictingType {
                module: module.to_string(),
                name,
            });
        }
        // A field may refer back to this type (behind a pointer): mark it as being declared, so
        // that reference finds it rather than declaring it again.
        declaring.push((module.to_path_buf(), name.clone(), ty.clone()));
        if let Type::Struct(st) = ty {
            self.declare_fields(&st.get(), module, &name, &format!("`{name}`"), before, declaring)?;
        }
        declaring.pop();
        self.locals.push(LocalType {
            module: module.to_path_buf(),
            name,
            ty: ty.clone(),
            doc,
            before: before.cloned(),
        });
        Ok(())
    }

    /// The types the bindings declare themselves, in declaration order.
    pub(crate) fn local_types(&self) -> &[LocalType] {
        &self.locals
    }

    /// Whether the generated module for the MASM module `at` defines a type named `name`: a type
    /// export of `at`, or a [`LocalType`] declared there.
    ///
    /// Such a module does not import the support item of that name (the two would clash), and
    /// refers to it by its full path instead.
    pub(crate) fn defines(&self, at: &Path, name: &str) -> bool {
        self.exports_in(at).any(|item| export_name(item) == name)
            || self.locals.iter().any(|l| same_module(&l.module, at) && l.name == name)
    }

    /// `ty` as Rust, written for use inside the generated module for the MASM module `at`.
    ///
    /// The support items are written by name, as every generated module imports them, except in a
    /// module that [`defines`](Self::defines) a type of the same name, which writes them by their
    /// full path.
    ///
    /// A struct or enum is the generated type of the type export it is equal to (same name, same
    /// fields or variants), chosen in this order: an export of the package's own, in `at` itself,
    /// then in the nearest ancestor of `at`, then anywhere in the package (first by path); then
    /// an export of a `with` package; then a [`LocalType`], in the same order of modules (first
    /// declared among equals). A type of the package's own is referred to relative to `at`, so
    /// the generated text is correct wherever it is mounted; a `with` package's type by the
    /// absolute path its [`External`] gives. A struct or enum with no Rust form, or one none of
    /// these matches, is [`Error::Unsupported`].
    pub(crate) fn rust_type(&self, ty: &Type, at: &Path) -> Result<RustType, Error> {
        self.render(ty, at, true)
    }

    /// `ty` as the type of a field of a generated struct in `at`: as [`Self::rust_type`], except
    /// that unless `words` (the struct's [`layout::StructForm::words`]), a `[felt; 4]` is
    /// `[Felt; 4]` rather than `Word`, in an array too.
    pub(crate) fn field_type(&self, ty: &Type, at: &Path, words: bool) -> Result<RustType, Error> {
        self.render(ty, at, words)
    }

    /// `ty` as Rust in `at`, with `[felt; 4]` as `Word` only if `words`.
    fn render(&self, ty: &Type, at: &Path, words: bool) -> Result<RustType, Error> {
        let scalar = |text: &str| RustType {
            text: text.to_string(),
            kind: RustTypeKind::Scalar,
        };
        Ok(match ty {
            Type::I1 => scalar("bool"),
            Type::I8 => scalar("i8"),
            Type::U8 => scalar("u8"),
            Type::I16 => scalar("i16"),
            Type::U16 => scalar("u16"),
            Type::I32 => scalar("i32"),
            Type::U32 => scalar("u32"),
            Type::I64 => scalar("i64"),
            Type::U64 => scalar("u64"),
            // Only ever reached as a pointee or a field: the rule set lowers no 128-bit value
            // (`lower_signature` has no carrier for one). In memory the forms agree exactly —
            // sixteen little-endian bytes at a 16-byte alignment on both sides.
            Type::I128 => scalar("i128"),
            Type::U128 => scalar("u128"),
            Type::Felt => RustType {
                text: self.support_item("Felt", at),
                kind: RustTypeKind::Felt,
            },
            Type::Array(_) if words && abi::is_word(ty) => RustType {
                text: self.support_item("Word", at),
                kind: RustTypeKind::Word,
            },
            Type::Array(array) => RustType {
                text: format!("[{}; {}]", self.render(&array.ty, at, words)?.text, array.len),
                kind: RustTypeKind::Array { len: array.len },
            },
            Type::Ptr(ptr) => {
                // The pointee's layout is its own, whatever holds the pointer.
                let pointee = self.rust_type(ptr.pointee(), at)?.text;
                let text = match ptr.addrspace() {
                    AddressSpace::Element => {
                        format!("{}<{pointee}>", self.support_item("ElementPtr", at))
                    }
                    AddressSpace::Byte => format!("*mut {pointee}"),
                };
                RustType {
                    text,
                    kind: RustTypeKind::Pointer { pointee },
                }
            }
            Type::Struct(_) | Type::Enum(_) => self.resolve_named(ty, at)?,
            Type::Unknown
            | Type::Never
            | Type::Variadic
            | Type::U256
            | Type::F64
            | Type::List(_)
            | Type::Function(_) => {
                return Err(Error::Unsupported {
                    path: at.to_string(),
                    reason: format!("type `{ty}` has no Rust form in a binding"),
                });
            }
        })
    }

    /// The support item `name` as written in `at`: by name, or by its full path where `at`
    /// [`defines`](Self::defines) a type of that name.
    pub(crate) fn support_item(&self, name: &str, at: &Path) -> String {
        if self.defines(at, name) {
            format!("{}::{name}", self.support)
        } else {
            name.to_string()
        }
    }

    /// The generated type a struct or enum `ty` refers to; see [`Self::rust_type`].
    fn resolve_named(&self, ty: &Type, at: &Path) -> Result<RustType, Error> {
        let what = || match ty {
            Type::Struct(st) => match st.name() {
                Some(name) => format!("struct `{name}`"),
                None => "an anonymous struct".to_string(),
            },
            Type::Enum(en) => format!("enum `{}`", en.name()),
            _ => format!("`{ty}`"),
        };
        if let Err(reason) = layout::check(ty) {
            return Err(Error::Unsupported {
                path: at.to_string(),
                reason: format!("{} has no Rust form: {reason}", what()),
            });
        }
        if let Some(export) = self.find_export(ty, at)? {
            return Ok(export);
        }
        if let Some(local) = self.find_local(at, ty) {
            let text = if same_module(&local.module, at) {
                local.name.clone()
            } else {
                names::relative_item_path(self.root, at, &local.module, &local.name)?
            };
            return Ok(RustType {
                text,
                kind: kind_of(ty, format!("{}::{}", local.module, local.name)),
            });
        }
        Err(Error::Unsupported {
            path: at.to_string(),
            reason: format!("{} in a signature has no type export to refer to", what()),
        })
    }

    /// The type export a struct or enum `ty` resolves to from `at`, if any: the package's own
    /// first, nearest first, then the `with` packages'.
    fn find_export(&self, ty: &Type, at: &Path) -> Result<Option<RustType>, Error> {
        // Nearest first: `at` itself, then its ancestors from the nearest, then every other
        // module; `min_by_key` keeps the first of equals, so ties go to the first by path.
        let own =
            self.own.iter().filter(|item| item.ty == *ty).min_by_key(|item| {
                ancestor_distance(at, item.path.parent().unwrap_or(Path::EMPTY))
            });
        if let Some(item) = own {
            return Ok(Some(RustType {
                text: names::relative_type_path(self.root, at, &item.path)?,
                kind: kind_of(ty, item.path.to_string()),
            }));
        }
        if let Some((item, with)) = self.external.iter().find(|(item, _)| item.ty == *ty) {
            return Ok(Some(RustType {
                text: names::type_path(&with.root, &item.path, &with.rust_path)?,
                kind: kind_of(ty, item.path.to_string()),
            }));
        }
        Ok(None)
    }

    /// The [`LocalType`] equal to `ty` that `at` refers to, if any: the one declared in `at`
    /// itself, else in its nearest ancestor, else the first declared anywhere. A struct a type
    /// export uses is declared in the export's module only, and a wrapper elsewhere that rebuilds
    /// the export's fields names it there.
    fn find_local(&self, at: &Path, ty: &Type) -> Option<&LocalType> {
        self.locals
            .iter()
            .filter(|local| local.ty == *ty)
            .min_by_key(|local| ancestor_distance(at, &local.module))
    }

    /// The [`LocalType`] declared in `module` that is equal to `ty`, if any.
    fn local_in(&self, module: &Path, ty: &Type) -> Option<&LocalType> {
        self.locals
            .iter()
            .find(|local| same_module(&local.module, module) && local.ty == *ty)
    }

    /// The package's own type exports in the module `module`.
    fn exports_in<'s>(&'s self, module: &'s Path) -> impl Iterator<Item = &'a TypeItem> + 's {
        self.own.iter().copied().filter(move |item| {
            item.path.parent().is_some_and(|parent| same_module(parent, module))
        })
    }
}

/// The kind of the struct or enum `ty`, which resolved to `path`.
fn kind_of(ty: &Type, path: String) -> RustTypeKind {
    match ty {
        Type::Enum(_) => RustTypeKind::Enum(path),
        _ => RustTypeKind::Struct(path),
    }
}

/// The doc line of a [`LocalType`] that has a name of its own.
fn not_exported(kind: &str, name: &str) -> String {
    format!("`{kind} {name}`, which no type export the bindings can refer to matches.")
}

/// The Rust name of a type export.
pub(crate) fn export_name(item: &TypeItem) -> String {
    names::type_ident(item.path.last().unwrap_or_default())
}

/// The name of field `index` of a struct: its declared name, or `_<index>` if it has none.
pub(crate) fn field_name(field: &StructField, index: usize) -> String {
    match &field.name {
        Some(name) => name.to_string(),
        None => format!("_{index}"),
    }
}

/// Whether two module paths name the same module; a leading `::` on either is ignored.
pub(crate) fn same_module(a: &Path, b: &Path) -> bool {
    a.to_relative() == b.to_relative()
}

/// A sort key for how near `module` is to `at`: `(false, 0)` for `at` itself, `(false, n)` for
/// its `n`th ancestor, and `(true, 0)` for any other module, which sorts after both.
fn ancestor_distance(at: &Path, module: &Path) -> (bool, usize) {
    match at.to_relative().strip_prefix(module.to_relative()) {
        Some(rest) => (false, rest.len()),
        None => (true, 0),
    }
}

/// The Rust type an `extern "C"` declaration uses for `scalar`: one whose wasm32 C ABI is the
/// scalar's Wasm carrier.
///
/// `bool` and every integer of 32 bits or narrower are carried in an `i32`, 64-bit integers in an
/// `i64`, a felt in an `f32` (`Felt`, which callers qualify with the support path), and a pointer
/// as its element address, a `u32`.
pub(crate) fn carrier(scalar: &WasmScalar) -> &'static str {
    match scalar {
        WasmScalar::I1 => "bool",
        WasmScalar::I8 => "i8",
        WasmScalar::U8 => "u8",
        WasmScalar::I16 => "i16",
        WasmScalar::U16 => "u16",
        WasmScalar::I32 => "i32",
        WasmScalar::U32 => "u32",
        WasmScalar::I64 => "i64",
        WasmScalar::U64 => "u64",
        WasmScalar::Felt => "Felt",
        WasmScalar::Ptr(_) => "u32",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use miden_assembly_syntax::ast::types::{ArrayType, PointerType};
    use midenc_package_interface::testing::{FIXTURE_SOURCE, assemble_fixture};

    use super::*;

    const SUPPORT: &str = "crate::__support";

    /// A protocol package exporting `AccountId`. It exports a procedure too: the assembler
    /// rejects a library without one.
    const PROTOCOL_SOURCE: &str = r#"
pub type AccountId = struct { suffix: felt, prefix: felt }

pub proc id() -> AccountId
    nop
end
"#;

    /// A standards package with a signature that uses `AccountId`.
    const STANDARDS_SOURCE: &str = r#"
pub type AccountId = struct { suffix: felt, prefix: felt }

pub proc owner() -> AccountId
    nop
end
"#;

    /// The same signature with the struct written out: the assembler accepts an anonymous
    /// struct in a signature, and the struct it records has no name.
    const ANONYMOUS_SOURCE: &str = r#"
pub proc owner() -> struct { suffix: felt, prefix: felt }
    nop
end
"#;

    /// A c-like enum, declared with `enum` (the assembler's syntax), and a procedure using it.
    const ENUM_SOURCE: &str = r#"
pub enum Kind : u8 {
    PRIVATE = 0,
    PUBLIC = 1,
}

pub proc kind_of(x: felt) -> Kind
    nop
end
"#;

    fn options(root: &str) -> Options {
        Options {
            root: root.into(),
            support: SUPPORT.into(),
            with: BTreeMap::new(),
        }
    }

    /// `FIXTURE_SOURCE` assembled as package `fixture` with root module `::fixture`.
    fn fixture_universe() -> (PackageInterface, Options) {
        let package = assemble_fixture("fixture", "::fixture", FIXTURE_SOURCE);
        (PackageInterface::from_package(&package), options("::fixture"))
    }

    fn interface(name: &str, root: &str, source: &str) -> PackageInterface {
        PackageInterface::from_package(&assemble_fixture(name, root, source))
    }

    /// The type of the first result of the procedure at `path`.
    fn first_result(iface: &PackageInterface, path: &str) -> Type {
        let procedure = iface.procedure(Path::new(path)).expect("procedure exists");
        procedure.signature.as_ref().expect("typed").results()[0].clone()
    }

    #[test]
    fn scalars_words_and_arrays_map_to_sdk_types() {
        let (iface, options) = fixture_universe();
        let u = TypeUniverse::new(&iface, &[], &options);
        let at = Path::new("::fixture");
        assert_eq!(u.rust_type(&Type::Felt, at).unwrap().text, "Felt");
        assert_eq!(u.rust_type(&Type::U16, at).unwrap().text, "u16");
        assert_eq!(u.rust_type(&Type::I1, at).unwrap().text, "bool");
        let word = Type::from(ArrayType::new(Type::Felt, 4));
        assert_eq!(u.rust_type(&word, at).unwrap().text, "Word");
        let three = Type::from(ArrayType::new(Type::Felt, 3));
        assert_eq!(u.rust_type(&three, at).unwrap().text, "[Felt; 3]");
        let limbs = Type::from(ArrayType::new(Type::U32, 8));
        assert_eq!(u.rust_type(&limbs, at).unwrap().text, "[u32; 8]");

        let kinds = [
            (Type::I8, "i8", RustTypeKind::Scalar),
            (Type::U8, "u8", RustTypeKind::Scalar),
            (Type::I16, "i16", RustTypeKind::Scalar),
            (Type::I32, "i32", RustTypeKind::Scalar),
            (Type::U32, "u32", RustTypeKind::Scalar),
            (Type::I64, "i64", RustTypeKind::Scalar),
            (Type::U64, "u64", RustTypeKind::Scalar),
            (Type::Felt, "Felt", RustTypeKind::Felt),
            (word, "Word", RustTypeKind::Word),
            (limbs, "[u32; 8]", RustTypeKind::Array { len: 8 }),
        ];
        for (ty, text, kind) in kinds {
            assert_eq!(
                u.rust_type(&ty, at).unwrap(),
                RustType {
                    text: text.into(),
                    kind
                }
            );
        }
    }

    #[test]
    fn element_pointers_are_element_ptr_and_byte_pointers_are_raw_pointers() {
        let (iface, options) = fixture_universe();
        let u = TypeUniverse::new(&iface, &[], &options);
        let ptr = Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U32,
            AddressSpace::Element,
        )));
        let ty = u.rust_type(&ptr, Path::new("::fixture")).unwrap();
        assert_eq!(ty.text, "ElementPtr<u32>");
        assert!(matches!(ty.kind, RustTypeKind::Pointer { ref pointee } if pointee == "u32"));

        // A byte-space pointer needs no conversion: it is a Rust raw pointer as it is.
        let word = Type::from(ArrayType::new(Type::Felt, 4));
        let byte_ptr = Type::Ptr(Arc::new(PointerType::new(word)));
        let ty = u.rust_type(&byte_ptr, Path::new("::fixture")).unwrap();
        assert_eq!(ty.text, "*mut Word");
        assert_eq!(
            ty.kind,
            RustTypeKind::Pointer {
                pointee: "Word".into()
            }
        );
    }

    /// Deviation from the plan's text: the plan expected `crate::raw::fixture::Pair`, an absolute
    /// path the generator cannot know (`Options` does not say where the text is mounted, and a
    /// user project mounts it at a module of its own choosing). A type of the package's own is
    /// referred to relative to the module that names it, as `Pair` from `::fixture` and
    /// `super::Pair` from a module below it.
    #[test]
    fn a_named_struct_resolves_to_its_type_export() {
        // `Pair` is exported by the fixture at `::fixture::Pair`, and `make_pair`'s result is
        // that struct.
        let (iface, options) = fixture_universe();
        let u = TypeUniverse::new(&iface, &[], &options);
        let make_pair = iface.procedure(Path::new("::fixture::make_pair")).unwrap();
        let result = &make_pair.signature.as_ref().unwrap().results()[0];
        let ty = u.rust_type(result, Path::new("::fixture")).unwrap();
        assert_eq!(ty.text, "Pair");
        assert!(matches!(ty.kind, RustTypeKind::Struct(ref path) if path == "::fixture::Pair"));

        let below = u.rust_type(result, Path::new("::fixture::nested")).unwrap();
        assert_eq!(below.text, "super::Pair");

        // A pointer to it names it the same way.
        let ptr = Type::Ptr(Arc::new(PointerType::new_with_address_space(
            result.clone(),
            AddressSpace::Element,
        )));
        let ty = u.rust_type(&ptr, Path::new("::fixture")).unwrap();
        assert_eq!(ty.text, "ElementPtr<Pair>");

        assert!(matches!(
            u.rust_type(result, Path::new("::elsewhere")),
            Err(Error::OutsideRoot { .. })
        ));
    }

    /// The real standards package uses the protocol's `AccountId` in its signatures without
    /// exporting it: it imports the type from the protocol package. An in-test fixture cannot
    /// import from another package, and a signature may only use a *public* type (the assembler
    /// rejects a private one), so the standards fixture exports its own `AccountId`, and the
    /// standards interface is then edited to drop that export, which leaves exactly the real
    /// shape: a signature naming a struct the package itself does not export.
    #[test]
    fn a_struct_from_an_external_package_resolves_through_the_with_map() {
        let proto = interface("miden-protocol", "::miden::protocol::types", PROTOCOL_SOURCE);
        let mut std = interface("miden-standards", "::miden::standards::access", STANDARDS_SOURCE);
        let account_id = first_result(&std, "::miden::standards::access::owner");
        let at = Path::new("::miden::standards::access");
        let mut options = options("::miden::standards");
        options.with.insert(
            "miden-protocol".into(),
            External {
                root: "::miden::protocol".into(),
                rust_path: "crate::raw::protocol".into(),
            },
        );

        // The standards package exports a type equal to it: its own export comes first.
        let u = TypeUniverse::new(&std, &[&proto], &options);
        let ty = u.rust_type(&account_id, at).unwrap();
        assert_eq!(ty.text, "AccountId");
        assert_eq!(ty.kind, RustTypeKind::Struct("::miden::standards::access::AccountId".into()));

        // Without that export, the protocol's, through the `with` map.
        std.types.clear();
        let u = TypeUniverse::new(&std, &[&proto], &options);
        let ty = u.rust_type(&account_id, at).unwrap();
        assert_eq!(ty.text, "crate::raw::protocol::types::AccountId");
        assert_eq!(ty.kind, RustTypeKind::Struct("::miden::protocol::types::AccountId".into()));

        // An external package with no `with` entry has no Rust path: nothing resolves.
        let no_with = self::options("::miden::standards");
        let u = TypeUniverse::new(&std, &[&proto], &no_with);
        assert!(matches!(u.rust_type(&account_id, at), Err(Error::Unsupported { .. })));

        // Nor does it once declared, as long as no `with` package exports it: the bindings then
        // declare it themselves, under its own name, in the module that uses it.
        let mut u = TypeUniverse::new(&std, &[&proto], &no_with);
        u.declare_local_types(&std).unwrap();
        let ty = u.rust_type(&account_id, at).unwrap();
        assert_eq!(ty.text, "AccountId");
        assert_eq!(ty.kind, RustTypeKind::Struct("::miden::standards::access::AccountId".into()));
        let [local] = u.local_types() else {
            panic!("one declared type, got {:?}", u.local_types());
        };
        assert_eq!(local.name, "AccountId");
        assert_eq!(local.before, None, "only a signature uses it");
        // With the `with` entry, there is nothing to declare.
        let mut u = TypeUniverse::new(&std, &[&proto], &options);
        u.declare_local_types(&std).unwrap();
        assert!(u.local_types().is_empty());

        // An anonymous struct in a signature assembles, but it has no name, so it is not equal
        // to the protocol's `AccountId` however its fields match: it resolves to nothing until
        // the bindings declare it, named after the procedure and the result it is.
        let anonymous =
            interface("miden-standards", "::miden::standards::access", ANONYMOUS_SOURCE);
        let unnamed = first_result(&anonymous, "::miden::standards::access::owner");
        let mut u = TypeUniverse::new(&anonymous, &[&proto], &options);
        let err = u.rust_type(&unnamed, at).unwrap_err();
        assert_eq!(
            err.to_string(),
            "`::miden::standards::access`: an anonymous struct in a signature has no type export \
             to refer to"
        );
        u.declare_local_types(&anonymous).unwrap();
        assert_eq!(u.rust_type(&unnamed, at).unwrap().text, "OwnerResult0");
    }

    #[test]
    fn the_nearest_own_export_wins() {
        // Three exports of the same struct (`Pair`, by name and fields) in different modules.
        let (mut iface, options) = fixture_universe();
        let pair = iface.types[0].ty.clone();
        let export = |path: &str| TypeItem {
            path: Path::new(path).into(),
            ty: pair.clone(),
        };
        iface.types.push(export("::fixture::inner::Twin"));
        iface.types.push(export("::fixture::other::Copy"));
        iface.types.sort_by(|a, b| a.path.cmp(&b.path));
        let u = TypeUniverse::new(&iface, &[], &options);
        let resolve = |at: &str| u.rust_type(&pair, Path::new(at)).unwrap().text;

        assert_eq!(resolve("::fixture"), "Pair", "an export in `at` itself");
        assert_eq!(resolve("::fixture::inner"), "Twin", "an export in `at` itself");
        assert_eq!(resolve("::fixture::inner::deep"), "super::Twin", "the nearest ancestor");
        assert_eq!(resolve("::fixture::elsewhere"), "super::Pair", "an ancestor");

        // No export in `at` or an ancestor: the first by path, anywhere in the package.
        iface.types.retain(|item| item.path.to_string() != "::fixture::Pair");
        let u = TypeUniverse::new(&iface, &[], &options);
        let text = u.rust_type(&pair, Path::new("::fixture::elsewhere")).unwrap().text;
        assert_eq!(text, "super::inner::Twin");
        let text = u.rust_type(&pair, Path::new("::fixture")).unwrap().text;
        assert_eq!(text, "self::inner::Twin");
    }

    #[test]
    fn an_enum_resolves_to_its_type_export() {
        let iface = interface("fixture", "::fixture", ENUM_SOURCE);
        let options = options("::fixture");
        let u = TypeUniverse::new(&iface, &[], &options);
        let kind = first_result(&iface, "::fixture::kind_of");
        let ty = u.rust_type(&kind, Path::new("::fixture")).unwrap();
        assert_eq!(
            ty,
            RustType {
                text: "Kind".into(),
                kind: RustTypeKind::Enum("::fixture::Kind".into()),
            }
        );
    }

    #[test]
    fn types_with_no_rust_form_are_unsupported() {
        let (iface, options) = fixture_universe();
        let u = TypeUniverse::new(&iface, &[], &options);
        let at = Path::new("::fixture");
        for ty in [
            Type::U256,
            Type::F64,
            Type::Unknown,
            Type::Never,
            Type::Variadic,
            Type::List(Arc::new(Type::Felt)),
        ] {
            let err = u.rust_type(&ty, at).unwrap_err();
            assert_eq!(
                err,
                Error::Unsupported {
                    path: "::fixture".into(),
                    reason: format!("type `{ty}` has no Rust form in a binding"),
                }
            );
        }
        // Inside an array or behind a pointer, too.
        let wide = Type::from(ArrayType::new(Type::U256, 2));
        assert!(matches!(u.rust_type(&wide, at), Err(Error::Unsupported { .. })));
    }

    /// A 128-bit integer has a Rust form where it sits in memory — as a pointee or a field —
    /// since the two layouts agree (sixteen little-endian bytes, 16-aligned); by value it never
    /// reaches here, as the rule set lowers no 128-bit scalar.
    #[test]
    fn wide_integers_have_a_rust_form_in_memory() {
        let (iface, options) = fixture_universe();
        let u = TypeUniverse::new(&iface, &[], &options);
        let at = Path::new("::fixture");
        assert_eq!(u.rust_type(&Type::U128, at).unwrap().text, "u128");
        assert_eq!(u.rust_type(&Type::I128, at).unwrap().text, "i128");
        let ptr = Type::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::U128,
            AddressSpace::Element,
        )));
        assert!(u.rust_type(&ptr, at).unwrap().text.ends_with("ElementPtr<u128>"));
        let pair = Type::from(ArrayType::new(Type::U128, 2));
        assert_eq!(u.rust_type(&pair, at).unwrap().text, "[u128; 2]");
    }

    #[test]
    fn carriers_are_the_wasm32_c_abi_types_of_the_scalars() {
        let ptr = WasmScalar::Ptr(Arc::new(PointerType::new_with_address_space(
            Type::Felt,
            AddressSpace::Element,
        )));
        let carriers = [
            (WasmScalar::I1, "bool"),
            (WasmScalar::I8, "i8"),
            (WasmScalar::U8, "u8"),
            (WasmScalar::I16, "i16"),
            (WasmScalar::U16, "u16"),
            (WasmScalar::I32, "i32"),
            (WasmScalar::U32, "u32"),
            (WasmScalar::I64, "i64"),
            (WasmScalar::U64, "u64"),
            (WasmScalar::Felt, "Felt"),
            (ptr, "u32"),
        ];
        for (scalar, carrier_text) in carriers {
            assert_eq!(carrier(&scalar), carrier_text, "{scalar:?}");
        }
    }

    /// A module that exports a type named like a support item cannot import that item: it
    /// refers to it by its full path, and to its own type by name.
    #[test]
    fn a_module_defining_a_support_name_writes_that_support_item_in_full() {
        let source = "pub type Word = word\n\npub proc id(x: felt) -> felt\n    nop\nend\n";
        let iface = interface("fixture", "::fixture", source);
        let options = options("::fixture");
        let u = TypeUniverse::new(&iface, &[], &options);
        let at = Path::new("::fixture");
        assert!(u.defines(at, "Word"));
        assert!(!u.defines(at, "Felt"));
        let word = Type::from(ArrayType::new(Type::Felt, 4));
        assert_eq!(u.rust_type(&word, at).unwrap().text, "crate::__support::Word");
        assert_eq!(u.rust_type(&Type::Felt, at).unwrap().text, "Felt");
        // Elsewhere, the module does not define it.
        assert_eq!(u.rust_type(&word, Path::new("::fixture::other")).unwrap().text, "Word");
    }

    /// Anonymous structs in fields, aliases and signatures, inside arrays and behind pointers.
    const ANONYMOUS_STRUCTS: &str = r#"
pub type Outer = struct { inner: struct { x: felt, deep: struct { y: u8 } }, tag: u8 }
pub type Pairs = [struct { x: felt }; 2]

pub proc owner() -> struct { suffix: felt, prefix: felt }
    nop
end

pub proc set_owner(
    owner: struct { suffix: felt, prefix: felt },
    at: ptr<struct { z: u32 }, addrspace(felt)>
)
    nop
end
"#;

    #[test]
    fn types_no_export_matches_are_declared_where_they_are_used() {
        let iface = interface("fixture", "::fixture", ANONYMOUS_STRUCTS);
        let options = options("::fixture");
        let mut u = TypeUniverse::new(&iface, &[], &options);
        u.declare_local_types(&iface).unwrap();
        let declared: Vec<(&str, Option<String>, &str)> = u
            .local_types()
            .iter()
            .map(|l| (l.name.as_str(), l.before.as_ref().map(|p| p.to_string()), l.doc.as_str()))
            .collect();
        let outer = Some("::fixture::Outer".to_string());
        let pairs = Some("::fixture::Pairs".to_string());
        assert_eq!(
            declared,
            [
                (
                    "OuterInnerDeep",
                    outer.clone(),
                    "An anonymous struct, in the type of field `deep` of `OuterInner`."
                ),
                (
                    "OuterInner",
                    outer,
                    "An anonymous struct, in the type of field `inner` of `::fixture::Outer`."
                ),
                ("PairsInner", pairs, "An anonymous struct, in the type of `::fixture::Pairs`."),
                (
                    "OwnerResult0",
                    None,
                    "An anonymous struct, in the type of result 0 of `::fixture::owner`."
                ),
                (
                    "SetOwnerParam1",
                    None,
                    "An anonymous struct, in the type of parameter 1 of `::fixture::set_owner`."
                ),
            ],
            "a struct after the structs of its fields; one struct used twice is declared once"
        );
        assert!(u.local_types().iter().all(|l| l.module.to_string() == "::fixture"));

        let at = Path::new("::fixture");
        let set_owner = iface.procedure(Path::new("::fixture::set_owner")).unwrap();
        let params = set_owner.signature.as_ref().unwrap().params();
        assert_eq!(u.rust_type(&params[0], at).unwrap().text, "OwnerResult0");
        let at_ptr = u.rust_type(&params[1], at).unwrap();
        assert_eq!(at_ptr.text, "ElementPtr<SetOwnerParam1>");
        assert_eq!(
            u.rust_type(&u.local_types()[0].ty, at).unwrap().kind,
            RustTypeKind::Struct("::fixture::OuterInnerDeep".into())
        );
    }

    #[test]
    fn a_declared_name_another_type_of_the_module_has_is_a_conflict() {
        let source = r#"
pub type OwnerResult0 = u8

pub proc owner() -> struct { suffix: felt, prefix: felt }
    nop
end
"#;
        let iface = interface("fixture", "::fixture", source);
        let options = options("::fixture");
        let mut u = TypeUniverse::new(&iface, &[], &options);
        assert_eq!(
            u.declare_local_types(&iface),
            Err(Error::ConflictingType {
                module: "::fixture".into(),
                name: "OwnerResult0".into(),
            })
        );
    }
}
