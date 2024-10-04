use core::fmt;

use super::{Context, Operation};
use crate::{
    formatter::PrettyPrint,
    traits::{CallableOpInterface, SingleBlock, SingleRegion},
    Entity, Value,
};

pub struct OpPrintingFlags;

/// The `OpPrinter` trait is expected to be implemented by all [Op] impls as a prequisite.
///
/// The actual implementation is typically generated as part of deriving [Op].
pub trait OpPrinter {
    fn print(
        &self,
        flags: &OpPrintingFlags,
        context: &Context,
        f: &mut fmt::Formatter,
    ) -> fmt::Result;
}

impl OpPrinter for Operation {
    #[inline]
    fn print(
        &self,
        _flags: &OpPrintingFlags,
        _context: &Context,
        f: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// The generic format for printed operations is:
///
/// <%result..> = <dialect>.<op>(%operand : <operand_ty>, ..) : <result_ty..> #<attr>.. {
///     // Region
/// ^<block_id>(<%block_argument...>):
///     // Block
/// };
///
/// Special handling is provided for SingleRegionSingleBlock and CallableOpInterface ops:
///
/// * SingleRegionSingleBlock ops with no operands will have the block header elided
/// * CallableOpInterface ops with no operands will be printed differently, using their
///   symbol and signature, as shown below:
///
/// <dialect>.<op> @<symbol>(<abi_params..>) -> <abi_results..> #<attr>.. {
///     ...
/// }
impl PrettyPrint for Operation {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let is_single_region_single_block =
            self.implements::<dyn SingleBlock>() && self.implements::<dyn SingleRegion>();
        let is_callable_op = self.implements::<dyn CallableOpInterface>();
        let is_symbol = self.is_symbol();
        let no_operands = self.operands().is_empty();

        let results = self.results();
        let mut doc = if !results.is_empty() {
            let results = results.iter().enumerate().fold(Document::Empty, |doc, (i, result)| {
                if i > 0 {
                    doc + const_text(", ") + display(result.borrow().id())
                } else {
                    doc + display(result.borrow().id())
                }
            });
            results + const_text(" = ")
        } else {
            Document::Empty
        };
        doc += display(self.name());
        let doc = if is_callable_op && is_symbol && no_operands {
            let name = self.as_symbol().unwrap().name();
            let callable = self.as_trait::<dyn CallableOpInterface>().unwrap();
            let signature = callable.signature();
            let mut doc = doc + display(signature.visibility) + text(format!(" @{}", name));
            if let Some(body) = callable.get_callable_region() {
                let body = body.borrow();
                let entry = body.entry();
                doc += entry.arguments().iter().enumerate().fold(
                    const_text("("),
                    |doc, (i, param)| {
                        let param = param.borrow();
                        let doc = if i > 0 { doc + const_text(", ") } else { doc };
                        doc + display(param.id()) + const_text(": ") + display(param.ty())
                    },
                ) + const_text(")");
                if !signature.results.is_empty() {
                    doc += signature.results().iter().enumerate().fold(
                        const_text(" -> "),
                        |doc, (i, result)| {
                            if i > 0 {
                                doc + const_text(", ") + display(&result.ty)
                            } else {
                                doc + display(&result.ty)
                            }
                        },
                    );
                }
            } else {
                doc += signature.render()
            }
            doc
        } else {
            let operands = self.operands();
            let doc = if !operands.is_empty() {
                operands.iter().enumerate().fold(doc + const_text("("), |doc, (i, operand)| {
                    let operand = operand.borrow();
                    let value = operand.value();
                    if i > 0 {
                        doc + const_text(", ")
                            + display(value.id())
                            + const_text(": ")
                            + display(value.ty())
                    } else {
                        doc + display(value.id()) + const_text(": ") + display(value.ty())
                    }
                }) + const_text(")")
            } else {
                doc
            };
            if !results.is_empty() {
                let results =
                    results.iter().enumerate().fold(Document::Empty, |doc, (i, result)| {
                        if i > 0 {
                            doc + const_text(", ") + text(format!("{}", result.borrow().ty()))
                        } else {
                            doc + text(format!("{}", result.borrow().ty()))
                        }
                    });
                doc + const_text(" : ") + results
            } else {
                doc
            }
        };

        let doc = self.attrs.iter().enumerate().fold(doc, |doc, (i, attr)| {
            let doc = if i > 0 { doc + const_text(" ") } else { doc };
            if let Some(value) = attr.value() {
                doc + const_text("#[")
                    + display(attr.name)
                    + const_text(" = ")
                    + value.render()
                    + const_text("]")
            } else {
                doc + text(format!("#[{}]", &attr.name))
            }
        });

        if self.has_regions() {
            self.regions.iter().fold(doc, |doc, region| {
                let blocks = region.body().iter().fold(Document::Empty, |doc, block| {
                    let ops =
                        block.body().iter().fold(Document::Empty, |doc, op| doc + op.render());
                    if is_single_region_single_block && no_operands {
                        doc + indent(4, nl() + ops) + nl()
                    } else {
                        let block_args = block.arguments().iter().enumerate().fold(
                            Document::Empty,
                            |doc, (i, arg)| {
                                if i > 0 {
                                    doc + const_text(", ") + arg.borrow().render()
                                } else {
                                    doc + arg.borrow().render()
                                }
                            },
                        );
                        let block_args = if block_args.is_empty() {
                            block_args
                        } else {
                            const_text("(") + block_args + const_text(")")
                        };
                        doc + indent(
                            4,
                            text(format!("^{}", block.id()))
                                + block_args
                                + const_text(":")
                                + nl()
                                + ops,
                        ) + nl()
                    }
                });
                doc + indent(4, const_text(" {") + nl() + blocks) + nl() + const_text("}")
            }) + const_text(";")
        } else {
            doc + const_text(";")
        }
    }
}
