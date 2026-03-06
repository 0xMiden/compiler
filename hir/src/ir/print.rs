//! ## Syntax
//!
//! The grammar for the printed assembly format is given below:
//!
//! ```text
//! # File
//! top-level := (operation | attribute-alias-def | type-alias-def)+
//!
//! # Operations
//! operation         := op-results? (generic-operation | custom-operation) trailing-location? ';'
//! custom-operation  := custom-op-name custom-operation-format
//! generic-operation := generic-op-name '(' value-uses? ')' successors? properties?
//!                        regions? attributes? ':' function-type
//!
//! # Locations
//!
//! trailing-location := 'loc' '(' location ')'
//! location := '?'
//!           | 'synthetic'
//!           | 'opaque' '<' decimal-literal '>'
//!           | file-line-col
//!           | file-line-col-range
//! file-line-col       := file-uri ':' line-number ':' column-number
//! file-line-col-range := file-line-col 'to' line-number? ':' column-number
//! file-uri := string
//! line-number   := nonzero-decimal-literal
//! column-number := nonzero-decimal-literal
//!
//! # Numbers
//!
//! decimal-literal := [0-9]+
//! nonzero-decimal-literal := [1-9][0-9]*
//! hex-literal := '0x' [A-Fa-f0-9]+
//! binary-literal := '0b' [0-1]+
//!
//! # Identifiers
//!
//! bare-id    := [A-Za-z_][A-Za-z0-9_$.]*
//! alias-name := bare-id
//! value-id   := '%' suffix-id
//! caret-id   := '^' suffix-id
//! block-id   := caret-id
//! suffix-id  := [0-9]+
//!             | [A-Za-z$._-][A-Za-z0-9$._-]*
//! symbol-ref-id   := '@' (suffix-id | string)
//! generic-op-name := string
//! custom-op-name  := bare-id
//! dialect-namespace := bare-id
//!
//! # Values
//!
//! op-results := op-result (',' op-result)* '='
//! op-result  := value-id (':' decimal-literal)?
//! value-uses := value-use (',' value-use)*
//! value-use  := value-id ('#' decimal-literal)
//! value-use-and-type      := value-use ':' type
//! value-use-and-type-list := value-use-and-type (',' value-use-and-type)*
//! value-id-and-type-list  := value-id-and-type (',' value-id-and-type)*
//! value-id-and-type       := value-id ':' type
//!
//! # Successors
//!
//! successors := '[' successor (',' successor)* ']'
//! successor  := block-id (':' block-arguments)?
//!
//! # Regions
//!
//! regions := '(' region (',' region)* ')'
//! region  := '{' entry-block? block* '}'
//!
//! # Blocks
//!
//! entry-block     := operation+
//! block           := block-label operation+
//! block-label     := block-id block-arguments? ':'
//! block-arguments := '(' value-id-and-type-list? ')'
//!
//! # Properties and Attributes
//!
//! properties := '<' dictionary-attribute '>'
//! attributes := dictionary-attribute
//! attribute-entry := attribute-name '=' attribute-value
//! attribute-name  := bare-id | string
//! attribute-value := attribute-alias | dialect-attribute | builtin-attribute
//! attribute-values := attribute-value (',' attribute-value)*
//!
//! # Attribute Aliases
//!
//! attribute-alias := '#' alias-name
//! attribute-alias-def := '#' alias-name '=' attribute-value ';'
//!
//! # Builtin Attributes
//!
//! builtin-attribute := dictionary-attribute
//!                    | list-attribute
//!                    | typed-array-attribute
//!                    | integer-attribute
//!                    | string-attribute
//!                    | opaque-attribute
//!                    | symbol-ref-attribute
//!                    | type-attribute
//!                    | unit-attribute
//!
//! dictionary-attribute := '{' (attribute-entry (',' attribute-entry)*)? '}'
//! list-attribute       := '[' attribute-values? ']'
//! array-attribute      := 'array' '<' type (':' attribute-values)? '>'
//!
//! integer-attribute := boolean-attribute
//!                    | (integer-literal (':' integer-type)?)
//! boolean-attribute := 'true' | 'false'
//!
//! string-attribute := string (':' type)?
//!
//! opaque-attribute := dialect-namespace '<' string '>'
//!
//! symbol-ref-attribute := symbol-ref-id ('::' symbol-ref-id)*
//!
//! type-attribute := type
//! unit-attribute := 'unit'
//!
//! # Dialect Attributes
//!
//! dialect-attribute := '#' (opaque-dialect-attr | pretty-dialect-attr)
//! opaque-dialect-attr := dialect-namespace dialect-attr-body
//! pretty-dialect-attr := dialect-namespace '.' pretty-dialect-attr-lead-id dialect-attr-body?
//! pretty-dialect-attr-lead-id := [A-Za-z][A-Za-z0-9._]*
//! dialect-attr-body := '<' dialect-attr-contents+ '>'
//! dialect-attr-contents := dialect-attr-body
//!                        | '(' dialect-attr-contents+ ')'
//!                        | '[' dialect-attr-contents+ ']'
//!                        | '{' dialect-attr-contents+ '}'
//!                        | [^\[<({\]>)}\0]+
//!
//! # Types
//!
//! type := type-alias | dialect-type | builtin-type
//! type-list-no-parens := type (',' type)*
//! type-list-parens := '(' ')' | type-list-no-parens
//!
//! # Type Aliases
//!
//! type-alias := '!' alias-name
//! type-alias-def := '!' alias-name '=' type ';'
//!
//! # Builtin Types
//!
//! builtin-type := unknown-type
//!               | never-type
//!               | integer-type
//!               | float-type
//!               | pointer-type
//!               | list-type
//!               | array-type
//!               | struct-type
//!               | tuple-type
//!               | function-type
//!
//! function-type := (type | type-list-parens) '->' (type | type-list-parens)
//!
//! unknown-type := '?'
//! never-type := 'never'
//!
//! integer-type := signed-integer-type | unsigned-integer-type | 'felt'
//! signed-integer-type := 'i1'
//!                      | 'i8'
//!                      | 'i16'
//!                      | 'i32'
//!                      | 'i64'
//!                      | 'i128'
//! unsigned-integer-type := 'u8'
//!                      | 'u16'
//!                      | 'u32'
//!                      | 'u64'
//!                      | 'u128'
//!                      | 'u256'
//!
//! float-type := 'f64'
//!
//! pointer-type := 'ptr' '<' type (',' address-space)? '>'
//! address-space := 'byte' | 'felt'
//!
//! list-type := 'list' '<' type '>'
//! array-type := 'array' '<' type ';' decimal-literal '>'
//!
//! struct-type   := 'struct' '<' (struct-repr ';')? struct-fields* '>'
//! struct-fields := struct-field (',' struct-field)*
//! struct-field  := type struct-repr-align?
//! struct-repr   := struct-repr-align
//!                | 'packed' ('(' nonzero-decimal-literal ')')?
//!                | 'transparent'
//! struct-repr-align := 'align' '(' nonzero-decimal-literal ')'
//!
//! tuple-type := 'tuple' '<' (type (',' type)*)? '>'
//!
//! # Dialect Types
//!
//! dialect-type := '!' (opaque-dialect-type | pretty-dialect-type)
//! opaque-dialect-type := dialect-namespace dialect-type-body
//! pretty-dialect-type := dialect-namespace '.' pretty-dialect-type-lead-id dialect-type-body?
//! pretty-dialect-type-lead-id := [A-Za-z][A-Za-z0-9._]*
//! dialect-type-body := '<' dialect-type-contents+ '>'
//! dialect-type-contents := dialect-type-body
//!                        | '(' dialect-type-contents+ ')'
//!                        | '[' dialect-type-contents+ ']'
//!                        | '{' dialect-type-contents+ '}'
//!                        | [^\[<({\]>)}\0]+
//!
//! ```
//!
//! ## Examples
//!
//! The following example demonstrates a few things:
//!
//! * A generic operation with results, a multi-block region, properties and attributes
//! * A few operations with custom formats
//! * Named values (e.g. `%flag`) and result packs (i.e. `%out:2`)
//! * An example location specifier
//!
//! ```
//! %flag, %out:2 = "dialect.op"(%arg0, %0) <{ prop = true }> ({
//!     builtin.br ^after(%arg0, %0);
//! ^after(%1: i32, %2 : ptr<u8>):
//!     %3 = arith.constant 0 : i32;
//!     %4 = arith.eq %1, %3 : i1;
//!     builtin.ret %4, %1, %3;
//! }) { attr = "string" } : (i32, ptr<u8>) -> (i1, i32, i32) loc<"file.hir":1:1 to :32>;
//! ```
//!
//! This example shows the `cf.cond_br` operation in generic form, with some other notable features:
//!
//! * The operation has no results, regions, properties, attributes, or location specifier
//! * The operand and successor arguments both show how individual values of a result pack produced
//!   by a previous operation can be referenced.
//!
//! ```text
//! %overflowing_add:2 = arith.add %lhs, %rhs <{ overflow = overflowing }>;
//! "cf.cond_br"(%overflowing_add#0) [
//!     ^overflowed,
//!     ^didnt:(%overflowing_add#1)
//! ] : (i1, i32) -> ();
//! ```
//!
//! Additional notes about the printed format:
//!
//! * If an operation has properties or attributes, they must all be printed - or in the case of
//!   the custom format, present in the printed output. If not, then it will not be possible to
//!   round-trip the operation through the printed form and back.

