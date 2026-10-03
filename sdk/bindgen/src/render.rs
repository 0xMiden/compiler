//! An indented text builder for the generated Rust source.
//!
//! The generated files are read by people and committed, so they are written line by line, as a
//! person would lay them out, rather than produced from a token stream. The writer never wraps a
//! line by itself: the backends break their lines where rustfmt (with the workspace's settings)
//! breaks them, so the text is what rustfmt would make of it, with the help of
//! [`Writer::signature`] for the one layout every backend shares.

/// The widest line the backends write: rustfmt's `max_width`.
pub(crate) const MAX_WIDTH: usize = 100;

/// The widest array literal written on one line: the workspace's rustfmt `array_width`.
pub(crate) const ARRAY_WIDTH: usize = 80;

/// The widest list of call arguments, or of tuple items or types, written on one line: the
/// workspace's rustfmt `fn_call_width`.
pub(crate) const CALL_WIDTH: usize = 80;

/// `Result` by its full path, as the generated text names it and its variants: a generated
/// module may define a type `Result` or a function `Ok` of its own.
pub(crate) const RESULT: &str = "::core::result::Result";

/// The width of `text` in columns.
pub(crate) fn width(text: &str) -> usize {
    text.chars().count()
}

/// Writes lines of Rust source, indenting the body of each block by four spaces.
#[derive(Debug, Default)]
pub(crate) struct Writer {
    text: String,
    depth: usize,
}

impl Writer {
    /// The number of spaces each block level indents its body by.
    pub(crate) const INDENT: usize = 4;

    /// An empty writer at the outermost level.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// An empty writer at this writer's level, for text that is [appended](Self::append) to it
    /// only if it can be written in full.
    pub(crate) fn nested(&self) -> Self {
        Self {
            text: String::new(),
            depth: self.depth,
        }
    }

    /// Append the text of `nested`, a writer from [`Self::nested`] whose blocks are all closed.
    ///
    /// # Panics
    ///
    /// If `nested` is not back at this writer's level.
    pub(crate) fn append(&mut self, nested: Self) {
        assert_eq!(nested.depth, self.depth, "appending a writer with a block still open");
        self.text.push_str(&nested.text);
    }

    /// The width, in columns, of the current indentation: what a line written now starts with.
    pub(crate) fn indentation(&self) -> usize {
        self.depth * Self::INDENT
    }

    /// Write `line` at the current indentation. An empty `line` is an empty line, with no
    /// trailing whitespace.
    pub(crate) fn line(&mut self, line: &str) {
        debug_assert!(!line.contains('\n'), "one line at a time: {line:?}");
        if !line.is_empty() {
            self.text.extend(core::iter::repeat_n(' ', self.depth * Self::INDENT));
            self.text.push_str(line);
        }
        self.text.push('\n');
    }

    /// Write an empty line.
    pub(crate) fn blank(&mut self) {
        self.line("");
    }

    /// Write `header {` and indent what follows, until the matching [`Self::close`].
    pub(crate) fn open(&mut self, header: &str) {
        self.open_with(&format!("{header} {{"));
    }

    /// Write `line`, which opens a bracket, and indent what follows, until the matching
    /// [`Self::close_with`]: `use path::{`, `const X: [u64; 4] = [`.
    pub(crate) fn open_with(&mut self, line: &str) {
        self.line(line);
        self.depth += 1;
    }

    /// End the innermost block opened by [`Self::open`]: dedent and write `}`.
    ///
    /// # Panics
    ///
    /// If no block is open.
    pub(crate) fn close(&mut self) {
        self.close_with("}");
    }

    /// End the innermost block opened by [`Self::open_with`]: dedent and write `line`, which
    /// closes its bracket (`};`, `];`).
    ///
    /// # Panics
    ///
    /// If no block is open.
    pub(crate) fn close_with(&mut self, line: &str) {
        self.depth = self.depth.checked_sub(1).expect("`close` without a matching `open`");
        self.line(line);
    }

    /// Write `line` one level deeper than the current indentation: the rest of a statement that
    /// does not fit on its first line.
    pub(crate) fn continuation(&mut self, line: &str) {
        self.depth += 1;
        self.line(line);
        self.depth -= 1;
    }

    /// Whether `line`, written at the current indentation, stays within [`MAX_WIDTH`].
    pub(crate) fn fits(&self, line: &str) -> bool {
        self.indentation() + width(line) <= MAX_WIDTH
    }

    /// Write the match arm `pattern => body,`: on one line if it fits, else with `body` on a line
    /// of its own in a block, as rustfmt lays it out.
    pub(crate) fn arm(&mut self, pattern: &str, body: &str) {
        let line = format!("{pattern} => {body},");
        if self.fits(&line) {
            self.line(&line);
        } else {
            self.open_with(&format!("{pattern} => {{"));
            self.line(body);
            self.close_with("}");
        }
    }

