use alloc::{format, vec::Vec};

use crate::{
    AttrPrinter, Type,
    derive::DialectAttribute,
    dialects::builtin::BuiltinDialect,
    formatter::{Document, PrettyPrint, const_text, text},
    interner::Symbol,
    print::AsmPrinter,
};

/// Represents the compilation unit associated with debug information.
///
/// The fields in this struct are intentionally aligned with the subset of
/// DWARF metadata we currently care about when tracking variable locations.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct DICompileUnit {
    pub language: Symbol,
    pub file: Symbol,
    pub directory: Option<Symbol>,
    pub producer: Option<Symbol>,
    pub optimized: bool,
}

impl Default for DICompileUnit {
    fn default() -> Self {
        Self {
            language: crate::interner::symbols::Empty,
            file: crate::interner::symbols::Empty,
            directory: None,
            producer: None,
            optimized: false,
        }
    }
}

impl DICompileUnit {
    pub fn new(language: Symbol, file: Symbol) -> Self {
        Self {
            language,
            file,
            directory: None,
            producer: None,
            optimized: false,
        }
    }
}

impl AttrPrinter for DICompileUnitAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        *printer += self.value.render();
    }
}

impl PrettyPrint for DICompileUnit {
    fn render(&self) -> Document {
        let mut doc = const_text("di.compile_unit(")
            + text(format!("language = {}", self.language.as_str()))
            + const_text(", file = ")
            + text(self.file.as_str());

        if let Some(directory) = self.directory {
            doc = doc + const_text(", directory = ") + text(directory.as_str());
        }
        if let Some(producer) = self.producer {
            doc = doc + const_text(", producer = ") + text(producer.as_str());
        }
        if self.optimized {
            doc += const_text(", optimized");
        }

        doc + const_text(")")
    }
}

/// Represents a subprogram (function) scope for debug information.
/// The compile unit is not embedded but typically stored separately on the module.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct DISubprogram {
    pub name: Symbol,
    pub linkage_name: Option<Symbol>,
    pub file: Symbol,
    pub line: u32,
    pub column: Option<u32>,
    pub is_definition: bool,
    pub is_local: bool,
}

impl Default for DISubprogram {
    fn default() -> Self {
        Self {
            name: crate::interner::symbols::Empty,
            linkage_name: None,
            file: crate::interner::symbols::Empty,
            line: 0,
            column: None,
            is_definition: false,
            is_local: false,
        }
    }
}

impl DISubprogram {
    pub fn new(name: Symbol, file: Symbol, line: u32, column: Option<u32>) -> Self {
        Self {
            name,
            linkage_name: None,
            file,
            line,
            column,
            is_definition: true,
            is_local: false,
        }
    }
}

impl AttrPrinter for DISubprogramAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        *printer += self.value.render();
    }
}

impl PrettyPrint for DISubprogram {
    fn render(&self) -> Document {
        let mut doc = const_text("di.subprogram(")
            + text(format!("name = {}", self.name.as_str()))
            + const_text(", file = ")
            + text(self.file.as_str())
            + const_text(", line = ")
            + text(format!("{}", self.line));

        if let Some(column) = self.column {
            doc = doc + const_text(", column = ") + text(format!("{}", column));
        }
        if let Some(linkage) = self.linkage_name {
            doc = doc + const_text(", linkage = ") + text(linkage.as_str());
        }
        if self.is_definition {
            doc += const_text(", definition");
        }
        if self.is_local {
            doc += const_text(", local");
        }

        doc + const_text(")")
    }
}

/// Represents a local variable debug record.
/// The scope (DISubprogram) is not embedded but instead stored on the containing function.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct DILocalVariable {
    pub name: Symbol,
    pub arg_index: Option<u32>,
    pub file: Symbol,
    pub line: u32,
    pub column: Option<u32>,
    pub ty: Option<Type>,
}

impl Default for DILocalVariable {
    fn default() -> Self {
        Self {
            name: crate::interner::symbols::Empty,
            arg_index: None,
            file: crate::interner::symbols::Empty,
            line: 0,
            column: None,
            ty: None,
        }
    }
}

impl DILocalVariable {
    pub fn new(name: Symbol, file: Symbol, line: u32, column: Option<u32>) -> Self {
        Self {
            name,
            arg_index: None,
            file,
            line,
            column,
            ty: None,
        }
    }
}

impl AttrPrinter for DILocalVariableAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        *printer += self.value.render();
    }
}

