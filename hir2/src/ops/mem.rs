use crate::*;

pub struct Store {
    pub op: Operation,
}
impl Store {
    pub fn addr(&self) -> Value {
        todo!()
    }

    pub fn value(&self) -> Value {
        todo!()
    }
}

pub struct StoreLocal {
    pub op: Operation,
    pub local: LocalId,
}
impl StoreLocal {
    pub fn value(&self) -> Value {
        todo!()
    }
}

pub struct Load {
    pub op: Operation,
}
impl Load {
    pub fn addr(&self) -> Value {
        todo!()
    }
}

pub struct LoadLocal {
    pub op: Operation,
    pub local: LocalId,
}
