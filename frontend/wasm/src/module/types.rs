//! Types for parsed core WebAssembly modules.

use core::{fmt, ops::Index};

use cranelift_entity::PrimaryMap;
use midenc_hir::{
    self as hir, Abi, AbiParam, CallConv, FxHashMap, Immediate, Signature, Visibility,
};
use midenc_session::diagnostics::{DiagnosticsHandler, Severity};
use wasmparser::types::CoreTypeId;

use crate::{component::SignatureIndex, error::WasmResult, module::Module, unsupported_diag};

/// Generates a new index type for each entity.
#[macro_export]
macro_rules! indices {
    ($(
        $(#[$a:meta])*
        pub struct $name:ident(u32);
    )*) => ($(
        $(#[$a])*
        #[derive(
            Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug
        )]
        #[repr(transparent)]
        pub struct $name(u32);
        ::cranelift_entity::entity_impl!($name);
    )*);
}

indices! {
/// Index type of a function (imported or defined) inside the WebAssembly module.
pub struct FuncIndex(u32);

/// Index type of a defined function inside the WebAssembly module.
pub struct DefinedFuncIndex(u32);

/// Index type of a defined table inside the WebAssembly module.
pub struct DefinedTableIndex(u32);

/// Index type of a defined memory inside the WebAssembly module.
pub struct DefinedMemoryIndex(u32);

/// Index type of a defined memory inside the WebAssembly module.
pub struct OwnedMemoryIndex(u32);

/// Index type of a defined global inside the WebAssembly module.
pub struct DefinedGlobalIndex(u32);

/// Index type of a table (imported or defined) inside the WebAssembly module.
pub struct TableIndex(u32);

/// Index type of a global variable (imported or defined) inside the WebAssembly module.
pub struct GlobalIndex(u32);

/// Index type of a linear memory (imported or defined) inside the WebAssembly module.
pub struct MemoryIndex(u32);

/// Index type of a passive data segment inside the WebAssembly module.
pub struct DataIndex(u32);

/// Index type of a passive element segment inside the WebAssembly module.
pub struct ElemIndex(u32);

/// Index type of a type inside the WebAssembly module.
pub struct TypeIndex(u32);

/// Index type of a data segment inside the WebAssembly module.
pub struct DataSegmentIndex(u32);

}

/// WebAssembly value type -- equivalent of `wasmparser`'s Type.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WasmType {
    /// I32 type
    I32,
    /// I64 type
    I64,
    /// F32 type
    F32,
    /// F64 type
    F64,
    /// V128 type
    V128,
    /// Reference type
    Ref(WasmRefType),
}

impl fmt::Display for WasmType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WasmType::I32 => write!(f, "i32"),
            WasmType::I64 => write!(f, "i64"),
            WasmType::F32 => write!(f, "f32"),
            WasmType::F64 => write!(f, "f64"),
            WasmType::V128 => write!(f, "v128"),
            WasmType::Ref(rt) => write!(f, "{rt}"),
        }
    }
}

/// WebAssembly reference type -- equivalent of `wasmparser`'s RefType
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WasmRefType {
    pub nullable: bool,
    pub heap_type: WasmHeapType,
}

impl WasmRefType {
    pub const EXTERNREF: WasmRefType = WasmRefType {
        nullable: true,
        heap_type: WasmHeapType::Extern,
    };
    pub const FUNCREF: WasmRefType = WasmRefType {
        nullable: true,
        heap_type: WasmHeapType::Func,
    };
}

impl fmt::Display for WasmRefType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::FUNCREF => write!(f, "funcref"),
            Self::EXTERNREF => write!(f, "externref"),
            _ => {
                if self.nullable {
                    write!(f, "(ref null {})", self.heap_type)
                } else {
                    write!(f, "(ref {})", self.heap_type)
                }
            }
        }
    }
}

/// WebAssembly heap type -- equivalent of `wasmparser`'s HeapType
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WasmHeapType {
    /// The abstract, untyped (any) function.
    ///
    /// Introduced in the references-types proposal.
    Func,
    /// The abstract, external heap type.
    ///
    /// Introduced in the references-types proposal.
    Extern,
}

