use core::fmt;

use super::*;
use crate::{
    formatter::{Document, PrettyPrint},
    FunctionIdent, Ident, Symbol,
};

pub struct DisplayInlineAsm<'a> {
    function: Option<FunctionIdent>,
    asm: &'a InlineAsm,
    dfg: &'a DataFlowGraph,
}
impl<'a> DisplayInlineAsm<'a> {
    pub fn new(
        function: Option<FunctionIdent>,
        asm: &'a InlineAsm,
        dfg: &'a DataFlowGraph,
    ) -> Self {
        Self { function, asm, dfg }
    }
}
impl PrettyPrint for DisplayInlineAsm<'_> {
    fn render(&self) -> Document {
        use crate::formatter::*;

        let params = self
            .asm
            .args
            .as_slice(&self.dfg.value_lists)
            .iter()
            .copied()
            .map(display)
            .reduce(|acc, p| acc + const_text(" ") + p);

        let body = DisplayMasmBlock {
            function: self.function,
            imports: None,
            blocks: &self.asm.blocks,
            block: self.asm.body,
        };
        let body =
            const_text("(") + const_text("masm") + indent(4, body.render()) + const_text(")");

        const_text("(")
            + const_text("asm")
            + const_text(" ")
            + body
            + params
                .map(|params| const_text(" ") + const_text("(") + params + const_text(")"))
                .unwrap_or_default()
            + const_text(")")
    }
}
impl fmt::Display for DisplayInlineAsm<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

pub struct DisplayMasmBlock<'a> {
    function: Option<FunctionIdent>,
    imports: Option<&'a ModuleImportInfo>,
    blocks: &'a PrimaryMap<MasmBlockId, MasmBlock>,
    block: MasmBlockId,
}
impl<'a> DisplayMasmBlock<'a> {
    pub fn new(
        function: Option<FunctionIdent>,
        imports: Option<&'a ModuleImportInfo>,
        blocks: &'a PrimaryMap<MasmBlockId, MasmBlock>,
        block: MasmBlockId,
    ) -> Self {
        Self {
            function,
            imports,
            blocks,
            block,
        }
    }
}
impl PrettyPrint for DisplayMasmBlock<'_> {
    fn render(&self) -> Document {
        use crate::formatter::*;

        let block = &self.blocks[self.block];
        let multiline = block
            .ops
            .iter()
            .map(|op| {
                let op = DisplayOp {
                    function: self.function,
                    imports: self.imports,
                    blocks: self.blocks,
                    op,
                };
                op.render()
            })
            .reduce(|acc, e| acc + nl() + e)
            .unwrap_or_default();

        if block.ops.len() < 5 && !block.ops.iter().any(|op| op.has_regions()) {
            let singleline = block
                .ops
                .iter()
                .map(|op| {
                    let op = DisplayOp {
                        function: self.function,
                        imports: self.imports,
                        blocks: self.blocks,
                        op,
                    };
                    op.render()
                })
                .reduce(|acc, e| acc + const_text(" ") + e)
                .unwrap_or_default();
            singleline | multiline
        } else {
            multiline
        }
    }
}
impl fmt::Display for DisplayMasmBlock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

