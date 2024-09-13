use crate::*;

#[derive(Debug, Clone)]
pub struct GlobalValueOp {
    pub id: GlobalValue,
    pub data: GlobalValueData,
    pub op: Operation,
}

impl Op for GlobalValueOp {
    type Id = GlobalValue;

    #[inline(always)]
    fn id(&self) -> Self::Id {
        self.id
    }

    fn name(&self) -> &'static str {
        match self.data {
            GlobalValueData::Symbol { .. } => "global.symbol",
            GlobalValueData::Load { .. } => "global.load",
            GlobalValueData::IAddImm { .. } => "global.iadd",
        }
    }

    #[inline(always)]
    fn as_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }
}