impl fmt::Display for WasmHeapType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Func => write!(f, "func"),
            Self::Extern => write!(f, "extern"),
        }
    }
}

/// WebAssembly function type -- equivalent of `wasmparser`'s FuncType.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WasmFuncType {
    params: Box<[WasmType]>,
    externref_params_count: usize,
    returns: Box<[WasmType]>,
    externref_returns_count: usize,
}

impl WasmFuncType {
    #[inline]
    pub fn new(params: Box<[WasmType]>, returns: Box<[WasmType]>) -> Self {
        let externref_params_count = params
            .iter()
            .filter(|p| match **p {
                WasmType::Ref(rt) => rt.heap_type == WasmHeapType::Extern,
                _ => false,
            })
            .count();
        let externref_returns_count = returns
            .iter()
            .filter(|r| match **r {
                WasmType::Ref(rt) => rt.heap_type == WasmHeapType::Extern,
                _ => false,
            })
            .count();
        WasmFuncType {
            params,
            externref_params_count,
            returns,
            externref_returns_count,
        }
    }

    /// Function params types.
    #[inline]
    pub fn params(&self) -> &[WasmType] {
        &self.params
    }

    /// How many `externref`s are in this function's params?
    #[inline]
    pub fn externref_params_count(&self) -> usize {
        self.externref_params_count
    }

    /// Returns params types.
    #[inline]
    pub fn returns(&self) -> &[WasmType] {
        &self.returns
    }

    /// How many `externref`s are in this function's returns?
    #[inline]
    pub fn externref_returns_count(&self) -> usize {
        self.externref_returns_count
    }
}

/// An index of an entity.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum EntityIndex {
    /// Function index.
    Function(FuncIndex),
    /// Table index.
    Table(TableIndex),
    /// Memory index.
    Memory(MemoryIndex),
    /// Global index.
    Global(GlobalIndex),
}

impl EntityIndex {
    pub fn unwrap_func(&self) -> FuncIndex {
        match self {
            EntityIndex::Function(f) => *f,
            eidx => panic!("not a func, but {eidx:?}"),
        }
    }
}

impl From<FuncIndex> for EntityIndex {
    fn from(value: FuncIndex) -> Self {
        Self::Function(value)
    }
}

impl From<TableIndex> for EntityIndex {
    fn from(value: TableIndex) -> Self {
        Self::Table(value)
    }
}

impl From<MemoryIndex> for EntityIndex {
    fn from(value: MemoryIndex) -> Self {
        Self::Memory(value)
    }
}

impl From<GlobalIndex> for EntityIndex {
    fn from(value: GlobalIndex) -> Self {
        Self::Global(value)
    }
}

/// A type of an item in a wasm module where an item is typically something that
/// can be exported.
#[derive(Clone, Debug)]
pub enum EntityType {
    /// A global variable with the specified content type
    Global(Global),
    /// A linear memory with the specified limits
    Memory(Memory),
    /// A table with the specified element type and limits
    Table(Table),
    /// A function type where the index points to the type section and records a
    /// function signature.
    Function(SignatureIndex),
}

impl EntityType {
    /// Assert that this entity is a global
    pub fn unwrap_global(&self) -> &Global {
        match self {
            EntityType::Global(g) => g,
            _ => panic!("not a global"),
        }
    }

    /// Assert that this entity is a memory
    pub fn unwrap_memory(&self) -> &Memory {
        match self {
            EntityType::Memory(g) => g,
            _ => panic!("not a memory"),
        }
    }

    /// Assert that this entity is a table
    pub fn unwrap_table(&self) -> &Table {
        match self {
            EntityType::Table(g) => g,
            _ => panic!("not a table"),
        }
    }

    /// Assert that this entity is a function
    pub fn unwrap_func(&self) -> SignatureIndex {
        match self {
            EntityType::Function(g) => *g,
            _ => panic!("not a func"),
        }
    }
}

