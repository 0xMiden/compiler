//! This module handles parsing pass pipelines adhering to the following format:
//!
//! ```text
//! pipeline          ::= op-anchor `(` pipeline-element (`,` pipeline-element)* `)`
//! pipeline-element  ::= pipeline | (pass-name | pass-pipeline-name) options?
//! options           ::= '{' (key ('=' value)?)+ '}'
//! ```
//!
//! * `op-anchor` is the operation name that anchors execution of the pass manager, this must be
//!   either a concrete operation name, or `any`, to apply against any operation type.
//! * `pass-name` and `pass-pipeline-name` correspond to the argument name of a registered pass or
//!   pass pipeline, e.g. `cse` or `canonicalize`
//! * `options` are specific key/value pairs representing options defined by a pass or pass pipeline,
//!   as described in the _Instance Specific Pass Options_ section below.
//!
//!
//! ## Examples
//!
//! For example, the following pipeline:
//!
//! ```text
//! $ hir-opt foo.hir --pass-pipeline='builtin.module(builtin.function(cse,canonicalize),convert-to-masm{key=value})'
//! ```
//!
//! * Runs `cse` and `canonicalize` passes on any functions in the provided module operation
//! * Applies the `convert-to-masm` pass to the module, with option `key` set to `value`

use core::fmt;
use std::{rc::Rc, str::FromStr};

use midenc_hir::{
    Context, FxHashMap,
    diagnostics::{LabeledSpan, PrintDiagnostic, Report, Severity, SourceId, miette::diagnostic},
    formatter::DisplayValues,
    interner::Symbol,
    parse::{Token, lexer::TokenStream, scanner::Scanner},
    pass::{Nesting, OpPassManager, PassManager},
};

#[derive(Default, Debug, Clone)]
pub struct PassPipeline {
    /// The anchor operation - if `None`, any operation will do
    pub anchor: Anchor,
    /// The set of passes to be applied to the anchor operation type
    pub passes: Vec<SelectedPass>,
    /// The set of nested pass pipelines to be applied to operations contained in regions of the
    /// anchor operation
    pub nested: Vec<PassPipeline>,
}

impl PassPipeline {
    pub fn load(&self, context: Rc<Context>) -> Result<PassManager, Report> {
        let mut pm = PassManager::new(context.clone(), self.anchor.to_string(), Nesting::Explicit);

        for nested in self.nested.iter() {
            let Some(nested_pm) = nested.load_nested(context.clone())? else {
                continue;
            };
            pm.nest_pass_manager(nested_pm);
        }

        for pass in self.passes.iter() {
            pass.add_to_pipeline(pm.op_pass_manager_mut(), &context)?;
        }

        Ok(pm)
    }

    fn load_nested(&self, context: Rc<Context>) -> Result<Option<OpPassManager>, Report> {
        if self.nested.is_empty() && self.passes.is_empty() {
            return Ok(None);
        }

        let mut pm =
            OpPassManager::new(&self.anchor.to_string(), Nesting::Explicit, context.clone());

        let mut nested_pms = Vec::with_capacity(self.nested.len());
        for nested in self.nested.iter() {
            let Some(nested_pm) = nested.load_nested(context.clone())? else {
                continue;
            };
            nested_pms.push(nested_pm);
        }

        if nested_pms.is_empty() && self.passes.is_empty() {
            return Ok(None);
        }

        for nested in nested_pms {
            pm.nest_pass_manager(nested);
        }

        for pass in self.passes.iter() {
            pass.add_to_pipeline(&mut pm, &context)?;
        }

        Ok(Some(pm))
    }
}

