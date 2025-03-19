macro_rules! has_no_effects {
    ($Op:ty) => {
        impl ::midenc_hir::effects::EffectOpInterface<::midenc_hir::effects::MemoryEffect> for $Op {
            fn has_no_effect(&self) -> bool {
                true
            }

            fn effects(
                &self,
            ) -> ::midenc_hir::effects::EffectIterator<::midenc_hir::effects::MemoryEffect> {
                ::midenc_hir::effects::EffectIterator::from_smallvec(smallvec::smallvec![])
            }
        }
    };
}

mod binary;
mod coercions;
mod constants;
mod unary;

pub use self::{binary::*, coercions::*, constants::*, unary::*};