/// A WebAssembly global.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Global {
    /// The Wasm type of the value stored in the global.
    pub ty: WasmType,
    /// A flag indicating whether the value may change at runtime.
    pub mutability: bool,
}

/// Globals are initialized via the `const` operators or by referring to another import.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum GlobalInit {
    /// An `i32.const`.
    I32Const(i32),
    /// An `i64.const`.
    I64Const(i64),
    /// An `f32.const`.
    F32Const(u32),
    /// An `f64.const`.
    F64Const(u64),
    /// A `vconst`.
    V128Const(u128),
    /// A `global.get` of another global.
    GetGlobal(GlobalIndex),
}

impl GlobalInit {
    /// Serialize the initializer constant expression into bytes (little-endian order).
    pub fn to_imm(
        self,
        module: &Module,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Immediate> {
        Ok(match self {
            GlobalInit::I32Const(x) => x.into(),
            GlobalInit::I64Const(x) => x.into(),
            GlobalInit::F32Const(x) => x.into(),
            GlobalInit::F64Const(x) => x.into(),
            GlobalInit::V128Const(x) => x.into(),
            GlobalInit::GetGlobal(global_idx) => {
                let global_init = module.try_global_initializer(global_idx, diagnostics)?;
                global_init.to_imm(module, diagnostics)?
            }
        })
    }

    pub fn as_i32(&self, module: &Module, diagnostics: &DiagnosticsHandler) -> WasmResult<i32> {
        Ok(match self {
            GlobalInit::I32Const(x) => *x,
            GlobalInit::GetGlobal(global_idx) => {
                let global_init = module.try_global_initializer(*global_idx, diagnostics)?;
                global_init.as_i32(module, diagnostics)?
            }
            g => {
                unsupported_diag!(diagnostics, "Expected global init to be i32, got: {:?}", g);
            }
        })
    }
}

/// WebAssembly table.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Table {
    /// The table elements' Wasm type.
    pub wasm_ty: WasmRefType,
    /// The minimum number of elements in the table.
    pub minimum: u32,
    /// The maximum number of elements in the table.
    pub maximum: Option<u32>,
}

/// WebAssembly linear memory.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Memory {
    /// The minimum number of pages in the memory.
    pub minimum: u64,
    /// The maximum number of pages in the memory.
    pub maximum: Option<u64>,
    /// Is this memory imported in the current [Module]
    pub imported: bool,
}

impl From<wasmparser::MemoryType> for Memory {
    fn from(ty: wasmparser::MemoryType) -> Memory {
        Memory {
            minimum: ty.initial,
            maximum: ty.maximum,
            imported: false,
        }
    }
}

/// WebAssembly event.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Tag {
    /// The event signature type.
    pub ty: TypeIndex,
}

impl From<wasmparser::TagType> for Tag {
    fn from(ty: wasmparser::TagType) -> Tag {
        match ty.kind {
            wasmparser::TagKind::Exception => Tag {
                ty: TypeIndex::from_u32(ty.func_type_idx),
            },
        }
    }
}
/// Offset of a data segment inside a linear memory.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum DataSegmentOffset {
    /// An `i32.const` offset.
    I32Const(i32),
    /// An offset as a `global.get` of another global.
    GetGlobal(GlobalIndex),
}

impl DataSegmentOffset {
    /// Returns the offset as a i32, resolving the global if necessary.
    pub fn as_i32(&self, module: &Module, diagnostics: &DiagnosticsHandler) -> WasmResult<i32> {
        Ok(match self {
            DataSegmentOffset::I32Const(x) => *x,
            DataSegmentOffset::GetGlobal(global_idx) => {
                let global_init = &module.try_global_initializer(*global_idx, diagnostics)?;
                match global_init.as_i32(module, diagnostics) {
                    Err(_) => {
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message(format!(
                                "Failed to get data segment offset from global init {:?} with \
                                 global index {global_idx:?}",
                                global_init,
                            ))
                            .into_report());
                    }
                    Ok(v) => v,
                }
            }
        })
    }
}

