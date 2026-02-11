use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use log::LevelFilter;

use super::directive::DirectiveKind;
use crate::filter::{Directive, FilterOp};

#[derive(Default, Debug)]
pub(crate) struct ParseResult {
    pub(crate) directives: Vec<Directive>,
    pub(crate) filter: Option<FilterOp>,
    pub(crate) errors: Vec<String>,
}

impl ParseResult {
    fn add_directive(&mut self, directive: Directive) {
        self.directives.push(directive);
    }

    fn set_filter(&mut self, filter: FilterOp) {
        self.filter = Some(filter);
    }

    fn add_error(&mut self, message: String) {
        self.errors.push(message);
    }

    pub(crate) fn ok(self) -> Result<(Vec<Directive>, Option<FilterOp>), ParseError> {
        let Self {
            directives,
            filter,
            errors,
        } = self;
        if let Some(error) = errors.into_iter().next() {
            Err(ParseError { details: error })
        } else {
            Ok((directives, filter))
        }
    }
}

/// Error during logger directive parsing process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseError {
    details: String,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "error parsing logger filter: {}", self.details)
    }
}

impl Error for ParseError {}

/// Parse a logging specification string (e.g: `crate1,crate2::mod3,crate3::x=error/foo`)
/// and return a vector with log directives.
pub(crate) fn parse_spec(s: &str) -> ParseResult {
    let mut result = ParseResult::default();

    let (spec, filter) = s.rsplit_once('/').unwrap_or((s, ""));
    if spec.contains('/') {
        result.add_error(format!("invalid logging spec '{s}': too many '/'"));
        return result;
    }
    let filter = if filter.is_empty() {
        None
    } else {
        Some(filter)
    };
    let directives = spec.split(',').map(|s| s.trim());
    for directive in directives {
        if directive.is_empty() {
            continue;
        }

        let (matcher_spec, level, negated) = match directive.rsplit_once('=') {
            Some((ms, "")) => {
                let ms = ms.trim();
                if ms.contains('=') {
                    result.add_error(format!(
                        "invalid logging spec '{directive}': '=' is not allowed in paths"
                    ));
                    continue;
                }
                if let Some(ms) = ms.strip_prefix('-') {
                    (Some(ms), LevelFilter::max(), true)
                } else {
                    (Some(ms), LevelFilter::max(), false)
                }
            }
            Some((ms, level)) => {
                let ms = ms.trim();
                if ms.contains('=') {
                    result.add_error(format!(
                        "invalid logging spec '{directive}': '=' is not allowed in paths"
                    ));
                    continue;
                }
                let level = level
                    .trim()
                    .parse::<LevelFilter>()
                    .map_err(|err| format!("invalid logging spec '{directive}': {err}"));
                match level {
                    Ok(level) => {
                        if let Some(ms) = ms.strip_prefix('-') {
                            (Some(ms), level, true)
                        } else {
                            (Some(ms), level, false)
                        }
                    }
                    Err(err) => {
                        result.add_error(err);
                        continue;
                    }
                }
            }
            None => {
                let (level, negated) = if let Some(level) = directive.strip_prefix('-') {
                    (level.trim(), true)
                } else {
                    (directive, false)
                };
                match level.parse::<LevelFilter>() {
                    Ok(level) => (None, level, negated),
                    Err(_) => (Some(directive), LevelFilter::max(), negated),
                }
            }
        };

        if let Some(matcher_spec) = matcher_spec {
            match matcher_spec.split_once(':') {
                Some((component, "*" | "")) => {
                    result.add_directive(Directive {
                        kind: DirectiveKind::Component {
                            component: component.to_owned(),
                        },
                        level,
                        negated,
                    });
                }
                // If `topic` starts with a ':', we've attempted to parse a module path as a filter
                // spec, e.g. `core::option`
                Some((_, topic)) if topic.starts_with(':') => {
                    result.add_directive(Directive {
                        kind: DirectiveKind::Module {
                            module: matcher_spec.to_owned(),
                        },
                        level,
                        negated,
                    });
                    continue;
                }
                Some((component, topic)) => {
                    let topic = match FilterOp::new(topic) {
                        Ok(topic) => topic,
                        Err(err) => {
                            result.add_error(format!("invalid logging spec '{directive}': {err}"));
                            continue;
                        }
                    };
                    result.add_directive(Directive {
                        kind: DirectiveKind::Topic {
                            component: component.to_owned(),
                            topic,
                        },
                        level,
                        negated,
                    });
                }
                None => {
                    result.add_directive(Directive {
                        kind: DirectiveKind::Component {
                            component: matcher_spec.to_owned(),
                        },
                        level,
                        negated,
                    });
                }
            }
        } else {
            result.add_directive(Directive {
                kind: DirectiveKind::Any,
                level,
                negated,
            });
        }
    }

    if let Some(filter) = filter {
        match FilterOp::new(filter) {
            Ok(filter_op) => result.set_filter(filter_op),
            Err(err) => result.add_error(format!("invalid regex filter - {err}")),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use log::LevelFilter;
    use snapbox::{Data, IntoData, assert_data_eq, str};

    use super::{ParseResult, parse_spec};
    use crate::filter::{ParseError, directive::DirectiveKind, op::FilterOp};

    impl IntoData for ParseError {
        fn into_data(self) -> Data {
            self.to_string().into_data()
        }
    }

    #[test]
    fn parse_spec_valid() {
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec(
            "crate1::mod1=error,crate1::mod2,crate2=debug,component:topic=trace,component2=trace",
        );

        assert_eq!(dirs.len(), 5);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Module {
                module: "crate1::mod1".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Error);

        assert_eq!(
            dirs[1].kind,
            DirectiveKind::Module {
                module: "crate1::mod2".to_owned()
            }
        );
        assert_eq!(dirs[1].level, LevelFilter::max());

        assert_eq!(
            dirs[2].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[2].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(
            dirs[3].kind,
            DirectiveKind::Topic {
                component: "component".to_owned(),
                topic: FilterOp::new("topic").unwrap()
            }
        );
        assert_eq!(dirs[3].level, LevelFilter::Trace);
        assert!(filter.is_none());

        assert_eq!(
            dirs[4].kind,
            DirectiveKind::Component {
                component: "component2".to_owned()
            }
        );
        assert_eq!(dirs[4].level, LevelFilter::Trace);
        assert!(filter.is_none());

        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_invalid_crate() {
        // test parse_spec with multiple = in specification
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=warn=info,crate2=debug");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 1);
        assert_data_eq!(
            &errors[0],
            str!["invalid logging spec 'crate1::mod1=warn=info': '=' is not allowed in paths"]
        );
    }

    #[test]
    fn parse_spec_invalid_level() {
        // test parse_spec with 'noNumber' as log level
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=noNumber,crate2=debug");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 1);
        assert_data_eq!(
            &errors[0],
            str![
                "invalid logging spec 'crate1::mod1=noNumber': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn parse_spec_string_level() {
        // test parse_spec with 'warn' as log level
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=wrong,crate2=warn");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Warn);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 1);
        assert_data_eq!(
            &errors[0],
            str![
                "invalid logging spec 'crate1::mod1=wrong': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn parse_spec_empty_level() {
        // test parse_spec with '' as log level
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=wrong,crate2=");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::max());
        assert!(filter.is_none());

        assert_eq!(errors.len(), 1);
        assert_data_eq!(
            &errors[0],
            str![
                "invalid logging spec 'crate1::mod1=wrong': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn parse_spec_empty_level_isolated() {
        // test parse_spec with "" as log level (and the entire spec str)
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec(""); // should be ignored
        assert_eq!(dirs.len(), 0);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_blank_level_isolated() {
        // test parse_spec with a white-space-only string specified as the log
        // level (and the entire spec str)
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("     "); // should be ignored
        assert_eq!(dirs.len(), 0);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_blank_level_isolated_comma_only() {
        // The spec should contain zero or more comma-separated string slices,
        // so a comma-only string should be interpreted as two empty strings
        // (which should both be treated as invalid, so ignored).
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec(","); // should be ignored
        assert_eq!(dirs.len(), 0);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_blank_level_isolated_comma_blank() {
        // The spec should contain zero or more comma-separated string slices,
        // so this bogus spec should be interpreted as containing one empty
        // string and one blank string. Both should both be treated as
        // invalid, so ignored.
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec(",     "); // should be ignored
        assert_eq!(dirs.len(), 0);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_blank_level_isolated_blank_comma() {
        // The spec should contain zero or more comma-separated string slices,
        // so this bogus spec should be interpreted as containing one blank
        // string and one empty string. Both should both be treated as
        // invalid, so ignored.
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("     ,"); // should be ignored
        assert_eq!(dirs.len(), 0);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_global() {
        // test parse_spec with no crate
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("warn,crate2=debug");
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0].kind, DirectiveKind::Any);
        assert_eq!(dirs[0].level, LevelFilter::Warn);
        assert_eq!(
            dirs[1].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[1].level, LevelFilter::Debug);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_global_bare_warn_lc() {
        // test parse_spec with no crate, in isolation, all lowercase
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("warn");
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].kind, DirectiveKind::Any);
        assert_eq!(dirs[0].level, LevelFilter::Warn);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_global_bare_warn_uc() {
        // test parse_spec with no crate, in isolation, all uppercase
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("WARN");
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].kind, DirectiveKind::Any);
        assert_eq!(dirs[0].level, LevelFilter::Warn);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_global_bare_warn_mixed() {
        // test parse_spec with no crate, in isolation, mixed case
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("wArN");
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].kind, DirectiveKind::Any);
        assert_eq!(dirs[0].level, LevelFilter::Warn);
        assert!(filter.is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_valid_filter() {
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=error,crate1::mod2,crate2=debug/abc");
        assert_eq!(dirs.len(), 3);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Module {
                module: "crate1::mod1".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Error);

        assert_eq!(
            dirs[1].kind,
            DirectiveKind::Module {
                module: "crate1::mod2".to_owned()
            }
        );
        assert_eq!(dirs[1].level, LevelFilter::max());

        assert_eq!(
            dirs[2].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[2].level, LevelFilter::Debug);
        assert!(filter.is_some() && filter.unwrap().to_string() == "abc");
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_invalid_crate_filter() {
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=error=warn,crate2=debug/a.c");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_some() && filter.unwrap().to_string() == "a.c");

        assert_eq!(errors.len(), 1);
        assert_data_eq!(
            &errors[0],
            str!["invalid logging spec 'crate1::mod1=error=warn': '=' is not allowed in paths"]
        );
    }

    #[test]
    fn parse_spec_empty_with_filter() {
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1/a*c");
        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate1".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::max());
        assert!(filter.is_some() && filter.unwrap().to_string() == "a*c");
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_spec_with_multiple_filters() {
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("debug/abc/a.c");
        assert!(dirs.is_empty());
        assert!(filter.is_none());

        assert_eq!(errors.len(), 1);
        assert_data_eq!(&errors[0], str!["invalid logging spec 'debug/abc/a.c': too many '/'"]);
    }

    #[test]
    fn parse_spec_multiple_invalid_crates() {
        // test parse_spec with multiple = in specification
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=warn=info,crate2=debug,crate3=error=error");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 2);
        assert_data_eq!(
            &errors[0],
            str!["invalid logging spec 'crate1::mod1=warn=info': '=' is not allowed in paths"]
        );
        assert_data_eq!(
            &errors[1],
            str!["invalid logging spec 'crate3=error=error': '=' is not allowed in paths"]
        );
    }

    #[test]
    fn parse_spec_multiple_invalid_levels() {
        // test parse_spec with 'noNumber' as log level
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=noNumber,crate2=debug,crate3=invalid");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 2);
        assert_data_eq!(
            &errors[0],
            str![
                "invalid logging spec 'crate1::mod1=noNumber': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
        assert_data_eq!(
            &errors[1],
            str![
                "invalid logging spec 'crate3=invalid': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn parse_spec_invalid_crate_and_level() {
        // test parse_spec with 'noNumber' as log level
        let ParseResult {
            directives: dirs,
            filter,
            errors,
        } = parse_spec("crate1::mod1=debug=info,crate2=debug,crate3=invalid");

        assert_eq!(dirs.len(), 1);
        assert_eq!(
            dirs[0].kind,
            DirectiveKind::Component {
                component: "crate2".to_owned()
            }
        );
        assert_eq!(dirs[0].level, LevelFilter::Debug);
        assert!(filter.is_none());

        assert_eq!(errors.len(), 2);
        assert_data_eq!(
            &errors[0],
            str!["invalid logging spec 'crate1::mod1=debug=info': '=' is not allowed in paths"]
        );
        assert_data_eq!(
            &errors[1],
            str![
                "invalid logging spec 'crate3=invalid': attempted to convert a string that \
                 doesn't match an existing log level"
            ]
        );
    }

    #[test]
    fn parse_error_message_single_error() {
        let error = parse_spec("crate1::mod1=debug=info,crate2=debug").ok().unwrap_err();
        assert_data_eq!(
            error,
            str![
                "error parsing logger filter: invalid logging spec 'crate1::mod1=debug=info': '=' \
                 is not allowed in paths"
            ]
        );
    }

    #[test]
    fn parse_error_message_multiple_errors() {
        let error = parse_spec("crate1::mod1=debug=info,crate2=debug,crate3=invalid")
            .ok()
            .unwrap_err();
        assert_data_eq!(
            error,
            str![
                "error parsing logger filter: invalid logging spec 'crate1::mod1=debug=info': '=' \
                 is not allowed in paths"
            ]
        );
    }
}
