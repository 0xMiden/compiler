use alloc::format;
use core::fmt;

use super::{Context, Operation};
use crate::{
    formatter::{Document, PrettyPrint},
    matchers::Matcher,
    traits::BranchOpInterface,
    AttributeValue, EntityWithId, SuccessorOperands, Value,
};

#[derive(Clone, Copy)]
pub struct OpPrintingFlags {
    pub print_entry_block_headers: bool,
    pub print_intrinsic_attributes: bool,
}

impl Default for OpPrintingFlags {
    fn default() -> Self {
        Self {
            print_entry_block_headers: true,
            print_intrinsic_attributes: false,
        }
    }
}

/// The `OpPrinter` trait is expected to be implemented by all [Op] impls as a prequisite.
///
/// The actual implementation is typically generated as part of deriving [Op].
pub trait OpPrinter {
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document;
}

impl OpPrinter for Operation {
    #[inline]
    fn print(&self, flags: &OpPrintingFlags, context: &Context) -> Document {
        if let Some(op_printer) = self.as_trait::<dyn OpPrinter>() {
            op_printer.print(flags, context)
        } else {
            let printer = OperationPrinter {
                op: self,
                flags,
                context,
            };
            printer.render()
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let flags = OpPrintingFlags::default();
        let context = self.context();
        let doc = self.print(&flags, context);
        write!(f, "{doc}")
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

pub fn render_operation_results(op: &Operation) -> crate::formatter::Document {
    use crate::formatter::*;

    let results = op.results();
    let doc = results.iter().fold(Document::Empty, |acc, result| {
        if acc.is_empty() {
            display(result.borrow().id())
        } else {
            acc + const_text(", ") + display(result.borrow().id())
        }
    });
    if doc.is_empty() {
        doc
    } else {
        doc + const_text(" = ")
    }
}

pub fn render_operation_operands(op: &Operation) -> crate::formatter::Document {
    use crate::formatter::*;

    let operands = op.operands();
    operands.iter().fold(Document::Empty, |acc, operand| {
        let operand = operand.borrow();
        let value = operand.value();
        if acc.is_empty() {
            display(value.id())
        } else {
            acc + const_text(", ") + display(value.id())
        }
    })
}

pub fn render_operation_result_types(op: &Operation) -> crate::formatter::Document {
    use crate::formatter::*;

    let results = op.results();
    let result_types = results.iter().fold(Document::Empty, |acc, result| {
        if acc.is_empty() {
            text(format!("{}", result.borrow().ty()))
        } else {
            acc + const_text(", ") + text(format!("{}", result.borrow().ty()))
        }
    });
    if result_types.is_empty() {
        result_types
    } else {
        const_text(" : ") + result_types
    }
}

pub fn render_regions(op: &Operation, flags: &OpPrintingFlags) -> crate::formatter::Document {
    use crate::formatter::*;
    const_text(" ")
        + op.regions.iter().fold(Document::Empty, |acc, region| {
            let doc = region.print(flags);
            if acc.is_empty() {
                doc
            } else {
                acc + const_text(" ") + doc
            }
        })
        + const_text(";")
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
impl PrettyPrint for OperationPrinter<'_> {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let doc = render_operation_results(self.op) + display(self.op.name()) + const_text(" ");
        let doc = if let Some(value) = crate::matchers::constant().matches(self.op) {
            doc + value.print(self.flags, self.context)
        } else if let Some(branch) = self.op.as_trait::<dyn BranchOpInterface>() {
            // Print non-successor operands
            let operands = branch.operands().group(0);
            let doc = if !operands.is_empty() {
                operands.iter().enumerate().fold(doc, |doc, (i, operand)| {
                    let operand = operand.borrow();
                    let value = operand.value();
                    if i > 0 {
                        doc + const_text(", ") + display(value.id())
                    } else {
                        doc + display(value.id())
                    }
                }) + const_text(" ")
            } else {
                doc
            };
            // Print successors
            branch.successors().iter().enumerate().fold(doc, |doc, (succ_index, succ)| {
                let doc = if succ_index > 0 {
                    doc + const_text(", ") + display(succ.block.borrow().successor())
                } else {
                    doc + display(succ.block.borrow().successor())
                };

                let operands = branch.get_successor_operands(succ_index);
                if !operands.is_empty() {
                    let doc = doc + const_text("(");
                    operands.forwarded().iter().enumerate().fold(doc, |doc, (i, operand)| {
                        if !operand.is_linked() {
                            if i > 0 {
                                doc + const_text(", ") + const_text("<unlinked>")
                            } else {
                                doc + const_text("<unlinked>")
                            }
                        } else {
                            let operand = operand.borrow();
                            let value = operand.value();
                            if i > 0 {
                                doc + const_text(", ") + display(value.id())
                            } else {
                                doc + display(value.id())
                            }
                        }
                    }) + const_text(")")
                } else {
                    doc
                }
            })
        } else {
            doc + render_operation_operands(self.op)
        };

        let doc = doc + render_operation_result_types(self.op);

        let attrs = self.op.attrs.iter().fold(Document::Empty, |acc, attr| {
            // Do not print intrinsic attributes unless explicitly configured
            if !self.flags.print_intrinsic_attributes && attr.intrinsic {
                return acc;
            }
            let doc = if let Some(value) = attr.value() {
                const_text("#[")
                    + display(attr.name)
                    + const_text(" = ")
                    + value.print(self.flags, self.context)
                    + const_text("]")
            } else {
                text(format!("#[{}]", &attr.name))
            };
            if acc.is_empty() {
                doc
            } else {
                acc + const_text(" ") + doc
            }
        });

        let doc = if attrs.is_empty() {
            doc
        } else {
            doc + const_text(" ") + attrs
        };

        if self.op.has_regions() {
            doc + render_regions(self.op, self.flags)
        } else {
            doc + const_text(";")
        }
    }
}