impl fmt::Display for PassPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.anchor)?;
        let needs_parens = !self.nested.is_empty() || !self.passes.is_empty();
        if needs_parens {
            f.write_str("(")?;
        }
        if !self.nested.is_empty() {
            write!(f, "{}", DisplayValues::new(self.nested.iter()))?;
        }
        if !self.passes.is_empty() {
            if !self.nested.is_empty() {
                f.write_str(", ")?;
            }
            write!(f, "{}", DisplayValues::new(self.passes.iter()))?;
        }
        if needs_parens {
            f.write_str(")")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SelectedPass {
    pub name: Symbol,
    pub options: FxHashMap<Symbol, String>,
}

impl SelectedPass {
    pub fn add_to_pipeline(&self, pm: &mut OpPassManager, context: &Context) -> Result<(), Report> {
        use midenc_hir::pass::registry::RegistryEntry;

        let Some(info) = midenc_hir::pass::PassInfo::lookup(self.name.as_str()) else {
            return Err(Report::msg(format!("unknown pass '{}'", &self.name)));
        };

        let options =
            DisplayValues::new(self.options.iter().map(|(k, v)| format!("{k}={v}"))).to_string();
        info.add_to_pipeline(pm, &options, context.diagnostics())?;

        Ok(())
    }
}

impl fmt::Display for SelectedPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name.as_str())?;
        if self.options.is_empty() {
            return Ok(());
        }
        write!(
            f,
            "{{{}}}",
            DisplayValues::new(self.options.iter().map(|(k, v)| format!("{k}={v}")))
        )
    }
}

impl FromStr for PassPipeline {
    type Err = Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Return a default pipeline on empty input
        if s.is_empty() {
            return Ok(Self::default());
        }

        let scanner = Scanner::new(s);
        let mut token_stream = TokenStream::new(SourceId::UNKNOWN, scanner);

        let (span, anchor) = token_stream
            .expect_map("anchor", |tok| match tok {
                Token::BareIdent(id) => Some(Symbol::intern(id)),
                _ => None,
            })
            .map_err(|err| Report::from(err).with_source_code(s.to_string()))?
            .into_parts();

        let anchor = anchor.as_str().parse::<Anchor>().map_err(|err| {
            let label = err.clone();
            Report::from(diagnostic!(
                severity = Severity::Error,
                labels = vec![LabeledSpan::at(span, label)],
                "invalid anchor: {}",
                err
            ))
            .with_source_code(s.to_string())
        })?;

        let mut context = PipelineParsingContext {
            stack: vec![PassPipeline {
                anchor,
                nested: vec![],
                passes: vec![],
            }],
        };

        parse_pipeline_recursively(&mut token_stream, &mut context)
            .map_err(|err| err.with_source_code(s.to_string()))?;

        Ok(context.stack.pop().unwrap())
    }
}

struct PipelineParsingContext {
    stack: Vec<PassPipeline>,
}

fn parse_pipeline_recursively(
    token_stream: &mut TokenStream<'_>,
    context: &mut PipelineParsingContext,
) -> Result<(), Report> {
    'start_pipeline: loop {
        if token_stream.peek()?.is_none_or(|(_, tok, _)| matches!(tok, Token::Eof))
            && context.stack.len() == 1
        {
            return Ok(());
        }
        token_stream.expect(Token::Lparen)?;

        loop {
            let (span, anchor_or_pass_name) = token_stream
                .expect_map("pass name or pass pipeline anchor", |tok| match tok {
                    Token::BareIdent(id) => Some(Symbol::intern(id)),
                    _ => None,
                })?
                .into_parts();

            if anchor_or_pass_name.as_str().contains('.') {
                // This must be an anchor
                let anchor = anchor_or_pass_name.as_str().parse::<Anchor>().map_err(|err| {
                    let label = err.clone();
                    Report::from(diagnostic!(
                        severity = Severity::Error,
                        labels = vec![LabeledSpan::at(span, label)],
                        "invalid anchor: {}",
                        err
                    ))
                })?;
                context.stack.push(PassPipeline {
                    anchor,
                    ..Default::default()
                });
                continue 'start_pipeline;
            } else {
                // This must be a pass name - validate it
                if !is_valid_pass_name(anchor_or_pass_name.as_str()) {
                    return Err(Report::from(diagnostic!(
                        severity = Severity::Error,
                        labels = vec![LabeledSpan::at(span, "contains invalid characters")],
                        "invalid pass name: '{}'",
                        anchor_or_pass_name
                    )));
                }

                context.stack.last_mut().unwrap().passes.push(SelectedPass {
                    name: Symbol::intern(anchor_or_pass_name),
                    options: Default::default(),
                });

                if token_stream.next_if_eq(Token::Lbrace)? {
                    // Parse options
                    while !token_stream.is_next(|tok| matches!(tok, Token::Rbrace)) {
                        let key = token_stream.expect_map("pass option key", |tok| match tok {
                            Token::BareIdent(key) | Token::String(key) => Some(Symbol::intern(key)),
                            tok if tok.is_keyword() => {
                                Some(Symbol::from(tok.into_compact_string()))
                            }
                            _ => None,
                        })?;
                        token_stream.expect(Token::Equal)?;
                        let value =
                            token_stream.expect_map("pass option value", |tok| match tok {
                                Token::BareIdent(value) | Token::String(value) => {
                                    Some(value.to_string())
                                }
                                tok if tok.is_keyword() => {
                                    Some(tok.into_compact_string().into_string())
                                }
                                Token::Int(n) | Token::Hex(n) | Token::Binary(n) => {
                                    Some(n.to_string())
                                }
                                _ => None,
                            })?;
                        context
                            .stack
                            .last_mut()
                            .unwrap()
                            .passes
                            .last_mut()
                            .unwrap()
                            .options
                            .insert(key.into_inner(), value.into_inner());
                    }
                    token_stream.expect(Token::Rbrace)?;
                }

                while !token_stream.next_if_eq(Token::Comma)? {
                    token_stream.expect(Token::Rparen)?;

                    if context.stack.len() == 1 {
                        token_stream.expect(Token::Eof)?;
                        return Ok(());
                    }

                    let pipeline = context.stack.pop().unwrap();
                    context.stack.last_mut().unwrap().nested.push(pipeline);
                }
            }
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
pub enum Anchor {
    #[default]
    Any,
    Operation {
        dialect: Symbol,
        opcode: Symbol,
    },
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => f.write_str("any"),
            Self::Operation { dialect, opcode } => write!(f, "{dialect}.{opcode}"),
        }
    }
}