mod asm_printer;
mod type_printer;

use alloc::{borrow::Cow, format};
use core::fmt;

use midenc_session::Options;

pub use self::{
    asm_printer::AsmPrinter,
    type_printer::{FunctionTypePrinter, TypePrinter},
};
use super::{OpOperandRange, OpResultRange, Operation, Region, RegionList, ValueRange};
use crate::{EntityWithId, Location, Value, formatter::Document};

/// Options which configure how IR entities are printed to IR assembly
#[derive(Default, Debug)]
pub struct OpPrintingFlags {
    /// When `true`, forces printing of entry block headers for all regions
    pub print_entry_block_headers: bool,
    /// When `true`, prints trailing location specifiers after all operations, i.e. `loc(..)`
    pub print_source_locations: bool,
}

impl From<&Options> for OpPrintingFlags {
    fn from(options: &Options) -> Self {
        Self {
            print_entry_block_headers: false,
            print_source_locations: options.print_hir_source_locations,
        }
    }
}

/// A trait which must be implemented by all attribute types, for printing the attribute in
/// assembly format
pub trait AttrPrinter {
    fn print(&self, printer: &mut AsmPrinter<'_>);
}

/// The `OpPrinter` trait is expected to be implemented by all [crate::Op] impls as a prequisite.
///
/// The actual implementation is typically generated as part of deriving [crate::Op].
pub trait OpPrinter {
    /// Prints this operation with the given `flags`
    fn print(&self, printer: &mut AsmPrinter<'_>);
}

impl OpPrinter for Operation {
    #[inline]
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        if let Some(custom_printer) = self.as_trait::<dyn OpPrinter>() {
            printer.print_results(self.results().all());
            *printer += display(self.name());
            custom_printer.print(printer);
            // Add source location if requested
            if printer.flags().print_source_locations {
                let loc = Location::from_span(self.span, self.context());
                printer.print_space();
                printer.print_trailing_location_specifier(&loc);
            }
            *printer += const_text(";");
        } else {
            printer.print_operation_generic(self);
        }
    }
}

impl crate::formatter::PrettyPrint for Operation {
    fn render(&self) -> Document {
        let flags = OpPrintingFlags::default();
        let context = self.context_rc();
        let mut printer = AsmPrinter::new(context, &flags);
        self.print(&mut printer);
        printer.finish()
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::formatter::PrettyPrint;

        write!(f, "{}", &self.render())
    }
}
