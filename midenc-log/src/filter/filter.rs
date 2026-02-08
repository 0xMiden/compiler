use std::{env, fmt, mem};

use log::{LevelFilter, Metadata, Record};

use crate::filter::{
    Directive, DirectiveKind, FilterOp, KvFilter, ParseError, enabled, kv_filter::KvFilterOp,
    parse_spec, parser::ParseResult,
};

/// A builder for a log filter.
///
/// It can be used to parse a set of directives from a string before building
/// a [`Filter`] instance.
///
/// ## Example
///
/// ```
/// # use std::env;
/// use env_filter::Builder;
///
/// let mut builder = Builder::new();
///
/// // Parse a logging filter from an environment variable.
/// if let Ok(rust_log) = env::var("RUST_LOG") {
///     builder.parse(&rust_log);
/// }
///
/// let filter = builder.build();
/// ```
pub struct Builder {
    directives: Vec<Directive>,
    kv_filters: Vec<KvFilter>,
    filter: Option<FilterOp>,
    built: bool,
}

impl Builder {
    /// Initializes the filter builder with defaults.
    pub fn new() -> Builder {
        Builder {
            directives: Vec::new(),
            kv_filters: Vec::new(),
            filter: None,
            built: false,
        }
    }

    /// Initializes the filter builder from an environment.
    pub fn from_env(env: &str) -> Builder {
        let mut builder = Builder::new();

        if let Ok(s) = env::var(env) {
            builder.parse(&s);
        }

        builder
    }

    /// Insert the directive but ignore duplicates
    fn insert_directive(&mut self, directive: Directive) {
        for d in self.directives.iter_mut() {
            if d.negated != directive.negated {
                continue;
            }
            match &d.kind {
                DirectiveKind::Any if matches!(&directive.kind, DirectiveKind::Any) => {
                    d.level = directive.level;
                    return;
                }
                DirectiveKind::Module { module } if matches!(&directive.kind, DirectiveKind::Module { module: m } if m == module) =>
                {
                    d.level = directive.level;
                    return;
                }
                DirectiveKind::Component { component } if matches!(&directive.kind, DirectiveKind::Component { component: c } if c == component) =>
                {
                    d.level = directive.level;
                    return;
                }
                DirectiveKind::Topic { component, topic } if matches!(&directive.kind, DirectiveKind::Topic { component: c, topic: t } if c == component && t == topic) =>
                {
                    d.level = directive.level;
                    return;
                }
                _ => continue,
            }
        }
        self.directives.push(directive);
    }

    /// Adds a directive to the filter for a specific module.
    pub fn filter_module(&mut self, module: &str, level: LevelFilter) -> &mut Self {
        self.insert_directive(Directive {
            kind: DirectiveKind::Module {
                module: module.to_string(),
            },
            level,
            negated: false,
        });
        self
    }

    /// Adds a directive to the filter for all modules.
    pub fn filter_level(&mut self, level: LevelFilter) -> &mut Self {
        self.insert_directive(Directive {
            kind: DirectiveKind::Any,
            level,
            negated: false,
        });
        self
    }

    /// Adds a component directive to the filter.
    pub fn filter_component(
        &mut self,
        component: &str,
        level: LevelFilter,
        negated: bool,
    ) -> &mut Self {
        self.insert_directive(Directive {
            kind: DirectiveKind::Component {
                component: component.to_string(),
            },
            level,
            negated,
        });
        self
    }

    /// Adds a component + topic directive to the filter.
    pub fn filter_topic(
        &mut self,
        component: &str,
        topic: &str,
        level: LevelFilter,
        negated: bool,
    ) -> &mut Self {
        self.insert_directive(Directive {
            kind: DirectiveKind::Topic {
                component: component.to_string(),
                topic: FilterOp::new(topic).expect("invalid topic filter"),
            },
            level,
            negated,
        });
        self
    }

    /// Adds a key/value filter to the overall filter.
    pub fn filter_key_value(&mut self, key: &str, value: &str, negated: bool) -> &mut Self {
        for kv_filter in self.kv_filters.iter_mut() {
            if kv_filter.key != key {
                continue;
            }

            kv_filter.insert_value_filter(value, negated);
            return self;
        }

        self.kv_filters.push(KvFilter {
            key: key.to_owned(),
            patterns: vec![KvFilterOp {
                value: FilterOp::new(value).expect("invalid value filter"),
                negated,
            }],
        });
        self
    }

