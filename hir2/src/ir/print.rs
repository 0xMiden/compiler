use core::fmt;

use super::{Context, Operation};
use crate::{
    formatter::{Document, PrettyPrint},
    matchers::Matcher,
    traits::{BranchOpInterface, SingleBlock, SingleRegion},
    AttributeValue, CallableOpInterface, EntityWithId, Op, Value,
};

#[derive(Default)]
pub struct OpPrintingFlags {
    /// This field is here to silence warnings about using Default with this struct when it has
    /// no fields. We plan on adding them in the future, so for future compatibility, we're
    /// ensuring at least one field is present.
    _placeholder: core::marker::PhantomData<()>,
}

/// The `OpPrinter` trait is expected to be implemented by all [Op] impls as a prequisite.
///
/// The actual implementation is typically generated as part of deriving [Op].
pub trait OpPrinter {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document;
}

impl<T: Op> OpPrinter for T {
    default fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        <Operation as OpPrinter>::print(self.as_operation(), flags, context)
    }
}

impl<T: PrettyPrint + Op> OpPrinter for T {
    default fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> Document {
        PrettyPrint::render(self)
    }
}

impl OpPrinter for Operation {
    #[inline]
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        let printer = OperationPrinter {
            op: self,
            flags,
            context,
        };
        printer.render()
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let flags = OpPrintingFlags::default();
        let context = self.context();
        let printer = OperationPrinter {
            op: self,
            flags: &flags,
            context,
        };
        write!(f, "{}", printer.render())
    }
}

pub trait AttrPrinter {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document;
}

impl<T: PrettyPrint + AttributeValue> AttrPrinter for T {
    default fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> Document {
        PrettyPrint::render(self)
    }
}

impl AttrPrinter for crate::Attribute {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        use crate::formatter::*;

        match self.value() {
            None => text(format!("#[{}]", self.name.as_str())),
            Some(value) => {
                const_text("#[")
                    + text(self.name.as_str())
                    + const_text(" = ")
                    + value.print(flags, context)
                    + const_text("]")
            }
        }
    }
}

impl AttrPrinter for crate::OpFoldResult {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        use crate::formatter::*;

        match self {
            Self::Attribute(attr) => attr.print(flags, context),
            Self::Value(value) => display(value.borrow().id()),
        }
    }
}

impl<T: AttrPrinter> AttrPrinter for [T] {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        use crate::formatter::*;

        let mut doc = Document::Empty;
        for (i, item) in self.iter().enumerate() {
            if i == 0 {
                doc += const_text(", ");
            }

            doc += item.print(flags, context);
        }
        doc
    }
}

struct OperationPrinter<'a> {
    op: &'a Operation,
    flags: &'a OpPrintingFlags,
    context: &'a Context,
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
impl PrettyPrint for OperationPrinter<'_> {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let is_single_region_single_block =
            self.op.implements::<dyn SingleBlock>() && self.op.implements::<dyn SingleRegion>();
        let is_callable_op = self.op.implements::<dyn CallableOpInterface>();
        let is_symbol = self.op.is_symbol();
        let no_operands = self.op.operands().is_empty();

        let results = self.op.results();
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
        doc += display(self.op.name()) + const_text(" ");
        let doc = if is_callable_op && is_symbol && no_operands {
            let name = self.op.as_symbol().unwrap().name();
            let callable = self.op.as_trait::<dyn CallableOpInterface>().unwrap();
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
            let mut is_constant = false;
            let doc = if let Some(value) = crate::matchers::constant().matches(self.op) {
                is_constant = true;
                doc + value.print(self.flags, self.context)
            } else {
                if let Some(branch) = self.op.as_trait::<dyn BranchOpInterface>() {
                    doc = branch.successors().iter().enumerate().fold(doc, |doc, (i, succ)| {
                        if i > 0 {
                            doc + const_text(", ") + display(succ.block.borrow().block)
                        } else {
                            doc + display(succ.block.borrow().block) + const_text(" ")
                        }
                    });
                }

                let operands = self.op.operands();
                if !operands.is_empty() {
                    operands.iter().enumerate().fold(doc, |doc, (i, operand)| {
                        let operand = operand.borrow();
                        let value = operand.value();
                        if i > 0 {
                            doc + const_text(", ") + display(value.id())
                        } else {
                            doc + display(value.id())
                        }
                    })
                } else {
                    doc
                }
            };
            let doc = if !results.is_empty() {
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
            };

            if is_constant {
                doc
            } else {
                self.op.attrs.iter().fold(doc, |doc, attr| {
                    let doc = doc + const_text(" ");
                    if let Some(value) = attr.value() {
                        doc + const_text("#[")
                            + display(attr.name)
                            + const_text(" = ")
                            + value.print(self.flags, self.context)
                            + const_text("]")
                    } else {
                        doc + text(format!("#[{}]", &attr.name))
                    }
                })
            }
        };

        if self.op.has_regions() {
            self.op.regions.iter().fold(doc, |doc, region| {
                let blocks = region.body().iter().enumerate().fold(
                    Document::Empty,
                    |mut doc, (block_index, block)| {
                        if block_index > 0 {
                            doc += nl();
                        }
                        let ops = block.body().iter().enumerate().fold(
                            Document::Empty,
                            |mut doc, (i, op)| {
                                if i > 0 {
                                    doc += nl();
                                }
                                doc + op.print(self.flags, self.context)
                            },
                        );
                        if is_single_region_single_block && no_operands {
                            doc + indent(4, nl() + ops)
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
                            )
                        }
                    },
                );
                doc + const_text(" {") + nl() + blocks + nl() + const_text("}")
            }) + const_text(";")
        } else {
            doc + const_text(";")
        }
    }
}
