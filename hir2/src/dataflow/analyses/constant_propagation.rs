use alloc::rc::Rc;
use core::fmt;

use crate::{dataflow::LatticeValue, AttributeValue, Dialect};

/// This lattice value represents a known constant value of a lattice.
#[derive(Default)]
pub struct ConstantValue {
    /// The constant value
    constant: Option<Box<dyn AttributeValue>>,
    /// The dialect that can be used to materialize this constant
    dialect: Option<Rc<dyn Dialect>>,
    /// A flag that indicates whether or not this value was explicitly initialized
    initialized: bool,
}
impl fmt::Debug for ConstantValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstantValue")
            .field("value", &self.constant)
            .field_with("dialect", |f| {
                if let Some(dialect) = self.dialect.as_deref() {
                    write!(f, "Some({})", dialect.name())
                } else {
                    f.write_str("None")
                }
            })
            .field("initialized", &self.initialized)
            .finish()
    }
}

impl Clone for ConstantValue {
    fn clone(&self) -> Self {
        let constant = self.constant.as_deref().map(|c| c.clone_value());
        Self {
            constant,
            dialect: self.dialect.clone(),
            initialized: self.initialized,
        }
    }
}

#[allow(unused)]
impl ConstantValue {
    pub fn new(constant: Box<dyn AttributeValue>, dialect: Rc<dyn Dialect>) -> Self {
        Self {
            constant: Some(constant),
            dialect: Some(dialect),
            initialized: true,
        }
    }

    pub fn unknown() -> Self {
        Self {
            initialized: true,
            ..Default::default()
        }
    }

    #[inline]
    pub fn uninitialized() -> Self {
        Self::default()
    }

    #[inline]
    pub const fn is_uninitialized(&self) -> bool {
        !self.initialized
    }

    pub fn constant_value(&self) -> Option<Box<dyn AttributeValue>> {
        assert!(self.initialized, "expected constant value to be initialized");
        self.constant.as_deref().map(|c| c.clone_value())
    }

    pub fn constant_dialect(&self) -> Option<Rc<dyn Dialect>> {
        self.dialect.clone()
    }
}

impl Eq for ConstantValue {}
impl PartialEq for ConstantValue {
    fn eq(&self, other: &Self) -> bool {
        if !self.initialized && !other.initialized {
            return true;
        } else if self.initialized != other.initialized {
            return false;
        }

        self.constant == other.constant
    }
}

impl LatticeValue for ConstantValue {
    /// The join of two constant values is:
    ///
    /// * `unknown` if they represent different values
    /// * The identity function if they represent the same value
    /// * The more defined value if one of the two is uninitialized
    fn join(&self, rhs: &Self) -> Self {
        if self.is_uninitialized() {
            return rhs.clone();
        }

        if rhs.is_uninitialized() || self == rhs {
            return self.clone();
        }

        Self::unknown()
    }

    fn meet(&self, _other: &Self) -> Self {
        self.clone()
    }
}
