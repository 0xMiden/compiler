//! The Rust backend's procedures.
//!
//! Every bindable procedure becomes one function in its module: a *wrapper* with the procedure's
//! declared types. On the Miden target (`all(target_family = "wasm", miden)`) the wrapper declares
//! the procedure's linker stub as an `extern "C"` function of Wasm carrier scalars, spreads its
//! arguments into them, and rebuilds the declared results from the stub's return value or from a
//! word-aligned return area, under the `midenc-package-interface` rule set the Wasm frontend
//! lowers the call with. On every other target its body is `unimplemented!`, so the bindings build
//! anywhere.
//!
//! An element-space pointer crosses the wrapper as an `ElementPtr`, an element address, and is not
//! converted, here or in the frontend: the wrapper passes `p.addr()` and wraps an address it gets
//! back with `ElementPtr::new`. Converting to and from a Rust byte address (`ElementPtr::from_ptr`
//! and `ElementPtr::to_ptr`, both checked) is left to the caller, where the data lives in Rust
//! memory: a byte address names only the first 2^30 elements, and an address a caller only hands
//! on to the next call needs no conversion at all.
//!
//! A result that is one value made only of felts (a `Word`, a struct of felts) is read back from
//! the return area whole, as that value, not felt by felt, as the hand-written bindings read a
//! `WordAligned<Word>`; it is the cheaper read, and it is also what keeps LLVM from carrying two
//! of the felts as one `i64` assembled with `extend`/`shl`/`or`, which traps on a felt outside the
//! `u32` range (it did so where two rebuilt `Word`s met at a branch). The backend splits such a
//! value only where it feeds a store directly, so tuple results and the felt fields of mixed
//! structs, which are still rebuilt felt by felt, remain exposed to that shape.
//!
//! Lines are broken where rustfmt, with the workspace's settings, breaks them (see [`Expr`] and
//! [`write_message`]), so the text is what rustfmt would make of it.

use std::collections::BTreeSet;

use miden_assembly_syntax::ast::{
    Path,
    types::{AddressSpace, EnumType, FunctionType, Type, TypeRepr},
};
use midenc_package_interface::{
    FieldStep, LoweredSignature, PackageInterface, ProcedureItem, ReturnStrategy, WasmScalar,
};

use crate::{
    Error, layout, names,
    render::{ARRAY_WIDTH, CALL_WIDTH, MAX_WIDTH, RESULT, Returns, Writer, width},
    types::{self, RustTypeKind, TypeUniverse},
};

/// rustfmt's `struct_lit_width` (18% of `max_width`): the widest body a struct literal keeps on
/// one line.
const STRUCT_LIT_WIDTH: usize = 18;

/// rustfmt's `short_array_element_width_threshold`: when every item of a list that does not fit
/// on one line is simple and at most this wide, rustfmt fills lines with them rather than writing
/// one per line.
const SHORT_ITEM_WIDTH: usize = 10;

/// The workspace's rustfmt `chain_width`: the widest method chain kept on one line.
const CHAIN_WIDTH: usize = 80;

/// The shortest piece rustfmt's `format_strings` breaks a string literal after.
const MIN_STRING_PIECE: usize = 10;

/// The `cfg` of the target the wrapper calls the procedure on.
const MIDEN: &str = r#"all(target_family = "wasm", miden)"#;

/// The name of the wrapper's return-area struct.
const RET: &str = "Ret";

/// The alignment of the return area, a `WordAligned`.
const WORD_ALIGNED: usize = 32;

