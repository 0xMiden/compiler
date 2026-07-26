use alloc::{collections::BTreeMap, format};
use core::fmt;

use midenc_session::diagnostics::Report;

use super::CheckpointId;
use crate::CompilerResult;

/// Identifies a registered frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrontendId(&'static str);

impl FrontendId {
    /// Construct a frontend id from a static string.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The stable string form of this frontend id.
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for FrontendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Declares a frontend: which target roots it handles, and what route it runs.
///
/// The route is ordered. Checkpoints carry no global ordering, so all comparisons —
/// which checkpoint is furthest, is this stop point reachable — are answered against a
/// single registration's route.
#[derive(Debug, Clone, Copy)]
pub struct FrontendRegistration {
    /// The frontend's stable identifier.
    pub id: FrontendId,
    /// Target-root file extensions this frontend handles, without a leading dot.
    pub extensions: &'static [&'static str],
    /// The checkpoints this frontend can reach, in the order it reaches them.
    pub route: &'static [CheckpointId],
    /// User-facing stop aliases mapped onto this route's checkpoints.
    pub aliases: &'static [(&'static str, CheckpointId)],
}

impl FrontendRegistration {
    /// Resolve a stop alias such as `parse` to this route's corresponding checkpoint.
    pub fn resolve_alias(&self, alias: &str) -> Option<CheckpointId> {
        self.aliases
            .iter()
            .find_map(|(name, checkpoint)| (*name == alias).then_some(*checkpoint))
    }

    /// The position of `checkpoint` in this route, if it is part of it.
    pub fn position(&self, checkpoint: CheckpointId) -> Option<usize> {
        self.route.iter().position(|candidate| *candidate == checkpoint)
    }

    /// Returns true if this route reaches `checkpoint`.
    pub fn supports(&self, checkpoint: CheckpointId) -> bool {
        self.position(checkpoint).is_some()
    }

    /// The final checkpoint of this route.
    ///
    /// # Panics
    ///
    /// Panics if the route is empty. [`FrontendRegistry::register`] rejects empty routes,
    /// so this is unreachable for registered frontends.
    pub fn terminal(&self) -> CheckpointId {
        *self.route.last().expect("frontend route must not be empty")
    }

    /// The aliases this route supports, for use in diagnostics.
    pub fn alias_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.aliases.iter().map(|(name, _)| *name)
    }
}

/// The set of frontends available to a compilation, keyed by target-root extension.
#[derive(Debug, Default)]
pub struct FrontendRegistry {
    by_extension: BTreeMap<&'static str, FrontendRegistration>,
}