    /// A function signature, `head(param, …) -> returns`, followed by ` {` and an indented body
    /// up to the matching [`Self::close`] if `body`, else by `;`. It is laid out as rustfmt lays
    /// it out:
    ///
    /// - on one line if it fits (and a tuple it returns is within [`CALL_WIDTH`]);
    /// - else, with parameters, one parameter per line, each with a trailing comma, then
    ///   `) -> returns`, a returned tuple one type per line if it does not fit;
    /// - else, without parameters, the return type on the line after `head()` unless it fits
    ///   within three columns of the width, the brace on a line of its own when only it does not
    ///   fit, and a returned tuple wider than [`CALL_WIDTH`] one type per line.
    pub(crate) fn signature(
        &mut self,
        head: &str,
        params: &[String],
        returns: &Returns,
        body: bool,
    ) {
        let end = if body { " {" } else { ";" };
        let line = format!("{head}({}){}{end}", params.join(", "), returns.flat());
        if self.fits(&line) && returns.one_line() {
            self.line(&line);
        } else if !params.is_empty() {
            self.open_with(&format!("{head}("));
            for param in params {
                self.line(&format!("{param},"));
            }
            self.depth -= 1;
            self.returns(")", returns, end);
        } else {
            let head = format!("{head}()");
            match returns {
                Returns::Nothing => {
                    self.line(&head);
                    // rustfmt keeps the space before a brace that follows a line it could not fit.
                    self.line(if self.fits(&head) {
                        end.trim_start()
                    } else {
                        end
                    });
                }
                Returns::Tuple(_) if !returns.one_line() => self.returns(&head, returns, end),
                Returns::Type(_) | Returns::Tuple(_) => {
                    let text = returns.text();
                    if self.indentation() + width(&head) + width(&text) + 3 <= MAX_WIDTH {
                        self.line(&format!("{head} -> {text}"));
                        self.line(end.trim_start());
                    } else {
                        self.line(&head);
                        self.line(&format!("-> {text}{end}"));
                    }
                }
            }
        }
        if body {
            self.depth += 1;
        }
    }

    /// `before -> returns end`, a returned tuple one type per line if it does not fit or is wider
    /// than [`CALL_WIDTH`].
    fn returns(&mut self, before: &str, returns: &Returns, end: &str) {
        let line = format!("{before}{}{end}", returns.flat());
        match returns {
            Returns::Tuple(types) if !returns.one_line() || !self.fits(&line) => {
                self.open_with(&format!("{before} -> ("));
                for ty in types {
                    self.line(&format!("{ty},"));
                }
                self.close_with(&format!("){end}"));
            }
            _ => self.line(&line),
        }
    }

    /// The text written so far.
    ///
    /// # Panics
    ///
    /// If a block is still open: the text would not be valid Rust.
    pub(crate) fn finish(self) -> String {
        assert_eq!(self.depth, 0, "{} block(s) still open", self.depth);
        self.text
    }
}

/// What a function returns, for [`Writer::signature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Returns {
    /// Nothing: the signature has no `->`.
    Nothing,
    /// One type.
    Type(String),
    /// A tuple of types.
    Tuple(Vec<String>),
}

impl Returns {
    /// The returned type: `T` or `(A, B)`, empty for [`Self::Nothing`].
    fn text(&self) -> String {
        match self {
            Self::Nothing => String::new(),
            Self::Type(ty) => ty.clone(),
            Self::Tuple(types) => format!("({})", types.join(", ")),
        }
    }

    /// ` -> T` on one line, empty for [`Self::Nothing`].
    fn flat(&self) -> String {
        match self {
            Self::Nothing => String::new(),
            returns => format!(" -> {}", returns.text()),
        }
    }

