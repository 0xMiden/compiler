use log::{Level, LevelFilter};

use super::FilterOp;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Directive {
    pub kind: DirectiveKind,
    pub level: LevelFilter,
    pub negated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveKind {
    Any,
    Module { module: String },
    Component { component: String },
    Topic { component: String, topic: FilterOp },
}

impl PartialOrd for DirectiveKind {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirectiveKind {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        match (self, other) {
            (Self::Any, Self::Any) => Ordering::Equal,
            (Self::Any, _) => Ordering::Less,
            (_, Self::Any) => Ordering::Greater,
            (Self::Module { module: l }, Self::Module { module: r }) => {
                l.len().cmp(&r.len()).then_with(|| l.cmp(r))
            }
            // Checking modules has greater complexity than components
            (Self::Module { module: _ }, Self::Component { component: _ }) => Ordering::Greater,
            (Self::Component { component: _ }, Self::Module { module: _ }) => Ordering::Less,
            // Checking modules has lesser complexity than the remaining kinds
            (Self::Module { module: _ }, _) => Ordering::Less,
            (_, Self::Module { module: _ }) => Ordering::Greater,
            (Self::Component { component: l }, Self::Component { component: r }) => {
                l.len().cmp(&r.len()).then_with(|| l.cmp(r))
            }
            (Self::Component { component: _ }, _) => Ordering::Less,
            (_, Self::Component { component: _ }) => Ordering::Greater,
            (
                Self::Topic {
                    component: lc,
                    topic: lt,
                },
                Self::Topic {
                    component: rc,
                    topic: rt,
                },
            ) => lc.len().cmp(&rc.len()).then_with(|| lc.cmp(rc)).then(lt.cmp(rt)),
        }
    }
}

impl DirectiveKind {
    pub fn matches(&self, target: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Module { module } => target.contains(module),
            Self::Component { component } => {
                let (target_component, _) = target.split_once(':').unwrap_or((target, ""));
                component == target_component
            }
            Self::Topic { component, topic } => {
                let (target_component, target_topic) =
                    target.split_once(':').unwrap_or((target, ""));
                component == target_component && topic.is_match(target_topic)
            }
        }
    }
}

// Check whether a level and target are enabled by the set of directives.
pub fn enabled(directives: &[Directive], level: Level, target: &str) -> bool {
    // Search for the longest match, the vector is assumed to be pre-sorted.
    let mut was_matched = false;
    for directive in directives.iter().rev() {
        // Don't bother applying further positive matches once we've had one positive match
        if was_matched && !directive.negated {
            continue;
        }

        // Setting level to `off` is equivalent to negation and takes precedence over it
        let (matches, negated) = match directive.level {
            LevelFilter::Off => (directive.kind.matches(target), true),
            filter if level <= filter => (directive.kind.matches(target), directive.negated),
            _ => continue,
        };
        // If we find a negative match, we don't need to do any more checking
        if matches && negated {
            return false;
        }
        was_matched |= matches;
    }
    was_matched
}