/// Write the wrapper of the bindable procedure `item` of `package`, whose Wasm shape is
/// `lowered`.
///
/// A procedure a type of which has no Rust form (a pointer to a `u128`), whose results are all
/// zero-sized, whose module defines a type its return area would shadow, or whose flattened
/// parameters would share a name, is [`Error::Unsupported`]; nothing written so far is then of
/// use, and the caller discards it.
pub(crate) fn emit(
    w: &mut Writer,
    item: &ProcedureItem,
    lowered: &LoweredSignature,
    package: &PackageInterface,
    universe: &TypeUniverse<'_>,
) -> Result<(), Error> {
    let at = item.namespace();
    let signature = &lowered.miden;
    let unsupported = |reason: String| Error::Unsupported {
        path: item.path.to_string(),
        reason,
    };
    if lowered.ret == ReturnStrategy::Void && !signature.results().is_empty() {
        return Err(unsupported(
            "its results are zero-sized: there is no value for the wrapper to return".to_string(),
        ));
    }
    let area = lowered.return_area();
    if area.is_some() && universe.defines(at, RET) {
        return Err(unsupported(format!(
            "its module defines a type named `{RET}`, which the wrapper's return area would shadow"
        )));
    }
    let wrapper = Wrapper {
        item,
        universe,
        at,
        params: Param::all(signature.params()),
    };

    // The `extern "C"` declaration, the arguments the wrapper passes it, and the pointers among
    // them.
    let mut extern_names = Vec::with_capacity(lowered.params.len());
    let mut extern_params = Vec::with_capacity(lowered.params.len() + 1);
    let mut args = Vec::with_capacity(lowered.params.len() + 1);
    let mut pointers = Vec::new();
    for param in &lowered.params {
        let declared = &wrapper.params[param.origin];
        let name = declared.extern_name(&param.path);
        extern_params.push(format!("{name}: {}", wrapper.carrier(&param.scalar)));
        extern_names.push(((), name));
        args.push(wrapper.argument(declared, &param.path, &mut pointers)?);
    }
    // A field `a_b` and a field `b` of a field `a` would both be `<param>_a_b`.
    if let Some(((), (), name)) = names::duplicate(extern_names) {
        return Err(unsupported(format!(
            "two of its flattened parameters would both be named `{name}`"
        )));
    }
    if area.is_some() {
        extern_params.push(format!("ret: *mut {RET}"));
        args.push(Expr::complex(format!("ret.as_mut_ptr() as *mut {RET}")));
    }
    let extern_returns = match &lowered.ret {
        ReturnStrategy::Direct(scalar) => Returns::Type(wrapper.carrier(scalar)),
        ReturnStrategy::Void | ReturnStrategy::OutPointer(_) => Returns::Nothing,
    };
    let call = Expr::Call {
        callee: names::extern_ident(item.name()),
        args,
    };

    // The signature and its documentation.
    let is_unsafe = !pointers.is_empty();
    w.line(&format!("/// `{}`", render_signature(item.name(), signature)));
    w.line("///");
    w.line(&format!(
        "/// Calls `{}` (package `{}` {}).",
        item.path,
        AsRef::<str>::as_ref(&package.name),
        package.version
    ));
    if is_unsafe {
        w.line("///");
        w.line("/// # Safety");
        w.line("///");
        for pointer in &pointers {
            w.line(&format!("/// {}", pointer.requirement()));
        }
    }
    // The declared types, an element-space pointer's included: it is an `ElementPtr` in the
    // wrapper's signature as in a struct field.
    let declared_type = |ty: &Type| Ok::<_, Error>(universe.rust_type(ty, at)?.text);
    let params = wrapper
        .params
        .iter()
        .map(|param| Ok(format!("{}: {}", param.ident, declared_type(&param.ty)?)))
        .collect::<Result<Vec<_>, Error>>()?;
    let results = signature.results();
    let returns = match results {
        [] => Returns::Nothing,
        [result] => Returns::Type(declared_type(result)?),
        results => {
            Returns::Tuple(results.iter().map(declared_type).collect::<Result<Vec<_>, Error>>()?)
        }
    };
    let qualifier = if is_unsafe { "unsafe " } else { "" };
    let name = names::item_ident(item.name());
    // A wrapper is a few conversions around one call, and it is called from other crates (the
    // SDK's ergonomic layer, user code): without `#[inline]` it stays an out-of-line call there.
    w.line("#[inline]");
    w.signature(&format!("pub {qualifier}fn {name}"), &params, &returns, true);

    // The call, on the Miden target.
    w.line(&format!("#[cfg({MIDEN})]"));
    w.open_with("{");
    if let Some(area) = &area {
        w.line("#[repr(C)]");
        w.open(&format!("struct {RET}"));
        for (index, slot) in area.slots.iter().enumerate() {
            w.line(&format!("r{index}: {},", wrapper.slot_carrier(&slot.scalar)));
        }
        w.close();
        w.line(&format!(
            "const _: () = assert!(::core::mem::size_of::<{RET}>() == {});",
            area.size
        ));
    }
    w.open(r#"unsafe extern "C""#);
    w.line(r#"#[linkage = "extern_weak"]"#);
    w.line(&format!("#[link_name = \"{}\"]", escape(&names::link_name(&item.path))));
    let extern_name = format!("fn {}", names::extern_ident(item.name()));
    w.signature(&extern_name, &extern_params, &extern_returns, false);
    w.close();
    match &lowered.ret {
        ReturnStrategy::Void => write_unsafe_call(w, "", &call, ""),
        ReturnStrategy::Direct(_) => {
            if let [result] = results
                && is_plain(result)
            {
                write_unsafe_call(w, "", &call, "");
            } else {
                write_unsafe_call(w, "let ret = ", &call, ";");
                let slot = Slot {
                    expr: "ret".to_string(),
                    widened: false,
                };
                wrapper.rebuild(w, results, vec![slot])?;
            }
        }
        ReturnStrategy::OutPointer(_) => {
            let area = area.as_ref().expect("an out pointer has a return area");
            let aligned = universe.support_item("WordAligned", at);
            w.line(&format!(
                "let mut ret = ::core::mem::MaybeUninit::<{aligned}<{RET}>>::uninit();"
            ));
            write_unsafe_call(w, "", &call, ";");
            // Kept word-aligned: the slots are read through `Deref`, and a result read whole (see
            // `Wrapper::rebuild`) is read from an address aligned for it.
            w.line("let ret = unsafe { ret.assume_init() };");
            let slots = area
                .slots
                .iter()
                .enumerate()
                .map(|(index, _)| Slot {
                    expr: format!("ret.r{index}"),
                    widened: true,
                })
                .collect();
            wrapper.rebuild(w, results, slots)?;
        }
    }
    w.close_with("}");

    // Every other target.
    w.line(&format!("#[cfg(not({MIDEN}))]"));
    w.open_with("{");
    match wrapper.params.as_slice() {
        [] => {}
        [param] => w.line(&format!("let _ = {};", param.ident)),
        params => {
            let idents = params.iter().map(|param| Expr::simple(param.ident.clone())).collect();
            write_expr(w, "let _ = ", &Expr::Tuple(idents), ";");
        }
    }
    let message = format!("`{}` is only available when compiled for the Miden VM", item.path);
    write_message(w, "", "unimplemented", &format_literal(&message), "");
    w.close_with("}");
    w.close();
    Ok(())
}

/// The Miden signature of the procedure `name`, as `pub proc name(params) -> results`.
///
/// Each type is written as the manifest renders it (`FunctionType`'s `Display`), except that a
/// named struct or enum is written by its name rather than with its fields or variants.
fn render_signature(name: &str, signature: &FunctionType) -> String {
    let list = |types: &[Type]| types.iter().map(render_type).collect::<Vec<_>>().join(", ");
    let params = list(signature.params());
    match signature.results() {
        [] => format!("pub proc {name}({params})"),
        results => format!("pub proc {name}({params}) -> {}", list(results)),
    }
}

/// `ty` as the manifest renders it, a named struct or enum by its name.
fn render_type(ty: &Type) -> String {
    match ty {
        Type::Struct(st) => {
            if let Some(name) = st.name() {
                return name.to_string();
            }
            let st = st.get();
            let repr = match st.repr() {
                TypeRepr::Default => String::new(),
                repr => format!("#[repr({repr})] "),
            };
            let fields: Vec<String> = st
                .fields()
                .iter()
                .map(|field| match &field.name {
                    Some(name) => format!("{name} : {}", render_type(&field.ty)),
                    None => render_type(&field.ty),
                })
                .collect();
            format!("struct {repr}{{{}}}", fields.join(", "))
        }
        Type::Enum(en) => en.name().to_string(),
        Type::Ptr(ptr) => format!("ptr<{}, {}>", ptr.addrspace(), render_type(ptr.pointee())),
        Type::Array(array) => format!("[{}; {}]", render_type(&array.ty), array.len),
        Type::List(ty) => format!("list<{}>", render_type(ty)),
        ty => ty.to_string(),
    }
}

/// One declared parameter of a procedure, as the wrapper names it.
struct Param {
    /// The wrapper's parameter: a valid identifier.
    ident: String,
    /// What the names of the `extern "C"` parameters it spreads into are built from: the
    /// identifier before it is made a raw identifier or given a trailing `_`.
    base: String,
    /// The declared type.
    ty: Type,
}

impl Param {
    /// The wrapper's parameters for the declared parameters `params`.
    ///
    /// A struct- or enum-typed parameter is named after its type, in `snake_case` (`pair`,
    /// `note_type`), with `_2`, `_3` added to repeat a name; every other parameter, an anonymous
    /// struct's included, is `arg<index>`. No parameter is named `ret`, the wrapper's local for
    /// the results.
    fn all(params: &[Type]) -> Vec<Self> {
        let type_name = |ty: &Type| match ty {
            Type::Struct(st) => st.name().map(|name| name.to_string()),
            Type::Enum(en) => Some(en.name().to_string()),
            _ => None,
        };
        let mut taken: BTreeSet<String> = BTreeSet::from(["ret".to_string()]);
        for (index, ty) in params.iter().enumerate() {
            if type_name(ty).is_none() {
                taken.insert(format!("arg{index}"));
            }
        }
        params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let base = match type_name(ty) {
                    Some(name) => {
                        let base = names::snake_case(&name);
                        let mut candidate = base.clone();
                        let mut repeat = 2;
                        while !taken.insert(candidate.clone()) {
                            candidate = format!("{base}_{repeat}");
                            repeat += 1;
                        }
                        candidate
                    }
                    None => format!("arg{index}"),
                };
                Self {
                    ident: names::item_ident(&base),
                    base,
                    ty: ty.clone(),
                }
            })
            .collect()
    }

    /// The `extern "C"` parameter for the scalar at `path` in this parameter: the parameter's own
    /// name for a scalar parameter, else that name and the steps of `path` joined by `_`
    /// (`pair_a`, `arg0_3`, `asset_id_0`).
    fn extern_name(&self, path: &[FieldStep]) -> String {
        if path.is_empty() {
            return self.ident.clone();
        }
        let steps: Vec<String> = path
            .iter()
            .map(|step| match &step.name {
                Some(name) => name.to_string(),
                None => step.index.to_string(),
            })
            .collect();
        names::item_ident(&format!("{}_{}", self.base, steps.join("_")))
    }
}