    /// Parses the directives string.
    ///
    /// See the [Enabling Logging] section for more details.
    ///
    /// [Enabling Logging]: ../index.html#enabling-logging
    pub fn parse(&mut self, filters: &str) -> &mut Self {
        #![allow(clippy::print_stderr)] // compatibility

        let ParseResult {
            directives,
            filter,
            errors,
        } = parse_spec(filters);

        for error in errors {
            eprintln!("warning: {error}, ignoring it");
        }

        self.filter = filter;

        for directive in directives {
            self.insert_directive(directive);
        }
        self
    }

    /// Parses the directive string, returning an error if the given directive string is invalid.
    ///
    /// See the [Enabling Logging] section for more details.
    ///
    /// [Enabling Logging]: ../index.html#enabling-logging
    pub fn try_parse(&mut self, filters: &str) -> Result<&mut Self, ParseError> {
        let (directives, filter) = parse_spec(filters).ok()?;

        self.filter = filter;

        for directive in directives {
            self.insert_directive(directive);
        }
        Ok(self)
    }

    /// Build a log filter.
    pub fn build(&mut self) -> Filter {
        assert!(!self.built, "attempt to re-use consumed builder");
        self.built = true;

        let mut directives = Vec::new();
        if self.directives.is_empty() {
            // Adds the default filter if none exist
            directives.push(Directive {
                kind: DirectiveKind::Any,
                level: LevelFilter::Error,
                negated: false,
            });
        } else {
            // Consume directives.
            directives = mem::take(&mut self.directives);
            // Sort the directives to place more efficient matches at the start
            directives.sort();
        }

        Filter {
            directives,
            kv_filters: mem::take(&mut self.kv_filters),
            filter: mem::take(&mut self.filter),
        }
    }
}

impl Default for Builder {
    fn default() -> Self {
        Builder::new()
    }
}

impl fmt::Debug for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.built {
            f.debug_struct("Filter").field("built", &true).finish()
        } else {
            f.debug_struct("Filter")
                .field("filter", &self.filter)
                .field("kv_filters", &self.kv_filters)
                .field("directives", &self.directives)
                .finish()
        }
    }
}

/// A log filter.
///
/// This struct can be used to determine whether or not a log record
/// should be written to the output.
/// Use the [`Builder`] type to parse and construct a `Filter`.
///
/// [`Builder`]: struct.Builder.html
#[derive(Clone)]
pub struct Filter {
    directives: Vec<Directive>,
    kv_filters: Vec<KvFilter>,
    filter: Option<FilterOp>,
}

impl Filter {
    /// Returns the maximum `LevelFilter` that this filter instance is
    /// configured to output.
    ///
    /// # Example
    ///
    /// ```rust
    /// use log::LevelFilter;
    /// use env_filter::Builder;
    ///
    /// let mut builder = Builder::new();
    /// builder.filter(Some("module1"), LevelFilter::Info);
    /// builder.filter(Some("module2"), LevelFilter::Error);
    ///
    /// let filter = builder.build();
    /// assert_eq!(filter.filter(), LevelFilter::Info);
    /// ```
    pub fn filter(&self) -> LevelFilter {
        self.directives.iter().map(|d| d.level).max().unwrap_or(LevelFilter::Off)
    }

    /// Checks if this record matches the configured filter.
    pub fn matches(&self, record: &Record<'_>) -> bool {
        let level = record.metadata().level();
        let target = record.metadata().target();

        let is_enabled = enabled(&self.directives, level, target);
        if !is_enabled {
            return false;
        }

        // We treat the lack of kv filters as a match by default - if there are any filters, and
        // any of those filters applied to the key/value data of the record, then the record is
        // matched so long as none of the matches were negated
        let mut was_matched = None;
        let kv = record.key_values();
        for kv_filter in self.kv_filters.iter() {
            match kv_filter.matches(kv) {
                None => continue,
                Some(false) => return false,
                Some(true) => {
                    was_matched = Some(true);
                }
            }
        }

        if was_matched.is_some_and(|matched| !matched) {
            return false;
        }

        if let Some(filter) = self.filter.as_ref()
            && !filter.is_match(&record.args().to_string())
        {
            return false;
        }

        true
    }

    /// Determines if a log message with the specified metadata would be logged.
    pub fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let level = metadata.level();
        let target = metadata.target();