    /// Whether rustfmt may write it on one line: a tuple only if its types are within
    /// [`CALL_WIDTH`].
    fn one_line(&self) -> bool {
        match self {
            Self::Tuple(types) => width(&types.join(", ")) <= CALL_WIDTH,
            Self::Nothing | Self::Type(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_indent_their_body_by_four_spaces() {
        let mut w = Writer::new();
        w.line("// header");
        w.blank();
        w.open("pub mod outer");
        w.line("use crate::x;");
        w.blank();
        w.open("pub fn f()");
        assert_eq!(w.indentation(), 8);
        w.line("g();");
        w.close();
        w.close();
        // Blank lines carry no indentation.
        let expected = r#"// header

pub mod outer {
    use crate::x;

    pub fn f() {
        g();
    }
}
"#;
        assert_eq!(w.finish(), expected);
    }

    #[test]
    fn brackets_and_continuations_indent_like_blocks() {
        let mut w = Writer::new();
        w.open_with("const X: [u64; 2] = [");
        w.line("1,");
        w.close_with("];");
        w.line("const Y: u64 =");
        w.continuation("2;");
        assert_eq!(w.indentation(), 0);
        let expected = r#"const X: [u64; 2] = [
    1,
];
const Y: u64 =
    2;
"#;
        assert_eq!(w.finish(), expected);
    }

    /// The lines `signature` writes for `head`, with no body.
    fn signature(head: &str, params: &[String], returns: &Returns) -> Vec<String> {
        let mut w = Writer::new();
        w.signature(head, params, returns, false);
        w.finish().lines().map(str::to_string).collect()
    }

    /// The lines `signature` writes for `head`, with an empty body.
    fn function(head: &str, returns: &Returns) -> Vec<String> {
        let mut w = Writer::new();
        w.signature(head, &[], returns, true);
        w.close();
        let text = w.finish();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.last(), Some(&"}"));
        lines[..lines.len() - 1].iter().map(|line| line.to_string()).collect()
    }

    /// A parameter list breaks one parameter per line when the signature would be wider than
    /// rustfmt's `max_width`, and a returned tuple one type per line when it does not fit after
    /// the parameters or its types are wider than `fn_call_width`, as rustfmt breaks them.
    #[test]
    fn parameter_lists_break_one_per_line_past_the_max_width() {
        let params: Vec<String> = (0..7).map(|i| format!("arg{i}: u32")).collect();
        let u32 = Returns::Type("u32".into());
        let one_line = |head: &str| format!("{head}({}) -> u32;", params.join(", "));
        let fits = format!("fn {}", "f".repeat(MAX_WIDTH - one_line("fn ").len()));
        assert_eq!(signature(&fits, &params, &u32), [one_line(&fits)]);
        let lines = signature(&format!("{fits}g"), &params, &u32);
        assert_eq!(lines.len(), 9, "one column too wide: {lines:#?}");
        assert_eq!(lines[0], format!("{fits}g("));
        assert_eq!(lines[1], "    arg0: u32,");
        assert_eq!(lines[8], ") -> u32;");

        // 81 columns of types: one per line, wherever they are.
        let tuple =
            |types: &[&str]| Returns::Tuple(types.iter().map(|ty| ty.to_string()).collect());
        let wide = tuple(&["bool", &"a".repeat(36), &"b".repeat(37)]);
        let lines = signature("fn f", &params[..1], &wide);
        assert_eq!(lines[2..5], [") -> (", "    bool,", &format!("    {},", "a".repeat(36))]);
        assert_eq!(lines.last().unwrap(), ");");
        let lines = signature("fn f", &[], &wide);
        assert_eq!(lines[0], "fn f() -> (");
        // 80 columns of types stay on the line after the parameters.
        let narrow = tuple(&["bool", &"a".repeat(36), &"b".repeat(36)]);
        let lines = signature("fn f", &params[..1], &narrow);
        assert_eq!(lines[2], format!(") -> (bool, {}, {});", "a".repeat(36), "b".repeat(36)));
    }

    /// Without parameters, rustfmt moves the return type, or the brace, to a line of its own.
    #[test]
    fn signatures_without_parameters_break_before_the_return_type() {
        let i32 = Returns::Type("i32".into());
        // `fn ` and `() -> i32 {`: 14 columns besides the name.
        let named = |columns: usize| format!("fn {}", "b".repeat(columns - 14));
        assert_eq!(function(&named(100), &i32).len(), 1);
        let head = named(101);
        assert_eq!(function(&head, &i32), [format!("{head}() -> i32"), "{".into()]);
        let head = named(103);
        assert_eq!(function(&head, &i32), [format!("{head}() -> i32"), "{".into()]);
        let head = named(104);
        assert_eq!(function(&head, &i32), [format!("{head}()"), "-> i32 {".into()]);

        // `fn ` and `() {`: 7 columns besides the name.
        let named = |columns: usize| format!("fn {}", "a".repeat(columns - 7));
        assert_eq!(function(&named(100), &Returns::Nothing).len(), 1);
        let head = named(102);
        assert_eq!(function(&head, &Returns::Nothing), [format!("{head}()"), "{".into()]);
        let head = named(103);
        assert_eq!(function(&head, &Returns::Nothing), [format!("{head}()"), " {".into()]);

        // A tuple within `fn_call_width` moves as a whole.
        let tuple = Returns::Tuple(vec!["a".repeat(38), "b".repeat(38)]);
        let head = format!("fn {}", "c".repeat(30));
        let lines = function(&head, &tuple);
        assert_eq!(
            lines,
            [format!("{head}()"), format!("-> ({}, {}) {{", "a".repeat(38), "b".repeat(38))]
        );
    }

    #[test]
    #[should_panic = "`close` without a matching `open`"]
    fn closing_an_unopened_block_panics() {
        Writer::new().close();
    }

    #[test]
    #[should_panic = "1 block(s) still open"]
    fn finishing_inside_a_block_panics() {
        let mut w = Writer::new();
        w.open("mod m");
        w.finish();
    }
}