/// A pointer the wrapper hands the callee, which makes the wrapper `unsafe`.
struct Pointer {
    /// The wrapper's expression for it: a parameter, or a field or element of one.
    expr: String,
    /// The pointer's address space.
    space: AddressSpace,
}

impl Pointer {
    /// What the caller must guarantee about it, for the `# Safety` section.
    fn requirement(&self) -> String {
        let expr = &self.expr;
        match self.space {
            AddressSpace::Element => {
                format!(
                    "`{expr}` must be a valid element address for the callee's reads and writes."
                )
            }
            AddressSpace::Byte => {
                format!("`{expr}` must be a valid address for the callee's reads and writes.")
            }
        }
    }
}

/// A result scalar as the wrapper reads it back.
struct Slot {
    /// The expression that reads it: `ret` for a direct result, `ret.r<i>` for a return-area slot.
    expr: String,
    /// Whether it is held in its return-area carrier, which widens a narrow integer to 32 bits,
    /// rather than in its own type.
    widened: bool,
}

/// What writing one wrapper needs.
struct Wrapper<'a> {
    item: &'a ProcedureItem,
    universe: &'a TypeUniverse<'a>,
    /// The module the wrapper is written in.
    at: &'a Path,
    params: Vec<Param>,
}

impl Wrapper<'_> {
    /// The type an `extern "C"` declaration gives `scalar`; see [`types::carrier`].
    fn carrier(&self, scalar: &WasmScalar) -> String {
        match scalar {
            WasmScalar::Felt => self.universe.support_item("Felt", self.at),
            scalar => types::carrier(scalar).to_string(),
        }
    }

    /// The type of `scalar`'s field in the return area: its carrier, except that a narrow integer
    /// occupies a 32-bit slot, zero- or sign-extended, as the frontend stores it (an `i32`).
    fn slot_carrier(&self, scalar: &WasmScalar) -> String {
        match scalar {
            WasmScalar::I1 | WasmScalar::U8 | WasmScalar::U16 => "u32".to_string(),
            WasmScalar::I8 | WasmScalar::I16 => "i32".to_string(),
            scalar => self.carrier(scalar),
        }
    }

    /// The argument the wrapper passes for the scalar at `path` in `param`: the field or element
    /// there, as its carrier. A pointer is passed as its address, unconverted, and added to
    /// `pointers`.
    fn argument(
        &self,
        param: &Param,
        path: &[FieldStep],
        pointers: &mut Vec<Pointer>,
    ) -> Result<Expr, Error> {
        let mut expr = param.ident.clone();
        let mut ty = param.ty.clone();
        for step in path {
            ty = match &ty {
                Type::Struct(st) => {
                    let st = st.get();
                    let field = &st.fields()[step.index];
                    let name = names::item_ident(&types::field_name(field, step.index));
                    expr = format!("{expr}.{name}");
                    field.ty.clone()
                }
                Type::Array(array) => {
                    expr = format!("{expr}[{}]", step.index);
                    array.ty.clone()
                }
                ty => return Err(self.inconsistent(format!("a field path enters `{ty}`"))),
            };
        }
        Ok(match &ty {
            Type::Ptr(ptr) => {
                pointers.push(Pointer {
                    expr: expr.clone(),
                    space: ptr.addrspace(),
                });
                match ptr.addrspace() {
                    AddressSpace::Element => Expr::complex(format!("{expr}.addr()")),
                    AddressSpace::Byte => Expr::complex(format!("{expr}.addr() as u32")),
                }
            }
            Type::Enum(en) => {
                let discriminant = self.universe.rust_type(en.get().discriminant(), self.at)?;
                Expr::simple(format!("{expr} as {}", discriminant.text))
            }
            _ => Expr::simple(expr),
        })
    }

    /// Write the declared `results`, rebuilt from `slots`, as the wrapper's value: an enum is
    /// converted from its discriminant by a `match` (a statement of its own when it is not the
    /// whole result), one value of two or more felts that fills the return area is read from it
    /// whole (see the module doc), anything else is built from its slots by an expression.
    fn rebuild(&self, w: &mut Writer, results: &[Type], slots: Vec<Slot>) -> Result<(), Error> {
        if let ([Type::Enum(en)], [slot]) = (results, slots.as_slice()) {
            let value = self.discriminant(slot, &en.get())?;
            let path = self.universe.rust_type(&results[0], self.at)?.text;
            write_enum_match(w, "", &path, &value, &self.undeclared(&en.get()), "");
            return Ok(());
        }
        if let [result] = results
            && let Some(felts) = felt_count(result)
            && felts >= 2
            && felts == slots.len()
            && result.size_in_bytes() == felts * 4
            && result.min_alignment() <= WORD_ALIGNED
        {
            // One value of felts fills the return area, laid out as the value itself: the felts one
            // after another, with no padding (the size check), and the area is aligned for it.
            let rust = self.universe.rust_type(result, self.at)?.text;
            write_whole_read(w, &rust);
            return Ok(());
        }
        let mut rebuilt = Rebuilt {
            slots: slots.into_iter().enumerate(),
            enums: Vec::new(),
        };
        let mut values = results
            .iter()
            .map(|result| self.value(result, true, &mut rebuilt))
            .collect::<Result<Vec<_>, Error>>()?;
        if rebuilt.slots.next().is_some() {
            return Err(self.inconsistent("the results take fewer scalars than the return has"));
        }
        for read in &rebuilt.enums {
            let prefix = format!("let {} = ", read.local);
            write_enum_match(w, &prefix, &read.path, &read.discriminant, &read.message, ";");
        }
        let value = match values.len() {
            1 => values.remove(0),
            _ => Expr::Tuple(values),
        };
        write_expr(w, "", &value, "");
        Ok(())
    }

    /// The value of the declared type `ty`, read from the next slots of `rebuilt`. `words` is
    /// whether a `[felt; 4]` in it is a `Word` (see [`layout::StructForm::words`]).
    fn value(&self, ty: &Type, words: bool, rebuilt: &mut Rebuilt) -> Result<Expr, Error> {
        match ty {
            Type::Struct(st) => {
                let path = self.universe.rust_type(ty, self.at)?.text;
                let st = st.get();
                let words = layout::struct_form(&st)
                    .map_err(|reason| Error::Unsupported {
                        path: self.item.path.to_string(),
                        reason,
                    })?
                    .words;
                let mut fields = Vec::with_capacity(st.len());
                for (index, field) in st.fields().iter().enumerate() {
                    let name = names::item_ident(&types::field_name(field, index));
                    fields.push((name, self.value(&field.ty, words, rebuilt)?));
                }
                return Ok(Expr::Struct { path, fields });
            }
            Type::Array(array) => {
                let items = (0..array.len)
                    .map(|_| self.value(&array.ty, words, rebuilt))
                    .collect::<Result<Vec<_>, Error>>()?;
                if self.universe.field_type(ty, self.at, words)?.kind == RustTypeKind::Word {
                    return Ok(Expr::Call {
                        callee: format!("{}::new", self.universe.support_item("Word", self.at)),
                        args: vec![Expr::Array(items)],
                    });
                }
                return Ok(Expr::Array(items));
            }
            _ => {}
        }
        let Some((index, slot)) = rebuilt.slots.next() else {
            return Err(self.inconsistent("the results take more scalars than the return has"));
        };
        let expr = &slot.expr;
        Ok(match ty {
            Type::Enum(en) => {
                let local = format!("r{index}");
                rebuilt.enums.push(EnumRead {
                    local: local.clone(),
                    path: self.universe.rust_type(ty, self.at)?.text,
                    discriminant: self.discriminant(&slot, &en.get())?,
                    message: self.undeclared(&en.get()),
                });
                Expr::simple(local)
            }
            Type::Ptr(ptr) => match ptr.addrspace() {
                AddressSpace::Element => {
                    let element_ptr = self.universe.support_item("ElementPtr", self.at);
                    Expr::complex(format!("{element_ptr}::new({expr})"))
                }
                AddressSpace::Byte => {
                    let pointee = self.universe.rust_type(ptr.pointee(), self.at)?.text;
                    Expr::simple(format!("{expr} as *mut {pointee}"))
                }
            },
            Type::I1 if slot.widened => Expr::complex(format!("{expr} != 0")),
            Type::I8 | Type::U8 | Type::I16 | Type::U16 if slot.widened => {
                Expr::simple(format!("{expr} as {}", self.universe.rust_type(ty, self.at)?.text))
            }
            _ => Expr::simple(expr.clone()),
        })
    }

    /// The discriminant of an `en` held in `slot`, in the discriminant's own type.
    fn discriminant(&self, slot: &Slot, en: &EnumType) -> Result<String, Error> {
        let ty = en.discriminant();
        let narrow = matches!(ty, Type::I8 | Type::U8 | Type::I16 | Type::U16);
        Ok(if slot.widened && narrow {
            format!("{} as {}", slot.expr, self.universe.rust_type(ty, self.at)?.text)
        } else {
            slot.expr.clone()
        })
    }

    /// The panic message for a discriminant of `en` that names none of its variants, as the
    /// content of a format string literal.
    fn undeclared(&self, en: &EnumType) -> String {
        let what =
            format!("`{}` returned an undeclared {} discriminant ", self.item.path, en.name());
        format!("{}{{v}}", format_literal(&what))
    }

    /// An error for a lowered signature that disagrees with its declared one.
    fn inconsistent(&self, what: impl core::fmt::Display) -> Error {
        Error::Message(format!(
            "`{}`: the lowered signature does not match the declared one: {what}",
            self.item.path
        ))
    }
}

