use midenc_hir::{
    diagnostics::{DiagnosticsHandler, Report, Severity, Spanned},
    *,
};

use super::Rule;

/// This validation rule ensures that all identifiers adhere to the rules of their respective items.
pub struct NamingConventions;
impl Rule<Module> for NamingConventions {
    fn validate(
        &mut self,
        module: &Module,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        // Make sure all functions in this module have the same module name in their id
        for function in module.functions() {
            let id = function.id;
            if id.module != module.name {
                let expected_name = FunctionIdent {
                    module: module.name,
                    function: id.function,
                };
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function name")
                    .with_primary_label(
                        function.id.span(),
                        format!("the fully-qualified name of this function is '{id}'"),
                    )
                    .with_secondary_label(
                        module.name.span(),
                        format!(
                            "but we expected '{expected_name}' because it belongs to this module"
                        ),
                    )
                    .into_report());
            }
        }

        // 1. Must not be empty
        let name = module.name.as_str();
        if name.is_empty() {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid module name")
                .with_primary_label(module.name.span, "module name cannot be empty")
                .into_report());
        }

        // 2. Must begin with a lowercase ASCII alphabetic character
        if !name.starts_with(is_lower_ascii_alphabetic) {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid module name")
                .with_primary_label(
                    module.name.span(),
                    "module name must start with a lowercase, ascii-alphabetic character",
                )
                .into_report());
        }

        // 3. May otherwise consist of any number of characters of the following classes:
        //   * `A-Z`
        //   * `a-z`
        //   * `0-9`
        //   * `_-+$@`
        // 4. May only contain `:` when used via the namespacing operator, e.g. `std::math`
        let mut char_indices = name.char_indices().peekable();
        let mut is_namespaced = false;
        while let Some((offset, c)) = char_indices.next() {
            let offset = offset as u32;
            match c {
                c if c.is_ascii_alphanumeric() => continue,
                '_' | '-' | '+' | '$' | '@' => continue,
                ':' => match char_indices.peek() {
                    Some((_, ':')) => {
                        char_indices.next();
                        is_namespaced = true;
                        continue;
                    }
                    _ => {
                        let module_name_span = module.name.span();
                        let source_id = module_name_span.source_id();
                        let pos = module_name_span.start() + offset;
                        let span = SourceSpan::at(source_id, pos);
                        return Err(diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid module name")
                            .with_primary_label(span, "module name contains invalid character ':'")
                            .with_help("Did you mean to use the namespacing operator '::'?")
                            .into_report());
                    }
                },
                c if c.is_whitespace() => {
                    let module_name_span = module.name.span();
                    let source_id = module_name_span.source_id();
                    let pos = module_name_span.start() + offset;
                    let span = SourceSpan::at(source_id, pos);
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid module name")
                        .with_primary_label(span, "module names may not contain whitespace")
                        .into_report());
                }
                c => {
                    let module_name_span = module.name.span();
                    let source_id = module_name_span.source_id();
                    let pos = module_name_span.start() + offset;
                    let span = SourceSpan::at(source_id, pos);
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid module name")
                        .with_primary_label(span, format!("'{c}' is not valid in module names"))
                        .into_report());
                }
            }
        }

        // 5. The namespacing operator may only appear between two valid module identifiers
        // 6. Namespaced module names must adhere to the above rules in each submodule identifier
        if is_namespaced {
            let mut offset = 0u32;
            for component in name.split("::") {
                let len = component.len() as u32;
                let module_name_span = module.name.span();
                let source_id = module_name_span.source_id();
                let start = module_name_span.start() + offset;
                let span = SourceSpan::new(source_id, start..(start + len));
                if component.is_empty() {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid module namespace")
                        .with_primary_label(span, "submodule names cannot be empty")
                        .into_report());
                }

                if !name.starts_with(is_lower_ascii_alphabetic) {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid module namespace")
                        .with_primary_label(
                            span,
                            "submodule name must start with a lowercase, ascii-alphabetic \
                             character",
                        )
                        .into_report());
                }

                offset += len + 2;
            }
        }

        Ok(())
    }
}
impl Rule<Function> for NamingConventions {
    fn validate(
        &mut self,
        function: &Function,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        let name = function.id.function.as_str();
        let span = function.id.function.span();

        // 1. Must not be empty
        if name.is_empty() {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid function name")
                .with_primary_label(span, "function names cannot be empty")
                .into_report());
        }