impl FromStr for Anchor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "any" {
            Ok(Self::Any)
        } else if let Some((dialect, opcode)) = s.split_once('.') {
            if !is_valid_symbol_name(dialect) {
                return Err("invalid anchor: dialect name is not a valid symbol".to_string());
            }
            if !is_valid_symbol_name(opcode) {
                return Err("invalid anchor: operation name is not a valid symbol".to_string());
            }
            let dialect = Symbol::intern(dialect);
            let opcode = Symbol::intern(opcode);
            Ok(Self::Operation { dialect, opcode })
        } else {
            Err(format!(
                "invalid anchor: operation name must be fully-qualified or 'any', got '{s}'"
            ))
        }
    }
}

fn is_valid_symbol_name(s: &str) -> bool {
    // 1. Non-empty
    // 2. Starts with _ or a-z
    // 3. Contains only _ or a-z or 0-9
    // 4. Contains at least one alphabetic character
    if s.is_empty() {
        return false;
    }

    let mut at_least_one_alphabetic = false;
    for (i, c) in s.char_indices() {
        match c {
            'a'..='z' => {
                at_least_one_alphabetic = true;
            }
            '0'..='9' if i > 0 => (),
            '_' => (),
            _ => return false,
        }
    }

    at_least_one_alphabetic
}

fn is_valid_pass_name(s: &str) -> bool {
    // 1. Non-empty
    // 2. Starts with a-z
    // 3. Contains only a-z, -, _, or 0-9
    // 4. Contains at least one alphabetic character
    if s.is_empty() {
        return false;
    }

    let mut at_least_one_alphabetic = false;
    for (i, c) in s.char_indices() {
        match c {
            'a'..='z' => {
                at_least_one_alphabetic = true;
            }
            '0'..='9' | '_' | '-' if i > 0 => (),
            _ => return false,
        }
    }

    at_least_one_alphabetic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_pipeline() -> Result<(), Report> {
        let pipeline_str =
            "builtin.module(builtin.function(cse, canonicalize), convert-to-masm{key=value})";

        let pipeline = pipeline_str.parse::<PassPipeline>()?;
        assert_eq!(pipeline.to_string(), pipeline_str);

        Ok(())
    }
}

impl clap::builder::ValueParserFactory for PassPipeline {
    type Parser = PassPipelineParser;

    fn value_parser() -> Self::Parser {
        PassPipelineParser
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct PassPipelineParser;

impl clap::builder::TypedValueParser for PassPipelineParser {
    type Value = PassPipeline;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;

        value
            .parse::<PassPipeline>()
            .map_err(|err| Error::raw(ErrorKind::InvalidValue, PrintDiagnostic::new(err)))
    }
}