/// The result slots still to read, and the enums read from them so far.
struct Rebuilt {
    slots: core::iter::Enumerate<std::vec::IntoIter<Slot>>,
    enums: Vec<EnumRead>,
}

/// An enum result, converted from its discriminant by a statement of its own.
struct EnumRead {
    /// The local the enum is bound to: `r<slot>`.
    local: String,
    /// The enum's Rust path.
    path: String,
    /// The discriminant, in its own type.
    discriminant: String,
    /// The panic message for a discriminant that names no variant.
    message: String,
}

/// The number of felts `ty` is made of, if it is made only of felts.
fn felt_count(ty: &Type) -> Option<usize> {
    match ty {
        Type::Felt => Some(1),
        Type::Array(array) => felt_count(&array.ty).map(|felts| felts * array.len),
        Type::Struct(st) => st.get().fields().iter().map(|field| felt_count(&field.ty)).sum(),
        _ => None,
    }
}

/// Whether a declared result of type `ty` is its own carrier, so the extern's return value is the
/// wrapper's.
fn is_plain(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I1
            | Type::I8
            | Type::U8
            | Type::I16
            | Type::U16
            | Type::I32
            | Type::U32
            | Type::I64
            | Type::U64
            | Type::Felt
    )
}

/// `text` escaped for a Rust string literal.
fn escape(text: &str) -> String {
    text.escape_default().to_string()
}

