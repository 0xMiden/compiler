use std::{collections::BTreeSet, fmt};

use cranelift_entity::PrimaryMap;
use miden_assembly::ast;
use midenc_hir::{diagnostics::Span, formatter::PrettyPrint, FunctionIdent, Ident};
use smallvec::smallvec;

use super::*;
use crate::InstructionPointer;

/// This struct represents a region of code in Miden Assembly.
///
/// A region is a tree of blocks with isolated scope. In many
/// ways a [Region] is basically a [Function], just without any
/// identity. Additionally, a [Region] does not have local variables,
/// those must be provided by a parent [Function].
///
/// In short, this represents both the body of a function, and the
/// body of a `begin` block in Miden Assembly.
#[derive(Debug, Clone)]
pub struct Region {
    pub body: BlockId,
    pub blocks: PrimaryMap<BlockId, Block>,
}
impl Default for Region {
    fn default() -> Self {
        let mut blocks = PrimaryMap::<BlockId, Block>::default();
        let id = blocks.next_key();
        let body = blocks.push(Block {
            id,
            ops: smallvec![],
        });
        Self { body, blocks }
    }
}
impl Region {
    /// Get the [BlockId] for the block which forms the body of this region
    #[inline(always)]
    pub const fn id(&self) -> BlockId {
        self.body
    }

    /// Get a reference to a [Block] by [BlockId]
    #[inline]
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id]
    }

    /// Get a mutable reference to a [Block] by [BlockId]
    #[inline]
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id]
    }

    /// Get the instruction under `ip`, if valid
    pub fn get(&self, ip: InstructionPointer) -> Option<Span<Op>> {
        self.blocks[ip.block].ops.get(ip.index).copied()
    }

    /// Allocate a new code block in this region
    pub fn create_block(&mut self) -> BlockId {
        let id = self.blocks.next_key();
        self.blocks.push(Block {
            id,
            ops: smallvec![],
        });
        id
    }

    /// Render the code in this region as Miden Assembly, at the specified indentation level (in
    /// units of 4 spaces)
    pub fn display<'a, 'b: 'a>(
        &'b self,
        function: Option<FunctionIdent>,
        imports: &'b ModuleImportInfo,
    ) -> DisplayRegion<'a> {
        DisplayRegion {
            region: self,
            function,
            imports,
        }
    }

    /// Convert this [Region] to a [miden_assembly::ast::Block] using the provided
    /// local/external function maps to handle calls present in the body of the region.
    pub fn to_block(
        &self,
        imports: &ModuleImportInfo,
        locals: &BTreeSet<FunctionIdent>,
    ) -> ast::Block {
        emit_block(self.body, &self.blocks, imports, locals)
    }

    /// Create a [Region] from a [miden_assembly::ast::CodeBody] and the set of imports
    /// and local procedures which will be used to map references to procedures to their
    /// fully-qualified names.
    pub fn from_block(current_module: Ident, code: &ast::Block) -> Self {
        let mut region = Self::default();

        let body = region.body;
        import_block(current_module, &mut region, body, code);

        region
    }
}
impl core::ops::Index<InstructionPointer> for Region {
    type Output = Op;

    #[inline]
    fn index(&self, ip: InstructionPointer) -> &Self::Output {
        &self.blocks[ip.block].ops[ip.index]
    }
}

#[doc(hidden)]
pub struct DisplayRegion<'a> {
    region: &'a Region,
    function: Option<FunctionIdent>,
    imports: &'a ModuleImportInfo,
}
impl midenc_hir::formatter::PrettyPrint for DisplayRegion<'_> {
    fn render(&self) -> midenc_hir::formatter::Document {
        use midenc_hir::DisplayMasmBlock;

        let block = DisplayMasmBlock::new(
            self.function,
            Some(self.imports),
            &self.region.blocks,
            self.region.body,
        );

        block.render()
    }
}
impl fmt::Display for DisplayRegion<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

/// Import code from a [miden_assembly::ast::Block] into the specified [Block] in `region`.
fn import_block(
    current_module: Ident,
    region: &mut Region,
    current_block_id: BlockId,
    block: &ast::Block,
) {
    for op in block.iter() {
        match op {
            ast::Op::Inst(ix) => {
                let span = ix.span();
                let current_block = region.block_mut(current_block_id);
                let ops = Op::from_masm(current_module, (**ix).clone());
                current_block.extend(ops.into_iter().map(|op| Span::new(span, op)));
            }
            ast::Op::If {
                span,
                ref then_blk,
                ref else_blk,
                ..
            } => {
                let then_blk_id = region.create_block();
                let else_blk_id = region.create_block();
                import_block(current_module, region, then_blk_id, then_blk);
                import_block(current_module, region, else_blk_id, else_blk);
                region.block_mut(current_block_id).push(Op::If(then_blk_id, else_blk_id), *span);
            }
            ast::Op::Repeat {
                span,
                count,
                ref body,
                ..
            } => {
                let body_blk = region.create_block();
                import_block(current_module, region, body_blk, body);
                let count = u16::try_from(*count).unwrap_or_else(|_| {
                    panic!("invalid repeat count: expected {count} to be less than 255")
                });
                region.block_mut(current_block_id).push(Op::Repeat(count, body_blk), *span);
            }
            ast::Op::While { span, ref body, .. } => {
                let body_blk = region.create_block();
                import_block(current_module, region, body_blk, body);
                region.block_mut(current_block_id).push(Op::While(body_blk), *span);
            }
        }
    }
}

/// Emit a [miden_assembly::ast::CodeBlock] by recursively visiting a tree of blocks
/// starting with `block_id`, using the provided imports and local/external procedure maps.
#[allow(clippy::only_used_in_recursion)]
fn emit_block(
    block_id: BlockId,
    blocks: &PrimaryMap<BlockId, Block>,
    imports: &ModuleImportInfo,
    locals: &BTreeSet<FunctionIdent>,
) -> ast::Block {
    let current_block = &blocks[block_id];
    let mut ops = Vec::with_capacity(current_block.ops.len());
    for op in current_block.ops.iter().copied() {
        let span = op.span();
        match op.into_inner() {
            Op::If(then_blk, else_blk) => {
                let then_blk = emit_block(then_blk, blocks, imports, locals);
                let else_blk = emit_block(else_blk, blocks, imports, locals);
                ops.push(ast::Op::If {
                    span,
                    then_blk,
                    else_blk,
                });
            }
            Op::While(blk) => {
                let body = emit_block(blk, blocks, imports, locals);
                ops.push(ast::Op::While { span, body });
            }
            Op::Repeat(n, blk) => {
                let body = emit_block(blk, blocks, imports, locals);
                ops.push(ast::Op::Repeat {
                    span,
                    count: n as u32,
                    body,
                });
            }
            op => {
                ops.extend(
                    op.into_masm(imports, locals)
                        .into_iter()
                        .map(|inst| ast::Op::Inst(Span::new(span, inst))),
                );
            }
        }
    }

    ast::Block::new(Default::default(), ops)
}