/// A WebAssembly data segment.
/// https://www.w3.org/TR/wasm-core-1/#data-segments%E2%91%A0
#[derive(Debug)]
pub struct DataSegment<'a> {
    /// The offset of the data segment inside the linear memory.
    pub offset: DataSegmentOffset,
    /// The initialization data.
    pub data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BlockType {
    pub params: Vec<hir::Type>,
    pub results: Vec<hir::Type>,
}

impl BlockType {
    pub fn from_wasm(
        block_ty: &wasmparser::BlockType,
        mod_types: &ModuleTypesBuilder,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Self> {
        Ok(match block_ty {
            wasmparser::BlockType::Empty => Self::default(),
            wasmparser::BlockType::Type(ty) => Self {
                params: vec![],
                results: vec![ir_type(convert_valtype(*ty), diagnostics)?],
            },
            wasmparser::BlockType::FuncType(ty_index) => {
                let func_type = &mod_types[SignatureIndex::from_u32(*ty_index)];
                let params = func_type
                    .params()
                    .iter()
                    .map(|t| ir_type(*t, diagnostics))
                    .collect::<WasmResult<Vec<hir::Type>>>()?;
                let results = func_type
                    .returns()
                    .iter()
                    .map(|t| ir_type(*t, diagnostics))
                    .collect::<WasmResult<Vec<hir::Type>>>()?;
                Self { params, results }
            }
        })
    }
}

/// Note that accesing this type is primarily done through the `Index`
/// implementations for this type.
#[derive(Default)]
pub struct ModuleTypes {
    wasm_signatures: PrimaryMap<SignatureIndex, WasmFuncType>,
}

impl ModuleTypes {
    /// Returns an iterator over all the wasm function signatures found within
    /// this module.
    pub fn wasm_signatures(&self) -> impl Iterator<Item = (SignatureIndex, &WasmFuncType)> {
        self.wasm_signatures.iter()
    }
}

impl Index<SignatureIndex> for ModuleTypes {
    type Output = WasmFuncType;

    fn index(&self, sig: SignatureIndex) -> &WasmFuncType {
        &self.wasm_signatures[sig]
    }
}

/// A builder for [`ModuleTypes`].
#[derive(Default)]
pub struct ModuleTypesBuilder {
    types: ModuleTypes,
    interned_func_types: FxHashMap<WasmFuncType, SignatureIndex>,
    wasmparser_to_our: FxHashMap<CoreTypeId, SignatureIndex>,
}

impl ModuleTypesBuilder {
    /// Reserves space for `amt` more type signatures.
    pub fn reserve_wasm_signatures(&mut self, amt: usize) {
        self.types.wasm_signatures.reserve(amt);
    }

    /// Interns the `sig` specified and returns a unique `SignatureIndex` that
    /// can be looked up within [`ModuleTypes`] to recover the [`WasmFuncType`]
    /// at runtime.
    pub fn wasm_func_type(&mut self, id: CoreTypeId, sig: WasmFuncType) -> SignatureIndex {
        let sig = self.intern_func_type(sig);
        self.wasmparser_to_our.insert(id, sig);
        sig
    }

    fn intern_func_type(&mut self, sig: WasmFuncType) -> SignatureIndex {
        if let Some(idx) = self.interned_func_types.get(&sig) {
            return *idx;
        }

        let idx = self.types.wasm_signatures.push(sig.clone());
        self.interned_func_types.insert(sig, idx);
        idx
    }

    /// Returns the result [`ModuleTypes`] of this builder.
    pub fn finish(self) -> ModuleTypes {
        self.types
    }