/// `text` escaped for a format string literal: as [`escape`], with braces doubled.
fn format_literal(text: &str) -> String {
    escape(text).replace('{', "{{").replace('}', "}}")
}

/// An expression of the wrapper's, laid out as rustfmt lays it out: on one line when it fits and
/// every list in it is within rustfmt's width for that kind of list, else broken, list by list.
enum Expr {
    /// Text that is never broken: a name, a field, a cast, a short method call. `simple` is
    /// rustfmt's notion (a name, field, index or cast of one), which decides how a list of them
    /// is broken.
    Atom { text: String, simple: bool },
    /// `callee(args)`.
    Call { callee: String, args: Vec<Expr> },
    /// `[items]`.
    Array(Vec<Expr>),
    /// `(items)`.
    Tuple(Vec<Expr>),
    /// `path { field: value, … }`.
    Struct {
        path: String,
        fields: Vec<(String, Expr)>,
    },
}

impl Expr {
    fn simple(text: String) -> Self {
        Self::Atom { text, simple: true }
    }

    fn complex(text: String) -> Self {
        Self::Atom {
            text,
            simple: false,
        }
    }

    /// The expression on one line.
    fn flat(&self) -> String {
        match self {
            Self::Atom { text, .. } => text.clone(),
            Self::Call { callee, args } => format!("{callee}({})", flat_list(args)),
            Self::Array(items) => format!("[{}]", flat_list(items)),
            Self::Tuple(items) => format!("({})", flat_list(items)),
            Self::Struct { path, fields } if fields.is_empty() => format!("{path} {{}}"),
            Self::Struct { path, fields } => format!("{path} {{ {} }}", flat_fields(fields)),
        }
    }

    /// Whether every list in the expression is within rustfmt's width for one-line lists of its
    /// kind.
    fn fits_flat(&self) -> bool {
        match self {
            Self::Atom { .. } => true,
            Self::Call { args: items, .. } | Self::Tuple(items) => {
                items.iter().all(Self::fits_flat) && width(&flat_list(items)) <= CALL_WIDTH
            }
            Self::Array(items) => {
                items.iter().all(Self::fits_flat) && width(&self.flat()) <= ARRAY_WIDTH
            }
            Self::Struct { fields, .. } => {
                fields.iter().all(|(_, value)| value.fits_flat())
                    && width(&flat_fields(fields)) <= STRUCT_LIT_WIDTH
            }
        }
    }

    /// Whether rustfmt counts the expression as simple; see [`Self::Atom`].
    fn is_simple(&self) -> bool {
        matches!(self, Self::Atom { simple: true, .. })
    }

    /// Whether, as the only argument of a call, the expression keeps its opening bracket on the
    /// call's line when it is broken (rustfmt's overflow of the last argument).
    fn overflows(&self) -> bool {
        !matches!(self, Self::Atom { .. })
    }
}

fn flat_list(items: &[Expr]) -> String {
    items.iter().map(Expr::flat).collect::<Vec<_>>().join(", ")
}

