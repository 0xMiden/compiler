//! Conditional ABI equalities are obligations, not facts to intersect at joins.
use alloc::{collections::BTreeMap, rc::Rc, vec::Vec};

use super::{
    VerificationError, fail,
    tokens::{Identity, Token, Tokens},
};
use crate::plan::{Atom, DiagnosticCategory};

/// The site index is stable in the registered unit definition. A subsequent iteration
/// of that same static site supersedes its preceding iteration's result.
pub(super) type Results = BTreeMap<(Atom, usize), Rc<Requirement>>;

/// Equality obligation for a published call result. Branch nodes preserve which calls
/// actually executed; absence on one arm cannot erase obligations on the other arm.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Requirement {
    /// The published token must equal at least one candidate identity.
    EqualAny(Vec<Token>),
    Branch {
        scope: u32,
        arms: [Option<Rc<Requirement>>; 2],
    },
}

impl Requirement {
    /// Require equality with this exact call-result token.
    pub fn equal(token: Token) -> Rc<Self> {
        Rc::new(Self::EqualAny(alloc::vec![token]))
    }

    /// Align the requirement with the very same dynamic join scope used for physical
    /// tokens. None means this call did not execute on that arm, not that it is optional
    /// on the arm where it did execute. Rc shares subgraphs rather than enumerating paths.
    pub fn merge(scope: u32, left: &Results, right: &Results) -> Results {
        let mut merged = Results::new();
        for key in left.keys().chain(right.keys()) {
            merged.entry(*key).or_insert_with(|| {
                let a = left.get(key);
                let b = right.get(key);
                if let (Some(a), Some(b)) = (a, b)
                    && Rc::ptr_eq(a, b)
                {
                    return a.clone();
                }
                Rc::new(Self::Branch {
                    scope,
                    arms: [a.cloned(), b.cloned()],
                })
            });
        }
        merged
    }

    /// Prove equality on every applicable dynamic branch, matching join scopes rather than
    /// treating alternatives from unrelated executions as interchangeable.
    pub fn satisfied(self: &Rc<Self>, actual: Token, tokens: &Tokens) -> bool {
        fn check(
            actual: Token,
            expected: &Rc<Requirement>,
            tokens: &Tokens,
            memo: &mut BTreeMap<(Token, *const Requirement), bool>,
        ) -> bool {
            let key = (actual, Rc::as_ptr(expected));
            if let Some(result) = memo.get(&key) {
                return *result;
            }
            let result = match expected.as_ref() {
                Requirement::EqualAny(candidates) if candidates.contains(&actual) => true,
                Requirement::Branch { scope, arms } => match tokens.identity(actual) {
                    Identity::Join(actual_scope, _, incoming) if actual_scope == scope => {
                        arms.iter().zip(incoming).all(|(required, actual)| {
                            required
                                .as_ref()
                                .is_none_or(|required| check(*actual, required, tokens, memo))
                        })
                    }
                    Identity::Join(actual_scope, _, incoming) if actual_scope > scope => {
                        incoming.iter().all(|actual| check(*actual, expected, tokens, memo))
                    }
                    _ => {
                        arms.iter().flatten().all(|required| check(actual, required, tokens, memo))
                    }
                },
                Requirement::EqualAny(_) => match tokens.identity(actual) {
                    Identity::Join(_, _, incoming) => {
                        incoming.iter().all(|actual| check(*actual, expected, tokens, memo))
                    }
                    _ => false,
                },
            };
            memo.insert(key, result);
            result
        }
        check(actual, self, tokens, &mut BTreeMap::new())
    }
}

/// Discharge an incoming state before forgetting its particular identities. This same
/// relation checks initialization and every backedge. Thus header positions represent
/// the applicable entry result on zero-trip paths and the latest result on paths which
/// reexecute the site. A requirement is checked on all its applicable arms before it is
/// generalized; no predecessor's exact token is retained as an arbitrary exit token.
/// Candidate sets decrease over finite invariant/carried positions, without histories.
pub(super) fn transport(
    previous: &Results,
    incoming: &Results,
    retained: &[(Token, Token)],
    tokens: &Tokens,
) -> Result<Results, VerificationError> {
    let mut results = Results::new();
    if previous.keys().any(|key| !incoming.contains_key(key)) {
        return Err(fail(
            DiagnosticCategory::AbiMismatch,
            "loop lost an applicable signature result requirement",
        ));
    }
    for (key, requirement) in incoming {
        let mut candidates = retained
            .iter()
            .filter(|(actual, _)| requirement.satisfied(*actual, tokens))
            .map(|(_, header)| *header)
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        if let Some(previous) = previous.get(key)
            && let Requirement::EqualAny(previous) = previous.as_ref()
        {
            candidates.retain(|token| previous.contains(token));
        }
        if candidates.is_empty() {
            return Err(fail(
                DiagnosticCategory::AbiMismatch,
                "loop discarded an applicable signature result",
            ));
        }
        results.insert(*key, Rc::new(Requirement::EqualAny(candidates)));
    }
    Ok(results)
}