    /// Returns an iterator over all the wasm function signatures found within
    /// this module.
    pub fn wasm_signatures(&self) -> impl Iterator<Item = (SignatureIndex, &WasmFuncType)> {
        self.types.wasm_signatures()
    }
}

// Forward the indexing impl to the internal `ModuleTypes`
impl<T> Index<T> for ModuleTypesBuilder
where
    ModuleTypes: Index<T>,
{
    type Output = <ModuleTypes as Index<T>>::Output;

    fn index(&self, sig: T) -> &Self::Output {
        &self.types[sig]
    }
}

/// Converts a Wasm function type into a Miden IR function type
pub fn ir_func_type(
    ty: &WasmFuncType,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<hir::FunctionType> {
    let params = ty
        .params()
        .iter()
        .map(|t| ir_type(*t, diagnostics))
        .collect::<WasmResult<Vec<hir::Type>>>()?;
    let results = ty
        .returns()
        .iter()
        .map(|t| ir_type(*t, diagnostics))
        .collect::<WasmResult<Vec<hir::Type>>>()?;
    Ok(hir::FunctionType {
        abi: Abi::Canonical,
        results,
        params,
    })
}

/// Converts a Wasm type into a Miden IR type
pub fn ir_type(ty: WasmType, diagnostics: &DiagnosticsHandler) -> WasmResult<hir::Type> {
    Ok(match ty {
        WasmType::I32 => hir::Type::I32,
        WasmType::I64 => hir::Type::I64,
        WasmType::F32 => hir::Type::Felt,
        ty @ (WasmType::F64 | WasmType::V128 | WasmType::Ref(_)) => {
            unsupported_diag!(diagnostics, "wasm error: unsupported type '{}'", ty)
        }
    })
}

/// Makes an IR function signature from a Wasm function type
pub fn ir_func_sig(
    func_type: &hir::FunctionType,
    call_conv: CallConv,
    visibility: Visibility,
) -> Signature {
    Signature {
        params: func_type.params.iter().map(|ty| AbiParam::new(ty.clone())).collect(),
        results: func_type.results.iter().map(|ty| AbiParam::new(ty.clone())).collect(),
        cc: call_conv,
        visibility,
    }
}

/// Converts a wasmparser table type
pub fn convert_global_type(ty: &wasmparser::GlobalType) -> Global {
    Global {
        ty: convert_valtype(ty.content_type),
        mutability: ty.mutable,
    }
}

/// Converts a wasmparser table type
pub fn convert_table_type(ty: &wasmparser::TableType) -> Table {
    assert!(!ty.table64, "64-bit tables are not supported");

    Table {
        wasm_ty: convert_ref_type(ty.element_type),
        minimum: ty.initial as u32,
        maximum: ty.maximum.map(|n| n as u32),
    }
}

/// Converts a wasmparser function type
pub fn convert_func_type(ty: &wasmparser::FuncType) -> WasmFuncType {
    let params = ty.params().iter().map(|t| convert_valtype(*t)).collect();
    let results = ty.results().iter().map(|t| convert_valtype(*t)).collect();
    WasmFuncType::new(params, results)
}

/// Converts a wasmparser value type
pub fn convert_valtype(ty: wasmparser::ValType) -> WasmType {
    match ty {
        wasmparser::ValType::I32 => WasmType::I32,
        wasmparser::ValType::I64 => WasmType::I64,
        wasmparser::ValType::F32 => WasmType::F32,
        wasmparser::ValType::F64 => WasmType::F64,
        wasmparser::ValType::V128 => WasmType::V128,
        wasmparser::ValType::Ref(t) => WasmType::Ref(convert_ref_type(t)),
    }
}

/// Converts a wasmparser reference type
pub fn convert_ref_type(ty: wasmparser::RefType) -> WasmRefType {
    WasmRefType {
        nullable: ty.is_nullable(),
        heap_type: convert_heap_type(ty.heap_type()),
    }
}

/// Converts a wasmparser heap type
pub fn convert_heap_type(ty: wasmparser::HeapType) -> WasmHeapType {
    use wasmparser::AbstractHeapType;
    match ty {
        wasmparser::HeapType::Abstract { ty, shared: _ } => match ty {
            AbstractHeapType::Func => WasmHeapType::Func,
            AbstractHeapType::Extern => WasmHeapType::Extern,
            ty => unimplemented!("unsupported abstract heap type {ty:?}"),
        },
        wasmparser::HeapType::Concrete(_) => {
            unimplemented!("user-defined types are not supported yet")
        }
    }
}
