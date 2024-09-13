use smallvec::SmallVec;

use crate::*;

pub struct Br {
    pub op: Operation,
}
impl Br {
    pub fn dest(&self) -> &Successor {
        &self.op.successors[0]
    }
}

pub struct CondBr {
    pub op: Operation,
}
impl CondBr {
    pub fn condition(&self) -> Value {
        todo!()
    }

    pub fn then_dest(&self) -> &Successor {
        &self.op.successors[0]
    }

    pub fn else_dest(&self) -> &Successor {
        &self.op.successors[1]
    }
}

pub struct Switch {
    pub op: Operation,
    pub cases: SmallVec<[u32; 4]>,
    pub default_successor: usize,
}
impl Switch {
    pub fn selector(&self) -> Value {
        todo!()
    }

    pub fn default_dest(&self) -> &Successor {
        &self.op.successors[self.default_successor]
    }
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: u32,
    pub successor: Successor,
}