        // 2. Must start with an ASCII-alphabetic character, underscore, `$` or `@`
        fn name_starts_with(c: char) -> bool {
            c.is_ascii_alphabetic() || matches!(c, '_' | '$' | '@')
        }

        // 3. Otherwise, no restrictions, but may not contain whitespace
        if let Err((offset, c)) = is_valid_identifier(name, name_starts_with, char::is_whitespace) {
            let offset = offset as u32;
            if c.is_whitespace() {
                let source_id = span.source_id();
                let pos = span.start() + offset;
                let span = SourceSpan::at(source_id, pos);
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function name")
                    .with_primary_label(span, "function names may not contain whitespace")
                    .into_report());
            } else {
                debug_assert_eq!(offset, 0);
                let source_id = span.source_id();
                let span = SourceSpan::at(source_id, span.start());
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function name")
                    .with_primary_label(
                        span,
                        "function names must start with an ascii-alphabetic character, '_', '$', \
                         or '@'",
                    )
                    .into_report());
            }
        }

        Ok(())
    }
}
impl Rule<GlobalVariableData> for NamingConventions {
    fn validate(
        &mut self,
        global: &GlobalVariableData,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        let span = global.name.span();
        let name = global.name.as_str();

        // 1. Must not be empty
        if name.is_empty() {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid global variable name")
                .with_primary_label(span, "global variable names cannot be empty")
                .into_report());
        }

        // 2. Must start with an ASCII-alphabetic character, underscore, `.`, `$` or `@`
        fn name_starts_with(c: char) -> bool {
            c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '$' | '@')
        }

        // 3. Otherwise, no restrictions, but may not contain whitespace
        if let Err((offset, c)) = is_valid_identifier(name, name_starts_with, char::is_whitespace) {
            let offset = offset as u32;
            if c.is_whitespace() {
                let source_id = span.source_id();
                let pos = span.start() + offset;
                let span = SourceSpan::at(source_id, pos);

                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid global variable name")
                    .with_primary_label(span, "global variable names may not contain whitespace")
                    .into_report());
            } else {
                debug_assert_eq!(offset, 0);
                let span = SourceSpan::at(span.source_id(), span.start());
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid global variable name")
                    .with_primary_label(
                        span,
                        "global variable names must start with an ascii-alphabetic character, \
                         '_', '.', '$', or '@'",
                    )
                    .into_report());
            }
        }

        Ok(())
    }
}

#[inline(always)]
fn is_lower_ascii_alphabetic(c: char) -> bool {
    c.is_ascii_alphabetic() && c.is_ascii_lowercase()
}

/// This is necessary until [std::str::Pattern] is stabilized
trait Pattern {
    fn matches(&self, c: char) -> bool;
}
impl Pattern for char {
    #[inline(always)]
    fn matches(&self, c: char) -> bool {
        *self == c
    }
}
impl<F> Pattern for F
where
    F: Fn(char) -> bool,
{
    #[inline(always)]
    fn matches(&self, c: char) -> bool {
        self(c)
    }
}

#[inline]
fn is_valid_identifier<P1, P2>(id: &str, start_with: P1, forbidden: P2) -> Result<(), (usize, char)>
where
    P1: Pattern,
    P2: Pattern,
{
    for (offset, c) in id.char_indices() {
        if offset == 0 && !start_with.matches(c) {
            return Err((offset, c));
        }

        if forbidden.matches(c) {
            return Err((offset, c));
        }
    }

    Ok(())
}