struct DisplayOp<'a> {
    function: Option<FunctionIdent>,
    imports: Option<&'a ModuleImportInfo>,
    blocks: &'a PrimaryMap<MasmBlockId, MasmBlock>,
    op: &'a MasmOp,
}
impl DisplayOp<'_> {
    #[inline(always)]
    pub fn is_local_module(&self, id: &Ident) -> bool {
        match self.function {
            Some(function) => &function.module == id,
            None => self.imports.map(|imports| !imports.is_import(id)).unwrap_or(false),
        }
    }

    // TODO(pauls): Remove when we no longer need to deal with referencing aliased identifiers
    #[allow(unused)]
    pub fn get_module_alias(&self, module: Ident) -> Symbol {
        self.imports
            .and_then(|imports| imports.alias(&module))
            .unwrap_or(module)
            .as_symbol()
    }
}
impl PrettyPrint for DisplayOp<'_> {
    fn render(&self) -> Document {
        use crate::formatter::*;

        match self.op {
            MasmOp::Push(imm) => const_text("push") + const_text(".") + display(*imm),
            MasmOp::Push2([a, b]) => {
                const_text("push") + const_text(".") + display(*a) + const_text(".") + display(*b)
            }
            MasmOp::Pushw(word) => {
                const_text("push")
                    + const_text(".")
                    + display(word[0])
                    + const_text(".")
                    + display(word[1])
                    + const_text(".")
                    + display(word[2])
                    + const_text(".")
                    + display(word[3])
            }
            MasmOp::PushU8(imm) => const_text("push") + const_text(".") + display(*imm),
            MasmOp::PushU16(imm) => const_text("push") + const_text(".") + display(*imm),
            MasmOp::PushU32(imm) => const_text("push") + const_text(".") + display(*imm),
            op @ (MasmOp::Dup(idx)
            | MasmOp::Dupw(idx)
            | MasmOp::Swap(idx)
            | MasmOp::Swapw(idx)
            | MasmOp::Movup(idx)
            | MasmOp::Movupw(idx)
            | MasmOp::Movdn(idx)
            | MasmOp::Movdnw(idx)) => text(format!("{op}")) + const_text(".") + display(*idx),
            op @ (MasmOp::LocAddr(id)
            | MasmOp::LocStore(id)
            | MasmOp::LocStorew(id)
            | MasmOp::LocLoad(id)
            | MasmOp::LocLoadw(id)) => {
                text(format!("{op}")) + const_text(".") + display(id.as_usize())
            }
            op @ (MasmOp::MemLoadImm(addr)
            | MasmOp::MemLoadwImm(addr)
            | MasmOp::MemStoreImm(addr)
            | MasmOp::MemStorewImm(addr)) => {
                text(format!("{op}"))
                    + const_text(".")
                    + text(format!("{:#x}", DisplayHex(addr.to_be_bytes().as_slice())))
            }
            MasmOp::AdvPush(n) => const_text("adv_push") + const_text(".") + display(*n),
            MasmOp::If(then_blk, else_blk) => {
                let then_body = DisplayMasmBlock {
                    function: self.function,
                    imports: self.imports,
                    blocks: self.blocks,
                    block: *then_blk,
                }
                .render();
                let else_body = DisplayMasmBlock {
                    function: self.function,
                    imports: self.imports,
                    blocks: self.blocks,
                    block: *else_blk,
                }
                .render();
                const_text("if.true")
                    + indent(4, nl() + then_body)
                    + nl()
                    + const_text("else")
                    + indent(4, nl() + else_body)
                    + nl()
                    + const_text("end")
            }
            MasmOp::While(blk) => {
                let body = DisplayMasmBlock {
                    function: self.function,
                    imports: self.imports,
                    blocks: self.blocks,
                    block: *blk,
                }
                .render();
                const_text("while.true") + indent(4, nl() + body) + nl() + const_text("end")
            }
            MasmOp::Repeat(n, blk) => {
                let body = DisplayMasmBlock {
                    function: self.function,
                    imports: self.imports,
                    blocks: self.blocks,
                    block: *blk,
                }
                .render();

                const_text("repeat")
                    + const_text(".")
                    + display(*n)
                    + indent(4, nl() + body)
                    + nl()
                    + const_text("end")
            }
            op @ (MasmOp::Exec(id)
            | MasmOp::Call(id)
            | MasmOp::Syscall(id)
            | MasmOp::ProcRef(id)) => {
                let FunctionIdent { module, function } = id;
                let function = if miden_assembly::ast::Ident::validate(function.as_str()).is_ok() {
                    text(function.as_str())
                } else {
                    text(format!("\"{}\"", function.as_str()))
                };
                if self.is_local_module(module) {
                    text(format!("{op}")) + const_text(".") + function
                } else {
                    text(format!("{op}"))
                        + const_text(".::")
                        + display(module.as_str())
                        + const_text("::")
                        + function
                }
            }
            op @ (MasmOp::AndImm(imm) | MasmOp::OrImm(imm) | MasmOp::XorImm(imm)) => {
                text(format!("{op}")) + const_text(".") + display(*imm)
            }
            MasmOp::ExpImm(imm) => const_text("exp") + const_text(".") + display(*imm),
            op @ (MasmOp::AddImm(imm)
            | MasmOp::SubImm(imm)
            | MasmOp::MulImm(imm)
            | MasmOp::DivImm(imm)
            | MasmOp::EqImm(imm)
            | MasmOp::NeqImm(imm)
            | MasmOp::GtImm(imm)
            | MasmOp::GteImm(imm)
            | MasmOp::LtImm(imm)
            | MasmOp::LteImm(imm)) => text(format!("{op}")) + const_text(".") + display(*imm),
            op @ (MasmOp::U32OverflowingAddImm(imm)
            | MasmOp::U32WrappingAddImm(imm)
            | MasmOp::U32OverflowingSubImm(imm)
            | MasmOp::U32WrappingSubImm(imm)
            | MasmOp::U32OverflowingMulImm(imm)
            | MasmOp::U32WrappingMulImm(imm)
            | MasmOp::U32DivImm(imm)
            | MasmOp::U32ModImm(imm)
            | MasmOp::U32DivModImm(imm)
            | MasmOp::U32ShlImm(imm)
            | MasmOp::U32ShrImm(imm)
            | MasmOp::U32RotlImm(imm)
            | MasmOp::U32RotrImm(imm)) => text(format!("{op}")) + const_text(".") + display(*imm),
            op @ (MasmOp::AdvInjectPushMapValImm(offset)
            | MasmOp::AdvInjectPushMapValNImm(offset)) => {
                text(format!("{op}")) + const_text(".") + display(*offset)
            }
            op @ MasmOp::AdvInjectInsertHdwordImm(domain) => {
                text(format!("{op}")) + const_text(".") + display(*domain)
            }
            op @ MasmOp::DebugStackN(n) => text(format!("{op}")) + const_text(".") + display(*n),
            op @ MasmOp::DebugMemoryAt(start) => {
                text(format!("{op}")) + const_text(".") + display(*start)
            }
            op @ MasmOp::DebugMemoryRange(start, end) => {
                text(format!("{op}"))
                    + const_text(".")
                    + display(*start)
                    + const_text(".")
                    + display(*end)
            }
            op @ MasmOp::DebugFrameAt(start) => {
                text(format!("{op}")) + const_text(".") + display(*start)
            }
            op @ MasmOp::DebugFrameRange(start, end) => {
                text(format!("{op}"))
                    + const_text(".")
                    + display(*start)
                    + const_text(".")
                    + display(*end)
            }
            op => text(format!("{op}")),
        }
    }
}
impl fmt::Display for DisplayOp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
