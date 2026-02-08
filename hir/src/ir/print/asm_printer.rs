use alloc::{borrow::Cow, rc::Rc};
use core::ops::AddAssign;

use super::*;
use crate::{
    AsValueRange, Attribute, Block, Context, EntityList, FunctionType, Immediate, Location,
    NamedAttribute, OpSuccessorRange, SymbolPath, Type, ValueRef,
    dialects::builtin::attributes::Signature, formatter::Document, interner,
};

/// [OperationPrinter] provides utilities for pretty-printing an operation in either the generic
/// or custom formats. It provides access to the current printer flags, and manages the output
/// document to ensure that custom printers adhere to the requirements expected of all printers.
pub struct AsmPrinter<'a> {
    context: Rc<Context>,
    flags: &'a OpPrintingFlags,
    document: Document,
}

impl<'a> AsmPrinter<'a> {
    /// Construct a new printer with the given [OpPrintingFlags]
    pub const fn new(context: Rc<Context>, flags: &'a OpPrintingFlags) -> Self {
        Self {
            context,
            flags,
            document: Document::Empty,
        }
    }

    /// Get a reference to the [Context] this printer was instantiated with
    #[inline(always)]
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Get a reference to the [Context] this printer was instantiated with
    #[inline]
    pub fn context_rc(&self) -> Rc<Context> {
        self.context.clone()
    }

    /// Render the current printer state as a [Document], and reset the printer to an empty buffer.
    ///
    /// This can be used to emit multiple separate documents using a single [AsmPrinter] instance.
    #[inline]
    pub fn render(&mut self) -> Document {
        core::mem::take(&mut self.document)
    }

    /// Consume the printer, and render its state as a [Document].
    #[inline(always)]
    pub fn finish(self) -> Document {
        self.document
    }
}

impl<'a> AsmPrinter<'a> {
    /// Print `op` using the generic assembly format
    ///
    /// See the [print](crate::print) module docs for the details of the generic format.
    pub fn print_operation_generic(&mut self, op: &Operation) {
        use crate::formatter::*;

        let context = op.context();
        self.print_results(op.results().all());

        self.document += text(format!("\"{}\"", &op.name()));

        // Print operand list and successors
        //
        // If the operation has successors, expect that the number of operand groups is equal to
        // the number of successors + 1, and only print operands in the default group in the generic
        // operand list.
        //
        // For ops without successors, put all operands in the operand list. This is _not_
        // round-trippable, but the only sane thing to do when printing in generic form.
        if op.has_successors() {
            assert_eq!(op.num_successors() + 1, op.operands().num_groups());
            self.print_operand_list(op.operands().group(0));

            self.document += const_text(" ");
            self.print_successors(op.successors().all());
        } else {
            self.print_operand_list(op.operands().all());
        }

        // Print properties
        if op.has_properties() {
            self.document += const_text(" <");
            self.print_attribute_dictionary(op.properties());
            self.document += const_text(">");
        }

        // Print regions
        if op.has_regions() {
            self.document += const_text(" (");
            self.print_regions(op.regions());
            self.document += const_text(")");
        }

        // Print attributes
        let attrs = op.attributes();
        if !attrs.is_empty() {
            self.document += const_text(" ");
            self.print_attribute_dictionary(
                op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }

        // Print operation type
        self.document += const_text(" : ");
        if op.has_successors() {
            if op.operands().group(0).len() == 1 {
                self.print_type(&op.operands()[0].borrow().ty());
            } else {
                self.print_type_list(
                    op.operands().group(0).iter().map(|operand| Cow::Owned(operand.borrow().ty())),
                );
            }
        } else if op.operands().len() == 1 {
            self.print_type(&op.operands()[0].borrow().ty());
        } else {
            self.print_type_list(
                op.operands().all().iter().map(|operand| Cow::Owned(operand.borrow().ty())),
            );
        }

        self.document += const_text(" ");
        self.print_arrow_type_list(
            /*elide_single_type_parens=*/ true,
            op.results().all().iter().map(|result| Cow::Owned(result.borrow().ty().clone())),
        );

        // Add source location if requested
        if self.flags.print_source_locations {
            let loc = Location::from_span(op.span, context);
            self.print_trailing_location_specifier(&loc);
        }

        self.document += const_text(";");
    }

    /// Prints operation results ungrouped, i.e. `%0, %1, %2 =`
    ///
    /// If an operation has no results, this is a no-op.
    pub fn print_results(&mut self, results: OpResultRange<'_>) {
        use crate::formatter::*;

        if results.is_empty() {
            return;
        }

        let doc = results.iter().fold(Document::Empty, |acc, result| {
            if acc.is_empty() {
                display(result.borrow().id())
            } else {
                acc + const_text(", ") + display(result.borrow().id())
            }
        });

        if doc.is_empty() {
            self.document += doc;
        } else {
            self.document += doc + const_text(" = ")
        }
    }

    /// Prints operation operands in parentheses, i.e. `(%0, %1)`
    pub fn print_operand_list(&mut self, operands: OpOperandRange<'_>) {
        use crate::formatter::*;

        self.document += const_text("(");
        self.print_value_uses(operands.as_value_range());
        self.document += const_text(")");
    }

    /// Prints zero or more comma-separated operands (i.e. used values)
    pub fn print_value_uses<const N: usize>(&mut self, values: ValueRange<'_, N>) {
        use crate::formatter::*;

        let doc = values.iter().fold(Document::Empty, |acc, value| {
            let value = value.borrow();
            if acc.is_empty() {
                display(value.id())
            } else {
                acc + const_text(", ") + display(value.id())
            }
        });
        self.document += doc;
    }

    /// Prints zero or more comma-separated value ids and their types in parentheses.
    ///
    /// This is intended for use in parameter lists (e.g. block arguments).
    ///
    /// See [`Self::print_value_ids_and_types`] for more information on correct usage of this method.
    pub fn print_value_id_and_type_list(
        &mut self,
        values: impl ExactSizeIterator<Item = ValueRef>,
    ) {
        use crate::formatter::*;

        self.document += const_text("(");
        self.print_value_ids_and_types(values);
        self.document += const_text(")");
    }

    /// Prints zero or more comma-separated value ids and their types.
    ///
    /// This differs from [`Self::print_value_uses`] in that it does not allow value indexing, e.g.
    /// `%value#0` or value packing, e.g. `%value:2`. Any attempt to print such values with this
    /// method will panic.
    pub fn print_value_ids_and_types(&mut self, values: impl ExactSizeIterator<Item = ValueRef>) {
        use crate::formatter::*;

        let doc = values.fold(Document::Empty, |mut acc, value| {
            let value = value.borrow();
            if !acc.is_empty() {
                acc += const_text(", ");
            }
            acc + display(value.id()) + const_text(": ") + text(TypePrinter(value.ty()))
        });
        self.document += doc;
    }

    /// Prints zero or more types in parentheses, i.e. `(i1, i32)`
    pub fn print_type_list<'t>(&mut self, types: impl IntoIterator<Item = Cow<'t, Type>>) {
        use crate::formatter::*;

        self.document += const_text("(");
        self.print_types(types);
        self.document += const_text(")");
    }

