#![allow(dead_code)]

use quote::ToTokens;

#[test]
fn derive_attribute_test() {
    let item_input: syn::DeriveInput = syn::parse_quote! {
        /// A simple boolean attribute
        #[derive(DialectAttribute, Debug, Copy, Clone, PartialEq, Eq)]
        #[attribute(
            dialect = HirDialect,
            traits(BooleanLikeAttr),
            implements(AttrPrinter)
        )]
        pub struct Bool(bool);
    };

    let output = crate::dialect::derive_attribute(&item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn derive_remote_attribute_test() {
    let item_input: syn::DeriveInput = syn::parse_quote! {
        /// A simple boolean attribute
        #[derive(DialectAttribute, Debug, Copy, Clone, PartialEq, Eq)]
        #[attribute(
            dialect = HirDialect,
            remote = "bool",
            default = "bool::default",
            traits(BooleanLikeAttr),
            implements(AttrPrinter)
        )]
        pub struct Bool;
    };

    let output = crate::dialect::derive_attribute(&item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn derive_effect_op_interface_test() {
    let item_input: syn::DeriveInput = syn::parse_quote! {
        /// A simple boolean attribute
        #[derive(EffectOpInterface)]
        #[effects(MemoryEffect(MemoryEffect::Write))]
        pub struct Load {
            ptr: Ptr
        }
    };

    let output = crate::operations::derive_effect_op_interface(&item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn derive_effect_op_interface_fields_test() {
    let item_input: syn::DeriveInput = syn::parse_quote! {
        /// A simple boolean attribute
        #[derive(EffectOpInterface)]
        pub struct Load {
            #[effects(MemoryEffect(MemoryEffect::Read))]
            ptr: Ptr
        }
    };

    let output = crate::operations::derive_effect_op_interface(&item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn operation_trait_no_verifier_test() {
    let meta = Vec::default();
    let item_input: syn::ItemTrait = syn::parse_quote! {
        #[operation_trait]
        pub trait SameOperandsAndResultType : SameTypeOperands {}
    };

    let output = crate::operations::derive_operation_trait(meta, item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn operation_trait_with_verifiers_test() {
    let meta = Vec::default();
    let item_input: syn::ItemTrait = syn::parse_quote! {
        #[operation_trait]
        pub trait SameOperandsAndResultType : SameTypeOperands {
            #[verifier]
            fn has_same_operands_and_result_type(_op: &::midenc_hir::Operation, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                true
            }

            #[verifier]
            fn additional_check(_op: &::midenc_hir::Operation, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                true
            }
        }
    };

    let output = crate::operations::derive_operation_trait(meta, item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn operation_trait_with_generics_test() {
    let meta = Vec::default();
    let item_input: syn::ItemTrait = syn::parse_quote! {
        #[operation_trait]
        pub trait SameOperandsAndResultType<T: AttributeRegistration> : SameTypeOperands<T> + UnusedGenerics {
            #[verifier]
            fn verify_it<T: AttributeRegistration>(_op: &::midenc_hir::Operation, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                true
            }
        }
    };

    let output = crate::operations::derive_operation_trait(meta, item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.into_token_stream().to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

#[test]
fn operation_impl_test() {
    let item_input: syn::DeriveInput = syn::parse_quote! {
        /// Two's complement sum
        #[operation(
            dialect = HirDialect,
            traits(BinaryOp, Commutative, SameTypeOperands),
            implements(InferTypeOpInterface),
        )]
        pub struct Add {
            /// The left-hand operand
            #[operand]
            lhs: AnyInteger,
            #[operand]
            rhs: AnyInteger,
            #[result]
            result: AnyInteger,
            #[attr]
            overflow: OverflowAttr,
        }
    };

    let output = crate::operation::derive_operation(item_input);
    match output {
        Ok(output) => {
            let formatted = format_output(&output.to_string());
            println!("{formatted}");
        }
        Err(err) => {
            panic!("{err}");
        }
    }
}

fn format_output(input: &str) -> String {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .args([
            "--config",
            "unstable_features=true,normalize_doc_attributes=true,use_field_init_shorthand=true,\
             condense_wildcard_suffixes=true,format_strings=true,group_imports=StdExternalCrate,\
             imports_granularity=Crate",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn 'rustfmt'");

    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).expect("failed to write input to 'rustfmt'");
    }
    let mut buf = String::new();
    let mut stdout = child.stdout.take().unwrap();
    stdout.read_to_string(&mut buf).expect("failed to read output from 'rustfmt'");
    match child.wait() {
        Ok(status) => {
            if status.success() {
                buf
            } else {
                let mut stderr = child.stderr.take().unwrap();
                let mut err_buf = String::new();
                let _ = stderr.read_to_string(&mut err_buf).ok();
                panic!(
                    "command 'rustfmt' failed with status {:?}\n\nReason: {}",
                    status.code(),
                    if err_buf.is_empty() {
                        "<no output>"
                    } else {
                        err_buf.as_str()
                    },
                );
            }
        }
        Err(err) => panic!("command 'rustfmt' failed with {err}"),
    }
}