impl PrettyPrint for DILocalVariable {
    fn render(&self) -> Document {
        let mut doc = const_text("di.local_variable(")
            + text(format!("name = {}", self.name.as_str()))
            + const_text(", file = ")
            + text(self.file.as_str())
            + const_text(", line = ")
            + text(format!("{}", self.line));

        if let Some(column) = self.column {
            doc = doc + const_text(", column = ") + text(format!("{}", column));
        }
        if let Some(arg_index) = self.arg_index {
            doc = doc + const_text(", arg = ") + text(format!("{}", arg_index));
        }
        if let Some(ty) = &self.ty {
            doc = doc + const_text(", ty = ") + ty.render();
        }

        doc + const_text(")")
    }
}

/// Represents DWARF expression operations for describing variable locations
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DIExpressionOp {
    /// DW_OP_WASM_location 0x00 - Variable is in a WebAssembly local
    WasmLocal(u32),
    /// DW_OP_WASM_location 0x01 - Variable is in a WebAssembly global
    WasmGlobal(u32),
    /// DW_OP_WASM_location 0x02 - Variable is on the WebAssembly operand stack
    WasmStack(u32),
    /// DW_OP_constu - Unsigned constant value
    ConstU64(u64),
    /// DW_OP_consts - Signed constant value
    ConstS64(i64),
    /// DW_OP_plus_uconst - Add unsigned constant to top of stack
    PlusUConst(u64),
    /// DW_OP_minus - Subtract top two stack values
    Minus,
    /// DW_OP_plus - Add top two stack values
    Plus,
    /// DW_OP_deref - Dereference the address at top of stack
    Deref,
    /// DW_OP_stack_value - The value on the stack is the value of the variable
    StackValue,
    /// DW_OP_piece - Describes a piece of a variable
    Piece(u64),
    /// DW_OP_bit_piece - Describes a piece of a variable in bits
    BitPiece { size: u64, offset: u64 },
    /// DW_OP_fbreg - Frame base register + offset.
    /// The variable is in WASM linear memory at `value_of(global[global_index]) + byte_offset`.
    FrameBase { global_index: u32, byte_offset: i64 },
    /// Placeholder for unsupported operations
    Unsupported(Symbol),
}

/// Represents a DWARF expression that describes how to compute or locate a variable's value
#[derive(DialectAttribute, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct DIExpression {
    pub operations: Vec<DIExpressionOp>,
}

impl DIExpression {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    pub fn with_ops(operations: Vec<DIExpressionOp>) -> Self {
        Self { operations }
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl AttrPrinter for DIExpressionAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        *printer += self.value.render();
    }
}

impl PrettyPrint for DIExpression {
    fn render(&self) -> Document {
        if self.operations.is_empty() {
            return const_text("di.expression()");
        }

        let mut doc = const_text("di.expression(");
        for (i, op) in self.operations.iter().enumerate() {
            if i > 0 {
                doc += const_text(", ");
            }
            doc += match op {
                DIExpressionOp::WasmLocal(idx) => text(format!("DW_OP_WASM_local {}", idx)),
                DIExpressionOp::WasmGlobal(idx) => text(format!("DW_OP_WASM_global {}", idx)),
                DIExpressionOp::WasmStack(idx) => text(format!("DW_OP_WASM_stack {}", idx)),
                DIExpressionOp::ConstU64(val) => text(format!("DW_OP_constu {}", val)),
                DIExpressionOp::ConstS64(val) => text(format!("DW_OP_consts {}", val)),
                DIExpressionOp::PlusUConst(val) => text(format!("DW_OP_plus_uconst {}", val)),
                DIExpressionOp::Minus => const_text("DW_OP_minus"),
                DIExpressionOp::Plus => const_text("DW_OP_plus"),
                DIExpressionOp::Deref => const_text("DW_OP_deref"),
                DIExpressionOp::StackValue => const_text("DW_OP_stack_value"),
                DIExpressionOp::Piece(size) => text(format!("DW_OP_piece {}", size)),
                DIExpressionOp::BitPiece { size, offset } => {
                    text(format!("DW_OP_bit_piece {} {}", size, offset))
                }
                DIExpressionOp::FrameBase {
                    global_index,
                    byte_offset,
                } => text(format!("DW_OP_fbreg global[{}]{:+}", global_index, byte_offset)),
                DIExpressionOp::Unsupported(name) => text(name.as_str()),
            };
        }
        doc + const_text(")")
    }
}