    /// Prints a colon and then zero or more types in parentheses, i.e. `: (i1, i32)`
    pub fn print_colon_type_list<'t>(&mut self, types: impl IntoIterator<Item = Cow<'t, Type>>) {
        use crate::formatter::*;

        self.document += const_text(":");
        self.print_type_list(types);
    }

    /// Prints an arrow and then zero or more types in parentheses, i.e. `-> (i1, i32)`.
    ///
    /// If `elide_single_type_parens` is true, then if the input collection only has a single type,
    /// then the parentheses will be elided
    pub fn print_arrow_type_list<'t>(
        &mut self,
        elide_single_type_parens: bool,
        types: impl ExactSizeIterator<Item = Cow<'t, Type>>,
    ) {
        use crate::formatter::*;

        self.document += const_text("-> ");
        if elide_single_type_parens && types.len() == 1 {
            self.print_types(types);
        } else {
            self.print_type_list(types);
        }
    }

    /// Prints zero or more comma-separated types
    pub fn print_types<'t>(&mut self, types: impl IntoIterator<Item = Cow<'t, Type>>) {
        use crate::formatter::*;

        let doc = types.into_iter().fold(Document::Empty, |acc, ty| {
            let ty = text(format!("{}", TypePrinter(ty.as_ref())));
            if acc.is_empty() {
                ty
            } else {
                acc + const_text(", ") + ty
            }
        });

        if !doc.is_empty() {
            self.document += doc;
        }
    }

    /// Print a single type
    pub fn print_type(&mut self, ty: &Type) {
        use crate::formatter::*;

        self.document += text(format!("{}", TypePrinter(ty)));
    }

    /// Print a function type (i.e. `(arg0, arg1) -> (result0, result1)`)
    ///
    /// The printed type will elide parens around single-result types
    pub fn print_function_type(&mut self, ty: &FunctionType) {
        self.print_type_list(ty.params().iter().map(Cow::Borrowed));
        self.print_space();
        self.print_arrow_type_list(
            /*elide_single_type_parens=*/ true,
            ty.results().iter().map(Cow::Borrowed),
        );
    }

    /// Print a function type (i.e. `(arg0, arg1) -> (result0, result1)`)
    ///
    /// The printed type will elide parens around single-result types
    pub fn print_signature(&mut self, ty: &Signature) {
        self.print_type_list(ty.params().iter().map(|p| Cow::Borrowed(&p.ty)));
        self.print_space();
        self.print_arrow_type_list(
            /*elide_single_type_parens=*/ true,
            ty.results().iter().map(|p| Cow::Borrowed(&p.ty)),
        );
    }

    /// Prints zero or more regions
    pub fn print_regions(&mut self, regions: &RegionList) {
        use crate::formatter::*;

        for (i, region) in regions.iter().enumerate() {
            if i > 0 {
                self.document += const_text(" ") + nl();
            }
            self.print_region(&region);
        }
    }

    /// Print a single region in `{` `}`
    pub fn print_region(&mut self, region: &Region) {
        use crate::formatter::*;

        if region.is_empty() {
            self.document += const_text("{ }");
            return;
        }

        self.document += const_text("{");
        let mut printer = AsmPrinter::new(self.context.clone(), self.flags);
        let body = region.body().iter().enumerate().fold(Document::Empty, |acc, (i, block)| {
            if i > 0 {
                printer.print_block_label_and_arguments(&block);
            }
            printer.print_block_body(block.body());

            acc + nl() + printer.render()
        });
        self.document += indent(4, body);
        self.document += nl() + const_text("}");
    }

    /// Print a single block
    ///
    /// This delegates to [`Block::print`] internally.
    pub fn print_block(&mut self, block: &Block) {
        let is_entry_block = block.is_entry_block() && !self.flags.print_entry_block_headers;

        if is_entry_block {
            self.print_block_body(block.body());
        } else {
            self.print_block_label_and_arguments(block);
            self.print_block_body(block.body());
        }
    }

    /// Print the body of a block, i.e. a sequence of newline-separated operations, indented by 4
    /// spaces.
    ///
    /// NOTE: This method inserts a newline before printing the first operation, in order to trigger
    /// indentation, so it is not required to emit one yourself first.
    pub fn print_block_body(&mut self, ops: &EntityList<Operation>) {
        use crate::formatter::*;

        let body = ops.iter().fold(Document::Empty, |acc, op| {
            let mut printer = AsmPrinter::new(self.context.clone(), self.flags);
            op.print(&mut printer);
            let doc = printer.finish();
            if acc.is_empty() {
                doc
            } else {
                acc + nl() + doc
            }
        });
        self.document += indent(4, nl() + body);
    }

    /// Print the block label and argument list for a block, i.e. `^block0(%0: i32):`
    ///
    /// No leading or trailing newlines are emitted by this method.
    pub fn print_block_label_and_arguments(&mut self, block: &Block) {
        use crate::formatter::*;

        self.document += display(block.id());
        if block.has_arguments() {
            self.print_value_id_and_type_list(block.argument_values());
        }
        self.document += const_text(":");
    }

    /// Print successors of an operation in `[` `]`
    ///
    /// If there are no successors, this is a no-op.
    pub fn print_successors(&mut self, successors: OpSuccessorRange<'_>) {
        use crate::formatter::*;

        if successors.is_empty() {
            return;
        }

        self.document += const_text("[ ");
        for (i, successor) in successors.iter().enumerate() {
            if i > 0 {
                self.document += const_text(", ");
            }
            self.document += display(successor.successor().borrow().id());
            let operands = successor.successor_operands();
            if operands.is_empty() {
                continue;
            }
            self.document += const_text(":(");
            self.print_value_uses(operands);
            self.document += const_text(")");
        }
        self.document += const_text(" ]");
    }

    /// Print an attribute value
    pub fn print_attribute_value(&mut self, value: &dyn Attribute) {
        if let Some(value) = value.as_attr().as_trait::<dyn AttrPrinter>() {
            value.print(self);
        } else {
            self.print_string(format!("{value:?}"));
        }
    }

    /// Print an attribute dictionary in `{` `}`
    pub fn print_attribute_dictionary(&mut self, attrs: impl IntoIterator<Item = NamedAttribute>) {
        use crate::formatter::*;

        self.document += const_text("{ ");
        for (i, NamedAttribute { name, value }) in attrs.into_iter().enumerate() {
            if i > 0 {
                self.document += const_text(", ");
            }
            self.print_identifier(name);
            self.document += const_text(" = ");
            self.print_attribute_value(&*value.borrow());
        }
        self.document += const_text(" }");
    }

    /// Print the optional trailing location specifier, i.e. `loc("file":1:1)`
    pub fn print_trailing_location_specifier(&mut self, loc: &Location) {
        use crate::formatter::*;

        self.document += text(format!("loc({loc})"));
    }

    /// Print an identifier as either a bare identifier or a string if it contains characters which
    /// are not valid in bare identifiers.
    ///
    /// This is only valid to call in positions where both bare identifiers and strings are valid.
    pub fn print_identifier(&mut self, ident: interner::Symbol) {
        use crate::formatter::*;

        let id = ident.as_str();
        if is_valid_bare_identifier(id) {
            self.document += const_text(id);
        } else {
            self.document += display(id.escape_default());
        }
    }

    /// Print an identifier bare.
    ///
    /// This function will panic if `ident` is not a valid bare identifier.
    pub fn print_bare_identifier(&mut self, ident: interner::Symbol) {
        use crate::formatter::*;

        let id = ident.as_str();
        assert!(is_valid_bare_identifier(id));
        self.document += const_text(id);
    }

    /// Print a possibly multi-component symbol path, i.e. `@foo::@bar`
    pub fn print_symbol_path(&mut self, path: &SymbolPath) {
        use crate::{SymbolNameComponent, formatter::*};
        for (i, component) in path.components().enumerate() {
            if i > 0 || matches!(component, SymbolNameComponent::Root) {
                self.document += const_text("::");
            }
            match component {
                SymbolNameComponent::Component(sym) | SymbolNameComponent::Leaf(sym) => {
                    self.print_symbol_name(sym);
                }
                SymbolNameComponent::Root => (),
            }
        }
    }

    /// Print a single-component symbol name, i.e. `@foo`
    pub fn print_symbol_name(&mut self, name: interner::Symbol) {
        use crate::formatter::*;

        self.document += text(format!("@{name}"));
    }

    /// Print a custom keyword.
    ///
    /// Keywords must be valid bare identifiers. This method will panic if the given keyword is
    /// not valid printed bare.
    pub fn print_keyword(&mut self, keyword: &'static str) {
        use crate::formatter::*;

        assert!(is_valid_bare_identifier(keyword));

        self.document += const_text(keyword);
    }

    /// Print a literal integer value in decimal format
    pub fn print_decimal_integer(&mut self, value: impl Into<Immediate>) {
        use crate::formatter::*;

        let value = value.into();
        if value.is_signed() {
            self.document += display(
                value.as_i128().unwrap_or_else(|| panic!("expected integer value, got {value}")),
            );
        } else {
            self.document += display(
                value.as_u128().unwrap_or_else(|| panic!("expected integer value, got {value}")),
            );
        }
    }

    /// Print a literal integer value in hexadecimal format with leading `0x`
    pub fn print_hex_integer(&mut self, value: impl Into<Immediate>) {
        use crate::formatter::*;

        let value = value.into();
        let raw = value
            .bitcast_u128()
            .unwrap_or_else(|| panic!("expected integer value, got {value}"));
        self.document += text(format!("{raw:0x}"));
    }

    /// Print a boolean value
    pub fn print_bool(&mut self, value: bool) {
        use crate::formatter::*;

        if value {
            self.document += const_text("true");
        } else {
            self.document += const_text("false");
        }
    }

    /// Print a literal string value
    pub fn print_string(&mut self, string: impl AsRef<str>) {
        use crate::formatter::*;

        self.document += text(string.as_ref().escape_default());
    }

    /// Print a single '('
    pub fn print_lparen(&mut self) {
        use crate::formatter::*;
        self.document += const_text("(");
    }

    /// Print a single ')'
    pub fn print_rparen(&mut self) {
        use crate::formatter::*;
        self.document += const_text(")");
    }

    /// Print a single space
    pub fn print_space(&mut self) {
        use crate::formatter::*;
        self.document += const_text(" ");
    }

    /// Print a single newline
    pub fn print_newline(&mut self) {
        use crate::formatter::*;
        self.document += nl();
    }
}

impl AddAssign<Document> for AsmPrinter<'_> {
    fn add_assign(&mut self, rhs: Document) {
        self.document += rhs;
    }
}

fn is_valid_bare_identifier(id: &str) -> bool {
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'))
}
