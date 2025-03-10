macro_rules! has_no_effects {
    ($Op:ty) => {
        impl ::midenc_hir2::effects::EffectOpInterface<::midenc_hir2::effects::MemoryEffect>
            for $Op
        {
            fn has_no_effect(&self) -> bool {
                true
            }

            fn effects(
                &self,
            ) -> ::midenc_hir2::effects::EffectIterator<::midenc_hir2::effects::MemoryEffect> {
                ::midenc_hir2::effects::EffectIterator::from_smallvec(smallvec::smallvec![])
            }
        }
    };
}

mod binary;
mod coercions;
mod constants;
mod unary;

pub use self::{binary::*, coercions::*, constants::*, unary::*};