impl FrontendRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `registration` for each of its extensions.
    ///
    /// Returns an error if the route is empty, if an alias names a checkpoint outside the
    /// route, or if any extension is already claimed by another frontend.
    pub fn register(&mut self, registration: FrontendRegistration) -> CompilerResult<()> {
        if registration.route.is_empty() {
            return Err(Report::msg(format!(
                "frontend '{}' declares an empty route",
                registration.id
            )));
        }
        for (alias, checkpoint) in registration.aliases {
            if !registration.supports(*checkpoint) {
                return Err(Report::msg(format!(
                    "frontend '{}' maps alias '{alias}' to '{checkpoint}', which is not on its \
                     route",
                    registration.id
                )));
            }
        }
        for extension in registration.extensions {
            if let Some(existing) = self.by_extension.get(extension) {
                return Err(Report::msg(format!(
                    "cannot register frontend '{}' for extension '{extension}': already \
                     registered by frontend '{}'",
                    registration.id, existing.id
                )));
            }
        }
        for extension in registration.extensions {
            self.by_extension.insert(extension, registration);
        }
        Ok(())
    }

    /// The frontend registered for `extension`, if any.
    pub fn for_extension(&self, extension: &str) -> Option<&FrontendRegistration> {
        self.by_extension.get(extension)
    }

    /// Every registered extension, in sorted order, for use in diagnostics.
    pub fn extensions(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_extension.keys().copied()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use alloc::format;

    use super::*;

    pub(crate) const WASM: FrontendRegistration = FrontendRegistration {
        id: FrontendId::new("wasm"),
        extensions: &["wasm", "wat"],
        route: &[
            CheckpointId::WASM_PARSED,
            CheckpointId::HIR_INITIAL,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::HIR_TRANSFORMED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        aliases: &[
            ("parse", CheckpointId::WASM_PARSED),
            ("analyze", CheckpointId::HIR_ANALYZED),
            ("transform", CheckpointId::HIR_TRANSFORMED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
    };

    const MASM: FrontendRegistration = FrontendRegistration {
        id: FrontendId::new("masm"),
        extensions: &["masm"],
        route: &[
            CheckpointId::MASM_PARSED,
            CheckpointId::HIR_ANALYZED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        aliases: &[
            ("parse", CheckpointId::MASM_PARSED),
            ("analyze", CheckpointId::HIR_ANALYZED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
    };

    fn registry() -> FrontendRegistry {
        let mut registry = FrontendRegistry::new();
        registry.register(WASM).expect("wasm should register");
        registry.register(MASM).expect("masm should register");
        registry
    }

    #[test]
    fn dispatch_is_by_target_root_extension() {
        let registry = registry();
        assert_eq!(registry.for_extension("wat").map(|r| r.id), Some(FrontendId::new("wasm")));
        assert_eq!(registry.for_extension("wasm").map(|r| r.id), Some(FrontendId::new("wasm")));
        assert_eq!(registry.for_extension("masm").map(|r| r.id), Some(FrontendId::new("masm")));
        assert!(registry.for_extension("rs").is_none());
    }

    #[test]
    fn duplicate_extension_registration_is_an_error() {
        const CONFLICT: FrontendRegistration = FrontendRegistration {
            id: FrontendId::new("other"),
            extensions: &["masm"],
            route: &[CheckpointId::PACKAGE_ASSEMBLED],
            aliases: &[],
        };
        let err = registry().register(CONFLICT).expect_err("masm is already registered");
        let rendered = format!("{err}");
        assert!(rendered.contains("masm"), "diagnostic should name the extension: {rendered}");
    }

    #[test]
    fn an_empty_route_is_rejected() {
        const EMPTY: FrontendRegistration = FrontendRegistration {
            id: FrontendId::new("empty"),
            extensions: &["nothing"],
            route: &[],
            aliases: &[],
        };
        let err = FrontendRegistry::new().register(EMPTY).expect_err("empty route");
        assert!(format!("{err}").contains("empty route"));
    }

    #[test]
    fn an_alias_outside_the_route_is_rejected() {
        const BAD: FrontendRegistration = FrontendRegistration {
            id: FrontendId::new("bad"),
            extensions: &["bad"],
            route: &[CheckpointId::PACKAGE_ASSEMBLED],
            aliases: &[("parse", CheckpointId::HIR_INITIAL)],
        };
        let err = FrontendRegistry::new().register(BAD).expect_err("alias off-route");
        assert!(format!("{err}").contains("hir.initial"));
    }

    #[test]
    fn aliases_resolve_within_a_route() {
        assert_eq!(WASM.resolve_alias("parse"), Some(CheckpointId::WASM_PARSED));
        assert_eq!(MASM.resolve_alias("parse"), Some(CheckpointId::MASM_PARSED));
        assert_eq!(WASM.resolve_alias("transform"), Some(CheckpointId::HIR_TRANSFORMED));
    }

    #[test]
    fn masm_has_no_transform_alias() {
        assert_eq!(MASM.resolve_alias("transform"), None);
        assert!(!MASM.supports(CheckpointId::HIR_TRANSFORMED));
    }

    #[test]
    fn route_position_orders_checkpoints_within_a_route() {
        assert!(
            WASM.position(CheckpointId::HIR_INITIAL) < WASM.position(CheckpointId::MASM_LOWERED),
            "hir.initial precedes masm.lowered on the wasm route"
        );
        assert_eq!(WASM.position(CheckpointId::MASM_PARSED), None);
        assert_eq!(WASM.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
    }
}
