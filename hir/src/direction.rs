/// A marker trait for abstracting over the direction in which a traversal is performed, or
/// information is propagated by an analysis, i.e. forward or backward.
///
/// This trait is sealed as there are only two possible directions.
#[allow(private_bounds)]
pub trait Direction: sealed::Direction {
    fn is_forward() -> bool {
        Self::IS_FORWARD
    }
    fn is_backward() -> bool {
        !Self::IS_FORWARD
    }
}

impl<D: sealed::Direction> Direction for D {}

mod sealed {
    pub trait Direction: Default {
        const IS_FORWARD: bool;
    }

    #[derive(Debug, Copy, Clone, Default)]
    pub struct Forward;
    impl Direction for Forward {
        const IS_FORWARD: bool = true;
    }

    #[derive(Debug, Copy, Clone, Default)]
    pub struct Backward;
    impl Direction for Backward {
        const IS_FORWARD: bool = false;
    }
}

pub use self::sealed::{Backward, Forward};