        enabled(&self.directives, level, target)
    }
}

impl fmt::Debug for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("filter", &self.filter)
            .field("directives", &self.directives)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use log::{Level, LevelFilter};
    use snapbox::{assert_data_eq, str};

    use super::{Builder, Directive, DirectiveKind, Filter, enabled};

    fn make_logger_filter(dirs: Vec<Directive>) -> Filter {
        let mut logger = Builder::new().build();
        logger.directives = dirs;
        logger
    }

    #[test]
    fn filter_info() {
        let logger = Builder::new().filter_level(LevelFilter::Info).build();
        assert!(enabled(&logger.directives, Level::Info, "crate1"));
        assert!(!enabled(&logger.directives, Level::Debug, "crate1"));
    }

    #[test]
    fn filter_beginning_longest_match() {
        let logger = Builder::new()
            .filter_module("crate2", LevelFilter::Info)
            .filter_module("crate2::mod", LevelFilter::Debug)
            .filter_module("crate1::mod1", LevelFilter::Warn)
            .build();
        assert!(enabled(&logger.directives, Level::Debug, "crate2::mod1"));
        assert!(!enabled(&logger.directives, Level::Debug, "crate2"));
    }

    // Some of our tests are only correct or complete when they cover the full
    // universe of variants for log::Level. In the unlikely event that a new
    // variant is added in the future, this test will detect the scenario and
    // alert us to the need to review and update the tests. In such a
    // situation, this test will fail to compile, and the error message will
    // look something like this:
    //
    //     error[E0004]: non-exhaustive patterns: `NewVariant` not covered
    //        --> src/filter/mod.rs:413:15
    //         |
    //     413 |         match level_universe {
    //         |               ^^^^^^^^^^^^^^ pattern `NewVariant` not covered
    #[test]
    fn ensure_tests_cover_level_universe() {
        let level_universe: Level = Level::Trace; // use of trace variant is arbitrary
        match level_universe {
            Level::Error | Level::Warn | Level::Info | Level::Debug | Level::Trace => (),
        }
    }

    #[test]
    fn parse_default() {
        let logger = Builder::new().parse("info,crate1::mod1=warn").build();
        assert!(enabled(&logger.directives, Level::Warn, "crate1::mod1"));
        assert!(enabled(&logger.directives, Level::Info, "crate2::mod2"));
    }

    #[test]
    fn parse_default_bare_level_off_lc() {
        let logger = Builder::new().parse("off").build();
        assert!(!enabled(&logger.directives, Level::Error, ""));
        assert!(!enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_off_uc() {
        let logger = Builder::new().parse("OFF").build();
        assert!(!enabled(&logger.directives, Level::Error, ""));
        assert!(!enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_error_lc() {
        let logger = Builder::new().parse("error").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(!enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_error_uc() {
        let logger = Builder::new().parse("ERROR").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(!enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_warn_lc() {
        let logger = Builder::new().parse("warn").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_warn_uc() {
        let logger = Builder::new().parse("WARN").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(!enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_info_lc() {
        let logger = Builder::new().parse("info").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_info_uc() {
        let logger = Builder::new().parse("INFO").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(!enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_debug_lc() {
        let logger = Builder::new().parse("debug").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_debug_uc() {
        let logger = Builder::new().parse("DEBUG").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(enabled(&logger.directives, Level::Debug, ""));
        assert!(!enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_trace_lc() {
        let logger = Builder::new().parse("trace").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(enabled(&logger.directives, Level::Debug, ""));
        assert!(enabled(&logger.directives, Level::Trace, ""));
    }

    #[test]
    fn parse_default_bare_level_trace_uc() {
        let logger = Builder::new().parse("TRACE").build();
        assert!(enabled(&logger.directives, Level::Error, ""));
        assert!(enabled(&logger.directives, Level::Warn, ""));
        assert!(enabled(&logger.directives, Level::Info, ""));
        assert!(enabled(&logger.directives, Level::Debug, ""));
        assert!(enabled(&logger.directives, Level::Trace, ""));
    }

    // In practice, the desired log level is typically specified by a token
    // that is either all lowercase (e.g., 'trace') or all uppercase (.e.g,
    // 'TRACE'), but this tests serves as a reminder that
    // log::Level::from_str() ignores all case variants.
    #[test]
    fn parse_default_bare_level_debug_mixed() {
        {
            let logger = Builder::new().parse("Debug").build();
            assert!(enabled(&logger.directives, Level::Error, ""));
            assert!(enabled(&logger.directives, Level::Warn, ""));
            assert!(enabled(&logger.directives, Level::Info, ""));
            assert!(enabled(&logger.directives, Level::Debug, ""));
            assert!(!enabled(&logger.directives, Level::Trace, ""));
        }
        {
            let logger = Builder::new().parse("debuG").build();
            assert!(enabled(&logger.directives, Level::Error, ""));
            assert!(enabled(&logger.directives, Level::Warn, ""));
            assert!(enabled(&logger.directives, Level::Info, ""));
            assert!(enabled(&logger.directives, Level::Debug, ""));
            assert!(!enabled(&logger.directives, Level::Trace, ""));
        }
        {
            let logger = Builder::new().parse("deBug").build();
            assert!(enabled(&logger.directives, Level::Error, ""));
            assert!(enabled(&logger.directives, Level::Warn, ""));
            assert!(enabled(&logger.directives, Level::Info, ""));
            assert!(enabled(&logger.directives, Level::Debug, ""));
            assert!(!enabled(&logger.directives, Level::Trace, ""));
        }
        {
            let logger = Builder::new().parse("DeBuG").build(); // LaTeX flavor!
            assert!(enabled(&logger.directives, Level::Error, ""));
            assert!(enabled(&logger.directives, Level::Warn, ""));
            assert!(enabled(&logger.directives, Level::Info, ""));
            assert!(enabled(&logger.directives, Level::Debug, ""));
            assert!(!enabled(&logger.directives, Level::Trace, ""));
        }
    }

    #[test]
    fn try_parse_valid_filter() {
        let logger = Builder::new()
            .try_parse("info,crate1::mod1=warn")
            .expect("valid filter returned error")
            .build();
        assert!(enabled(&logger.directives, Level::Warn, "crate1::mod1"));
        assert!(enabled(&logger.directives, Level::Info, "crate2::mod2"));
    }

    #[test]
    fn try_parse_invalid_filter() {
        let error = Builder::new().try_parse("info,crate1=invalid").unwrap_err();
        assert_data_eq!(
            error,
            str![
                "error parsing logger filter: invalid logging spec 'crate1=invalid': attempted to \
                 convert a string that doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn match_full_path() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate2".to_owned(),
                },
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Warn,
                negated: false,
            },
        ]);
        assert!(enabled(&logger.directives, Level::Warn, "crate1::mod1"));
        assert!(!enabled(&logger.directives, Level::Info, "crate1::mod1"));
        assert!(enabled(&logger.directives, Level::Info, "crate2"));
        assert!(!enabled(&logger.directives, Level::Debug, "crate2"));
    }

    #[test]
    fn no_match() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate2".to_owned(),
                },
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Warn,
                negated: false,
            },
        ]);
        assert!(!enabled(&logger.directives, Level::Warn, "crate3"));
    }

    #[test]
    fn match_beginning() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate2".to_owned(),
                },
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Warn,
                negated: false,
            },
        ]);
        assert!(enabled(&logger.directives, Level::Info, "crate2::mod1"));
    }

    #[test]
    fn match_beginning_longest_match() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate2".to_owned(),
                },
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate2::mod".to_owned(),
                },
                level: LevelFilter::Debug,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Warn,
                negated: false,
            },
        ]);
        assert!(enabled(&logger.directives, Level::Debug, "crate2::mod1"));
        assert!(!enabled(&logger.directives, Level::Debug, "crate2"));
    }

    #[test]
    fn match_default() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Any,
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Warn,
                negated: false,
            },
        ]);
        assert!(enabled(&logger.directives, Level::Warn, "crate1::mod1"));
        assert!(enabled(&logger.directives, Level::Info, "crate2::mod2"));
    }

    #[test]
    fn zero_level() {
        let logger = make_logger_filter(vec![
            Directive {
                kind: DirectiveKind::Any,
                level: LevelFilter::Info,
                negated: false,
            },
            Directive {
                kind: DirectiveKind::Module {
                    module: "crate1::mod1".to_owned(),
                },
                level: LevelFilter::Off,
                negated: false,
            },
        ]);
        assert!(!enabled(&logger.directives, Level::Error, "crate1::mod1"));
        assert!(enabled(&logger.directives, Level::Info, "crate2::mod2"));
    }
}
