use crate::*;

pub struct If {
    pub op: Operation,
}
impl If {
    pub fn condition(&self) -> Value {
        todo!()
    }

    pub fn then_dest(&self) -> &Successor {
        todo!()
    }

    pub fn else_dest(&self) -> &Successor {
        todo!()
    }
}

/// A while is a loop structure composed of two regions: a "before" region, and an "after" region.
///
/// The "before" region's entry block parameters correspond to the operands expected by the
/// operation, and can be used to compute the condition that determines whether the "after" body
/// is executed or not, or simply forwarded to the "after" region. The "before" region must
/// terminate with a [Condition] operation, which will be evaluated to determine whether or not
/// to continue the loop.
///
/// The "after" region corresponds to the loop body, and must terminate with a [Yield] operation,
/// whose operands must be of the same arity and type as the "before" region's argument list. In
/// this way, the "after" body can feed back input to the "before" body to determine whether to
/// continue the loop.
pub struct While {
    pub op: Operation,
}
impl While {
    pub fn before_region(&self) -> RegionId {
        self.op.regions[0]
    }

    pub fn after_region(&self) -> RegionId {
        self.op.regions[1]
    }
}

pub struct Condition {
    pub op: Operation,
}
impl Condition {
    pub fn condition(&self) -> Value {
        todo!()
    }
}

pub struct Yield {
    pub op: Operation,
}
