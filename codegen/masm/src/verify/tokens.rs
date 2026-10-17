//! Interned raw identities and conservative address-origin facts for one verification run.
//! Token equality proves contents; origin bits separately track possible frame-address escape.
use alloc::{collections::BTreeMap, vec::Vec};

use miden_core::Felt;

use crate::plan::ControlId;

/// Raw-content identity owned by one `Tokens` arena; never interchangeable with an SSA ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Token(pub u32);

/// May-origin information is independent of value equality. Abstraction can forget an
/// exact address, but cannot turn compiler storage into ordinary source data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Origins(u8);
impl Origins {
    /// Add possible origins monotonically and report whether any new possibility was learned.
    pub fn union(&mut self, other: Self) -> bool {
        let previous = *self;
        self.0 |= other.0;
        *self != previous
    }

    /// Whether this value may derive from any frame address, including source locals.
    pub fn has_frame(self) -> bool {
        self.0 != 0
    }

    /// Whether this value may derive from compiler-owned spill or scratch storage.
    pub fn compiler(self) -> bool {
        self.0 & 2 != 0
    }
}

/// Closed identity constructors used by replay. Dynamic scopes prevent unrelated executions
/// of the same static control or compute site from acquiring accidental equality.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Identity {
    /// Incoming lane of the abstract procedure entry.
    Input(u32),
    Literal(u64),
    /// Opaque value from a fresh computation scope.
    Computed(u32),
    /// Path-correlated alternatives, sharing a dynamic join scope with result obligations.
    Join(u32, ControlId, Vec<Token>),
    /// Generalized parameter position of one inductive loop scope.
    LoopParam(u32, u32),
    /// Exact element offset in the assigned local frame.
    FrameAddress(u32),
}

/// Arena shared by all states in one replay. Interning preserves exact equality while
/// separate monotone origin facts survive loss of exact identity at joins and loop headers.
#[derive(Default)]
pub(super) struct Tokens {
    identities: Vec<Identity>,
    interned: BTreeMap<Identity, Token>,
    next_scope: u32,
    source_extent: u32,
    origins: BTreeMap<Token, Origins>,
}
impl Tokens {
    /// Set the boundary between source-owned and compiler-owned frame addresses.
    pub fn new(source_extent: u32) -> Self {
        Self {
            source_extent,
            ..Self::default()
        }
    }

    /// Collect possible origins, including inherited origins from all join alternatives.
    pub fn origins(&self, token: Token) -> Origins {
        fn collect(tokens: &Tokens, token: Token, memo: &mut BTreeMap<Token, Origins>) -> Origins {
            if let Some(origins) = memo.get(&token) {
                return *origins;
            }
            let mut origins = tokens.origins.get(&token).copied().unwrap_or_default();
            match tokens.identity(token) {
                Identity::FrameAddress(offset) => {
                    origins.union(Origins(if *offset < tokens.source_extent { 1 } else { 2 }));
                }
                Identity::Join(_, _, incoming) => {
                    for token in incoming {
                        origins.union(collect(tokens, *token, memo));
                    }
                }
                _ => {}
            }
            memo.insert(token, origins);
            origins
        }
        collect(self, token, &mut BTreeMap::new())
    }

    /// Conservatively taint an existing token; report whether the origin set grew.
    pub fn add_origins(&mut self, token: Token, origins: Origins) -> bool {
        self.origins.entry(token).or_default().union(origins)
    }

    /// Reuse the token for an identical constructor; child tokens must belong to this arena.
    pub fn intern(&mut self, identity: Identity) -> Token {
        if let Some(token) = self.interned.get(&identity) {
            return *token;
        }
        let token = Token(self.identities.len().try_into().expect("token capacity exceeded"));
        self.identities.push(identity.clone());
        self.interned.insert(identity, token);
        token
    }

    /// Allocate an opaque computation identity distinct from every earlier fresh value.
    pub fn fresh(&mut self) -> Token {
        let scope = self.scope();
        self.intern(Identity::Computed(scope))
    }

    /// Reserve a distinct dynamic abstraction scope; exhaustion is an internal capacity panic.
    pub fn scope(&mut self) -> u32 {
        let result = self.next_scope;
        self.next_scope = self.next_scope.checked_add(1).expect("verification scope overflow");
        result
    }

    /// Intern the field's canonical literal representation.
    pub fn literal(&mut self, value: Felt) -> Token {
        self.intern(Identity::Literal(value.as_canonical_u64()))
    }

    /// Inspect a token created by this arena; foreign tokens are outside this API's contract.
    pub fn identity(&self, token: Token) -> &Identity {
        &self.identities[token.0 as usize]
    }

    /// Return only an exact frame offset; possible origin alone does not establish an address.
    pub fn frame_address(&self, token: Token) -> Option<u32> {
        match self.identity(token) {
            Identity::FrameAddress(offset) => Some(*offset),
            _ => None,
        }
    }

    /// Return only an exact literal; joins and opaque computations are not evaluated here.
    pub fn constant(&self, token: Token) -> Option<u64> {
        match self.identity(token) {
            Identity::Literal(value) => Some(*value),
            _ => None,
        }
    }
}