fn flat_fields(fields: &[(String, Expr)]) -> String {
    fields
        .iter()
        .map(|(name, value)| flat_field(name, value))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `name: value`, or `name` alone when the value is a variable of that name, as rustfmt's
/// `use_field_init_shorthand` writes it.
fn flat_field(name: &str, value: &Expr) -> String {
    match value {
        Expr::Atom { text, .. } if text == name => name.to_string(),
        value => format!("{name}: {}", value.flat()),
    }
}

/// Write `prefix`, `expr` and `suffix`, on one line if they fit, else with `expr` broken.
fn write_expr(w: &mut Writer, prefix: &str, expr: &Expr, suffix: &str) {
    let line = format!("{prefix}{}{suffix}", expr.flat());
    if expr.fits_flat() && w.fits(&line) {
        w.line(&line);
        return;
    }
    match expr {
        Expr::Atom { .. } => w.line(&line),
        Expr::Call { callee, args } if args.len() == 1 && args[0].overflows() => {
            write_expr(w, &format!("{prefix}{callee}("), &args[0], &format!("){suffix}"));
        }
        Expr::Call { callee, args } => {
            write_list(w, &format!("{prefix}{callee}("), args, &format!("){suffix}"))
        }
        Expr::Tuple(items) => write_list(w, &format!("{prefix}("), items, &format!("){suffix}")),
        Expr::Array(items) => write_list(w, &format!("{prefix}["), items, &format!("]{suffix}")),
        Expr::Struct { path, fields } => {
            w.open_with(&format!("{prefix}{path} {{"));
            for (name, value) in fields {
                match value {
                    Expr::Atom { text, .. } if text == name => w.line(&format!("{name},")),
                    value => write_expr(w, &format!("{name}: "), value, ","),
                }
            }
            w.close_with(&format!("}}{suffix}"));
        }
    }
}

/// Write `open`, `items` and `close`, the items broken onto lines of their own: filling the lines
/// when every item is simple and short, else one per line; each is followed by a comma.
fn write_list(w: &mut Writer, open: &str, items: &[Expr], close: &str) {
    w.open_with(open);
    let short = |item: &Expr| item.is_simple() && width(&item.flat()) <= SHORT_ITEM_WIDTH;
    if items.iter().all(short) {
        // A comma follows the last item too, so a line holds one column less than the width.
        let room = MAX_WIDTH - w.indentation() - 1;
        let mut line = String::new();
        for item in items {
            let item = format!("{},", item.flat());
            if !line.is_empty() && width(&line) + 1 + width(&item) > room {
                w.line(&core::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&item);
        }
        w.line(&line);
    } else {
        for item in items {
            write_expr(w, "", item, ",");
        }
    }
    w.close_with(close);
}

/// Write `prefix unsafe { call }suffix`: on one line if it fits; else, where `prefix` is a `let`,
/// with the block on the next line if it fits there, as rustfmt moves the value of a `let`; else
/// with the call on lines of its own inside the block.
fn write_unsafe_call(w: &mut Writer, prefix: &str, call: &Expr, suffix: &str) {
    let block = format!("unsafe {{ {} }}{suffix}", call.flat());
    if call.fits_flat() && w.fits(&format!("{prefix}{block}")) {
        w.line(&format!("{prefix}{block}"));
        return;
    }
    if !prefix.is_empty()
        && call.fits_flat()
        && w.indentation() + Writer::INDENT + width(&block) <= MAX_WIDTH
    {
        w.line(prefix.trim_end());
        w.continuation(&block);
        return;
    }
    w.open_with(&format!("{prefix}unsafe {{"));
    write_expr(w, "", call, "");
    w.close_with(&format!("}}{suffix}"));
}

/// Write `unsafe { (&raw const *ret).cast::<rust>().read() }`, the return area read whole as the
/// type `rust`: on one line if it fits, else with the chain inside the block, broken one call per
/// line when it is wider than rustfmt's `chain_width` or does not fit.
fn write_whole_read(w: &mut Writer, rust: &str) {
    let chain = format!("(&raw const *ret).cast::<{rust}>().read()");
    let line = format!("unsafe {{ {chain} }}");
    let short = width(&chain) <= CHAIN_WIDTH;
    if short && w.fits(&line) {
        w.line(&line);
        return;
    }
    w.open_with("unsafe {");
    if short && w.fits(&chain) {
        w.line(&chain);
    } else {
        w.line("(&raw const *ret)");
        w.continuation(&format!(".cast::<{rust}>()"));
        w.continuation(".read()");
    }
    w.close_with("}");
}

/// Write `prefix match <path>::try_from(<value>) { … }suffix`, which converts a discriminant to
/// the enum at `path` and panics with the format string `message` (literal content) on one that
/// names no variant. `Ok` and `Err` are written in full: the module may have a procedure of
/// either name.
fn write_enum_match(
    w: &mut Writer,
    prefix: &str,
    path: &str,
    value: &str,
    message: &str,
    suffix: &str,
) {
    w.open_with(&format!("{prefix}match {path}::try_from({value}) {{"));
    w.arm(&format!("{RESULT}::Ok(value)"), "value");
    let err = format!("{RESULT}::Err(v)");
    let panic = format!("panic!(\"{message}\")");
    // On the arm's line if it fits, else in a block if it fits there; else broken after the
    // arrow.
    if w.fits(&format!("{err} => {panic},"))
        || w.indentation() + Writer::INDENT + width(&panic) <= MAX_WIDTH
    {
        w.arm(&err, &panic);
    } else {
        write_message(w, &format!("{err} => "), "panic", message, ",");
    }
    w.close_with(&format!("}}{suffix}"));
}

/// Write `prefix name!("message")suffix`, a macro call whose one argument is the string literal
/// with content `message`, as rustfmt (with `format_strings`) lays it out: on one line if it fits,
/// else the literal on lines of its own, broken after a space where it is still too wide (see
/// [`break_string`]). A literal rustfmt cannot break to fit is left on one line, as rustfmt leaves
/// it.
fn write_message(w: &mut Writer, prefix: &str, name: &str, message: &str, suffix: &str) {
    let line = format!("{prefix}{name}!(\"{message}\"){suffix}");
    if w.fits(&line) {
        w.line(&line);
        return;
    }
    // The literal starts one level deeper; a comma or the closing parenthesis may follow it, and
    // each of its lines carries a quote or a space before and a quote or backslash after.
    let column = w.indentation() + Writer::INDENT;
    let Some(pieces) = break_string(message, MAX_WIDTH.saturating_sub(column + 3)) else {
        w.line(&line);
        return;
    };
    w.open_with(&format!("{prefix}{name}!("));
    let last = pieces.len() - 1;
    for (index, piece) in pieces.iter().enumerate() {
        let open = if index == 0 { '"' } else { ' ' };
        let close = if index == last { '"' } else { '\\' };
        w.line(&format!("{open}{piece}{close}"));
    }
    w.close_with(&format!("){suffix}"));
}

/// The pieces rustfmt's `format_strings` breaks the string literal content `text` into, at most
/// `max` columns each, or `None` if it cannot break it so.
///
/// A piece ends after the last space that leaves it at most `max` columns (and at least
/// [`MIN_STRING_PIECE`]), taking the spaces after it along; failing a space, after the last
/// punctuation mark, where a `:` of a `::` is not one. A literal with no such place has no
/// rustfmt layout.
fn break_string(text: &str, max: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let is_space = |c: char| c.is_whitespace();
    let is_break = |rest: &[char], i: usize| {
        let c = rest[i];
        let path_separator = c == ':'
            && ((i > 0 && rest[i - 1] == ':') || rest.get(i + 1).is_some_and(|&n| n == ':'));
        r##"!"#%&'*,./:;?@\"##.contains(c) && !path_separator
    };
    let mut pieces = Vec::new();
    let mut start = 0;
    loop {
        let rest = &chars[start..];
        if rest.len() <= max {
            pieces.push(rest.iter().collect());
            return Some(pieces);
        }
        let at = (0..max)
            .rev()
            .find(|&i| is_space(rest[i]))
            .filter(|&i| i >= MIN_STRING_PIECE)
            .or_else(|| {
                (0..max).rev().find(|&i| is_break(rest, i)).filter(|&i| i >= MIN_STRING_PIECE)
            })
            .or_else(|| (max..rest.len()).find(|&i| is_space(rest[i]) || is_break(rest, i)))?;
        let mut end = at + 1;
        while end < rest.len() && is_space(rest[end]) {
            end += 1;
        }
        if end > max {
            return None;
        }
        pieces.push(rest[..end].iter().collect());
        start += end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn felt(name: &str) -> Expr {
        Expr::simple(name.to_string())
    }

    /// Lay `expr` out at `depth` block levels, as `prefix` + `expr` + `suffix`.
    fn layout(depth: usize, prefix: &str, expr: &Expr, suffix: &str) -> Vec<String> {
        let mut w = Writer::new();
        for _ in 0..depth {
            w.open_with("{");
        }
        write_expr(&mut w, prefix, expr, suffix);
        for _ in 0..depth {
            w.close_with("}");
        }
        let text = w.finish();
        let lines: Vec<&str> = text.lines().collect();
        lines[depth..lines.len() - depth].iter().map(|line| line.to_string()).collect()
    }

    /// A return area read whole stays on one line while it fits and its chain is within 80
    /// columns, rustfmt's `chain_width`; past that the chain moves into the block, and is broken
    /// one call per line when it is itself wider than 80 columns or does not fit.
    #[test]
    fn whole_reads_break_their_chain_as_rustfmt_does() {
        let read = |depth: usize, rust: &str| {
            let mut w = Writer::new();
            for _ in 0..depth {
                w.open_with("{");
            }
            write_whole_read(&mut w, rust);
            for _ in 0..depth {
                w.close_with("}");
            }
            let text = w.finish();
            let lines: Vec<&str> = text.lines().collect();
            lines[depth..lines.len() - depth]
                .iter()
                .map(|line| line.trim_end().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(read(1, "Word"), ["    unsafe { (&raw const *ret).cast::<Word>().read() }"]);
        // A chain of 80 columns fits the block at this depth but not the one-line form.
        let rust = format!("a::{}", "b".repeat(42));
        assert_eq!(
            read(3, &rust),
            [
                "            unsafe {".to_string(),
                format!("                (&raw const *ret).cast::<{rust}>().read()"),
                "            }".to_string(),
            ]
        );
        // A chain of 81 columns is broken whatever the room.
        let rust = format!("a::{}", "b".repeat(43));
        assert_eq!(
            read(0, &rust),
            [
                "unsafe {".to_string(),
                "    (&raw const *ret)".to_string(),
                format!("        .cast::<{rust}>()"),
                "        .read()".to_string(),
                "}".to_string(),
            ]
        );
    }

    /// A struct literal stays on one line up to a body of 18 columns, rustfmt's
    /// `struct_lit_width`; one field per line past it.
    #[test]
    fn struct_literals_break_past_eighteen_columns() {
        let pair = |b: &str| Expr::Struct {
            path: "Ab".into(),
            fields: vec![("a".into(), felt("ret.r0")), (b.into(), felt("ret"))],
        };
        assert_eq!(layout(1, "", &pair("bb"), ""), ["    Ab { a: ret.r0, bb: ret }"]);
        assert_eq!(
            layout(1, "", &pair("bbb"), ""),
            ["    Ab {", "        a: ret.r0,", "        bbb: ret,", "    }"]
        );
        // A field whose value is a variable of its name is written in shorthand.
        assert_eq!(layout(0, "", &pair("ret"), ""), ["Ab { a: ret.r0, ret }"]);
    }

    /// A call or tuple stays on one line up to arguments of 80 columns, rustfmt's
    /// `fn_call_width`; past it, short simple items fill lines (a comma after each, the last
    /// included), and anything else takes a line of its own.
    #[test]
    fn lists_break_past_eighty_columns_filling_lines_with_short_simple_items() {
        let tuple = |items: &[&str]| Expr::Tuple(items.iter().map(|item| felt(item)).collect());
        let nineteen = "a".repeat(19);
        let fits = tuple(&[&nineteen, &nineteen, &nineteen, &"a".repeat(17)]);
        assert_eq!(layout(1, "", &fits, "").len(), 1, "80 columns of arguments");
        let over = tuple(&[&nineteen, &nineteen, &nineteen, &"a".repeat(18)]);
        assert_eq!(layout(1, "", &over, "").len(), 6, "81 columns: one item per line");

        // Items of 9 and 10 columns at depth 3: a line holds 87 columns, the comma included.
        let mut items = vec!["y".repeat(9); 7];
        items.push("z".repeat(9));
        items.extend(vec!["w".repeat(10); 3]);
        let items: Vec<&str> = items.iter().map(String::as_str).collect();
        let lines = layout(2, "let _ = ", &tuple(&items), ";");
        assert_eq!(lines.len(), 4, "{lines:#?}");
        assert_eq!(lines[1].len(), 99, "{lines:#?}");
        assert_eq!(lines[2], format!("            {}", ["wwwwwwwwww,"; 3].join(" ")));
        assert_eq!(lines[3], "        );");

        // A cast of a method call is not simple.
        let call = Expr::Call {
            callee: "__f".into(),
            args: vec![
                felt("a"),
                felt(&"b".repeat(60)),
                Expr::complex("ret.as_mut_ptr() as *mut Ret".into()),
            ],
        };
        let mut lines =
            vec!["__f(".to_string(), "    a,".into(), format!("    {},", "b".repeat(60))];
        lines.extend(["    ret.as_mut_ptr() as *mut Ret,".to_string(), ")".into()]);
        assert_eq!(layout(0, "", &call, ""), lines);
    }

    /// A `let` whose call does not fit on its line moves the `unsafe` block to the next line if it
    /// fits there, and breaks the call inside the block if it does not.
    #[test]
    fn a_let_moves_its_call_to_the_next_line_before_breaking_it() {
        let call = |width: usize| Expr::Call {
            callee: "__f".into(),
            args: vec![felt(&"a".repeat(width))],
        };
        let lines = |width: usize| {
            let mut w = Writer::new();
            w.open_with("{");
            write_unsafe_call(&mut w, "let ret = ", &call(width), ";");
            w.close_with("}");
            let text = w.finish();
            text.lines().skip(1).map(str::to_string).collect::<Vec<_>>()
        };
        // `let ret = unsafe { __f() };` is 27 columns besides the argument, at 4.
        assert_eq!(
            lines(69),
            [format!("    let ret = unsafe {{ __f({}) }};", "a".repeat(69)), "}".into()]
        );
        assert_eq!(
            lines(70),
            [
                "    let ret =".to_string(),
                format!("        unsafe {{ __f({}) }};", "a".repeat(70)),
                "}".into()
            ]
        );
        // `unsafe { __f() };` is 17 columns besides the argument, at 8.
        assert_eq!(lines(75).len(), 3);
        assert_eq!(
            lines(76),
            [
                "    let ret = unsafe {".to_string(),
                format!("        __f({})", "a".repeat(76)),
                "    };".into(),
                "}".into()
            ]
        );
    }

    /// `Word::new([...])` keeps its bracket on the call's line when it breaks.
    #[test]
    fn the_only_argument_of_a_call_overflows_it() {
        let word = Expr::Call {
            callee: "Word::new".into(),
            args: vec![Expr::Array((0..4).map(|i| felt(&format!("ret.r{i}"))).collect())],
        };
        assert_eq!(layout(0, "", &word, ""), ["Word::new([ret.r0, ret.r1, ret.r2, ret.r3])"]);
        let lines = layout(0, &"x".repeat(60), &word, "");
        assert_eq!(lines[0], format!("{}Word::new([", "x".repeat(60)));
        assert_eq!(lines[1], "    ret.r0, ret.r1, ret.r2, ret.r3,");
        assert_eq!(lines[2], "])");
    }

    /// The pieces rustfmt broke these literals into, with the workspace's settings.
    #[test]
    fn strings_break_where_rustfmt_breaks_them() {
        let message =
            |path: &str| format!("`{path}` is only available when compiled for the Miden VM");
        // At column 12 a piece holds 85 columns.
        let text = message(&format!("::f::{}", "x".repeat(30)));
        assert_eq!(
            break_string(&text, 85).unwrap(),
            [&text[..84], "VM"],
            "after the last space that fits, the space kept"
        );
        assert_eq!(
            break_string(&message(&format!("::f::{}", "x".repeat(29))), 85).unwrap().len(),
            1
        );
        let text = message(&format!("::m::{}", "x".repeat(75)));
        assert_eq!(break_string(&text, 85).unwrap()[0], &text[..83]);

        // No space in reach: after the last punctuation mark that is not part of a `::`.
        let colons = ["abcdefghij"; 5].join(":");
        let text = format!("a{colons}::{} tail words here", ["abcdefghij"; 4].join("::"));
        let pieces = break_string(&text, 85).unwrap();
        assert_eq!(pieces, [&text[..45], &text[45..]]);
        assert!(pieces[0].ends_with("abcdefghij:"));
        // A path of `::`s alone cannot be broken; rustfmt leaves such a literal as it is.
        let path = format!("`{}` is", ["abcdefghij"; 9].join("::"));
        assert_eq!(break_string(&path, 85), None);
    }

    #[test]
    fn messages_go_on_lines_of_their_own_when_they_do_not_fit() {
        let mut w = Writer::new();
        w.open_with("{");
        w.open_with("{");
        let fits =
            format!("`::f::{}` is only available when compiled for the Miden VM", "x".repeat(18));
        write_message(&mut w, "", "unimplemented", &fits, "");
        let over =
            format!("`::f::{}` is only available when compiled for the Miden VM", "x".repeat(19));
        write_message(&mut w, "", "unimplemented", &over, "");
        let broken =
            format!("`::f::{}` is only available when compiled for the Miden VM", "x".repeat(30));
        write_message(&mut w, "", "unimplemented", &broken, "");
        w.close_with("}");
        w.close_with("}");
        let text = w.finish();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[2].len(), 100, "{text}");
        assert_eq!(lines[3], "        unimplemented!(");
        assert_eq!(lines[4], format!("            \"{over}\""));
        assert_eq!(lines[5], "        )");
        assert_eq!(lines[7], format!("            \"{}\\", &broken[..84]));
        assert_eq!(lines[8], "             VM\"");
    }

    #[test]
    fn signatures_are_written_as_the_manifest_renders_them_with_named_types_by_name() {
        use std::sync::Arc;

        use miden_assembly_syntax::ast::types::{ArrayType, CallConv, PointerType, StructType};

        let word = Type::from(ArrayType::new(Type::Felt, 4));
        let pair = Type::from(StructType::named("Pair".into(), [Type::Felt, Type::Felt]));
        let anonymous = Type::from(StructType::new([Type::Felt, Type::U32]));
        let ptr = Type::Ptr(Arc::new(PointerType::new_with_address_space(
            pair.clone(),
            AddressSpace::Element,
        )));
        let signature = FunctionType::new(CallConv::Fast, [ptr, Type::I1], [word, pair, anonymous]);
        assert_eq!(
            render_signature("f", &signature),
            "pub proc f(ptr<element, Pair>, i1) -> [felt; 4], Pair, struct {felt, u32}"
        );
        let none = FunctionType::new(CallConv::Fast, [], []);
        assert_eq!(render_signature("g", &none), "pub proc g()");
    }
}
